//! Address types and TCP/TLS constructors for Electrum connections.
//!
//! [`ServerAddr`] keeps the hostname unresolved so TLS SNI and certificate validation can use
//! the original name. Use [`blocking::connect_tcp`] / [`blocking::connect_ssl`] or the tokio
//! equivalents, or the client wrappers [`crate::BlockingClient::connect_tcp`] /
//! [`crate::AsyncClient::connect_tcp`].

use std::fmt;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;
#[cfg(feature = "ssl")]
use std::sync::Arc;

#[cfg(feature = "ssl")]
mod tls;

/// The host portion of a [`ServerAddr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// A domain name, e.g. `electrum.example.com`.
    Domain(String),
    /// An IP literal.
    Ip(IpAddr),
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Domain(domain) => f.write_str(domain),
            Host::Ip(IpAddr::V4(ip)) => write!(f, "{}", ip),
            Host::Ip(IpAddr::V6(ip)) => write!(f, "[{}]", ip),
        }
    }
}

/// An Electrum server address: a [`Host`] and a port, without a connection scheme.
///
/// Parses from `"host:port"`. IPv6 literals must be bracketed (`"[::1]:50001"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddr {
    host: Host,
    port: u16,
}

impl ServerAddr {
    /// Creates a new `ServerAddr` from a [`Host`] and port.
    pub fn new(host: Host, port: u16) -> Self {
        Self { host, port }
    }

    /// The host portion of this address.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The port of this address.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl fmt::Display for ServerAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl FromStr for ServerAddr {
    type Err = ParseServerAddrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ParseServerAddrError(s.to_string());

        if s.contains("://") {
            return Err(invalid());
        }

        // Bracketed IP literal: "[<ip>]:<port>".
        if let Some(rest) = s.strip_prefix('[') {
            let (ip_str, port_str) = rest.split_once("]:").ok_or_else(invalid)?;
            return Ok(Self {
                host: Host::Ip(ip_str.parse().map_err(|_| invalid())?),
                port: port_str.parse().map_err(|_| invalid())?,
            });
        }

        let (host_str, port_str) = s.rsplit_once(':').ok_or_else(invalid)?;
        if host_str.is_empty() || host_str.contains(':') {
            // Empty host, or an unbracketed IPv6 literal.
            return Err(invalid());
        }
        Ok(Self {
            host: match host_str.parse::<IpAddr>() {
                Ok(ip) => Host::Ip(ip),
                Err(_) => Host::Domain(host_str.to_string()),
            },
            port: port_str.parse().map_err(|_| invalid())?,
        })
    }
}

impl ToSocketAddrs for ServerAddr {
    type Iter = std::vec::IntoIter<SocketAddr>;

    /// Resolves this address via **local DNS**.
    ///
    /// Do not use this for `.onion` hosts — they are not resolvable via local DNS.
    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        match &self.host {
            Host::Ip(ip) => Ok(vec![SocketAddr::new(*ip, self.port)].into_iter()),
            Host::Domain(domain) => (domain.as_str(), self.port).to_socket_addrs(),
        }
    }
}

/// An error parsing a [`ServerAddr`] from a string.
///
/// The payload is the full input string that failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseServerAddrError(pub String);

impl fmt::Display for ParseServerAddrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid server address '{}'", self.0)
    }
}

impl std::error::Error for ParseServerAddrError {}

/// Error establishing an Electrum connection.
#[non_exhaustive]
#[derive(Debug)]
pub enum ConnectError {
    /// Transport or socket failure, including DNS, TCP, timeout, and handshake EOF/reset.
    Io(std::io::Error),
    /// TLS configuration, protocol, or certificate-validation failure.
    #[cfg(feature = "ssl")]
    Tls(TlsError),
}

#[cfg(feature = "ssl")]
impl ConnectError {
    /// Returns the certificate-validation error, if present.
    pub fn certificate_error(&self) -> Option<&rustls::CertificateError> {
        match self {
            ConnectError::Tls(e) => e.certificate_error(),
            _ => None,
        }
    }

