pub mod reader;
pub mod writer;
use std::string::FromUtf8Error;

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

    #[error("Unknown PFA error")]
    Unknown,
}
