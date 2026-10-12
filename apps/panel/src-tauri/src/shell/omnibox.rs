// The address model — a source port of the shell's own address dialects
// (state/destinations.ts parseAddressInput) fused with Chromium's
// AutocompleteInput shape (components/omnibox/browser/autocomplete_input.h):
// the model classifies, the view renders, the command layer commits.

use serde::{Deserialize, Serialize};

use crate::shell::tabs::{Destination, ServerPage, Strip, TabId};

/// One parsed address request — the closed classification of everything
/// an operator can type (the founder's dialect order preserved):
/// internal `zim://` URLs, join addresses (host:port, port-only,
/// bare host — including the `0` bind-all shorthand), free text queries.
/// The join dialect carries the server-page path (§8–§12): the authority
/// half resolves the registry's port, the path half names the page under
/// it — `0.0.0.0:25552/console`, `0.0.0.0:25552/files/eula.txt`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AddressRequest {
    Internal(Destination),
    Join {
        host: Option<String>,
        port: u16,
        #[serde(default)]
        page: Option<ServerPage>,
        #[serde(default)]
        file_path: Option<String>,
    },
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
                page: None,
                file_path: None,
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

    // The IPv6 literal law (Chrome's fixup): the brackets are input
    // sugar — "::1" and "[::1]" both join. Runs before the dialect so a
    // bare address is never mistaken for host:port words; the dialect's
    // own spellings never parse as addresses ("box:25565" has non-hex
    // chars, "0:25565"'s port exceeds a 16-bit group), so nothing else
    // moves. normalize_host rides the raw spelling ("::" is the
    // founder's bind-all → localhost). An IPv6 literal never carries a
    // path (no `/` survives the bracket law), so the whole line is it.
    let unbracketed = trimmed
        .strip_prefix('[')
        .map(|rest| rest.strip_suffix(']').unwrap_or(rest))
        .unwrap_or(trimmed);
    if unbracketed.contains(':') && unbracketed.parse::<std::net::Ipv6Addr>().is_ok() {
        return AddressRequest::Join {
            host: Some(normalize_host(trimmed)),
            port: 0,
            page: None,
            file_path: None,
        };
    }

    // The path dialect (§8–§12): split the authority half off FIRST — a
    // join address never contains a `/` — then run the join dialect on
    // the authority. `0.0.0.0:25552/console` is a join WITH a page,
    // never a query; a path whose first word names no page is an honest
    // miss, never a silent dashboard.
    let (authority, path_segments): (&str, Vec<&str>) = match trimmed.split_once('/') {
        Some((head, tail)) => (
            head,
            tail.split('/').filter(|seg| !seg.is_empty()).collect(),
        ),
        None => (trimmed, Vec::new()),
    };
    // The path half, pending the authority's own verdict: only a
    // join-shaped authority earns the page semantics — a search phrase
    // with a slash ("config world/foo") stays a query, exactly the TS
    // twin's law.
    let typed_page = parse_server_path(&path_segments);
    let path_names_no_page = !path_segments.is_empty() && typed_page.is_none();

    // host:port | port-only | bare host.
    if let Some((head, tail)) = authority.rsplit_once(':') {
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
                    if path_names_no_page {
                        // A path was typed on a join address and it names
                        // no page: the line reads as a URL and lands as
                        // an honest miss, never a silent dashboard.
                        return AddressRequest::Internal(Destination::Missing {
                            url: trimmed.to_owned(),
                        });
                    }
                    let (page, file_path) = typed_page
                        .map(|(page, file)| (Some(page), file))
                        .unwrap_or((None, None));
                    return AddressRequest::Join {
                        host,
                        port,
                        page,
                        file_path,
                    };
                }
            }
        }
    } else if is_host_charset(authority) {
        // A bare word could be a port ("25565") — only digits qualify,
        // and an out-of-range bare number is a query, never a join (the
        // TS twin's law in parseAddressInput).
        if authority.len() <= 5 && authority.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(port) = authority.parse::<u16>() {
                if (1..=65535).contains(&port) {
                    if path_names_no_page {
                        return AddressRequest::Internal(Destination::Missing {
                            url: trimmed.to_owned(),
                        });
                    }
                    let (page, file_path) = typed_page
                        .map(|(page, file)| (Some(page), file))
                        .unwrap_or((None, None));
                    return AddressRequest::Join {
                        host: None,
                        port,
                        page,
                        file_path,
                    };
                }
            }
            return AddressRequest::Query(trimmed.to_owned());
        }
        // The bare-word law (Chrome's fixup posture): a host-looking
        // word navigates — the dot is the host's own signal — and
        // "localhost" is navigable by its name (Chrome's exception, and
        // the most common join of all). A plain word is a discovery
        // search: "paper" must never join a host called "paper". The
        // word carries no port — 0, and the Join page speaks the
        // verdict (§7: nothing is invented, the page answers honestly).
        // A PATH on a portless host names no server page (the page model
        // resolves by port): the line reads as a URL and lands as an
        // honest miss, never a join that silently drops the words.
        if authority.contains('.') || authority.eq_ignore_ascii_case("localhost") {
            if !path_segments.is_empty() {
                return AddressRequest::Internal(Destination::Missing {
                    url: trimmed.to_owned(),
                });
            }
            return AddressRequest::Join {
                host: Some(normalize_host(authority)),
                port: 0,
                page: None,
                file_path: None,
            };
        }
    }

    AddressRequest::Query(trimmed.to_owned())
}

