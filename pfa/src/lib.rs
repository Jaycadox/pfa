pub mod reader;
pub mod shared;
pub mod writer;
use std::string::FromUtf8Error;

use lz4_flex::block::DecompressError;
use thiserror::Error;
pub use writer::builder;

#[derive(Error, Debug)]
pub enum PfaError {
    #[error("Generic PFA error: {0}")]
    CustomError(String),

    #[error("PFA IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("invalid utf8 string: {0}")]
    StringDecodeError(#[from] FromUtf8Error),

    #[error("Failed to decompress: {0}")]
    FailedDecompressionError(#[from] DecompressError),

    #[error("Unknown PFA error")]
    Unknown,
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use crate::{builder::PfaBuilder, reader::PfaReader};

    #[test]
    fn test_1() {
        let mut builder = PfaBuilder::new("epic_name");
        builder
            .add_file("dir_name/file.txt", vec![5; 1200], true)
            .unwrap();
        builder
            .add_file("dir_name/file2.txt", vec![1, 2, 3, 4, 5, 7], false)
            .unwrap();
        builder
            .add_file("dir_name/dir/file3.txt", vec![1, 2, 3, 4, 5, 7], false)
            .unwrap();

        let bytes = builder.build().unwrap();
        let mut f = std::fs::File::create("out.pfa").unwrap();
        f.write_all(&bytes).unwrap();
        let mut reader = PfaReader::new(Cursor::new(bytes)).unwrap();
        let mut files = vec![];
        reader.traverse_files("/", |file| {
            files.push(file);
        });

        assert_eq!(&reader.get_name(), &"epic_name");
        assert_eq!(reader.get_version(), 1);
        assert_eq!(reader.get_extra_data().len(), 0);

        let f = files.pop().unwrap();
        assert_eq!(&f.get_name(), "file3.txt");
        assert_eq!(&f.get_path().to_string(), "/dir_name/dir/file3.txt");
        assert_eq!(f.get_contents(), &[1, 2, 3, 4, 5, 7]);

        let f = files.pop().unwrap();
        assert_eq!(&f.get_name(), "file2.txt");
        assert_eq!(&f.get_path().to_string(), "/dir_name/file2.txt");
        assert_eq!(f.get_contents(), &[1, 2, 3, 4, 5, 7]);

        let f = files.pop().unwrap();
        assert_eq!(&f.get_name(), "file.txt");
        assert_eq!(&f.get_path().to_string(), "/dir_name/file.txt");
        assert_eq!(f.get_contents(), &[5; 1200]);

        assert!(files.is_empty());
    }
}
