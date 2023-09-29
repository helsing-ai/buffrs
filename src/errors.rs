use thiserror::Error;

#[derive(Error, Debug)]
#[error("{msg}. Cause: {source}")]
pub struct IoError {
    source: std::io::Error,
    msg: String,
}

impl IoError {
    pub fn new(source: std::io::Error, msg: impl Into<String>) -> Self {
        Self {
            source,
            msg: msg.into(),
        }
    }
}
