//! The transport: bytes to a server and back, with every way it can fail
//! given a name.
//!
//! One rule governs the whole file. **A failure is never an empty answer.**
//! In prior work a timeout and a server that returned nothing were the same
//! empty string by the time anything read them, and the census counted the
//! first as the second for a whole campaign. Here the only way to get a
//! string out of this module is for a server to have sent one.

use std::error::Error;
use std::fmt;
use std::io::{self, Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

/// Somewhere to send a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The host, as written.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The path, beginning with `/`.
    pub path: String,
}

/// Why a URL is not an endpoint this client will use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    /// A scheme this client does not speak.
    ///
    /// `https` is refused rather than downgraded. This client speaks to
    /// servers on a machine somebody owns, and it has no TLS; quietly sending
    /// a prompt in the clear to a URL that asked for TLS is the silent
    /// fallback the isolation rules forbid one layer down, and it is no
    /// better here.
    Scheme(String),
    /// No host between the scheme and the path.
    NoHost,
    /// A port that is not a number, or is zero.
    Port(String),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheme(scheme) => write!(
                f,
                "`{scheme}` is not a scheme this client speaks; it has no TLS and will not \
                 silently send in the clear"
            ),
            Self::NoHost => f.write_str("the URL names no host"),
            Self::Port(text) => write!(f, "`{text}` is not a port"),
        }
    }
}

impl Error for EndpointError {}

impl Endpoint {
    /// The endpoint `url` names.
    ///
    /// # Errors
    ///
    /// Returns [`EndpointError`] for a scheme other than `http`, a URL with
    /// no host, or an unreadable port.
    pub fn parse(url: &str) -> Result<Self, EndpointError> {
        let rest = match url.split_once("://") {
            Some((scheme, rest)) => {
                if scheme != "http" {
                    return Err(EndpointError::Scheme(scheme.to_owned()));
                }
                rest
            }
            None => return Err(EndpointError::Scheme(String::new())),
        };

        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(EndpointError::NoHost);
        }

        let (host, port) = match authority.rsplit_once(':') {
            Some((host, text)) => {
                let port: u16 = text
                    .parse()
                    .map_err(|_| EndpointError::Port(text.to_owned()))?;
                if port == 0 {
                    return Err(EndpointError::Port(text.to_owned()));
                }
                (host, port)
            }
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err(EndpointError::NoHost);
        }

        Ok(Self {
            host: host.to_owned(),
            port,
            path: path.to_owned(),
        })
    }
}

/// What a server sent back at the HTTP layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    /// The status code.
    pub status: u16,
    /// The body, as bytes decoded lossily into text. A server that sends
    /// something that is not UTF-8 has still sent something, and a reader
    /// downstream will say so about the JSON rather than the socket.
    pub body: String,
}

/// Why a request did not produce a reply.
///
/// Every variant is a fact about the transport. None of them is an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportFailure {
    /// The connection was never made.
    Connect(String),
    /// The request could not be written.
    Write(String),
    /// The reply could not be read.
    Read(String),
    /// The server did not finish in time.
    Timeout {
        /// How long it had.
        after: Duration,
    },
    /// The server announced more body than it sent.
    Truncated {
        /// The length the header announced.
        announced: usize,
        /// How much arrived.
        received: usize,
    },
    /// The first line was not an HTTP status line.
    Malformed(String),
}

impl fmt::Display for TransportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(why) => write!(f, "could not connect: {why}"),
            Self::Write(why) => write!(f, "could not send the request: {why}"),
            Self::Read(why) => write!(f, "could not read the reply: {why}"),
            Self::Timeout { after } => {
                write!(f, "the server did not answer within {after:?}")
            }
            Self::Truncated {
                announced,
                received,
            } => write!(
                f,
                "the reply announced {announced} bytes of body and sent {received}"
            ),
            Self::Malformed(line) => {
                write!(f, "the reply's first line is not a status line: {line}")
            }
        }
    }
}

impl Error for TransportFailure {}