/// The typed path half, resolved against the page vocabulary: `["console"]`
/// answers `(Console, None)`; `["files", "eula.txt"]` answers
/// `(Files, Some("eula.txt"))`. An empty path answers `None` (the root
/// join); a first segment outside the vocabulary answers `None` AND the
/// caller answers the honest miss (the two-None distinction lives in the
/// caller — parse never invents a page).
fn parse_server_path(segments: &[&str]) -> Option<(ServerPage, Option<String>)> {
    let first = segments.first()?;
    let page = ServerPage::from_word(first)?;
    if page == ServerPage::Files && segments.len() > 1 {
        return Some((ServerPage::Files, Some(segments[1..].join("/"))));
    }
    Some((page, None))
}

// --- the commit's landing (AutocompleteEditModel::OpenURL) -------------------
//
// The edit model classifies; the disposition decides WHERE the request
// lands. Upstream's OpenURL carries the displacement: kCurrentTab (the
// plain Enter) navigates the tab the field sits in; Alt-Enter's
// kNEW_FOREGROUND_TAB opens a fresh foreground tab that takes the
// activation and inherits the previous active as its opener (the
// TabStripModel::AppendWebContents law Strip::insert already speaks).

/// What a committed request did to the model — the command layer's
/// receipt. The query case carries the tab that must hear the discovery
/// query (under Alt-Enter that is the NEW tab, not the one the hand was
/// in; the text waits on the tab itself, hello delivers it at boot).
#[derive(Clone, Debug, PartialEq)]
pub enum Landing {
    Destination {
        tab: TabId,
        destination: Destination,
    },
    Query {
        tab: TabId,
        text: String,
    },
}

/// The commit's model law, shared by every entry dialect (typed Enter,
/// Alt-Enter, paste-and-go): classify outside, land here. `new_tab`
/// false demands an active tab (None = the caller's refusal); true
/// never refuses — a fresh foreground tab exists to take the request.
///
/// The join dialect resolves against the FRAME's fleet projection
/// (§8–§12's address model): a typed `host:port` the registry owns
/// lands that SERVER's dashboard, and a typed path names the page under
/// it (`host:port/console`, `host:port/files/eula.txt`). An unmanaged
/// address keeps the Join page — its own verdict (§7) still speaks.
pub fn land(
    strip: &mut Strip,
    request: AddressRequest,
    new_tab: bool,
    fleet: &[FleetServer],
    host_hint: &str,
) -> Option<Landing> {
    match request {
        AddressRequest::Internal(destination) => {
            if new_tab {
                let tab = strip.append(destination.clone(), true);
                Some(Landing::Destination { tab, destination })
            } else {
                let active = strip.active?;
                strip.navigate(active, destination.clone());
                Some(Landing::Destination {
                    tab: active,
                    destination,
                })
            }
        }
        AddressRequest::Join {
            host,
            port,
            page,
            file_path,
        } => {
            let destination = match resolve_fleet_join(&host, port, fleet, host_hint) {
                Some(server_id) => Destination::Server {
                    server_id,
                    page,
                    file_path,
                },
                None => Destination::Join { host, port },
            };
            if new_tab {
                let tab = strip.append(destination.clone(), true);
                Some(Landing::Destination { tab, destination })
            } else {
                let active = strip.active?;
                strip.navigate(active, destination.clone());
                Some(Landing::Destination {
                    tab: active,
                    destination,
                })
            }
        }
        AddressRequest::Query(text) => {
            // The search dialect rides the new-tab page in BOTH
            // dispositions — plain Enter turns the current tab into the
            // discovery page, Alt-Enter opens a fresh one (the query is
            // never dropped either way).
            let tab = if new_tab {
                strip.append(Destination::New, true)
            } else {
                let active = strip.active?;
                strip.navigate(active, Destination::New);
                active
            };
            let tab_obj = strip.tabs.iter_mut().find(|t| t.id == tab)?;
            tab_obj.pending_query = Some(text.clone());
            Some(Landing::Query { tab, text })
        }
    }
}

