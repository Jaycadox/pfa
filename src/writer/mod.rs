use thiserror::Error;

pub mod pfa_builder;
mod pfa_writer;

pub use pfa_builder as builder;

#[derive(Error, Debug)]
pub enum PfaError {
    #[error("Generic PFA error: {0}")]
    CustomError(String),

    #[error("PFA IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Unknown PFA error")]
    Unknown,
}
