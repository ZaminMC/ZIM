//! Crash diagnosis (§53's crash card, deepened 2026-10-12): the evidence
//! the daemon already captures — the dying process's last log lines —
//! is PATTERN-MATCHED against the small set of failure families a
//! Minecraft server actually dies of, and the match rides the
//! classification as a typed diagnosis the panel can state plainly.
//!
//! THE HONESTY LAW (bug #16's no-cheating rule): a diagnosis is only
//! ever derived from the REAL process output — no invented causes, no
//! guesses beyond the pattern table, and an evidence tail that matches
//! nothing simply diagnoses nothing (the card falls back to the raw
//! excerpt, exactly as before). The founder's own case is the law's
//! first citizen: a vanilla-jar download dying inside Paperclip with
//! `java.net.UnknownHostException: launcher.mojang.com` is a NETWORK
//! failure on the operator's machine, not a ZIM fault and not a mystery
//! — the card must say so with words that can be acted on.
//!
//! The WIRE types live in zamin-protocol (server.rs — the card's
//! payload); this module owns only the matching table that produces
//! them, so the table can grow without a protocol change.

pub use zamin_protocol::server::{CrashDiagnosis, CrashDiagnosisKind};

/// Match the evidence tail against the pattern table. The FIRST hit
/// wins; the table is ordered by specificity (the vanilla-jar download
/// failure is named before the generic DNS family it also belongs to).
/// `None` = no family matched — the card's honest fallback.
pub fn diagnose_evidence(evidence: &str) -> Option<CrashDiagnosis> {
    use zamin_protocol::server::CrashDiagnosisKind as K;
    // Lowercase once; the JVM's own casing varies across vendors.
    let hay = evidence.to_ascii_lowercase();
    let has = |needle: &str| hay.contains(&needle.to_ascii_lowercase());

    // The founder's case, verbatim: Paperclip's vanilla-jar check dies
    // on a DNS failure and the stack names the host it could not
    // resolve. The host is pulled out of the line so the title names
    // THE host that failed, not a generic "the internet".
    if has("Failed to download vanilla jar") || has("checkvanillajar") {
        if let Some(host) = first_unknown_host(&hay) {
            return Some(CrashDiagnosis {
                kind: K::NetworkDns,
                title: format!(
                    "The start failed because this machine could not resolve {host} — the server downloads its vanilla jar on first run."
                ),
                remediation: vec![
                    "Check this machine's internet connection, then start again.".into(),
                    format!(
                        "If a VPN, proxy, or firewall blocks {host}, allow it or disable the blocker and start again."
                    ),
                    "A DNS hiccup also heals itself — retrying in a minute often just works.".into(),
                ],
            });
        }
    }

    if has("unknownhostexception") {
        let host =
            first_unknown_host(&hay).unwrap_or_else(|| "the server's download host".to_owned());
        return Some(CrashDiagnosis {
            kind: K::NetworkDns,
            title: format!("The process failed because this machine could not resolve {host}."),
            remediation: vec![
                "Check this machine's internet connection, then start again.".into(),
                format!(
                    "If a VPN, proxy, or firewall blocks {host}, allow it or disable the blocker."
                ),
            ],
        });
    }

    if has("sockettimeoutexception")
        || has("connectexception")
        || has("connection timed out")
        || has("connection refused")
    {
        return Some(CrashDiagnosis {
            kind: K::NetworkTimeout,
            title: "A download the server performs timed out mid-connection.".into(),
            remediation: vec![
                "Check this machine's internet connection, then start again.".into(),
                "Slow or filtered networks often need a retry — the download resumes.".into(),
            ],
        });
    }

    if has("bindexception")
        || has("address already in use")
        || has("failed to bind to port")
        || has("could not bind to port")
    {
        return Some(CrashDiagnosis {
            kind: K::PortInUse,
            title: "The server's port is already in use by another process.".into(),
            remediation: vec![
                "Change the port on the Network tab, or stop the process holding it.".into(),
                "An older instance of this server may still be running — check the fleet list."
                    .into(),
            ],
        });
    }

    if has("outofmemoryerror")
        || has("could not reserve enough space for object heap")
        || has("insufficient memory")
    {
        return Some(CrashDiagnosis {
            kind: K::Memory,
            title: "The JVM could not get the memory it was configured to use.".into(),
            remediation: vec![
                "Lower the memory on the Startup tab, or free memory on this machine.".into(),
                "A 32-bit Java cannot address large heaps even on a big machine — ZIM's Java picker prefers 64-bit.".into(),
            ],
        });
    }

    if has("unsupportedclassversionerror")
        || has("has been compiled by a more recent version of the java runtime")
        || has("class file has wrong version")
    {
        return Some(CrashDiagnosis {
            kind: K::JavaVersion,
            title: "The configured Java runtime is too old for this server software.".into(),
            remediation: vec![
                "Pick a newer Java on the Startup tab (modern Paper wants Java 21+).".into(),
                "ZIM can fetch a matching JDK for you from the Java picker.".into(),
            ],
        });
    }

    if has("invalid or corrupt jarfile") || has("error opening zip file") {
        return Some(CrashDiagnosis {
            kind: K::CorruptJar,
            title: "The server jar is corrupt or not a runnable server jar.".into(),
            remediation: vec![
                "Re-download the server jar from the catalog (the software tab's update lane)."
                    .into(),
                "If this jar came from a manual install, re-download it from its official source."
                    .into(),
            ],
        });
    }

    None
}

