use pfa::builder::*;
use pfa::reader::*;
use std::io::Cursor;

fn main() {
    let mut builder = PfaBuilder::new("epic_name");
    builder
        .add_file("dir_name/file.txt", vec![1, 2, 3, 4, 5, 6])
        .unwrap();
    builder
        .add_file("dir_name/file2.txt", vec![1, 2, 3, 4, 5, 7])
        .unwrap();

    let bytes = builder.build().unwrap();
    let mut reader = PfaReader::new(Cursor::new(bytes)).unwrap();
    let file = reader.get_file("/dir_name2/file2.txt");
    println!("got file: {:?}", file);
}