/// resolveJoin (state/destinations.ts) — the port+bind law, in the Rust
/// twin's own words. The port is the match; the host must agree with the
/// server's bind: an all- or unset-bind server answers this machine's
/// aliases, a specifically bound server answers its own address. Several
/// servers agreeing (a future remote fleet) prefer the exact join
/// spelling, else say nothing — nothing is invented (§16).
fn resolve_fleet_join(
    typed_host: &Option<String>,
    port: u16,
    fleet: &[FleetServer],
    host_hint: &str,
) -> Option<String> {
    let bind_says = |server: &FleetServer| -> bool {
        let bind = server.bind.as_deref().map(str::trim).unwrap_or("");
        if bind.is_empty() || bind == "0.0.0.0" || bind == "::" || bind == "*" {
            // All interfaces: this machine's own aliases reach it.
            return match typed_host {
                None => true,
                Some(host) => is_local_alias(host, host_hint),
            };
        }
        match typed_host {
            None => true,
            Some(host) => bind.eq_ignore_ascii_case(host),
        }
    };
    let matches: Vec<&FleetServer> = fleet
        .iter()
        .filter(|s| s.port == Some(port) && bind_says(s))
        .collect();
    if matches.is_empty() {
        return None;
    }
    if matches.len() == 1 {
        return Some(matches[0].server_id.clone());
    }
    // Several agree: prefer the exact join spelling (the typed host
    // arrives normalized — the aliases folded in classify — so the
    // comparison is one law), else say nothing yet.
    let expected = match typed_host {
        Some(host) => format!("{host}:{port}"),
        None => format!("{host_hint}:{port}"),
    };
    matches
        .iter()
        .find(|s| server_join_spelling(s, host_hint) == expected)
        .map(|s| s.server_id.clone())
}

/// A fleet server's join spelling — `host:port`, the address the bar
/// shows for it (joinAddress's law: the hint is the window's reach).
fn server_join_spelling(server: &FleetServer, host: &str) -> String {
    format!(
        "{host}:{}",
        server.port.map(|p| p.to_string()).unwrap_or_default()
    )
}

/// The local aliases a typed host may use for "this machine" — §7's join
/// dialect, including the founder's own `0` shorthand for bind-all.
fn is_local_alias(host: &str, hint: &str) -> bool {
    let normalized = host.to_lowercase();
    normalized == "localhost"
        || normalized == "0"
        || normalized == "0.0.0.0"
        || normalized == "127.0.0.1"
        || normalized == "::"
        || normalized == "::1"
        || normalized == hint.to_lowercase()
}

// --- the popup's matches (OmniboxPopupViewViews over our dialects) ----------
//
// The edit model classifies and lands; the POPUP proposes. Upstream's
// AutocompleteResult orders matches by relevance; ours orders by how
// directly the text names the thing — the exact join first, the fleet's
// own names next, the internal pages, the discovery search last. Every
// row's `text` re-enters the SAME commit door a typed Enter uses (one
// classifier, one landing law); the popup only proposes, the host still
// decides.

/// The frame's fleet projection, passed per call (the frame's favicon
/// lane holds the registry's names; the host keeps no server list).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetServer {
    pub server_id: String,
    pub display_name: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// server.properties' server-ip: `None`/empty = all interfaces; a
    /// specific value must equal the typed host for the address to
    /// resolve (resolveJoin's bind law — the frame passes it so the ONE
    /// commit door can resolve `host:port[/path]` by port, §8–§12).
    #[serde(
        default,
        rename = "bind_address",
        skip_serializing_if = "Option::is_none"
    )]
    pub bind: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Join,
    Server,
    Console,
    Page,
    Search,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    /// The committable text — what a typed Enter would have carried.
    pub text: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub kind: SuggestionKind,
}

/// kMaxMatches posture: the popup never scrolls in a 400 DIP window.
pub const MAX_SUGGESTIONS: usize = 8;

/// The popup's rows for the typed text against the frame's fleet
/// projection. Deterministic: same text, same list, same rows. The local
/// spelling — subtitles label a fleet server's port as localhost — kept
/// for the tests only (the frame always speaks `suggest_with_host`).
#[cfg(test)]
pub fn suggest(text: &str, servers: &[FleetServer]) -> Vec<Suggestion> {
    suggest_with_host(text, servers, "localhost")
}

