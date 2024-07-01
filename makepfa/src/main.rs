use std::{io::Write, path::Path};

use pfa::shared::DataFlags;

fn usage() -> ! {
    eprintln!("USAGE:");
    eprintln!("\tmakepfa [directory]");
    std::process::exit(0);
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 || args[0] == "--help" || args[0] == "-h" {
        usage()
    }
    let directory_name = args.pop().unwrap();
    if let Ok(meta) = std::fs::metadata(&directory_name) {
        if !meta.is_dir() {
            eprintln!("Found '{directory_name}', but it is not a directory");
            usage()
        }
        let path = Path::new(&directory_name);
        let canon_path = path.canonicalize().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut pfa = pfa::builder::PfaBuilder::new(&name);
        pfa.include_directory(canon_path.to_str().unwrap(), DataFlags::auto())
            .unwrap();
        let bytes = pfa.build().unwrap();
        let mut file = std::fs::File::create(format!("{name}.pfa")).unwrap();
        file.write_all(&bytes).unwrap();
    } else {
        eprintln!("Directory '{directory_name}' not found");
        usage()
    }
}
