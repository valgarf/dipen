use std::{io, num::ParseIntError, str::Utf8Error};

use crate::ETCDConfigBuilderError;

#[derive(thiserror::Error, Debug)]
pub enum PetriError {
    // #[error("data store disconnected")]
    // Disconnect(#[from] io::Error),
    // #[error("the data for key `{0}` is not available")]
    // Redaction(String),
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader { expected: String, found: String },
    #[error("Filesystem error")]
    IOError(#[from] io::Error),
    #[error("ETCD error")]
    ETCDError(#[from] etcd_client::Error),
    #[error("Configuration errpr")]
    ETCDConfigError(#[from] ETCDConfigBuilderError),
    #[error("Not connected")]
    NotConnected(),
    #[error("Conversion to/from utf8 failed")]
    UTF8ConversionError(#[from] Utf8Error),
    #[error("Parsing a string into an integer failed")]
    ParseError(#[from] ParseIntError),
    #[error("Config error")]
    ConfigError(String),
    #[error("Action Cancelled")]
    Cancelled(),
    #[error("Inappropriate value")]
    ValueError(String),
    #[error("Other error")]
    Other(String),
    #[error("State is inconsistent")]
    InconsistentState(String),
    #[error("Not implemented yet")]
    NotImplemented(),
}

pub type Result<T> = std::result::Result<T, PetriError>;
