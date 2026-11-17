//! Connect over TLS: each device has its own self-signed certificate (made once and kept), and the others know it by
//! its FINGERPRINT (SHA-256 of the certificate), pinned when they pair. No certificate authority: on a home network
//! there is none, and a pinned fingerprint is stronger than one anyway (only that one key is accepted, ever).
//!
//! TLS 1.3 only. Everything that carries a secret (pairing, the token, commands, the files of a pass) goes through
//! here; only `GET /info` (who answers at this address, to be found) stays in the clear.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, ServerConfig, ServerConnection, SignatureScheme, StreamOwned};
use sha2::{Digest, Sha256};

use super::resilient::Resilient;
use crate::Location;
use crate::storage::Storage;

/// The first byte of a TLS connection (a handshake record): what tells it apart from plain HTTP on the same port.
pub const HANDSHAKE_BYTE: u8 = 0x16;
/// The name a client says it connects to. Not checked by anybody: what is checked is the fingerprint.
const SERVER_NAME: &str = "soundify-connect";

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The SHA-256 of a certificate, in lower-case hex: how a device is known.
pub fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert).iter().map(|b| format!("{b:02x}")).collect()
}

/// Is it a fingerprint as [`fingerprint`] writes them?
pub fn is_fingerprint(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// This device's certificate and key.
pub struct TlsIdentity {
    cert: CertificateDer<'static>,
    fingerprint: String,
    server: Arc<ServerConfig>,
}

impl TlsIdentity {
    /// The one kept in `cert` / `key`, or a new one written there. A broken pair is replaced (the others then see another
    /// fingerprint and must pair again: better than not starting).
    pub fn load_or_create(storage: &dyn Storage, cert: &Location, key: &Location) -> io::Result<TlsIdentity> {
        if let (Ok(cert_der), Ok(key_der)) = (storage.read(cert), storage.read(key))
            && let Ok(identity) = Self::from_der(cert_der, key_der)
        {
            return Ok(identity);
        }
        let (cert_der, key_der) = generate()?;
        // The key first: a certificate without its key is useless, a key without its certificate is made again.
        storage.write(key, &key_der)?;
        storage.write(cert, &cert_der)?;
        Self::from_der(cert_der, key_der)
    }

    /// One that lives only in memory (tests).
    pub fn ephemeral() -> TlsIdentity {
        let (cert, key) = generate().expect("a certificate can be made");
        Self::from_der(cert, key).expect("it is valid")
    }

    fn from_der(cert: Vec<u8>, key: Vec<u8>) -> io::Result<TlsIdentity> {
        let cert = CertificateDer::from(cert);
        let key = PrivatePkcs8KeyDer::from(key);
        let server = ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], PrivateKeyDer::Pkcs8(key))
            .map_err(io::Error::other)?;
        let fingerprint = fingerprint(&cert);
        Ok(TlsIdentity { cert, fingerprint, server: Arc::new(server) })
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn cert_der(&self) -> &[u8] {
        &self.cert
    }


    /// Takes a connection whose first byte was [`HANDSHAKE_BYTE`] and finishes the handshake.
    pub fn accept(&self, stream: TcpStream) -> io::Result<Conn> {
        let connection = ServerConnection::new(self.server.clone()).map_err(io::Error::other)?;
        let mut tls = StreamOwned::new(connection, Resilient::new(stream));
        // Now, not on the first read: a client that never finishes it is let go of within the socket's timeouts.
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock)?;
        }
        Ok(Conn::Server(Box::new(tls)))
    }
}

/// A new self-signed certificate (ECDSA P-256), with its key as PKCS#8.
fn generate() -> io::Result<(Vec<u8>, Vec<u8>)> {
    let key = rcgen::KeyPair::generate().map_err(io::Error::other)?;
    let params = rcgen::CertificateParams::new(vec![SERVER_NAME.to_string()]).map_err(io::Error::other)?;
    let cert = params.self_signed(&key).map_err(io::Error::other)?;
    Ok((cert.der().to_vec(), key.serialize_der()))
}

/// What a client accepts from the device it connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// Only this certificate (a device paired before, or the one a pass came from).
    Pinned(String),
    /// Whatever it shows, to learn it: the first contact while pairing (the PIN, through SPAKE2, then proves it is
    /// the device the user reads the PIN off) or a device paired before there was TLS (pinned from then on).
    Learn,
}

