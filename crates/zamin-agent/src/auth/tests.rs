// The tests (ADR-0008: platform-conditional assertions live in
// files named tests.rs — the seam guard's test exemption).

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
