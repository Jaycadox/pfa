use pfa::builder::*;
use pfa::reader::*;
use std::io::Cursor;
use std::io::Write;

fn main() {
    let mut builder = PfaBuilder::new("epic_name");
    builder
        .add_file("dir_name/file.txt", vec![1, 2, 3, 4, 5, 6])
        .unwrap();
    builder
        .add_file("dir_name/file2.txt", vec![1, 2, 3, 4, 5, 7])
        .unwrap();
    builder
        .add_file("dir_name/dir/file3.txt", vec![1, 2, 3, 4, 5, 7])
        .unwrap();

    let bytes = builder.build().unwrap();
    let mut f = std::fs::File::create("out").unwrap();
    f.write_all(&bytes).unwrap();
    let mut reader = PfaReader::new(Cursor::new(bytes)).unwrap();
    reader.traverse_files("/", |file| {
        println!("Found file: {}", file.get_path());
    })
}
