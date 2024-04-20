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

    #[error("Tried to decrypt unencrypted file")]
    DecryptUnencryptedFileError,

    #[error("Failed to decrypt file")]
    FileDecryptError,

    #[error("Key to encrypted file was not provided")]
    EncryptedFileKeyNotProvided,

    #[error("PFA IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Malformed path")]
    MalformedPathError,

    #[error("Failed to apply error correction: {0}")]
    ErrorCorrectionError(String),

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

    use crate::{builder::PfaBuilder, reader::PfaReader, shared::DataFlags};

    #[test]
    fn test_1() {
        let mut builder = PfaBuilder::new("epic_name");
        builder
            .add_file(
                "dir_name/file.txt",
                vec![5; 1200],
                DataFlags::forced_compression().error_correction(Some(0.3)),
            )
            .unwrap();
        builder
            .add_file(
                "dir_name/file2.txt",
                vec![1, 2, 3, 4, 5, 7],
                DataFlags::no_compression().error_correction(Some(0.1)),
            )
            .unwrap();
        builder
            .add_file(
                "dir_name/dir/file3.txt",
                vec![1, 2, 3, 4, 5, 7],
                DataFlags::auto(),
            )
            .unwrap();

        let encrypted_key = DataFlags::generate_key();

        builder
            .add_file(
                "dir_name/dir/encrypted_file.txt",
                vec![5; 80],
                DataFlags::auto().encryption(Some(encrypted_key)),
            )
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
        let f = reader
            .get_file("/dir_name/dir/encrypted_file.txt", Some(encrypted_key))
            .unwrap()
            .unwrap();
        assert_eq!(&f.get_name(), "encrypted_file.txt");
        assert_eq!(
            &f.get_path().to_string(),
            "/dir_name/dir/encrypted_file.txt"
        );
        assert_eq!(f.get_contents(), [5; 80]);
    }

    #[test]
    fn test_include_directory() {
        let mut builder = PfaBuilder::new("epic_name");
        builder
            .include_directory("./src", DataFlags::auto())
            .unwrap();

        let _ = builder.build().unwrap();
    }
}
