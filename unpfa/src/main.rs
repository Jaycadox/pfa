use anyhow::{anyhow, Context, Result};
use pfa::reader::PfaReader;
use std::io::Write;
use std::path::PathBuf;

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let file_path = args.next().ok_or(anyhow!("no file path specified"))?;
    let view = args.next().map(|arg| arg == "--view").unwrap_or(false);

    let f = std::fs::File::open(&file_path).context(format!("failed to open file: {file_path}"))?;
    let mut reader = PfaReader::new(f).context("failed to read PFA file")?;
    let root_dir_path = format!("./{}", reader.get_name());
    let root_dir = std::path::Path::new(&root_dir_path);

    if !view {
        std::fs::create_dir(root_dir).context(format!(
            "failed to create root directory at: {}",
            root_dir.display()
        ))?;
    }

    let mut res: Result<()> = Ok(());
    println!("{}", reader.get_name());

    reader.traverse_files("/", |file| {
        if res.is_err() {
            return;
        }

        res = (|| {
            let contents = file.get_contents();
            let path = file.get_path();
            if !view {
                let full_path = PathBuf::from(&format!("{}{}", root_dir_path, path));
                let parent_path = full_path
                    .parent()
                    .ok_or(anyhow!("parent not found: {}", full_path.display()))?;

                std::fs::create_dir_all(parent_path).context(format!(
                    "could not create directory: {}",
                    parent_path.display()
                ))?;
                let mut system_file = std::fs::File::create(&full_path).context(format!(
                    "failed to create file '{}' on system to replicate archive file with path: {}",
                    full_path.display(),
                    path
                ))?;
                system_file.write_all(contents).context(format!(
                    "failed to write {}b to: {}",
                    contents.len(),
                    full_path.display()
                ))?;
            }

            println!("\t'{}' ({}b)", path, contents.len());
            Ok(())
        })();
    });

    res
}

fn main() {
    if let Err(e) = run() {
        println!("unpfa -- PFA extractor");
        println!("usage: unpfa [file_path] (--view)");
        eprintln!("ERROR: {}", e);
        e.chain()
            .skip(1)
            .for_each(|c| eprintln!("\tCaused by: {c}"))
    }
}
