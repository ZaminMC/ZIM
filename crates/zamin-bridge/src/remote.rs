//! The panel host's remote decision (ADR-0011): local socket or TLS relay.
//!
//! The desktop panel's connection profiles pick which daemon the host
//! bridges to. This module turns the profile's fields into either `None`
//! (the per-user local transport, the only mode before Phase 8) or a
//! [`RemoteConnect`] for the agent's TLS seam. Pure and fallible, so the
//! malformed-profile cases get honest errors here — before any network —
//! and the Tauri host stays a thin shell over it.

use zamin_agent::client::{RemoteConnect, Trust};

/// Resolve the host's connection target from a connection profile's fields.
///
/// - No address (or a blank one) is the local profile: `Ok(None)`.
/// - A remote address demands a token (the hello credential); a missing
///   token is an error, not a silent local fallback.
/// - A fingerprint — the hex the agent prints at startup, colons allowed —
///   is normalized (whitespace/colons stripped, lowercased) and pinned.
/// - No fingerprint is the documented skip-verify escape hatch; the caller
///   is expected to have warned (the connections modal does) and the host
///   logs the warning again when it connects.
pub fn resolve_remote(
    addr: Option<String>,
    token: Option<String>,
    fingerprint: Option<String>,
) -> Result<Option<RemoteConnect>, String> {
    let Some(addr) = addr.map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()) else {
        return Ok(None);
    };

    let token = token
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            "the remote profile has no token: paste the contents of the agent's \
             token file (<data>/agent/token)"
                .to_owned()
        })?;

    let trust = match fingerprint
        .map(|f| f.replace(':', "").trim().to_lowercase())
        .filter(|f| !f.is_empty())
    {
        Some(hex) => {
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!(
                    "the fingerprint must be the 64 hex digits the agent printed \
                     at startup (got {hex_len} characters)",
                    hex_len = hex.len()
                ));
            }
            Trust::Fingerprint(hex)
        }
        None => Trust::InsecureSkipVerify,
    };

    Ok(Some(RemoteConnect { addr, token, trust }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 64 hex digits: the SHA-256 fingerprint shape the agent prints.
    fn hex() -> String {
        "ab".repeat(32)
    }

    #[test]
    fn no_address_is_the_local_profile() {
        assert!(resolve_remote(None, None, None).unwrap().is_none());
        assert!(resolve_remote(Some("  ".into()), Some("tok".into()), None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_remote_address_demands_a_token() {
        let error = resolve_remote(Some("box:7443".into()), None, Some(hex()))
            .expect_err("missing token must be an error");
        assert!(error.contains("no token"), "honest message: {error}");

        let error = resolve_remote(Some("box:7443".into()), Some("  ".into()), Some(hex()))
            .expect_err("a blank token is no token");
        assert!(error.contains("no token"), "honest message: {error}");
    }

    #[test]
    fn a_pinned_profile_normalizes_the_fingerprint() {
        // The agent prints colon-separated hex; the operator pastes it.
        let pinned = hex();
        let colons: String = pinned
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        let cfg = resolve_remote(
            Some(" box.example:7443 ".into()),
            Some(" tok ".into()),
            Some(colons.to_uppercase()),
        )
        .unwrap()
        .expect("remote");
        assert_eq!(cfg.addr, "box.example:7443");
        assert_eq!(cfg.token, "tok");
        assert_eq!(cfg.trust, Trust::Fingerprint(pinned));
    }

    #[test]
    fn a_malformed_fingerprint_fails_before_any_network() {
        let error = resolve_remote(
            Some("box:7443".into()),
            Some("tok".into()),
            Some("xyz".into()),
        )
        .expect_err("not hex");
        assert!(error.contains("64 hex"), "honest message: {error}");

        let error = resolve_remote(
            Some("box:7443".into()),
            Some("tok".into()),
            Some("ab".repeat(31)),
        )
        .expect_err("too short");
        assert!(error.contains("62 characters"), "honest count: {error}");
    }

    #[test]
    fn an_empty_fingerprint_is_the_discouraged_escape_hatch() {
        let cfg = resolve_remote(Some("box:7443".into()), Some("tok".into()), None)
            .unwrap()
            .expect("remote");
        assert_eq!(cfg.trust, Trust::InsecureSkipVerify);
    }

    #[test]
    fn the_debug_shape_never_carries_the_token() {
        // Config structs end up in logs and error reports; the token is
        // the credential (ADR-0011).
        let cfg = resolve_remote(Some("box:7443".into()), Some("sekrit".into()), Some(hex()))
            .unwrap()
            .expect("remote");
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("sekrit"), "token leaked: {printed}");
        assert!(
            printed.contains("<redacted>"),
            "honest redaction: {printed}"
        );
    }
}
