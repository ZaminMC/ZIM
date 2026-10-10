// The address model — a source port of the shell's own address dialects
// (state/destinations.ts parseAddressInput) fused with Chromium's
// AutocompleteInput shape (components/omnibox/browser/autocomplete_input.h):
// the model classifies, the view renders, the command layer commits.

use serde::{Deserialize, Serialize};

use crate::shell::tabs::Destination;

/// One parsed address request — the closed classification of everything
/// an operator can type (the founder's dialect order preserved):
/// internal `zim://` URLs, join addresses (host:port, port-only,
/// bare host — including the `0` bind-all shorthand), free text queries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AddressRequest {
    Internal(Destination),
    Join { host: Option<String>, port: u16 },
    Query(String),
}

const INTERNAL_PAGES: &[&str] = &[
    "new",
    "servers",
    "settings",
    "jobs",
    "audit",
    "about",
    "feedback",
    "extensions",
    "downloads",
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
    if let Some(rest) = trimmed.strip_prefix("zim://") {
        let (page, arg) = match rest.split_once('/') {
            Some((page, arg)) => (page, arg.trim_end_matches('/')),
            None => (rest, ""),
        };
        if page == "server" && !arg.is_empty() {
            return AddressRequest::Internal(Destination::Server {
                server_id: arg.into(),
            });
        }
        if page == "console" && !arg.is_empty() {
            return AddressRequest::Internal(Destination::Console {
                server_id: arg.into(),
            });
        }
        // The join URL dialect (§7): Destination::url writes
        // `zim://join/{host}:{port}` and the omnibox RESTS on that string —
        // a re-commit must navigate, not fall to Missing (the user's own
        // "No page at zim://join/…" screenshot was exactly that fall).
        // Destination::parse owns the spelling, so the closed loop
        // url → parse → url stays one law in one place.
        if page == "join" && !arg.is_empty() {
            return AddressRequest::Internal(Destination::parse(trimmed));
        }
        if INTERNAL_PAGES.contains(&page) {
            return AddressRequest::Internal(Destination::parse(trimmed));
        }
        // An unknown internal page is a real destination request the
        // shell answers honestly (§58) — never silently a search.
        return AddressRequest::Internal(Destination::Missing {
            url: trimmed.to_owned(),
        });
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
        // A bare word could be a port ("25565") — only digits qualify,
        // and an out-of-range bare number is a query, never a join (the
        // TS twin's law in parseAddressInput). Any other host-charset
        // word joins by default port 0 — the Join page speaks the
        // verdict (§7: nothing is invented, the page answers honestly).
        if trimmed.len() <= 5 && trimmed.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(port) = trimmed.parse::<u16>() {
                if (1..=65535).contains(&port) {
                    return AddressRequest::Join { host: None, port };
                }
            }
            return AddressRequest::Query(trimmed.to_owned());
        }
        return AddressRequest::Join {
            host: Some(normalize_host(trimmed)),
            port: 0,
        };
    }

    AddressRequest::Query(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_urls_are_typed() {
        assert_eq!(
            classify("zim://servers/"),
            AddressRequest::Internal(Destination::Servers)
        );
        assert_eq!(
            classify("zim://server/abc"),
            AddressRequest::Internal(Destination::Server {
                server_id: "abc".into()
            })
        );
        assert_eq!(
            classify("zim://nonsense/"),
            AddressRequest::Internal(Destination::Missing {
                url: "zim://nonsense/".into()
            })
        );
    }

    #[test]
    fn join_dialects() {
        assert_eq!(
            classify("localhost:25565"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565
            }
        );
        // The founder's 0:25565 bind-all shorthand.
        assert_eq!(
            classify("0:25565"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565
            }
        );
        assert_eq!(
            classify("25565"),
            AddressRequest::Join {
                host: None,
                port: 25565
            }
        );
        // An out-of-range bare number is a query, never a join (the TS
        // twin's law in parseAddressInput).
        assert_eq!(classify("99999"), AddressRequest::Query("99999".into()));
        assert_eq!(
            classify("box.example.com"),
            AddressRequest::Join {
                host: Some("box.example.com".into()),
                port: 0
            }
        );
        // Out-of-range ports fall back to queries.
        assert_eq!(
            classify("host:99999"),
            AddressRequest::Query("host:99999".into())
        );
    }

    #[test]
    fn the_join_url_round_trips() {
        // The omnibox rests on a join tab's produced URL — a re-commit
        // must navigate to that tab, not fall to Missing (the user's
        // "No page at zim://join/…" screenshot).
        assert_eq!(
            classify("zim://join/localhost:25565"),
            AddressRequest::Internal(Destination::Join {
                host: Some("localhost".into()),
                port: 25565
            })
        );
        // The port-only spelling url() writes for a host-less join.
        assert_eq!(
            classify("zim://join/:25565"),
            AddressRequest::Internal(Destination::Join {
                host: None,
                port: 25565
            })
        );
        // The last colon is the port separator — an IPv6 host keeps its
        // own colons.
        assert_eq!(
            classify("zim://join/::1:25565"),
            AddressRequest::Internal(Destination::Join {
                host: Some("::1".into()),
                port: 25565
            })
        );
        // Not a produced URL → the honest Missing, never a guess.
        assert_eq!(
            classify("zim://join/nonsense"),
            AddressRequest::Internal(Destination::Missing {
                url: "zim://join/nonsense".into()
            })
        );
        assert_eq!(
            classify("zim://join/box:99999"),
            AddressRequest::Internal(Destination::Missing {
                url: "zim://join/box:99999".into()
            })
        );
    }

    #[test]
    fn free_text_is_a_query() {
        assert_eq!(
            classify("best modpack 2026"),
            AddressRequest::Query("best modpack 2026".into())
        );
    }
}
