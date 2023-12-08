use pfa::builder::*;
use std::io::Write;

fn main() {
    let mut args = std::env::args();
    let _ = args.next();

    let Some(file_path) = args.next() else {
        panic!("no file input")
    };

    let mut builder = PfaBuilder::new("epic_name");
    builder
        .add_file("dir_name/file.txt", vec![1, 2, 3, 4, 5])
        .unwrap();

    let bytes = builder.build().unwrap();

    let mut f = std::fs::File::create(file_path).unwrap();
    f.write_all(&bytes).unwrap();
}
