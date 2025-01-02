use std::{io, num::ParseIntError, str::Utf8Error};

use crate::etcd::ETCDConfigBuilderError;

#[derive(thiserror::Error, Debug)]
pub enum PetriError {
    #[error("Filesystem error: {0}")]
    IOError(#[from] io::Error),
    #[error("ETCD error: {0}")]
    ETCDError(#[from] etcd_client::Error),
    #[error("Configuration error: {0}")]
    ETCDConfigError(#[from] ETCDConfigBuilderError),
    #[error("Not connected")]
    NotConnected(),
    #[error("Conversion to/from utf8 failed: {0}")]
    UTF8ConversionError(#[from] Utf8Error),
    #[error("Parsing a string into an integer failed: {0}")]
    ParseError(#[from] ParseIntError),
    #[error("Config error: {0}")]
    ConfigError(String),
    #[error("Action Cancelled")]
    Cancelled(),
    #[error("Inappropriate value: {0}")]
    ValueError(String),
    #[error("Other error: {0}")]
    Other(String),
    #[error("State is inconsistent: {0}")]
    InconsistentState(String),
    #[error("Not implemented yet")]
    NotImplemented(),
}

pub type Result<T> = std::result::Result<T, PetriError>;
