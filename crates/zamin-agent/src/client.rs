//! The remote client side: connect to an agent over TLS, trust decided by
//! the pinned certificate fingerprint (default) or explicitly skipped.
//! This module is the shared client seam for every remote protocol client
//! — CLI, tests, and later the panel host (ADR-0011).

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use zamin_ipc::Connection;
use zamin_protocol::handshake::{ClientInfo, HelloParams, HelloResult};
use zamin_protocol::methods;

use crate::{ct_eq, AgentError};

/// How the client decides the agent is who it claims to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// SHA-256 hex of the agent certificate's DER — the fingerprint the
    /// agent prints at startup. The default and recommended mode.
    Fingerprint(String),
    /// Accept any server certificate. TLS still encrypts, but no server
    /// identity is proven; the token can then be stolen by a MITM. An
    /// explicit, documented escape hatch — never a default.
    InsecureSkipVerify,
}

#[derive(Clone)]
pub struct RemoteConnect {
    /// `host:port` of the agent.
    pub addr: String,
    /// The agent's token, sent in `daemon.hello` auth.
    pub token: String,
    pub trust: Trust,
}

// The Debug shape must never carry the token: config structs get printed
// into logs and error reports, and the token is the credential (ADR-0011).
impl std::fmt::Debug for RemoteConnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteConnect")
            .field("addr", &self.addr)
            .field("token", &"<redacted>")
            .field("trust", &self.trust)
            .finish()
    }
}

/// A TLS connection to the agent, framed and ready for `handshake`.
pub async fn connect(cfg: &RemoteConnect) -> Result<Connection, AgentError> {
    let tcp = TcpStream::connect(&cfg.addr).await?;
    let connector = TlsConnector::from(Arc::new(client_config(&cfg.trust)?));
    let name = server_name(&cfg.addr)?;
    let tls = connector.connect(name, tcp).await?;
    Ok(Connection::new(tls))
}

/// The mandatory first exchange over a [`connect`] connection: `daemon.hello`
/// carrying the token in `auth`. A typed protocol error (auth rejection,
/// daemon unreachable, version mismatch) is returned as an error.
pub async fn handshake(
    conn: &mut Connection,
    token: &str,
    client: ClientInfo,
) -> Result<HelloResult, AgentError> {
    let params = HelloParams {
        protocol: zamin_protocol::PROTOCOL_VERSION,
        auth: Some(token.to_owned()),
        client,
    };
    let request = zamin_protocol::envelope::Request::new(
        zamin_protocol::envelope::RequestId::Number(0),
        methods::DAEMON_HELLO,
        Some(serde_json::to_value(params)?),
    );
    conn.send(Bytes::from(serde_json::to_vec(&request)?))
        .await?;
    let frame = match conn.recv().await? {
        Some(frame) => frame,
        None => {
            return Err(AgentError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "agent closed the connection during the handshake",
            )))
        }
    };
    let response = serde_json::from_slice::<zamin_protocol::envelope::Response>(&frame)?;
    if let Some(error) = response.error {
        return Err(AgentError::Protocol(error));
    }
    let result = response.result.unwrap_or(serde_json::Value::Null);
    Ok(serde_json::from_value(result)?)
}

fn client_config(trust: &Trust) -> Result<ClientConfig, AgentError> {
    let builder = ClientConfig::builder();
    match trust {
        Trust::Fingerprint(hex) => {
            let verifier = FingerprintVerifier::new(hex)?;
            Ok(builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth())
        }
        Trust::InsecureSkipVerify => Ok(builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth()),
    }
}

