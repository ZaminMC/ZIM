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

#[derive(Debug, Clone)]
pub struct RemoteConnect {
    /// `host:port` of the agent.
    pub addr: String,
    /// The agent's token, sent in `daemon.hello` auth.
    pub token: String,
    pub trust: Trust,
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
fn server_name(addr: &str) -> Result<ServerName<'static>, AgentError> {
    if let Ok(socket) = addr.parse::<SocketAddr>() {
        return Ok(ServerName::IpAddress(socket.ip().into()));
    }
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    ServerName::try_from(host.to_owned()).map_err(|e| {
        AgentError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            e.to_string(),
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
