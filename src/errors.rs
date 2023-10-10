use miette::Diagnostic;

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("environment variable {0} is not set")]
pub(crate) struct EnvVarNotSet(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("failed to determine if {0} file exists")]
pub(crate) struct FileExistsError(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("could not write to {0} file")]
pub(crate) struct WriteError(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("could not read from {0} file")]
pub(crate) struct ReadError(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("could not deserialize {0}")]
pub(crate) struct DeserializationError(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("could not serialize {0}")]
pub(crate) struct SerializationError(pub &'static str);

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("file `{0}` is missing")]
pub(crate) struct FileNotFound(pub String);
