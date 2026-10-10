//! The connection under both transports: a TCP socket, or TLS over one
//! (#555).
//!
//! **TLS is bought, not written** (ruling 7: a cryptographic primitive is
//! the canonical buy). `rustls` with `ring`'s primitives, and the Mozilla
//! root store as `webpki-roots` carries it, so a certificate is checked the
//! same way on every machine and nothing is read from the platform. An
//! `https` endpoint is spoken over TLS or not at all: a handshake that
//! fails is a connection that was never made, never a fallback to the
//! clear.
//!
//! The deadline and the stopper are the TCP socket's, as before. Timeouts
//! are set on the socket under the TLS session, and a stop shuts that
//! socket, which ends a read blocked inside the session as it ends a plain
//! one.

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Conn, Trust, connect};
#[cfg(target_arch = "wasm32")]
pub use unsupported::{Conn, Trust, connect};

/// The wasm32 build's: no TLS, since the exercise's wasm build has no
/// sockets to put it on. A plain connection as before; an `https` one
/// refused, never sent in the clear.
#[cfg(target_arch = "wasm32")]
mod unsupported {
    use std::net::{TcpStream, ToSocketAddrs as _};
    use std::time::{Duration, Instant};

    use super::super::transport::{Endpoint, TransportFailure};

    /// No roots: there is no TLS to trust them for.
    #[derive(Debug, Clone, Default)]
    pub struct Trust;

    impl Trust {
        /// # Errors
        ///
        /// Always: this build has no TLS.
        pub fn only(_root: &[u8]) -> Result<Self, String> {
            Err("this build has no TLS".to_owned())
        }
    }

    /// A plain connection.
    #[derive(Debug)]
    pub struct Conn(TcpStream);

    impl Conn {
        /// The socket: where the deadline is set and what a stop shuts.
        #[must_use]
        pub fn socket(&self) -> &TcpStream {
            &self.0
        }
    }

    impl std::io::Read for Conn {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl std::io::Write for Conn {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    /// A plain connection to `endpoint`; an `https` one is refused.
    ///
    /// # Errors
    ///
    /// Always for `https`; otherwise as the socket fails.
    pub fn connect(
        endpoint: &Endpoint,
        _trust: &Trust,
        deadline: Instant,
        started: Instant,
    ) -> Result<Conn, TransportFailure> {
        if endpoint.tls {
            return Err(TransportFailure::Connect(
                "this build has no TLS".to_owned(),
            ));
        }
        let address = (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|why| TransportFailure::Connect(why.to_string()))?
            .next()
            .ok_or_else(|| {
                TransportFailure::Connect("the host resolves to no address".to_owned())
            })?;
        let left = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        TcpStream::connect_timeout(&address, left)
            .map(Conn)
            .map_err(|_| TransportFailure::Timeout {
                after: started.elapsed(),
            })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::{self, Read, Write};
    use std::net::{TcpStream, ToSocketAddrs as _};
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};

    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

    use super::super::transport::{Endpoint, TransportFailure, is_timeout};

    /// Which certificate authorities a TLS connection trusts.
    #[derive(Clone)]
    pub struct Trust(Arc<ClientConfig>);

    impl std::fmt::Debug for Trust {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Trust(..)")
        }
    }

    impl Default for Trust {
        /// The Mozilla root store, once per process.
        fn default() -> Self {
            static WEBPKI: OnceLock<Arc<ClientConfig>> = OnceLock::new();
            Self(Arc::clone(WEBPKI.get_or_init(|| {
                let roots = RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                };
                Arc::new(config(roots))
            })))
        }
    }

    impl Trust {
        /// Trust exactly `root`, a DER certificate, and nothing else: a local
        /// server's own self-signed root, which is how the TLS path is tested
        /// without a call leaving the machine.
        ///
        /// # Errors
        ///
        /// When `root` is not a certificate the root store accepts.
        pub fn only(root: &[u8]) -> Result<Self, String> {
            let mut roots = RootCertStore::empty();
            roots
                .add(CertificateDer::from(root.to_vec()))
                .map_err(|why| format!("not a root certificate: {why}"))?;
            Ok(Self(Arc::new(config(roots))))
        }
    }