    /// Classifies embedded rustls errors as TLS failures and preserves other I/O errors.
    fn from_handshake_io(err: std::io::Error) -> Self {
        match err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        {
            Some(tls) => ConnectError::Tls(TlsError::Rustls(tls.clone())),
            None => ConnectError::Io(err),
        }
    }
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::Io(e) => write!(f, "connection I/O error: {e}"),
            #[cfg(feature = "ssl")]
            ConnectError::Tls(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConnectError::Io(e) => Some(e),
            #[cfg(feature = "ssl")]
            ConnectError::Tls(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ConnectError {
    fn from(e: std::io::Error) -> Self {
        ConnectError::Io(e)
    }
}

#[cfg(feature = "ssl")]
impl From<TlsError> for ConnectError {
    fn from(e: TlsError) -> Self {
        ConnectError::Tls(e)
    }
}

/// Error in the TLS layer of a connection: configuration, protocol, or
/// certificate-validation failures. Transport failures are [`ConnectError::Io`].
#[cfg(feature = "ssl")]
#[non_exhaustive]
#[derive(Debug)]
pub enum TlsError {
    /// TLS protocol or certificate-validation failure.
    Rustls(rustls::Error),
    /// `validate_domain` is true and the host is [`Host::Ip`].
    MissingDomain,
    /// Host string is not a valid TLS [`rustls::pki_types::ServerName`].
    InvalidServerName(String),
}

#[cfg(feature = "ssl")]
impl TlsError {
    /// Server certificate rejected by the local verifier.
    pub fn certificate_error(&self) -> Option<&rustls::CertificateError> {
        match self {
            Self::Rustls(rustls::Error::InvalidCertificate(error)) => Some(error),
            _ => None,
        }
    }
}

#[cfg(feature = "ssl")]
impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TlsError::MissingDomain => {
                write!(f, "TLS certificate validation requires a domain name")
            }
            TlsError::InvalidServerName(name) => {
                write!(f, "invalid TLS server name '{name}'")
            }
            TlsError::Rustls(e) => write!(f, "TLS error: {e}"),
        }
    }
}

#[cfg(feature = "ssl")]
impl std::error::Error for TlsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TlsError::Rustls(e) => Some(e),
            TlsError::MissingDomain | TlsError::InvalidServerName(_) => None,
        }
    }
}

#[cfg(feature = "ssl")]
impl From<rustls::Error> for TlsError {
    fn from(e: rustls::Error) -> Self {
        TlsError::Rustls(e)
    }
}

#[cfg(feature = "ssl")]
impl From<rustls::Error> for ConnectError {
    fn from(e: rustls::Error) -> Self {
        ConnectError::Tls(TlsError::Rustls(e))
    }
}

/// Process-default provider if the application installed one; otherwise aws-lc-rs.
#[cfg(feature = "ssl")]
fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
}

#[cfg(feature = "ssl")]
fn client_config(validate_domain: bool) -> Result<Arc<rustls::ClientConfig>, rustls::Error> {
    let provider = crypto_provider();
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()?;
    let config = if validate_domain {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                (*provider).clone(),
            )))
            .with_no_client_auth()
    };
    Ok(Arc::new(config))
}

#[cfg(feature = "ssl")]
fn server_name(
    addr: &ServerAddr,
    validate_domain: bool,
) -> Result<rustls::pki_types::ServerName<'static>, TlsError> {
    match addr.host() {
        Host::Domain(domain) => rustls::pki_types::ServerName::try_from(domain.clone())
            .map_err(|_| TlsError::InvalidServerName(domain.clone())),
        Host::Ip(ip) => {
            if validate_domain {
                Err(TlsError::MissingDomain)
            } else {
                Ok(rustls::pki_types::ServerName::from(*ip))
            }
        }
    }
}

#[cfg(feature = "ssl")]
mod danger {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::DigitallySignedStruct;

    #[derive(Debug)]
    pub struct NoCertificateVerification(CryptoProvider);

    impl NoCertificateVerification {
        pub fn new(provider: CryptoProvider) -> Self {
            Self(provider)
        }
    }

    impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

/// Blocking (std I/O) transport constructors.
pub mod blocking {
    use std::io;
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;
    #[cfg(feature = "ssl")]
    use std::time::Instant;

    use super::ServerAddr;

    /// Connects to `addr` over plaintext TCP using blocking I/O.
    ///
    /// `timeout` bounds TCP connection, not DNS. No read/write timeout
    /// on the returned stream.
    pub fn connect_tcp(addr: &ServerAddr, timeout: Option<Duration>) -> io::Result<TcpStream> {
        let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
        match timeout {
            Some(timeout) => connect_with_total_timeout(&addrs, timeout),
            None => TcpStream::connect(addrs.as_slice()),
        }
    }

    #[cfg(feature = "ssl")]
    pub use super::tls::{TlsReadHalf, TlsStream, TlsWriteHalf};

    /// Connects to `addr` over TLS using blocking I/O.
    ///
    /// `timeout` is a single deadline covering TCP connect and the TLS handshake.
    /// The stream can be used as a single `Read + Write`, or split with
    /// [`TlsStream::into_split`] so a reader and writer can run on separate threads.
    #[cfg(feature = "ssl")]
    pub fn connect_ssl(
        addr: &ServerAddr,
        validate_domain: bool,
        timeout: Option<Duration>,
    ) -> Result<TlsStream, super::ConnectError> {
        let deadline = timeout.map(|timeout| Instant::now() + timeout);
        let server_name = super::server_name(addr, validate_domain)?;
        let mut tcp = connect_tcp(addr, timeout).map_err(super::ConnectError::Io)?;

        let mut conn =
            rustls::ClientConnection::new(super::client_config(validate_domain)?, server_name)?;
        conn.complete_io(&mut HandshakeIo {
            tcp: &mut tcp,
            deadline,
        })
        .map_err(super::ConnectError::from_handshake_io)?;

        tcp.set_read_timeout(None)
            .map_err(super::ConnectError::Io)?;
        tcp.set_write_timeout(None)
            .map_err(super::ConnectError::Io)?;
        TlsStream::new(conn, tcp).map_err(super::ConnectError::Io)
    }

    /// Remaining time until `deadline`, or `TimedOut` if it has passed.
    #[cfg(feature = "ssl")]
    fn remaining_timeout(deadline: Instant) -> io::Result<Duration> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "TLS handshake timed out",
            ))
        } else {
            Ok(remaining)
        }
    }

    /// `Read + Write` adapter for rustls `complete_io`, enforcing a single
    /// deadline for the whole handshake.
    ///
    /// Without it, socket timeouts would apply per operation, so the handshake
    /// could take several times longer than the intended deadline.
    #[cfg(feature = "ssl")]
    struct HandshakeIo<'a> {
        tcp: &'a mut TcpStream,
        deadline: Option<Instant>,
    }

    /// Make socket timeouts readable as `TimedOut` rather than rustls's
    /// bare `WouldBlock` propagation.
    #[cfg(feature = "ssl")]
    fn map_timeout<T>(deadline: Option<Instant>, result: io::Result<T>) -> io::Result<T> {
        match result {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && deadline.is_some() => {
                Err(io::Error::new(io::ErrorKind::TimedOut, e))
            }
            other => other,
        }
    }

    #[cfg(feature = "ssl")]
    impl HandshakeIo<'_> {
        fn apply_deadline(&mut self) -> io::Result<()> {
            let Some(deadline) = self.deadline else {
                return Ok(());
            };
            let remaining = remaining_timeout(deadline)?;
            self.tcp.set_read_timeout(Some(remaining))?;
            self.tcp.set_write_timeout(Some(remaining))?;
            Ok(())
        }
    }

    #[cfg(feature = "ssl")]
    impl io::Read for HandshakeIo<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.apply_deadline()?;
            let result = self.tcp.read(buf);
            map_timeout(self.deadline, result)
        }
    }

    #[cfg(feature = "ssl")]
    impl io::Write for HandshakeIo<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.apply_deadline()?;
            let result = self.tcp.write(buf);
            map_timeout(self.deadline, result)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.apply_deadline()?;
            let result = self.tcp.flush();
            map_timeout(self.deadline, result)
        }
    }

    /// Tries each addr, splitting `timeout` across attempts.
    fn connect_with_total_timeout(
        addrs: &[SocketAddr],
        mut timeout: Duration,
    ) -> io::Result<TcpStream> {
        // Use the same algorithm as curl: 1/2 of the timeout on the first address, 1/4 on the
        // second one, etc. https://curl.se/mail/lib-2014-11/0164.html
        let mut last_err = None;
        for (index, addr) in addrs.iter().enumerate() {
            if index < addrs.len() - 1 {
                timeout = timeout.div_f32(2.0);
            }
            match TcpStream::connect_timeout(addr, timeout) {
                Ok(stream) => return Ok(stream),
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "could not resolve to any addresses",
            )
        }))
    }
}

