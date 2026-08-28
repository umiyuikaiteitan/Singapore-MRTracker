//! Error types for the DataMall client.

use crate::transport::TransportError;

/// An error that occurs when the client talks to LTA DataMall.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DataMallError {
    /// The account key is empty.
    #[error("the account key is empty")]
    EmptyKey,

    /// The environment variable with the account key is not set.
    #[error("the environment variable {0} is not set")]
    MissingEnv(&'static str),

    /// DataMall rejected the account key.
    #[error("DataMall rejected the account key (HTTP 401)")]
    InvalidKey,

    /// DataMall rejected the request rate. Wait, then try again.
    #[error("DataMall rejected the request rate (HTTP 429)")]
    RateLimited,

    /// The server returned an unexpected HTTP status.
    #[error("unexpected HTTP status {status} from {url}")]
    Http {
        /// The HTTP status code.
        status: u16,
        /// The requested URL.
        url: String,
    },

    /// The transport could not complete the request.
    #[error(transparent)]
    Transport(#[from] TransportError),

    /// The client cannot decode the response body.
    #[error("cannot decode the response from {url}: {message}")]
    Decode {
        /// The requested URL.
        url: String,
        /// A description of the problem.
        message: String,
    },

    /// A download link does not use HTTPS.
    ///
    /// A pre-signed link carries a signature but no confidentiality,
    /// so the client refuses to fetch it over any other scheme.
    #[error("the download link {url} uses the scheme \"{scheme}\"; a dataset link must use HTTPS")]
    InsecureScheme {
        /// The requested URL, with its query redacted.
        url: String,
        /// The scheme that the link uses, without the `://`.
        scheme: String,
    },

    /// The response body is larger than the accepted limit.
    ///
    /// The client refuses the response instead of returning the first
    /// `limit` bytes of it.
    #[error("the response from {url} is larger than the limit of {limit} bytes")]
    TooLarge {
        /// The requested URL, with its query redacted.
        url: String,
        /// The limit that the response exceeded, in bytes.
        limit: usize,
    },

    /// The response contains no download link.
    #[error("the response from {url} contains no download link")]
    NoLink {
        /// The requested URL.
        url: String,
    },
}
