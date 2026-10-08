// The address model — a source port of the shell's own address dialects
// (state/destinations.ts parseAddressInput) fused with Chromium's
// AutocompleteInput shape (components/omnibox/browser/autocomplete_input.h):
// the model classifies, the view renders, the command layer commits.

use serde::{Deserialize, Serialize};

use crate::shell::tabs::Destination;

/// One parsed address request — the closed classification of everything
/// an operator can type (the founder's dialect order preserved):
/// internal `zaminpanel://` URLs, join addresses (host:port, port-only,
/// bare host — including the `0` bind-all shorthand), free text queries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AddressRequest {
    Internal(Destination),
    Join { host: Option<String>, port: u16 },
    Query(String),
}

const INTERNAL_PAGES: &[&str] = &[
    "new", "servers", "settings", "jobs", "audit", "about", "feedback",
    "extensions", "downloads",
];

fn normalize_host(host: &str) -> String {
    let unbracketed = host.trim().trim_start_matches('[').trim_end_matches(']');
    // The bind-all and loopback spellings reach the same local server —
    // including the founder's own `0:25565` shorthand.
    match unbracketed {
        "0" | "0.0.0.0" | "127.0.0.1" | "::" => "localhost".into(),
        other => other.to_owned(),
    }
}

fn is_host_charset(text: &str) -> bool {
    // The JOIN dialect's host charset: [a-zA-Z0-9._-]+ or [bracketed IPv6].
    if text.starts_with('[') && text.ends_with(']') {
        return text.len() > 2;
    }
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// parseAddressInput — ported rule for rule.
pub fn classify(text: &str) -> AddressRequest {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("zaminpanel://") {
        let (page, arg) = match rest.split_once('/') {
            Some((page, arg)) => (page, arg.trim_end_matches('/')),
            None => (rest, ""),
        };
        if page == "server" && !arg.is_empty() {
            return AddressRequest::Internal(Destination::Server { server_id: arg.into() });
        }
        if page == "console" && !arg.is_empty() {
            return AddressRequest::Internal(Destination::Console { server_id: arg.into() });
        }
        if INTERNAL_PAGES.contains(&page) {
            return AddressRequest::Internal(Destination::parse(trimmed));
        }
        // An unknown internal page is a real destination request the
        // shell answers honestly (§58) — never silently a search.
        return AddressRequest::Internal(Destination::Missing { url: trimmed.to_owned() });
    }

    // host:port | port-only | bare host.
    if let Some((head, tail)) = trimmed.rsplit_once(':') {
        if !tail.is_empty() && tail.len() <= 5 && tail.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(port) = tail.parse::<u16>() {
                if (1..=65535).contains(&port) {
                    let host = if head.is_empty() {
                        None
                    } else if is_host_charset(head) {
                        Some(normalize_host(head))
                    } else {
                        None
                    };
                    return AddressRequest::Join { host, port };
                }
            }
        }
    } else if is_host_charset(trimmed) {
        return AddressRequest::Join { host: Some(normalize_host(trimmed)), port: 0 };
    }

    AddressRequest::Query(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_urls_are_typed() {
        assert_eq!(
            classify("zaminpanel://servers/"),
            AddressRequest::Internal(Destination::Servers)
        );
        assert_eq!(
            classify("zaminpanel://server/abc"),
            AddressRequest::Internal(Destination::Server { server_id: "abc".into() })
        );
        assert_eq!(
            classify("zaminpanel://nonsense/"),
            AddressRequest::Internal(Destination::Missing { url: "zaminpanel://nonsense/".into() })
        );
    }

    #[test]
    fn join_dialects() {
        assert_eq!(
            classify("localhost:25565"),
            AddressRequest::Join { host: Some("localhost".into()), port: 25565 }
        );
        // The founder's 0:25565 bind-all shorthand.
        assert_eq!(
            classify("0:25565"),
            AddressRequest::Join { host: Some("localhost".into()), port: 25565 }
        );
        assert_eq!(classify("25565"), AddressRequest::Join { host: None, port: 25565 });
        assert_eq!(
            classify("box.example.com"),
            AddressRequest::Join { host: Some("box.example.com".into()), port: 0 }
        );
        // Out-of-range ports fall back to queries.
        assert_eq!(classify("host:99999"), AddressRequest::Query("host:99999".into()));
    }

    #[test]
    fn free_text_is_a_query() {
        assert_eq!(
            classify("best modpack 2026"),
            AddressRequest::Query("best modpack 2026".into())
        );
    }
}
