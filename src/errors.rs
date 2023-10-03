use displaydoc::Display;
use url::Url;

/// Wrapper for a generic serialization error
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct DeserializationError(#[from] toml::de::Error);

/// Wrapper for a generic serialization error
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct SerializationError(#[from] toml::ser::Error);

/// Request failure context
#[derive(thiserror::Error, Debug)]
#[error("{method} request to {url}")]
pub struct RequestError {
    url: Url,
    method: reqwest::Method,
    headers: reqwest::header::HeaderMap,
    source: reqwest::Error,
}

impl RequestError {
    pub(crate) fn create(
        method: reqwest::Method,
        url: Url,
        source: reqwest::Error,
        headers: reqwest::header::HeaderMap,
    ) -> Self {
        Self {
            url,
            method,
            headers,
            source,
        }
    }

    /// The target URL of the request
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The HTTP method used
    pub fn method(&self) -> &str {
        self.method.as_str()
    }

    /// An iterator over header entries (keys may repeat)
    pub fn header_iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers.iter().map(|(k, v)| {
            (
                k.as_str(),
                v.to_str()
                    .expect("unexpected error: request header value contains opaque bytes"),
            )
        })
    }
}

/// Response context for a failed request
#[derive(thiserror::Error, Debug)]
#[error("{status}")]
pub struct ResponseError {
    #[source]
    request: RequestError,
    status: reqwest::StatusCode,
}

impl ResponseError {
    pub(crate) fn new(request: RequestError, status: reqwest::StatusCode) -> Self {
        Self { request, status }
    }

    /// The HTTP status code
    pub fn status_code(&self) -> u16 {
        self.status.as_u16()
    }

    /// The HTTP status as a message
    pub fn status_message(&self) -> &str {
        self.status.as_str()
    }

    /// Context for the request that produced this error
    pub fn request(&self) -> &RequestError {
        &self.request
    }
}

#[derive(thiserror::Error, Display, Debug)]
#[allow(missing_docs)]
pub enum ConfigError {
    /// unknown error
    Unknown,
}

#[derive(thiserror::Error, Display, Debug)]
#[allow(missing_docs)]
pub enum HttpError {
    /// failed to make an http request
    Request(#[from] RequestError),
    /// remote produced an error response
    Response(#[from] ResponseError),
    /// unauthorized - please provide registry credentials with `buffrs login`
    Unauthorized,
    /// {0}
    Other(String),
}