/// Pull the first host a `UnknownHostException: <host>` line names —
/// the JVM's stack prints the host right after the exception class.
/// Case-insensitive by construction (the JVM's own casing varies
/// across vendors; the caller may pass raw or pre-lowercased evidence).
fn first_unknown_host(text: &str) -> Option<String> {
    let hay = text.to_ascii_lowercase();
    let marker = "unknownhostexception";
    let mut from = 0usize;
    while let Some(at) = hay[from..].find(marker) {
        let after = &hay[from + at + marker.len()..];
        // The host runs to the end of the line; trim the colon the JVM
        // prints and any surrounding whitespace.
        let line = after.split('\n').next().unwrap_or("");
        let host = line.trim_start_matches(':').trim().trim_end_matches(',');
        if !host.is_empty() && !host.contains(' ') {
            return Some(host.to_owned());
        }
        from += at + marker.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use zamin_protocol::server::CrashDiagnosisKind as K;

    // The founder's own console output, condensed to the lines that
    // matter — the exact case that motivated the diagnosis lane.
    #[test]
    fn the_vanilla_jar_dns_failure_names_its_host() {
        let evidence = "\
Downloading vanilla jar...
Failed to download vanilla jar
java.net.UnknownHostException: launcher.mojang.com
\tat java.base/sun.nio.ch.NioSocketImpl.connect(NioSocketImpl.java:572)
\tat io.papermc.paperclip.Paperclip.checkVanillaJar(Paperclip.java:191)";
        let diagnosis = diagnose_evidence(evidence).expect("the family must match");
        assert_eq!(diagnosis.kind, K::NetworkDns);
        assert!(
            diagnosis.title.contains("launcher.mojang.com"),
            "the title names the host that failed: {}",
            diagnosis.title
        );
        assert!(diagnosis.remediation.iter().any(|r| r.contains("VPN")));
    }

    #[test]
    fn a_generic_dns_failure_still_diagnoses() {
        let evidence = "java.net.UnknownHostException: api.modrinth.com";
        let diagnosis = diagnose_evidence(evidence).expect("the family must match");
        assert_eq!(diagnosis.kind, K::NetworkDns);
        assert!(diagnosis.title.contains("api.modrinth.com"));
    }

    #[test]
    fn port_conflicts_are_diagnosed() {
        let diagnosis =
            diagnose_evidence("java.net.BindException: Address already in use").expect("match");
        assert_eq!(diagnosis.kind, K::PortInUse);
    }

    #[test]
    fn heap_failures_are_diagnosed() {
        let diagnosis = diagnose_evidence(
            "Error occurred during initialization of VM\nCould not reserve enough space for object heap",
        )
        .expect("match");
        assert_eq!(diagnosis.kind, K::Memory);
    }

    #[test]
    fn an_unmatched_tail_diagnoses_nothing() {
        // The honesty law: no pattern, no invention — the card's raw
        // excerpt remains the whole story.
        assert!(diagnose_evidence("some random mod threw a fit").is_none());
    }

    #[test]
    fn the_host_extraction_takes_the_first_line_only() {
        assert_eq!(
            first_unknown_host("java.net.UnknownHostException: launcher.mojang.com\n\tat anywhere"),
            Some("launcher.mojang.com".to_owned())
        );
        // The JVM sometimes prints trailing context — anything with
        // spaces is not a bare host, and the search continues.
        assert_eq!(
            first_unknown_host("UnknownHostException: not a host string"),
            None
        );
    }

    #[test]
    fn the_wire_speaks_kebab_case_kinds() {
        let json = serde_json::to_string(&K::NetworkDns).expect("serialize");
        assert_eq!(json, r#""network-dns""#);
        let parsed: K = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, K::NetworkDns);
    }
}