/// SNI name for the connection. With fingerprint pinning the name carries
/// no trust — but TLS still requires a syntactically valid one.
///
/// The fallback slicing has one rule: never feed TLS a fragment of an
/// address we could not understand. A bare (unbracketed) IPv6 literal with
/// a port — `::1:7777` — used to slice into the nonsense SNI `::1`, whose
/// failure surfaced as a confusing TLS hostname error. Such an address is
/// refused here, with the fix spelled into the message.
fn server_name(addr: &str) -> Result<ServerName<'static>, AgentError> {
    if let Ok(socket) = addr.parse::<SocketAddr>() {
        return Ok(ServerName::IpAddress(socket.ip().into()));
    }
    // Bracketed literal `[::1]:7777` (or `[::1]`): the host is what the
    // brackets hold. (The full form already parsed as SocketAddr above.)
    if addr.starts_with('[') {
        if let Some(end) = addr.find(']') {
            let host = &addr[1..end];
            return ip_or_dns(host, addr);
        }
    }
    // `host:port` with a plain hostname (one colon, no v6 ambiguity).
    if let Some((host, port)) = addr.rsplit_once(':') {
        if !host.contains(':') && !host.contains(']') && port.bytes().all(|b| b.is_ascii_digit()) {
            return ip_or_dns(host, addr);
        }
    }
    // Everything else is not an address this client accepts — a bare
    // IPv6 literal ("::1") has no port to connect to, and an unbracketed
    // one with a port ("::1:7777") is ambiguous down to the hex digits
    // (1:7777 is a legal v6 suffix). Both are refused HERE, with the fix
    // spelled out, before TLS gets a chance to mispronounce a fragment
    // of them (the old slicing fed SNI a nonsense host).
    Err(AgentError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "invalid remote address {addr:?}: use host:port, or bracket an \
             IPv6 literal like [::1]:7777"
        ),
    )))
}

/// The host string as a TLS ServerName: an IP literal when it parses as
/// one, the DNS name otherwise.
fn ip_or_dns(host: &str, addr: &str) -> Result<ServerName<'static>, AgentError> {
    ServerName::try_from(host.to_owned()).map_err(|e| {
        AgentError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("remote address {addr:?}: {e}"),
        ))
    })
}

#[derive(Debug)]
struct FingerprintVerifier {
    pinned: Vec<u8>,
    provider: CryptoProvider,
}

impl FingerprintVerifier {
    fn new(fingerprint_hex: &str) -> Result<Self, AgentError> {
        let pinned = hex_decode(fingerprint_hex)?;
        Ok(FingerprintVerifier {
            pinned,
            provider: rustls::crypto::ring::default_provider(),
        })
    }
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let digest = ring::digest::digest(&ring::digest::SHA256, end_entity.as_ref());
        if ct_eq(digest.as_ref(), &self.pinned) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "server certificate does not match the pinned fingerprint".to_owned(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The documented escape hatch. Every check succeeds; TLS still encrypts.
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, AgentError> {
    let hex = hex.trim().replace(':', "");
    if hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AgentError::Fingerprint(hex.len()));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AgentError::Fingerprint(hex.len()))
}

#[cfg(test)]
mod tests {
    use super::server_name;

    #[test]
    fn the_documented_host_port_forms() {
        // The SocketAddr path: a full host:port parses whole.
        let name = server_name("127.0.0.1:7443").unwrap();
        assert_eq!(name.to_str(), "127.0.0.1");
        // A plain hostname keeps its name (the old slicing's one true case).
        let name = server_name("box.example:7443").unwrap();
        assert_eq!(name.to_str(), "box.example");
    }

    #[test]
    fn ipv6_literals_arrive_whole() {
        // Bracketed with a port — the one correct IPv6 spelling.
        let name = server_name("[2001:db8::1]:7443").unwrap();
        assert_eq!(name.to_str(), "2001:db8::1");
    }

    #[test]
    fn unbracketed_ipv6_is_refused_with_the_fix() {
        // The old code sliced "::1:7443" into the nonsense SNI "::1" and
        // let TLS produce the confusing error. Refused now, before any
        // wire — and the bare literal with no port is refused too (the
        // field is host:port; there is nothing to connect to).
        for bad in ["::1:7443", "::1", "fe80::1%eth0:7443"] {
            let error = server_name(bad).unwrap_err().to_string();
            assert!(error.contains("bracket"), "{bad:?}: {error}");
        }
        // An ambiguous mess is refused the same way.
        let error = server_name("not an address").unwrap_err().to_string();
        assert!(error.contains("invalid remote address"), "{error}");
    }
}