/// Tokio-based async transport constructors.
#[cfg(feature = "tokio")]
pub mod tokio {
    use std::io;
    use std::net::SocketAddr;
    use std::time::Duration;

    use tokio::net::TcpStream;

    use super::{Host, ServerAddr};

    /// Connects to `addr` over plaintext TCP using the Tokio runtime.
    ///
    /// `timeout` bounds DNS and TCP connection.
    pub async fn connect_tcp(
        addr: &ServerAddr,
        timeout: Option<Duration>,
    ) -> io::Result<TcpStream> {
        let connect_fut = async {
            match addr.host() {
                Host::Domain(domain) => TcpStream::connect((domain.as_str(), addr.port())).await,
                Host::Ip(ip) => TcpStream::connect(SocketAddr::new(*ip, addr.port())).await,
            }
        };
        match timeout {
            Some(timeout) => match tokio::time::timeout(timeout, connect_fut).await {
                Ok(res) => res,
                Err(_elapsed) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connection to '{}' timed out", addr),
                )),
            },
            None => connect_fut.await,
        }
    }

    /// Connects to `addr` over TLS using the Tokio runtime.
    ///
    /// `timeout` bounds DNS, TCP connect, and the TLS handshake.
    /// `validate_domain` requires a [`super::Host::Domain`].
    #[cfg(feature = "ssl")]
    pub async fn connect_ssl(
        addr: &ServerAddr,
        validate_domain: bool,
        timeout: Option<Duration>,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>, super::ConnectError> {
        let server_name = super::server_name(addr, validate_domain)?;
        let connector = tokio_rustls::TlsConnector::from(super::client_config(validate_domain)?);
        let handshake = async {
            let tcp = connect_tcp(addr, None)
                .await
                .map_err(super::ConnectError::Io)?;
            connector
                .connect(server_name, tcp)
                .await
                .map_err(super::ConnectError::from_handshake_io)
        };
        match timeout {
            Some(timeout) => match tokio::time::timeout(timeout, handshake).await {
                Ok(res) => res,
                Err(_elapsed) => Err(super::ConnectError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connection to '{}' timed out", addr),
                ))),
            },
            None => handshake.await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn addr(s: &str) -> ServerAddr {
        s.parse().unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    #[test]
    fn server_addr_parse() {
        let a = addr("127.0.0.1:50001");
        assert_eq!(a.host(), &Host::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert_eq!(a.port(), 50001);
        assert_eq!(a.to_string(), "127.0.0.1:50001");

        let a = addr("localhost:50001");
        assert_eq!(a.host(), &Host::Domain("localhost".into()));
        assert_eq!(a.port(), 50001);

        let a = addr("[::1]:50001");
        assert_eq!(a.host(), &Host::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert_eq!(a.port(), 50001);
        assert_eq!(a.to_string(), "[::1]:50001");

        for bad in [
            "tcp://127.0.0.1:50001",
            "ssl://host:50002",
            "::1:50001",
            ":50001",
            "host",
            "host:",
            "host:99999",
            "[::1]50001",
            "[example.com]:50001",
        ] {
            assert!(bad.parse::<ServerAddr>().is_err(), "{bad}");
        }
    }
}
