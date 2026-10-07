//! §47, verbatim in spirit: marketplace/API credentials must never be
//! stored in plain project configuration. This module is the whole
//! credential story the daemon implements:
//!
//! - The publish config (a project file) has NO credential field.
//! - The publication record and receipts have NO credential field.
//! - The audit log is never handed a credential.
//! - A provider that needs a credential reads it, at execute time, from
//!   the environment channel: `ZAMIN_PUBLISH_CREDENTIAL_<PROVIDER_ID>`.
//!
//! The OS's secure storage (Windows Credential Manager, libsecret, the
//! macOS keychain) is the desktop host's reserved room: it should PUT
//! the secret into that environment channel for the daemon it spawns
//! (or wire its own `PublishProvider`s). The daemon itself never
//! persists what it receives — there is nowhere honest to put it, so
//! there is nowhere it goes.

/// The env-var prefix every publish credential rides under.
pub const CREDENTIAL_ENV_PREFIX: &str = "ZAMIN_PUBLISH_CREDENTIAL_";

/// The env var name for a provider id: `builtbybit` (or `built-by-bit`)
/// becomes `ZAMIN_PUBLISH_CREDENTIAL_BUILT_BY_BIT` / `..._BUILTBYBIT`.
pub fn credential_env_var(provider_id: &str) -> String {
    format!(
        "{CREDENTIAL_ENV_PREFIX}{}",
        provider_id
            .to_ascii_uppercase()
            .replace(['-', '.', ' '], "_")
    )
}

/// Resolve a credential from the process environment. Empty or
/// whitespace-only values count as absent — a placeholder is not a
/// credential.
pub fn resolve_credential(provider_id: &str) -> Option<String> {
    resolve_credential_in(provider_id, &|name| std::env::var(name).ok())
}

/// The injectable core of `resolve_credential`, so tests never race the
/// real process environment.
pub fn resolve_credential_in(
    provider_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let name = credential_env_var(provider_id);
    env(&name).filter(|v| !v.trim().is_empty())
}

/// Redact a secret for any surface that must not carry it whole. The
/// shape (a few leading characters plus the length) is enough for an
/// operator to recognize WHICH value it was without the value itself.
pub fn redact(secret: &str) -> String {
    let n = secret.chars().count();
    if n == 0 {
        return "(empty)".to_owned();
    }
    let head: String = secret.chars().take(4).collect();
    format!("{head}…[redacted] (len {n})")
}