/// The popup's rows with the BOX the panel actually speaks to (ADR-0011):
/// the frame passes the active connection profile's host, so a panel
/// pointed at a remote agent labels its fleet's ports with the box's own
/// host — the same law the new tab's input matches typed joins by. The
/// host only spells the subtitle labels; matching and ordering never
/// read it (a typed join resolves by port and bind, not by label).
pub fn suggest_with_host(text: &str, servers: &[FleetServer], host: &str) -> Vec<Suggestion> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let needle = trimmed.to_lowercase();
    let mut rows: Vec<Suggestion> = Vec::new();

    match classify(trimmed) {
        AddressRequest::Internal(destination) => match &destination {
            // A typed server or console reference names one thing.
            Destination::Server { .. } | Destination::Console { .. } => {
                rows.push(Suggestion {
                    text: destination.url(),
                    title: destination.label(),
                    subtitle: None,
                    kind: match destination {
                        Destination::Console { .. } => SuggestionKind::Console,
                        _ => SuggestionKind::Server,
                    },
                });
            }
            // zim://typed — the page word may be a prefix ("zim://set").
            Destination::Missing { url } => {
                let word = url.strip_prefix("zim://").unwrap_or(url).to_lowercase();
                for page in INTERNAL_PAGES {
                    if page.starts_with(&word) {
                        let page_destination = Destination::parse(&format!("zim://{page}/"));
                        rows.push(Suggestion {
                            text: page_destination.url(),
                            title: page_destination.label(),
                            subtitle: Some(page_destination.url()),
                            kind: SuggestionKind::Page,
                        });
                    }
                }
                if rows.is_empty() {
                    // §58's honesty, proposed: the row that will land.
                    rows.push(Suggestion {
                        text: url.clone(),
                        title: destination.label(),
                        subtitle: Some("No ZIM page — nothing lives here".into()),
                        kind: SuggestionKind::Page,
                    });
                }
            }
            other => {
                rows.push(Suggestion {
                    text: other.url(),
                    title: other.label(),
                    subtitle: Some(other.url()),
                    kind: SuggestionKind::Page,
                });
            }
        },
        AddressRequest::Join {
            host: typed,
            port,
            page,
            file_path,
        } => {
            // The URL model's own order (§8–§12): when the fleet owns the
            // typed address, the SERVER'S row rides first — the page the
            // operator is going to (`host:port[/path]`); the address's
            // own join row (the verdict page) follows; the fleet's other
            // name matches below.
            let resolved = resolve_fleet_join(&typed, port, servers, host);
            if let Some(server_id) = &resolved {
                if let Some(server) = servers.iter().find(|s| &s.server_id == server_id) {
                    let destination = Destination::Server {
                        server_id: server_id.clone(),
                        page,
                        file_path: file_path.clone(),
                    };
                    rows.push(Suggestion {
                        text: destination.url(),
                        title: server.display_name.clone(),
                        subtitle: Some(match server.port {
                            Some(port) => format!("{host}:{port} · {}", server.state),
                            None => server.state.clone(),
                        }),
                        kind: SuggestionKind::Server,
                    });
                }
            }
            // The join row itself — the address's own verdict page.
            let destination = Destination::Join {
                host: typed.clone(),
                port,
            };
            rows.push(Suggestion {
                text: destination.url(),
                title: destination.label(),
                subtitle: Some("Minecraft server address".into()),
                kind: SuggestionKind::Join,
            });
            // The fleet answers too: the server that owns the typed
            // port, or one whose name/id matches the host words. The
            // resolved server already rode first — never twice.
            let mut owned: Vec<&FleetServer> = servers
                .iter()
                .filter(|s| {
                    (s.port == Some(port)
                        || s.display_name.to_lowercase().contains(&needle)
                        || s.server_id.to_lowercase().contains(&needle))
                        && Some(s.server_id.as_str()) != resolved.as_deref()
                })
                .collect();
            owned.sort_by(|a, b| {
                let a_owned = a.port == Some(port);
                let b_owned = b.port == Some(port);
                b_owned
                    .cmp(&a_owned)
                    .then_with(|| a.display_name.cmp(&b.display_name))
            });
            for server in owned {
                rows.push(server_row(server, host));
            }
        }
        AddressRequest::Query(_) => {
            // The typed "console <name>" dialect: the console row rides
            // above the fleet's dashboards (the founder's reach — the
            // console is one word away, not one navigation away).
            let console_needle = trimmed
                .strip_prefix("console ")
                .map(str::trim)
                .filter(|rest| !rest.is_empty())
                .map(str::to_lowercase);
            let matches_for = |needle: &str| -> Vec<&FleetServer> {
                let mut hit: Vec<&FleetServer> = servers
                    .iter()
                    .filter(|s| {
                        s.display_name.to_lowercase().contains(needle)
                            || s.server_id.to_lowercase().contains(needle)
                    })
                    .collect();
                hit.sort_by(|a, b| {
                    let a_rank = rank_match(needle, &a.display_name.to_lowercase());
                    let b_rank = rank_match(needle, &b.display_name.to_lowercase());
                    a_rank
                        .cmp(&b_rank)
                        .then_with(|| a.display_name.cmp(&b.display_name))
                });
                hit
            };
            if let Some(rest) = &console_needle {
                for server in matches_for(rest).into_iter().take(3) {
                    rows.push(console_row(server, host));
                }
            }
            let console_proposed: std::collections::HashSet<String> = rows
                .iter()
                .filter(|row| row.kind == SuggestionKind::Console)
                .map(|row| {
                    row.text
                        .strip_prefix("zim://console/")
                        .unwrap_or(&row.text)
                        .to_owned()
                })
                .collect();
            for server in matches_for(&needle) {
                if console_proposed.contains(server.server_id.as_str()) {
                    continue; // the console row already proposed this server
                }
                rows.push(server_row(server, host));
            }
            if console_needle.is_none() || rows.is_empty() {
                // The search row — the honest floor (§22): the text is
                // never dropped, discovery answers it.
                rows.push(Suggestion {
                    text: trimmed.to_owned(),
                    title: trimmed.to_owned(),
                    subtitle: Some("Search discovery".into()),
                    kind: SuggestionKind::Search,
                });
            }
        }
    }

    rows.truncate(MAX_SUGGESTIONS);
    rows
}

/// starts_with outranks contains — Chromium's relevance instinct, ours
/// is alphabetical inside a rank (deterministic, no counters).
fn rank_match(needle: &str, haystack: &str) -> u8 {
    if haystack.starts_with(needle) {
        0
    } else {
        1
    }
}

