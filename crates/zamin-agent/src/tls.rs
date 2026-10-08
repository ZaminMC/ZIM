//! TLS material for the remote transport (ADR-0011): a self-signed
//! certificate generated once per agent install and reused after. Remote
//! clients pin the certificate's SHA-256 fingerprint — there is no CA and
//! no OS trust store; the fingerprint is the server identity. The token
//! stays the actual credential; TLS protects it in transit.

use std::fs;
use std::net::IpAddr;
use std::path::Path;

use rcgen::{CertificateParams, DnType, KeyPair, SanType};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;

pub const CERT_FILE: &str = "agent-cert.pem";
pub const KEY_FILE: &str = "agent-key.pem";

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("certificate generation: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("rustls: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("pem decode: {0}")]
    Pem(#[from] rustls::pki_types::pem::Error),
}

pub struct TlsMaterial {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

impl TlsMaterial {
    /// Load the material from `dir`, generating a fresh self-signed pair on
    /// first use. Idempotent: every later load yields the same certificate
    /// and therefore the same fingerprint clients pin.
    pub fn load_or_generate(dir: &Path) -> Result<TlsMaterial, TlsError> {
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);
        if let (Ok(cert), Ok(key)) = (
            CertificateDer::from_pem_file(&cert_path),
            PrivateKeyDer::from_pem_file(&key_path),
        ) {
            return Ok(TlsMaterial { cert, key });
        }
        fs::create_dir_all(dir)?;
        let (cert, key) = generate_self_signed()?;
        write_private(&cert_path, pem_encode("CERTIFICATE", cert.as_ref()))?;
        write_private(&key_path, pem_encode("PRIVATE KEY", key.secret_der()))?;
        Ok(TlsMaterial { cert, key })
    }

    /// SHA-256 of the certificate DER as lowercase hex — the string remote
    /// clients pin as the server identity.
    pub fn fingerprint_hex(&self) -> String {
        hex(&ring::digest::digest(
            &ring::digest::SHA256,
            self.cert.as_ref(),
        ))
    }

    /// The certificate PEM — what a remote operator copies out to pin.
    pub fn cert_pem(&self) -> String {
        pem_encode("CERTIFICATE", self.cert.as_ref())
    }

    /// The server-side TLS configuration: this one certificate, no client
    /// certificate requirement (the hello token is the client credential).
    pub fn server_config(&self) -> Result<ServerConfig, TlsError> {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![self.cert.clone()], self.key.clone_key())
            .map_err(TlsError::from)
    }
}

/// Grouped hex for humans: `ab:cd:…` in logs and printouts.
pub fn fingerprint_display(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap_or_default().to_owned())
        .collect::<Vec<_>>()
        .join(":")
}

fn generate_self_signed() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), TlsError> {
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::new(vec!["localhost".to_owned()])?;
    params
        .distinguished_name
        .push(DnType::CommonName, "zaminagent");
    if let Ok(ip) = "127.0.0.1".parse::<IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }
    let cert = params.self_signed(&key)?;
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der());
    Ok((cert.der().to_owned(), PrivateKeyDer::Pkcs8(key_der)))
}

/// Credential-adjacent files are 0600; the directory is created by the
/// caller (`load_or_generate`).
fn write_private(path: &Path, contents: String) -> Result<(), TlsError> {
    // The private-by-default write is the platform seam's call (ADR-0008).
    zamin_core::platform::private_file::write_private_file(path, &contents)?;
    Ok(())
}

/// Standard PEM (RFC 7468): 64-column base64 between BEGIN/END lines — the
/// exact shape `PemObject::from_pem_file` parses back on reload. Written
/// here because the resolved pki-types ships no encoder and one more
/// dependency for ten lines is a bad trade.
fn pem_encode(label: &str, der: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let body = b64
        .as_bytes()
        .chunks(64)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
}

fn hex(digest: &ring::digest::Digest) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
