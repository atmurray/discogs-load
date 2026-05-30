use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use log::info;
use quick_xml::{events::Event, Reader};
use std::{fs::File, io::BufReader, path::PathBuf};
use structopt::StructOpt;

mod artist;
mod db;
mod label;
mod master;
mod parser;
mod release;

const BUF_SIZE: usize = 4096; // 4kb at once

#[derive(StructOpt, Debug)]
#[structopt(name = "discogs-load")]
struct Opt {
    /// Path to one or more discogs monthly data dump files, still compressed
    #[structopt(name = "FILE(S)", parse(from_os_str))]
    files: Vec<PathBuf>,

    // DB related arguments
    #[structopt(flatten)]
    dbopts: db::DbOpt,
}

fn main() -> Result<()> {
    let log_env = env_logger::Env::default().default_filter_or("info");
    env_logger::Builder::from_env(log_env).init();

    let opt = Opt::from_args();

    read_files(&opt)
}

fn read_files(opt: &Opt) -> Result<()> {
    for file in &opt.files {
        let gzfile = File::open(file).with_context(|| format!("opening {}", file.display()))?;
        let xmlfile = GzDecoder::new(gzfile);
        let xmlfile = BufReader::new(xmlfile);
        let mut xmlfile = Reader::from_reader(xmlfile);
        let mut buf = Vec::with_capacity(BUF_SIZE);

        // Parse fileinput on type (label/release/artist)
        let mut parser: Box<dyn parser::Parser> = loop {
            match xmlfile.read_event(&mut buf)? {
                Event::Start(ref e) => {
                    match e.name() {
                        b"labels" => {
                            db::init(&opt.dbopts, "tables/label.sql")?;
                            break Box::new(label::LabelsParser::new(&opt.dbopts));
                        }
                        b"releases" => {
                            db::init(&opt.dbopts, "tables/release.sql")?;
                            break Box::new(release::ReleasesParser::new(&opt.dbopts));
                        }
                        b"artists" => {
                            db::init(&opt.dbopts, "tables/artist.sql")?;
                            break Box::new(artist::ArtistsParser::new(&opt.dbopts));
                        }
                        b"masters" => {
                            db::init(&opt.dbopts, "tables/master.sql")?;
                            break Box::new(master::MastersParser::new(&opt.dbopts));
                        }
                        _ => (),
                    };
                }
                Event::Eof => bail!(
                    "{} does not contain a supported Discogs root tag",
                    file.display()
                ),
                _ => (),
            }
            buf.clear();
        };

        // Parse and insert file
        let gzfile = File::open(file).with_context(|| format!("opening {}", file.display()))?;
        let xmlfile = GzDecoder::new(gzfile);
        let xmlfile = BufReader::new(xmlfile);
        let mut xmlfile = Reader::from_reader(xmlfile);
        let mut buf = Vec::with_capacity(BUF_SIZE);
        info!("Parsing and inserting: {}", file.display());
        loop {
            match xmlfile.read_event(&mut buf)? {
                Event::Eof => break,
                ev => parser.process(ev)?,
            };
            buf.clear();
        }
    }

    if opt.dbopts.create_indexes {
        db::indexes(&opt.dbopts, "indexes_safe.sql")?;
    }

    Ok(())
}
