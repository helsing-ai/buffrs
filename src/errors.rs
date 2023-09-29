use thiserror::Error;

/// Generic input/output error with context message
#[derive(Error, Debug)]
#[error("{msg}. Cause: {source}")]
pub struct IoError {
    source: std::io::Error,
    msg: String,
}

impl IoError {
    /// Constructs an IoError from an std IO error and a context message
    pub fn new(source: std::io::Error, msg: impl Into<String>) -> Self {
        Self {
            source,
            msg: msg.into(),
        }
    }
}
