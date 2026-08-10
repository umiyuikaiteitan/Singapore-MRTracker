//! The HTTP transport abstraction.
//!
//! The client sends requests through the [`Transport`] trait. The
//! default implementation, [`UreqTransport`], is behind the
//! `http-ureq` feature. Supply your own implementation to use a
//! different HTTP stack, to add retries, or to test without a network.

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
/// 500. It must return `Err` only when no HTTP exchange completed.
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
        use std::io::Read;
        // Cap the body size at 512 MiB to protect the process.
        const LIMIT: u64 = 512 * 1024 * 1024;
        let mut body = Vec::new();
        response
            .into_reader()
            .take(LIMIT)
            .read_to_end(&mut body)
            .map_err(|e| TransportError(e.to_string()))?;
        Ok(body)
    }
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
}