/// Something that can put a body in front of a server and get one back.
///
/// A trait so that a replay can stand where a socket stands. The seam
/// controller and the integration lane drive the same client; only this
/// changes.
pub trait Transport {
    /// Send `body` and return what came back, giving up at `deadline`.
    ///
    /// # Errors
    ///
    /// Returns [`TransportFailure`] when no reply arrived. An HTTP error
    /// status is a reply and is returned as one -- deciding what a 4xx means
    /// is the client's job, not the transport's.
    fn send(&self, body: &str, deadline: Instant) -> Result<HttpReply, TransportFailure>;

    /// Where this transport sends, for the record.
    fn describes(&self) -> String;
}

/// HTTP/1.1 over a socket.
///
/// One request per connection, `Connection: close`. Keep-alive is a
/// throughput concern and this client's throughput numbers are not yet
/// anybody's claim; a connection pool whose reuse policy nobody measured
/// would be a variable in every later measurement.
#[derive(Debug, Clone)]
pub struct Http {
    endpoint: Endpoint,
}

impl Http {
    /// A transport pointed at `endpoint`.
    #[must_use]
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }
}

/// How much reply this client will read before it stops.
///
/// A server that streams forever is a server that fills memory. The cap is
/// generous against any single completion and finite against a broken one.
const MAX_REPLY_BYTES: usize = 64 * 1024 * 1024;

/// Whether an I/O error is the deadline arriving.
fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

impl Transport for Http {
    fn send(&self, body: &str, deadline: Instant) -> Result<HttpReply, TransportFailure> {
        // How long the call actually took is what a timeout reports. The
        // budget it was given is already in the regimen; the elapsed time is
        // the measurement, and reporting the budget back as though it were
        // one is a number that always agrees with itself.
        let started = Instant::now();
        let remaining = |now: Instant| deadline.checked_duration_since(now);

        let Some(budget) = remaining(started) else {
            return Err(TransportFailure::Timeout {
                after: started.elapsed(),
            });
        };

        let address = (self.endpoint.host.as_str(), self.endpoint.port)
            .to_socket_addrs()
            .map_err(|why| TransportFailure::Connect(why.to_string()))?
            .next()
            .ok_or_else(|| {
                TransportFailure::Connect("the host resolves to no address".to_owned())
            })?;

        let mut stream = TcpStream::connect_timeout(&address, budget).map_err(|why| {
            if is_timeout(&why) {
                TransportFailure::Timeout {
                    after: started.elapsed(),
                }
            } else {
                TransportFailure::Connect(why.to_string())
            }
        })?;

        // Both directions carry the deadline. A socket with a read timeout and
        // no write timeout stalls forever against a server that never drains
        // its receive buffer, which is a hang that looks like a slow model.
        let Some(budget) = remaining(Instant::now()) else {
            return Err(TransportFailure::Timeout {
                after: started.elapsed(),
            });
        };
        stream
            .set_write_timeout(Some(budget))
            .and_then(|()| stream.set_read_timeout(Some(budget)))
            .map_err(|why| TransportFailure::Connect(why.to_string()))?;

        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\n\
             Accept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.endpoint.path,
            self.endpoint.host,
            self.endpoint.port,
            body.len(),
            body
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|why| {
                if is_timeout(&why) {
                    TransportFailure::Timeout {
                        after: started.elapsed(),
                    }
                } else {
                    TransportFailure::Write(why.to_string())
                }
            })?;

        let mut raw = Vec::new();
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            // Re-arm the read timeout on every pass. A single `set_read_timeout`
            // bounds each read, not the call: a server dribbling one byte per
            // interval would never trip it, and this loop would be the stall it
            // exists to catch.
            let Some(budget) = remaining(Instant::now()) else {
                return Err(TransportFailure::Timeout {
                    after: started.elapsed(),
                });
            };
            stream
                .set_read_timeout(Some(budget))
                .map_err(|why| TransportFailure::Read(why.to_string()))?;

            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    raw.extend_from_slice(&buffer[..count]);
                    if raw.len() > MAX_REPLY_BYTES {
                        return Err(TransportFailure::Read(format!(
                            "the reply passed {MAX_REPLY_BYTES} bytes and was not finished"
                        )));
                    }
                    if let Some(reply) = complete(&raw)? {
                        return Ok(reply);
                    }
                }
                Err(why) if why.kind() == io::ErrorKind::Interrupted => {}
                Err(why) if is_timeout(&why) => {
                    return Err(TransportFailure::Timeout {
                        after: started.elapsed(),
                    });
                }
                Err(why) => return Err(TransportFailure::Read(why.to_string())),
            }
        }

        finish(&raw)
    }

    fn describes(&self) -> String {
        format!(
            "http://{}:{}{}",
            self.endpoint.host, self.endpoint.port, self.endpoint.path
        )
    }
}