    /// A client configuration over `roots`, with `ring`'s primitives and the
    /// protocol versions `rustls` defaults to.
    fn config(roots: RootCertStore) -> ClientConfig {
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("ring supports rustls' default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth()
    }

    /// A connection, plain or under TLS.
    pub enum Conn {
        /// Bytes as written.
        Plain(TcpStream),
        /// A TLS session over the socket.
        Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
    }

    impl std::fmt::Debug for Conn {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Plain(_) => "Conn::Plain",
                Self::Tls(_) => "Conn::Tls",
            })
        }
    }

    impl Conn {
        /// The socket under it: where the deadline is set and what a stop shuts.
        #[must_use]
        pub fn socket(&self) -> &TcpStream {
            match self {
                Self::Plain(socket) => socket,
                Self::Tls(stream) => &stream.sock,
            }
        }
    }

    impl Read for Conn {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Plain(socket) => socket.read(buffer),
                Self::Tls(stream) => stream.read(buffer),
            }
        }
    }

    impl Write for Conn {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            match self {
                Self::Plain(socket) => socket.write(bytes),
                Self::Tls(stream) => stream.write(bytes),
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            match self {
                Self::Plain(socket) => socket.flush(),
                Self::Tls(stream) => stream.flush(),
            }
        }
    }

    /// A connection to `endpoint` before `deadline`: the socket, and for an
    /// `https` endpoint the TLS handshake completed under `trust`, the socket's
    /// timeouts bounding it. A timeout reports the time since `started`.
    ///
    /// # Errors
    ///
    /// [`TransportFailure::Connect`] when the host does not resolve, the
    /// socket does not open, or the handshake fails -- the certificate
    /// included; [`TransportFailure::Timeout`] when the budget runs out first.
    pub fn connect(
        endpoint: &Endpoint,
        trust: &Trust,
        deadline: Instant,
        started: Instant,
    ) -> Result<Conn, TransportFailure> {
        let timeout = || TransportFailure::Timeout {
            after: started.elapsed(),
        };
        let left = || -> Result<Duration, TransportFailure> {
            deadline
                .checked_duration_since(Instant::now())
                .filter(|left| !left.is_zero())
                .ok_or_else(timeout)
        };
        // Each address the host resolves to, in order, until one connects: a
        // host with an IPv6 and an IPv4 address (`localhost`, a hosted API)
        // answers on whichever it listens on.
        let addresses: Vec<_> = (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|why| TransportFailure::Connect(why.to_string()))?
            .collect();
        let mut failed = TransportFailure::Connect("the host resolves to no address".to_owned());
        let mut opened = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, left()?) {
                Ok(socket) => {
                    opened = Some(socket);
                    break;
                }
                Err(why) if is_timeout(&why) => return Err(timeout()),
                Err(why) => failed = TransportFailure::Connect(why.to_string()),
            }
        }
        let socket = opened.ok_or(failed)?;
        if !endpoint.tls {
            return Ok(Conn::Plain(socket));
        }
        let left = left()?;
        socket
            .set_read_timeout(Some(left))
            .and_then(|()| socket.set_write_timeout(Some(left)))
            .map_err(|why| TransportFailure::Connect(why.to_string()))?;
        let name = ServerName::try_from(endpoint.host.clone())
            .map_err(|why| TransportFailure::Connect(format!("not a TLS server name: {why}")))?;
        let session = ClientConnection::new(Arc::clone(&trust.0), name)
            .map_err(|why| TransportFailure::Connect(format!("TLS: {why}")))?;
        let mut stream = StreamOwned::new(session, socket);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock).map_err(|why| {
                if is_timeout(&why) {
                    timeout()
                } else {
                    TransportFailure::Connect(format!("TLS handshake: {why}"))
                }
            })?;
        }
        Ok(Conn::Tls(Box::new(stream)))
    }
    #[cfg(test)]
    mod tests {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use rustls::{ServerConfig, ServerConnection};

        use super::*;
        use crate::client::shape::{Limits, Message, RequestShape, Role, SamplerCard};
        use crate::client::stream::{Cancel, Ended, HttpStream, Piece, Streaming as _};

        const CAPTURED: &[u8] =
            include_bytes!("../../client/fixtures/llama-server-4df29be-stream.http");

        /// A certificate for `name`, self-signed, made now: its DER, and the
        /// server's configuration presenting it.
        fn certified(name: &str) -> (Vec<u8>, Arc<ServerConfig>) {
            let made =
                rcgen::generate_simple_self_signed(vec![name.to_owned()]).expect("a certificate");
            let der = made.cert.der().to_vec();
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.key_pair.serialize_der()));
            let config = ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(der.clone())], key)
            .expect("a server configuration");
            (der, Arc::new(config))
        }

        /// What one TLS connection to the server did: the request it read, if
        /// the handshake got that far.
        type Heard = mpsc::Receiver<Option<String>>;

        /// A local TLS server on loopback that answers one connection with
        /// `reply`, then, when `hold` is given, keeps the connection open until
        /// it fires. Its port, and what it heard.
        fn serving(
            config: Arc<ServerConfig>,
            reply: &'static [u8],
            hold: Option<mpsc::Receiver<()>>,
        ) -> (u16, Heard) {
            let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
            let port = TcpListener::local_addr(&listener)
                .expect("an address")
                .port();
            let (said, heard) = mpsc::channel();
            thread::spawn(move || {
                let Ok((socket, _)) = listener.accept() else {
                    return;
                };
                let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
                let session = ServerConnection::new(config).expect("a session");
                let mut stream = StreamOwned::new(session, socket);
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let read = loop {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break None,
                        Ok(n) => {
                            request.extend_from_slice(&buffer[..n]);
                            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                                let headers = String::from_utf8_lossy(&request[..end]).into_owned();
                                let length = headers
                                    .lines()
                                    .find_map(|line| {
                                        line.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .map(|n| n.trim().parse::<usize>().unwrap_or(0))
                                    })
                                    .unwrap_or(0);
                                if request.len() >= end + 4 + length {
                                    break Some(String::from_utf8_lossy(&request).into_owned());
                                }
                            }
                        }
                    }
                };
                let answered = read.is_some();
                let _ = said.send(read);
                if answered {
                    let _ = stream.write_all(reply);
                    let _ = stream.flush();
                    if let Some(hold) = hold {
                        let _ = hold.recv_timeout(Duration::from_secs(10));
                    }
                }
            });
            (port, heard)
        }

        fn endpoint(host: &str, port: u16) -> Endpoint {
            Endpoint::parse(&format!("https://{host}:{port}/v1/chat/completions"))
                .expect("an https endpoint")
        }

        fn shape() -> RequestShape {
            RequestShape {
                model: "a-model".to_owned(),
                messages: vec![Message::new(Role::User, "an ask")],
                sampler: SamplerCard::empty(),
                limits: Limits {
                    attempt: Duration::from_secs(5),
                    call: Duration::from_secs(5),
                    max_output_tokens: 16,
                    retries: 0,
                    context_window: None,
                    idle: None,
                },
                grammar: None,
                template_kwargs: std::collections::BTreeMap::new(),
                tools: Vec::new(),
            }
        }

        /// #555: a streamed call to an `https` endpoint is made over TLS to a
        /// server whose root is trusted, and reads the answer as a plain one
        /// reads it; the request the server decrypted is the one sent.
        #[test]
        fn a_streamed_call_over_tls_reads_the_answer_a_plain_one_reads() {
            let (root, config) = certified("localhost");
            let (port, heard) = serving(config, CAPTURED, None);
            let transport = HttpStream::new(endpoint("localhost", port))
                .with_trust(Trust::only(&root).expect("a root"));
            let mut text = String::new();
            let ended = transport
                .stream(
                    &shape(),
                    Instant::now() + Duration::from_secs(10),
                    &Cancel::new(),
                    &mut |piece| {
                        if let Piece::Text(piece) = piece {
                            text.push_str(piece);
                        }
                    },
                )
                .expect("an answer over TLS");
            assert!(matches!(ended, Ended::Finished { .. }), "{ended:?}");
            assert_eq!(text, "mittelсу polity polity polity polity");
            let request = heard.recv().expect("the server heard").expect("a request");
            assert!(
                request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")
                    && request.contains("\"model\":\"a-model\""),
                "{request}"
            );
            assert!(transport.describes().starts_with("https://localhost:"));
        }

        /// #555: a certificate no trusted root signed -- the public roots
        /// against a self-signed one -- and one for another name both refuse
        /// the connection before a byte of the request is sent.
        #[test]
        fn an_untrusted_or_misnamed_certificate_refuses_before_anything_is_sent() {
            let deadline = || Instant::now() + Duration::from_secs(10);
            let (_, config) = certified("localhost");
            let (port, heard) = serving(config, CAPTURED, None);
            let refused = connect(
                &endpoint("localhost", port),
                &Trust::default(),
                deadline(),
                Instant::now(),
            )
            .expect_err("a self-signed certificate under the public roots");
            assert!(
                matches!(&refused, TransportFailure::Connect(why) if why.starts_with("TLS handshake")),
                "{refused:?}"
            );
            assert_eq!(
                heard.recv().expect("the server ended"),
                None,
                "nothing was sent"
            );

            let (root, config) = certified("other.example");
            let (port, heard) = serving(config, CAPTURED, None);
            let refused = connect(
                &endpoint("localhost", port),
                &Trust::only(&root).expect("a root"),
                deadline(),
                Instant::now(),
            )
            .expect_err("a certificate for another name");
            assert!(
                matches!(&refused, TransportFailure::Connect(why) if why.starts_with("TLS handshake")),
                "{refused:?}"
            );
            assert_eq!(
                heard.recv().expect("the server ended"),
                None,
                "nothing was sent"
            );
        }

        /// #555: the unstreamed transport's GET goes over TLS too.
        #[test]
        fn a_get_over_tls_reads_the_reply() {
            let (root, config) = certified("localhost");
            let (port, heard) = serving(
                config,
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                None,
            );
            let reply = HttpStream::new(endpoint("localhost", port))
                .with_trust(Trust::only(&root).expect("a root"))
                .get("/v1/models", Instant::now() + Duration::from_secs(10))
                .expect("a reply over TLS");
            assert_eq!((reply.status, reply.body.as_str()), (200, "{}"));
            let request = heard.recv().expect("heard").expect("a request");
            assert!(
                request.starts_with("GET /v1/models HTTP/1.1\r\n"),
                "{request}"
            );
        }

        /// #555: a stop ends a read blocked inside the TLS session, as it ends
        /// a plain one: the stopper shuts the socket under it.
        #[test]
        fn a_stop_ends_a_read_blocked_inside_tls() {
            const OPENED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
            Connection: close\r\n\r\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n";
            let (root, config) = certified("localhost");
            let (release, hold) = mpsc::channel();
            let (port, _heard) = serving(config, OPENED, Some(hold));
            let transport = HttpStream::new(endpoint("localhost", port))
                .with_trust(Trust::only(&root).expect("a root"));
            let cancel = Cancel::new();
            let stopper = cancel.clone();
            let started = Instant::now();
            let ended = transport
                .stream(
                    &shape(),
                    Instant::now() + Duration::from_secs(10),
                    &cancel,
                    &mut |piece| {
                        if matches!(piece, Piece::Text(_)) {
                            let stopper = stopper.clone();
                            thread::spawn(move || {
                                thread::sleep(Duration::from_millis(100));
                                stopper.ask();
                            });
                        }
                    },
                )
                .expect("a stop is not a failure");
            let _ = release.send(());
            assert_eq!(ended, Ended::Cancelled);
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the stop, not the deadline, ended it: {:?}",
                started.elapsed()
            );
        }
    }
}
