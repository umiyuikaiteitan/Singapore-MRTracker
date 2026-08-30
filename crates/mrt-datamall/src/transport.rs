//! The HTTP transport abstraction.
//!
//! The client sends requests through the [`Transport`] trait. The
//! default implementation, [`UreqTransport`], is behind the
//! `http-ureq` feature. Supply your own implementation to use a
//! different HTTP stack, to add retries, or to test without a network.
//!
//! # The size contract
//!
//! One constant, [`MAX_DATASET_BYTES`], bounds every response body
//! that this crate accepts, and it is a refusal, not a truncation. The
//! built-in transport reads one byte past the limit and reports an
//! oversized body as a [`TransportError`]; the client checks the
//! delivered body against the same constant, so a custom transport
//! cannot smuggle a larger body past it either. No caller ever
//! receives a body that was cut short and presented as complete.

/// The largest response body that this crate accepts, in bytes.
///
/// The train GTFS Schedule archive is a few megabytes, and the
/// realtime messages are far smaller, so 256 MiB leaves several orders
/// of magnitude of headroom for a growing feed while still bounding
/// the memory that a misbehaving or hostile host can make this process
/// allocate.
///
/// Every download in the crate measures against this one value:
///
/// - [`UreqTransport`] stops reading one byte beyond it and fails.
/// - `DataMallClient::download` and `DataMallClient::download_limited`
///   reject a body beyond it, or beyond the stricter limit that the
///   caller passed.
///
/// A body past the limit is always an error. The crate never returns a
/// truncated body as if it were the whole file, because a shortened
/// GTFS archive or Protocol Buffer message would decode into a
/// plausible but wrong timetable.
pub const MAX_DATASET_BYTES: usize = 256 * 1024 * 1024;

/// An HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    /// The HTTP status code.
    pub status: u16,
    /// The response body.
    pub body: Vec<u8>,
}

impl Response {
    /// Get the body as text. Invalid UTF-8 bytes become replacement
    /// characters.
    pub fn body_text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }
}

/// An error from the transport layer, for example a connection
/// failure.
#[derive(Debug, thiserror::Error)]
#[error("transport error: {0}")]
pub struct TransportError(pub String);

/// A minimal HTTP client abstraction.
///
/// An implementation must return `Ok` with the real status code for
/// every completed HTTP exchange, also for status codes such as 401 or
/// 500. It must return `Err` only when no HTTP exchange completed, or
/// when the response body breaks the size contract below.
///
/// An implementation must never truncate a body. A response larger
/// than [`MAX_DATASET_BYTES`] is an error, not a short `Response`. The
/// client checks the delivered body against the same limit, so a
/// truncating implementation is caught rather than believed.
pub trait Transport {
    /// Send a GET request and return the response.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, TransportError>;
}

/// The default transport, based on the `ureq` HTTP client.
#[cfg(feature = "http-ureq")]
pub struct UreqTransport {
    agent: ureq::Agent,
}

#[cfg(feature = "http-ureq")]
impl UreqTransport {
    /// Make a transport with a 30-second request timeout.
    ///
    /// The transport reads the standard proxy environment variables
    /// (`HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy`) and
    /// sends the requests through the proxy when one is set. It
    /// trusts the certificate store of the operating system, which
    /// honors `SSL_CERT_FILE`.
    pub fn new() -> Self {
        let mut builder = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30));
        if let Some(url) = proxy_url_from_env(|name| std::env::var(name).ok()) {
            if let Ok(proxy) = ureq::Proxy::new(&url) {
                builder = builder.proxy(proxy);
            }
        }
        UreqTransport {
            agent: builder.build(),
        }
    }

    /// Make a transport from a configured `ureq` agent.
    pub fn from_agent(agent: ureq::Agent) -> Self {
        UreqTransport { agent }
    }

    fn read_body(response: ureq::Response) -> Result<Vec<u8>, TransportError> {
        read_capped(response.into_reader(), MAX_DATASET_BYTES)
    }
}

/// Read a body of at most `limit` bytes, and refuse a larger one.
///
/// The function reads one byte beyond the limit. That extra byte is
/// the whole point: it distinguishes a body that fits from a body that
/// does not, without buffering the rest of an unbounded stream. A
/// plain `take(limit)` would return the first `limit` bytes of a huge
/// response and call it a success.
#[cfg(feature = "http-ureq")]
fn read_capped(reader: impl std::io::Read, limit: usize) -> Result<Vec<u8>, TransportError> {
    use std::io::Read;
    let mut body = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| TransportError(e.to_string()))?;
    if body.len() > limit {
        return Err(TransportError(format!(
            "the response body is larger than the limit of {limit} bytes"
        )));
    }
    Ok(body)
}

#[cfg(feature = "http-ureq")]
impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "http-ureq")]
impl Transport for UreqTransport {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, TransportError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        match request.call() {
            Ok(response) => {
                let status = response.status();
                Ok(Response {
                    status,
                    body: Self::read_body(response)?,
                })
            }
            // ureq reports HTTP error statuses as an Error value. The
            // Transport contract wants them as a normal Response.
            Err(ureq::Error::Status(status, response)) => Ok(Response {
                status,
                body: Self::read_body(response)?,
            }),
            Err(e) => Err(TransportError(e.to_string())),
        }
    }
}

/// Pick the proxy URL from environment-style variables.
///
/// The uppercase names win over the lowercase names, and HTTPS wins
/// over HTTP, which matches the common tool behavior.
#[cfg(feature = "http-ureq")]
fn proxy_url_from_env(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|name| get(name).filter(|value| !value.trim().is_empty()))
}

#[cfg(all(test, feature = "http-ureq"))]
mod tests {
    use super::*;

    #[test]
    fn proxy_selection_prefers_https_and_uppercase() {
        let pick = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            proxy_url_from_env(move |name| {
                vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
            })
        };

        assert_eq!(pick(&[]), None);
        assert_eq!(
            pick(&[("http_proxy", "http://p:1")]),
            Some("http://p:1".into())
        );
        assert_eq!(
            pick(&[("http_proxy", "http://p:1"), ("HTTPS_PROXY", "http://p:2")]),
            Some("http://p:2".into())
        );
        // Empty values do not count.
        assert_eq!(pick(&[("HTTPS_PROXY", " ")]), None);
    }

    #[test]
    fn a_body_within_the_limit_arrives_whole() {
        let body = read_capped(&b"0123456789"[..], 10).unwrap();
        assert_eq!(body, b"0123456789");
        assert!(read_capped(&b""[..], 0).unwrap().is_empty());
    }

    #[test]
    fn an_oversized_body_is_an_error_and_not_a_truncation() {
        let error = read_capped(&b"0123456789"[..], 4).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("larger than the limit of 4 bytes"),
            "{message}"
        );
        // One byte over the limit is already over the limit.
        assert!(read_capped(&b"12345"[..], 4).is_err());
    }
}