fn server_row(server: &FleetServer, host: &str) -> Suggestion {
    let destination = Destination::Server {
        server_id: server.server_id.clone(),
        page: None,
        file_path: None,
    };
    Suggestion {
        text: destination.url(),
        title: server.display_name.clone(),
        subtitle: Some(match server.port {
            Some(port) => format!("{host}:{port} · {}", server.state),
            None => server.state.clone(),
        }),
        kind: SuggestionKind::Server,
    }
}

fn console_row(server: &FleetServer, host: &str) -> Suggestion {
    let destination = Destination::Console {
        server_id: server.server_id.clone(),
    };
    Suggestion {
        text: destination.url(),
        title: destination.label(),
        subtitle: Some(match server.port {
            Some(port) => format!("{host}:{port} · {}", server.state),
            None => server.state.clone(),
        }),
        kind: SuggestionKind::Console,
    }
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
                server_id: "abc".into(),
                page: None,
                file_path: None,
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
                port: 25565,
                page: None,
                file_path: None,
            }
        );
        // The founder's 0:25565 bind-all shorthand.
        assert_eq!(
            classify("0:25565"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565,
                page: None,
                file_path: None,
            }
        );
        assert_eq!(
            classify("25565"),
            AddressRequest::Join {
                host: None,
                port: 25565,
                page: None,
                file_path: None,
            }
        );
        // An out-of-range bare number is a query, never a join (the TS
        // twin's law in parseAddressInput).
        assert_eq!(classify("99999"), AddressRequest::Query("99999".into()));
        assert_eq!(
            classify("box.example.com"),
            AddressRequest::Join {
                host: Some("box.example.com".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        // The bare-word law (Chrome's fixup posture): a host-looking
        // word navigates (the dot is the host's own signal), "localhost"
        // is navigable by its name, and a PLAIN word is a discovery
        // search — the twins' old divergence (Rust joined "paper") is
        // closed here.
        assert_eq!(
            classify("localhost"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        assert_eq!(classify("paper"), AddressRequest::Query("paper".into()));
        assert_eq!(
            classify("mc-server"),
            AddressRequest::Query("mc-server".into())
        );
        // The IPv6 literal law (Chrome's fixup): the brackets are input
        // sugar — a bare address parses as an address, never as
        // host:port words.
        assert_eq!(
            classify("::1"),
            AddressRequest::Join {
                host: Some("::1".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        assert_eq!(
            classify("[::1]"),
            AddressRequest::Join {
                host: Some("::1".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        // The founder's bind-all spelling — normalize_host's own alias.
        assert_eq!(
            classify("::"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        assert_eq!(
            classify("::ffff:10.0.0.1"),
            AddressRequest::Join {
                host: Some("::ffff:10.0.0.1".into()),
                port: 0,
                page: None,
                file_path: None,
            }
        );
        // Not addresses — the join dialect's own words keep their
        // answers (a 16-bit group cannot hold 25565, and "1:2" is two
        // groups where eight are required).
        assert_eq!(
            classify("25565:8080"),
            AddressRequest::Join {
                host: Some("25565".into()),
                port: 8080,
                page: None,
                file_path: None,
            }
        );
        assert_eq!(
            classify("1:2"),
            AddressRequest::Join {
                host: Some("1".into()),
                port: 2,
                page: None,
                file_path: None,
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

    // --- the landing (OpenURL's disposition law) ---------------------------

    #[test]
    fn plain_enter_lands_in_the_tab_the_hand_is_in() {
        let mut strip = Strip::new();
        let first = strip.active.unwrap();
        strip.navigate(first, Destination::Servers);
        let landing = land(
            &mut strip,
            AddressRequest::Internal(Destination::Settings),
            false,
            &[],
            "localhost",
        )
        .unwrap();
        // Same tab, new destination — the history turned, the count did not.
        assert_eq!(
            landing,
            Landing::Destination {
                tab: first,
                destination: Destination::Settings
            }
        );
        assert_eq!(strip.tabs.len(), 1);
        assert_eq!(strip.active, Some(first));
        assert_eq!(*strip.tabs[0].destination(), Destination::Settings);
        // The query dialect never waits on a plain commit: no pending text.
        assert_eq!(strip.tabs[0].pending_query, None);
    }

    #[test]
    fn alt_enter_lands_a_fresh_foreground_tab_with_the_opener() {
        let mut strip = Strip::new();
        let first = strip.active.unwrap();
        let landing = land(
            &mut strip,
            AddressRequest::Internal(Destination::Settings),
            true,
            &[],
            "localhost",
        )
        .unwrap();
        let Landing::Destination { tab, destination } = landing else {
            panic!("a destination landing");
        };
        assert_ne!(tab, first);
        assert_eq!(destination, Destination::Settings);
        // The fresh tab: second in the strip, ACTIVE (foreground), its
        // opener the tab the omnibox sat in — AppendWebContents' law.
        assert_eq!(strip.tabs.len(), 2);
        assert_eq!(strip.active, Some(tab));
        assert_eq!(*strip.tabs[1].destination(), Destination::Settings);
        assert_eq!(strip.tabs[1].opener, Some(first));
        // The ONE activation law: the selection collapsed to the new tab.
        assert!(strip.selection.contains(&tab));
        assert_eq!(strip.selection.len(), 1);
        // The old tab's history never moved.
        assert_eq!(*strip.tabs[0].destination(), Destination::New);
    }

    #[test]
    fn alt_enter_join_opens_a_fresh_join_tab() {
        let mut strip = Strip::new();
        let landing = land(
            &mut strip,
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565,
                page: None,
                file_path: None,
            },
            true,
            &[],
            "localhost",
        )
        .unwrap();
        let Landing::Destination { destination, .. } = landing else {
            panic!("a destination landing");
        };
        assert_eq!(
            destination,
            Destination::Join {
                host: Some("localhost".into()),
                port: 25565
            }
        );
        assert_eq!(strip.tabs.len(), 2);
    }

    #[test]
    fn alt_enter_query_waits_on_the_new_tab_itself() {
        let mut strip = Strip::new();
        let first = strip.active.unwrap();
        let landing = land(
            &mut strip,
            AddressRequest::Query("paper".into()),
            true,
            &[],
            "localhost",
        )
        .unwrap();
        let Landing::Query { tab, text } = landing else {
            panic!("a query landing");
        };
        assert_ne!(tab, first);
        assert_eq!(text, "paper");
        assert_eq!(strip.active, Some(tab));
        assert_eq!(*strip.tabs[1].destination(), Destination::New);
        // The text waits ON THE TAB — its webview does not exist yet
        // (sync creates it), so hello is the delivery that cannot race.
        assert_eq!(strip.tabs[1].pending_query.as_deref(), Some("paper"));
        assert_eq!(strip.tabs[0].pending_query, None);
    }

    #[test]
    fn plain_query_rings_the_current_tab_and_waits_there() {
        let mut strip = Strip::new();
        let first = strip.active.unwrap();
        let landing = land(
            &mut strip,
            AddressRequest::Query("paper".into()),
            false,
            &[],
            "localhost",
        )
        .unwrap();
        let Landing::Query { tab, .. } = landing else {
            panic!("a query landing");
        };
        assert_eq!(tab, first);
        assert_eq!(strip.tabs.len(), 1);
        assert_eq!(*strip.tabs[0].destination(), Destination::New);
        assert_eq!(strip.tabs[0].pending_query.as_deref(), Some("paper"));
    }

    #[test]
    fn a_current_tab_commit_without_an_active_tab_is_refused() {
        let mut strip = Strip::new();
        // The empty strip: every tab pulled (the model's own close law
        // would mint a fresh New tab — this shape is the raw refusal case).
        strip.tabs.clear();
        strip.active = None;
        strip.selection.clear();
        strip.anchor = None;
        // Alt-Enter never refuses — the fresh tab exists to take it.
        assert!(land(
            &mut strip,
            AddressRequest::Internal(Destination::Settings),
            true,
            &[],
            "localhost"
        )
        .is_some());
        strip.tabs.clear();
        strip.active = None;
        // The plain commit has no hand to land in — the caller's refusal.
        assert!(land(
            &mut strip,
            AddressRequest::Internal(Destination::Settings),
            false,
            &[],
            "localhost"
        )
        .is_none());
        assert!(land(
            &mut strip,
            AddressRequest::Query("x".into()),
            false,
            &[],
            "localhost"
        )
        .is_none());
    }

    // --- the popup's matches (suggest) -------------------------------------

    fn fleet() -> Vec<FleetServer> {
        vec![
            FleetServer {
                server_id: "survival".into(),
                display_name: "Survival".into(),
                state: "running".into(),
                port: Some(25565),
                bind: None,
            },
            FleetServer {
                server_id: "creative".into(),
                display_name: "Creative".into(),
                state: "stopped".into(),
                port: Some(25566),
                bind: Some("127.0.0.1".into()),
            },
            FleetServer {
                server_id: "skyfactory".into(),
                display_name: "SkyFactory 4".into(),
                state: "crashed".into(),
                port: None,
                bind: None,
            },
        ]
    }

    #[test]
    fn empty_text_proposes_nothing() {
        assert!(suggest("", &fleet()).is_empty());
        assert!(suggest("   ", &fleet()).is_empty());
    }

    #[test]
    fn the_join_proposes_the_port_owner_then_the_address() {
        let rows = suggest("localhost:25565", &fleet());
        assert_eq!(rows.len(), 2);
        // Row one: the server that OWNS the typed address — the URL
        // model's own order (§8–§12): the operator's address names this
        // server's dashboard, and the commit door lands exactly this
        // row's destination.
        assert_eq!(rows[0].kind, SuggestionKind::Server);
        assert_eq!(rows[0].text, "zim://server/survival");
        assert_eq!(rows[0].title, "Survival");
        assert_eq!(
            rows[0].subtitle.as_deref(),
            Some("localhost:25565 · running")
        );
        // Row two: the join itself, its text the produced URL (the
        // commit door re-enters the same dialect a typed Enter speaks).
        assert_eq!(rows[1].kind, SuggestionKind::Join);
        assert_eq!(rows[1].text, "zim://join/localhost:25565");
        assert_eq!(rows[1].title, "Join localhost:25565");
    }

    #[test]
    fn a_port_only_join_finds_its_owner_too() {
        let rows = suggest("25566", &fleet());
        // The owner rides first (the URL model's order); the join row
        // follows. NOTE: creative binds 127.0.0.1 — the port-only
        // dialect (no typed host) reaches it by the bind law's own
        // "absent host matches" branch.
        assert_eq!(rows[0].kind, SuggestionKind::Server);
        assert_eq!(rows[0].text, "zim://server/creative");
        assert_eq!(
            rows[0].subtitle.as_deref(),
            Some("localhost:25566 · stopped")
        );
        assert_eq!(rows[1].kind, SuggestionKind::Join);
        assert_eq!(rows[1].text, "zim://join/:25566");
    }

    #[test]
    fn the_box_host_labels_the_fleets_ports_on_a_remote_panel() {
        // A panel pointed at an agent on 10.0.0.5: the subtitle spells
        // the box the fleet actually lives on — the same law the new
        // tab's input matches typed joins by. The join row's own text is
        // the typed address, untouched by the hint.
        let rows = suggest_with_host("25566", &fleet(), "10.0.0.5");
        // The owner rides first (the URL model's order), labeled with the
        // box the fleet actually lives on; the join row follows, its own
        // text the typed address, untouched by the hint.
        assert_eq!(rows[0].text, "zim://server/creative");
        assert_eq!(
            rows[0].subtitle.as_deref(),
            Some("10.0.0.5:25566 · stopped")
        );
        assert_eq!(rows[1].text, "zim://join/:25566");
        // Matching and ordering never read the hint: the port owner is
        // found by the port, exactly as on a local panel.
        assert_eq!(rows[0].kind, SuggestionKind::Server);
    }

    #[test]
    fn the_console_dialect_labels_with_the_box_host_too() {
        let rows = suggest_with_host("console creative", &fleet(), "10.0.0.5");
        assert_eq!(rows[0].kind, SuggestionKind::Console);
        assert_eq!(rows[0].text, "zim://console/creative");
        assert_eq!(
            rows[0].subtitle.as_deref(),
            Some("10.0.0.5:25566 · stopped")
        );
    }

    #[test]
    fn a_typed_name_proposes_the_fleet_before_the_search() {
        let rows = suggest("surv", &fleet());
        // starts_with ("Survival") outranks contains; the search row is
        // the honest floor underneath.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, SuggestionKind::Server);
        assert_eq!(rows[0].title, "Survival");
        assert_eq!(rows[1].kind, SuggestionKind::Search);
        assert_eq!(rows[1].text, "surv");
        assert_eq!(rows[1].subtitle.as_deref(), Some("Search discovery"));
    }

    #[test]
    fn the_search_row_survives_with_no_fleet_hit() {
        let rows = suggest("best modpack 2026", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SuggestionKind::Search);
        assert_eq!(rows[0].text, "best modpack 2026");
    }

    #[test]
    fn the_console_dialect_proposes_consoles_not_dashboards() {
        let rows = suggest("console surv", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SuggestionKind::Console);
        assert_eq!(rows[0].text, "zim://console/survival");
        assert_eq!(rows[0].title, "Console survival");
    }

    #[test]
    fn an_empty_console_dialect_is_just_a_search() {
        // "console " with no name: no console rows, the honest search.
        let rows = suggest("console ", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SuggestionKind::Search);
    }

    #[test]
    fn a_typed_page_prefix_proposes_the_page() {
        let rows = suggest("zim://set", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SuggestionKind::Page);
        assert_eq!(rows[0].text, "zim://settings/");
        assert_eq!(rows[0].title, "Settings");
        assert_eq!(rows[0].subtitle.as_deref(), Some("zim://settings/"));
    }

    #[test]
    fn an_unknown_page_proposes_its_own_honesty() {
        let rows = suggest("zim://nonsense", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "zim://nonsense");
        assert_eq!(
            rows[0].subtitle.as_deref(),
            Some("No ZIM page — nothing lives here")
        );
    }

    #[test]
    fn a_typed_server_reference_names_one_thing() {
        let rows = suggest("zim://server/survival", &fleet());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SuggestionKind::Server);
        assert_eq!(rows[0].title, "Server survival");
        assert_eq!(rows[0].subtitle, None);
    }

    #[test]
    fn the_popup_never_exceeds_k_max_matches() {
        let mut many = fleet();
        for i in 0..12 {
            many.push(FleetServer {
                server_id: format!("server{i}"),
                display_name: format!("Farm {i}"),
                state: "stopped".into(),
                port: Some(30000 + i as u16),
                bind: None,
            });
        }
        let rows = suggest("farm", &many);
        assert_eq!(rows.len(), MAX_SUGGESTIONS);
    }

    // --- the server-page URL model (§8–§12) ----------------------------------

    #[test]
    fn the_path_dialect_classifies_pages_and_file_paths() {
        // host:port/page, host:port/files/<path>, the port-only spelling,
        // and the dashboard root — the closed set the visible addresses
        // speak, parsed into the join request's own page fields.
        assert_eq!(
            classify("0.0.0.0:25552/console"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25552,
                page: Some(ServerPage::Console),
                file_path: None,
            }
        );
        assert_eq!(
            classify("localhost:25552/files/eula.txt"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25552,
                page: Some(ServerPage::Files),
                file_path: Some("eula.txt".into()),
            }
        );
        assert_eq!(
            classify("localhost:25552/files/tmp"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25552,
                page: Some(ServerPage::Files),
                file_path: Some("tmp".into()),
            }
        );
        // The dashboard root: the join without a page.
        assert_eq!(
            classify("0.0.0.0:25552"),
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25552,
                page: None,
                file_path: None,
            }
        );
    }

    #[test]
    fn a_path_that_names_no_page_is_an_honest_miss() {
        // `host:port/xyz` reads as a URL and lands as Missing — never a
        // silent dashboard, never a masked search.
        assert_eq!(
            classify("localhost:25552/nonsense"),
            AddressRequest::Internal(Destination::Missing {
                url: "localhost:25552/nonsense".into()
            })
        );
        // A search phrase with a slash is still a search: only a
        // join-shaped authority earns the page semantics.
        assert_eq!(
            classify("config world/foo"),
            AddressRequest::Query("config world/foo".into())
        );
        // A path on a portless host names no page (the model resolves by
        // port): honest miss, the words are never dropped silently.
        assert_eq!(
            classify("localhost/console"),
            AddressRequest::Internal(Destination::Missing {
                url: "localhost/console".into()
            })
        );
    }

    #[test]
    fn a_resolved_join_lands_the_servers_page() {
        // The founder's §10/§11 law: typing the server's own address
        // opens ITS dashboard; the path under it names the page; the
        // files path lands the file itself.
        let mut strip = Strip::new();
        let fleet = fleet();
        let landing = land(
            &mut strip,
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565,
                page: None,
                file_path: None,
            },
            false,
            &fleet,
            "localhost",
        )
        .unwrap();
        let Landing::Destination { destination, .. } = landing else {
            panic!("a destination landing");
        };
        assert_eq!(
            destination,
            Destination::Server {
                server_id: "survival".into(),
                page: None,
                file_path: None,
            }
        );

        let landing = land(
            &mut strip,
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565,
                page: Some(ServerPage::Console),
                file_path: None,
            },
            false,
            &fleet,
            "localhost",
        )
        .unwrap();
        let Landing::Destination { destination, .. } = landing else {
            panic!("a destination landing");
        };
        assert_eq!(
            destination,
            Destination::Server {
                server_id: "survival".into(),
                page: Some(ServerPage::Console),
                file_path: None,
            }
        );

        let landing = land(
            &mut strip,
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25565,
                page: Some(ServerPage::Files),
                file_path: Some("eula.txt".into()),
            },
            false,
            &fleet,
            "localhost",
        )
        .unwrap();
        let Landing::Destination { destination, .. } = landing else {
            panic!("a destination landing");
        };
        assert_eq!(
            destination,
            Destination::Server {
                server_id: "survival".into(),
                page: Some(ServerPage::Files),
                file_path: Some("eula.txt".into()),
            }
        );
    }

    #[test]
    fn an_unmanaged_address_keeps_the_join_page() {
        // Nothing in the fleet owns the port: the Join page speaks the
        // verdict, exactly as before — the resolution never invents.
        let mut strip = Strip::new();
        let landing = land(
            &mut strip,
            AddressRequest::Join {
                host: Some("localhost".into()),
                port: 25599,
                page: Some(ServerPage::Console),
                file_path: None,
            },
            false,
            &fleet(),
            "localhost",
        )
        .unwrap();
        let Landing::Destination { destination, .. } = landing else {
            panic!("a destination landing");
        };
        assert_eq!(
            destination,
            Destination::Join {
                host: Some("localhost".into()),
                port: 25599,
            }
        );
    }

    #[test]
    fn the_bind_law_governs_the_resolution() {
        // creative binds 127.0.0.1 (a specific interface): the typed
        // host must agree. The classify layer already normalizes
        // 127.0.0.1 → localhost, so a direct bind test speaks the raw
        // law: a specific bind only answers its own address.
        let fleet = fleet();
        // Port-only (no typed host): the absent host matches any bind
        // (resolveJoin's own branch).
        assert_eq!(
            resolve_fleet_join(&None, 25566, &fleet, "localhost"),
            Some("creative".into())
        );
        // All-interfaces survival answers the localhost alias.
        assert_eq!(
            resolve_fleet_join(&Some("localhost".into()), 25565, &fleet, "localhost"),
            Some("survival".into())
        );
        // A host the specific bind does not spell: no resolution.
        assert_eq!(
            resolve_fleet_join(&Some("box.example.com".into()), 25566, &fleet, "localhost"),
            None
        );
    }

    #[test]
    fn the_page_dialects_suggest_the_resolved_page_first() {
        let rows = suggest("localhost:25565/console", &fleet());
        // Row one: the server's console page — where the operator goes.
        assert_eq!(rows[0].kind, SuggestionKind::Server);
        assert_eq!(rows[0].text, "zim://server/survival/console");
        assert_eq!(rows[0].title, "Survival");
        // Row two: the join row — the address's own verdict page.
        assert_eq!(rows[1].kind, SuggestionKind::Join);
        assert_eq!(rows[1].text, "zim://join/localhost:25565");
    }
}
