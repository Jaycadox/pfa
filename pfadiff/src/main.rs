use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pfa::reader::PfaReader;
use pfadiff_lib::{apply_diff, create_diff};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Create {
        #[clap(short, long)]
        old: PathBuf,
        #[clap(short, long)]
        new: PathBuf,
        #[clap(short, long)]
        diff_output: PathBuf,
    },
    Apply {
        #[clap(short, long)]
        old: PathBuf,
        #[clap(short, long)]
        diff: PathBuf,
        #[clap(short, long)]
        new_output: PathBuf,
    },
}

fn run() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Commands::Create {
            old,
            new,
            diff_output,
        } => {
            let old = PfaReader::new(BufReader::new(File::open(old).context("open old file")?))
                .context("parse old file pfa")?;
            let new = PfaReader::new(BufReader::new(File::open(new).context("open new file")?))
                .context("open new file pfa")?;
            let out = BufWriter::new(File::create(diff_output).context("create output file")?);
            create_diff(old, new, out).context("create diff")?
        }
        Commands::Apply {
            old,
            diff,
            new_output,
        } => {
            let old = PfaReader::new(BufReader::new(File::open(old).context("open old file")?))
                .context("parse old file pfa")?;
            let diff = PfaReader::new(BufReader::new(File::open(diff).context("open diff file")?))
                .context("open new file pfa")?;
            let out = BufWriter::new(File::create(new_output).context("create output file")?);
            apply_diff(old, diff, out).context("create diff")?
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ERROR: {}", e);
        e.chain()
            .skip(1)
            .for_each(|c| eprintln!("\tCaused by: {c}"))
    }
}