/// Where the headers end, if they have.
fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

/// The announced body length, if the headers announce one.
fn announced_length(headers: &str) -> Option<usize> {
    for line in headers.split("\r\n") {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

/// A reply, if `raw` already holds a whole one.
///
/// Returns `Ok(None)` while more is still expected, which is how a
/// `Content-Length` reply is answered before the server closes: waiting for
/// the close would add the server's linger to every call.
fn complete(raw: &[u8]) -> Result<Option<HttpReply>, TransportFailure> {
    let Some(end) = header_end(raw) else {
        return Ok(None);
    };
    let headers = String::from_utf8_lossy(&raw[..end]);
    let Some(announced) = announced_length(&headers) else {
        return Ok(None);
    };
    let body = &raw[end + 4..];
    if body.len() < announced {
        return Ok(None);
    }
    Ok(Some(HttpReply {
        status: status_of(&headers)?,
        body: String::from_utf8_lossy(&body[..announced]).into_owned(),
    }))
}

/// A reply from everything the server sent before closing.
fn finish(raw: &[u8]) -> Result<HttpReply, TransportFailure> {
    let Some(end) = header_end(raw) else {
        return Err(TransportFailure::Malformed(
            String::from_utf8_lossy(raw).chars().take(120).collect(),
        ));
    };
    let headers = String::from_utf8_lossy(&raw[..end]);
    let body = &raw[end + 4..];
    if let Some(announced) = announced_length(&headers)
        && body.len() < announced
    {
        return Err(TransportFailure::Truncated {
            announced,
            received: body.len(),
        });
    }
    Ok(HttpReply {
        status: status_of(&headers)?,
        body: String::from_utf8_lossy(body).into_owned(),
    })
}

/// The status code the first line carries.
fn status_of(headers: &str) -> Result<u16, TransportFailure> {
    let first = headers.split("\r\n").next().unwrap_or_default();
    let mut parts = first.split(' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err(TransportFailure::Malformed(first.to_owned()));
    }
    parts
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| TransportFailure::Malformed(first.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{Endpoint, EndpointError};

    #[test]
    fn an_https_url_is_refused_rather_than_quietly_sent_in_the_clear() {
        assert_eq!(
            Endpoint::parse("https://box.example:8080/v1/chat/completions"),
            Err(EndpointError::Scheme("https".to_owned())),
            "this client has no TLS; downgrading silently is the fallback the \
             isolation rules forbid one layer down"
        );
    }

    #[test]
    fn an_endpoint_is_read_or_refused_with_a_reason() {
        assert_eq!(
            Endpoint::parse("http://127.0.0.1:8080/v1/chat/completions"),
            Ok(Endpoint {
                host: "127.0.0.1".to_owned(),
                port: 8080,
                path: "/v1/chat/completions".to_owned(),
            })
        );
        assert_eq!(
            Endpoint::parse("http://localhost"),
            Ok(Endpoint {
                host: "localhost".to_owned(),
                port: 80,
                path: "/".to_owned(),
            }),
            "no port means the scheme's port, and no path means the root"
        );
        assert_eq!(Endpoint::parse("http:///v1"), Err(EndpointError::NoHost));
        assert_eq!(
            Endpoint::parse("http://host:0/v1"),
            Err(EndpointError::Port("0".to_owned())),
            "port zero asks the kernel to choose, which is not a server to call"
        );
        assert_eq!(
            Endpoint::parse("http://host:eighty/v1"),
            Err(EndpointError::Port("eighty".to_owned()))
        );
        assert_eq!(
            Endpoint::parse("127.0.0.1:8080"),
            Err(EndpointError::Scheme(String::new())),
            "a bare authority names no scheme, and guessing one is guessing"
        );
    }
}
