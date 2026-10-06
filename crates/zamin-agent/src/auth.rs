//! The token credential and the first-frame auth gate (ADR-0011).
//!
//! The token is 32 random bytes, base64url, one line in a 0600 file. The
//! gate inspects the remote client's first frame: it must be `daemon.hello`
//! with `auth` present and matching, decided before any local daemon
//! connection is made. Verdicts are typed protocol errors carrying the
//! client's own request id; the original hello is forwarded unchanged —
//! the daemon ignores `auth` on local transports, by spec (§2).

use std::fs;
use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use zamin_protocol::envelope::{Request, RequestId, Response};
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::handshake::HelloParams;

pub const DEFAULT_TOKEN_FILE: &str = "token";

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("token file is empty")]
    Empty,
    #[error("system randomness unavailable")]
    Randomness,
}

/// Load the token from `path`, generating one on first use. The file is
/// one base64url line; 0600.
pub fn load_or_generate_token(path: &Path) -> Result<String, TokenError> {
    if let Ok(existing) = fs::read_to_string(path) {
        let token = existing.trim();
        if token.is_empty() {
            return Err(TokenError::Empty);
        }
        return Ok(token.to_owned());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let token = generate_token()?;
    write_private(path, format!("{token}\n"))?;
    Ok(token)
}

pub fn generate_token() -> Result<String, TokenError> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| TokenError::Randomness)?;
    Ok(B64.encode(bytes))
}

/// Fixed-length, constant-time comparison: both sides are digested first,
/// so length and content differences look identical to the compare.
fn token_matches(provided: &str, expected: &str) -> bool {
    let digest = |s: &str| ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
    crate::ct_eq(digest(provided).as_ref(), digest(expected).as_ref())
}

/// The gate verdict on a remote client's first frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Authenticated: the original frame is forwarded; the request id is
    /// kept for a typed reply if the local daemon cannot be reached.
    Forward { request_id: RequestId },
    /// Rejected: the response is written and the connection closed.
    Reject(Response),
}

pub fn gate(frame: &[u8], token: &str) -> Gate {
    let value: serde_json::Value = match serde_json::from_slice(frame) {
        Ok(value) => value,
        Err(_) => {
            return Gate::Reject(Response::err(
                RequestId::Null,
                ProtocolError::new(
                    ErrorCode::ProtocolInvalidRequest,
                    "The first frame could not be parsed; the remote transport expects a daemon.hello request.",
                ),
            ))
        }
    };
    // Deserializing into a Request requires id + method; a notification,
    // a response, or garbage lands here and answers with a Null id
    // (JSON-RPC 2.0 §4.1: never guess the id of an unreadable request).
    let request: Request = match serde_json::from_value(value) {
        Ok(request) => request,
        Err(_) => {
            return Gate::Reject(Response::err(
                RequestId::Null,
                ProtocolError::new(
                    ErrorCode::ProtocolInvalidRequest,
                    "The first frame must be a daemon.hello request.",
                ),
            ))
        }
    };
    if request.method != zamin_protocol::methods::DAEMON_HELLO {
        // Mirrors the daemon's own first-exchange rule (§2).
        return Gate::Reject(Response::err(
            request.id,
            ProtocolError::new(
                ErrorCode::ProtocolVersionUnsupported,
                "The first exchange must be daemon.hello.",
            ),
        ));
    }
    let params: HelloParams = match request.parse_params() {
        Ok(params) => params,
        Err(_) => {
            return Gate::Reject(Response::err(
                request.id,
                ProtocolError::new(
                    ErrorCode::ProtocolInvalidRequest,
                    "Unreadable hello params.",
                ),
            ))
        }
    };
    let auth = match params.auth.as_deref().filter(|a| !a.is_empty()) {
        Some(auth) => auth,
        None => {
            return Gate::Reject(Response::err(
                request.id,
                ProtocolError::new(
                    ErrorCode::AuthRequired,
                    "This transport requires the hello auth field (the agent's token).",
                )
                .with_remediation(&["pass the token from the agent's token file in hello.auth"]),
            ))
        }
    };
    if !token_matches(auth, token) {
        return Gate::Reject(Response::err(
            request.id,
            ProtocolError::new(ErrorCode::AuthRejected, "The hello token was rejected.")
                .with_remediation(&["compare the token against the agent's token file"]),
        ));
    }
    Gate::Forward {
        request_id: request.id,
    }
}

fn write_private(path: &Path, contents: String) -> Result<(), TokenError> {
    fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zamin_protocol::methods::DAEMON_HELLO;

    fn hello_frame(auth: Option<&str>, id: u64) -> Vec<u8> {
        let params = HelloParams {
            protocol: zamin_protocol::PROTOCOL_VERSION,
            auth: auth.map(str::to_owned),
            client: zamin_protocol::handshake::ClientInfo {
                name: "test-client".to_owned(),
                version: "0".to_owned(),
            },
        };
        serde_json::to_vec(&Request::new(
            RequestId::Number(id),
            DAEMON_HELLO,
            Some(serde_json::to_value(params).unwrap()),
        ))
        .unwrap()
    }

    fn reject_code(gate: Gate) -> Option<ErrorCode> {
        match gate {
            Gate::Forward { .. } => None,
            Gate::Reject(response) => response.error.map(|e| e.code),
        }
    }

    #[test]
    fn forwards_matching_token_with_request_id() {
        let gate = gate(&hello_frame(Some("secret"), 7), "secret");
        assert_eq!(
            gate,
            Gate::Forward {
                request_id: RequestId::Number(7)
            }
        );
    }

    #[test]
    fn missing_and_empty_auth_require_auth() {
        assert_eq!(
            reject_code(gate(&hello_frame(None, 1), "t")),
            Some(ErrorCode::AuthRequired)
        );
        assert_eq!(
            reject_code(gate(&hello_frame(Some(""), 1), "t")),
            Some(ErrorCode::AuthRequired)
        );
    }

    #[test]
    fn wrong_token_rejected() {
        assert_eq!(
            reject_code(gate(&hello_frame(Some("wrong"), 1), "secret")),
            Some(ErrorCode::AuthRejected)
        );
    }

    #[test]
    fn non_hello_first_frame_is_a_hard_error() {
        let other = serde_json::to_vec(&Request::new(
            RequestId::Number(2),
            zamin_protocol::methods::SERVER_LIST,
            None,
        ))
        .unwrap();
        assert_eq!(
            reject_code(gate(&other, "t")),
            Some(ErrorCode::ProtocolVersionUnsupported)
        );
        // Unparseable JSON and non-request frames answer with a Null id.
        for bad in [
            b"not json".as_slice(),
            b"{\"method\":\"daemon.hello\"}".as_slice(),
        ] {
            match gate(bad, "t") {
                Gate::Reject(response) => {
                    assert_eq!(response.id, RequestId::Null);
                    assert_eq!(
                        response.error.map(|e| e.code),
                        Some(ErrorCode::ProtocolInvalidRequest)
                    );
                }
                Gate::Forward { .. } => panic!("malformed frame forwarded"),
            }
        }
    }

    #[test]
    fn token_round_trips_and_is_private() {
        let dir = std::env::temp_dir().join(format!("zamin-agent-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(DEFAULT_TOKEN_FILE);
        let first = load_or_generate_token(&path).expect("generate");
        assert_eq!(first.len(), 43); // 32 bytes base64url, no padding
        assert_eq!(load_or_generate_token(&path).expect("reload"), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("token file")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