/// Accepts the certificate the client was told to (and checks that the other side holds its key, as TLS always does).
#[derive(Debug)]
struct PinnedVerifier {
    expect: Expect,
    seen: Mutex<Option<String>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        let seen = fingerprint(end_entity);
        if let Expect::Pinned(wanted) = &self.expect
            && !same(wanted.as_bytes(), seen.as_bytes())
        {
            return Err(rustls::Error::General(UNTRUSTED.into()));
        }
        *self.seen.lock().unwrap() = Some(seen);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        // TLS 1.2 is never offered.
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// The text of the error when the certificate is not the pinned one (a device that was reinstalled, or somebody in between).
pub const UNTRUSTED: &str = "the device's certificate is not the one it was paired with";

/// Is this error the one of a certificate that is not the pinned one?
pub fn is_untrusted(error: &io::Error) -> bool {
    error.to_string().contains(UNTRUSTED)
}

/// Opens a TLS connection to `addr` and finishes the handshake; gives back the connection and the fingerprint it saw.
pub fn connect(addr: SocketAddr, expect: &Expect, connect_wait: Duration, io_wait: Duration) -> io::Result<(Conn, String)> {
    let provider = provider();
    let verifier = Arc::new(PinnedVerifier { expect: expect.clone(), seen: Mutex::new(None), algorithms: provider.signature_verification_algorithms });
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    let name = ServerName::try_from(SERVER_NAME).map_err(io::Error::other)?;
    let connection = ClientConnection::new(Arc::new(config), name).map_err(io::Error::other)?;
    let stream = TcpStream::connect_timeout(&addr, connect_wait)?;
    stream.set_read_timeout(Some(io_wait))?;
    stream.set_write_timeout(Some(io_wait))?;
    stream.set_nodelay(true)?;
    let mut tls = StreamOwned::new(connection, Resilient::new(stream));
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    let seen = verifier.seen.lock().unwrap().clone().ok_or_else(|| io::Error::other("no certificate"))?;
    Ok((Conn::Client(Box::new(tls)), seen))
}

/// A connection of Connect: in the clear (only `/info`) or TLS, on either side.
pub enum Conn {
    Plain(Resilient<TcpStream>),
    Server(Box<StreamOwned<ServerConnection, Resilient<TcpStream>>>),
    Client(Box<StreamOwned<ClientConnection, Resilient<TcpStream>>>),
}

impl Conn {
    /// The socket underneath (to change its timeouts).
    pub fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s.inner(),
            Conn::Server(s) => s.sock.inner(),
            Conn::Client(s) => s.sock.inner(),
        }
    }

    pub fn is_tls(&self) -> bool {
        !matches!(self, Conn::Plain(_))
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.tcp().peer_addr()
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Server(s) => s.read(buf),
            Conn::Client(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Server(s) => s.write(buf),
            Conn::Client(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Server(s) => s.flush(),
            Conn::Client(s) => s.flush(),
        }
    }
}

/// Compared in a time that does not depend on where they differ.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use std::net::{Ipv4Addr, TcpListener};

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let storage = MemoryStorage::default();
        storage.create_dir_all(&Location::path("/c")).unwrap();
        let (cert, key) = (Location::path("/c/tls-cert.der"), Location::path("/c/tls-key.der"));
        let first = TlsIdentity::load_or_create(&storage, &cert, &key).unwrap();
        let again = TlsIdentity::load_or_create(&storage, &cert, &key).unwrap();
        assert!(is_fingerprint(first.fingerprint()));
        assert_eq!(first.fingerprint(), again.fingerprint());
        // A broken file is replaced rather than leaving Connect without TLS.
        storage.write(&key, b"broken").unwrap();
        let replaced = TlsIdentity::load_or_create(&storage, &cert, &key).unwrap();
        assert_ne!(replaced.fingerprint(), first.fingerprint());
    }

    fn serve_once(identity: Arc<TlsIdentity>) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            if let Ok(mut conn) = identity.accept(stream) {
                let mut buf = [0u8; 5];
                if conn.read_exact(&mut buf).is_ok() {
                    let _ = conn.write_all(&buf);
                    let _ = conn.flush();
                }
            }
        });
        addr
    }

    #[test]
    fn a_pinned_certificate_connects_and_another_one_is_refused() {
        let identity = Arc::new(TlsIdentity::ephemeral());
        let wait = Duration::from_secs(5);
        let (mut conn, seen) = connect(serve_once(identity.clone()), &Expect::Pinned(identity.fingerprint().into()), wait, wait).unwrap();
        assert_eq!(seen, identity.fingerprint());
        conn.write_all(b"hello").unwrap();
        conn.flush().unwrap();
        let mut back = [0u8; 5];
        conn.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"hello");

        // Somebody else's certificate (a device in between, or another device at that address).
        let impostor = Arc::new(TlsIdentity::ephemeral());
        let refused = connect(serve_once(impostor), &Expect::Pinned(identity.fingerprint().into()), wait, wait).err().unwrap();
        assert!(is_untrusted(&refused), "{refused}");
    }

    #[test]
    fn learning_gives_back_the_fingerprint_it_saw() {
        let identity = Arc::new(TlsIdentity::ephemeral());
        let wait = Duration::from_secs(5);
        let (_conn, seen) = connect(serve_once(identity.clone()), &Expect::Learn, wait, wait).unwrap();
        assert_eq!(seen, identity.fingerprint());
    }
}
