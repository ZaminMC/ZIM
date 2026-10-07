//! The §44/§46 security scan that stands between a selection and the
//! packaging stage. Extensible detectors, redacted excerpts, honest
//! limits — and one founder rule taken verbatim: this is a safety
//! mechanism, NOT a guarantee. A clean scan is not a certificate; it is
//! one layer of defense in front of an operator who can still review,
//! exclude, or publish anyway explicitly.
//!
//! Detector vocabulary (the `kind` field; open — new detectors arrive
//! additively and readers stay tolerant):
//!
//! | kind                  | detector            | default severity |
//! |-----------------------|---------------------|------------------|
//! | private-key           | token-pattern       | critical         |
//! | discord-bot-token     | token-pattern       | high (critical inside DiscordSRV) |
//! | aws-access-key        | token-pattern       | critical         |
//! | github-token          | token-pattern       | critical         |
//! | slack-token           | token-pattern       | critical         |
//! | stripe-secret-key     | token-pattern       | critical         |
//! | google-api-key        | token-pattern       | high             |
//! | jwt                   | token-pattern       | high             |
//! | webhook-url           | token-pattern       | critical         |
//! | config-secret-key     | config-key          | medium (critical for DiscordSRV bot tokens) |
//! | high-entropy-string   | high-entropy        | low              |
//! | sensitive-filename    | sensitive-filename  | high             |
//! | discord-srv-config    | discord-srv-advisory| critical         |
//!
//! DiscordSRV's special treatment (§44) is two-layered: the advisory
//! flags the plugin's config as especially suspicious whenever the
//! server is actually running with the plugin loaded, and any bot-token
//! detection inside its directory escalates to critical. Both ride the
//! same review mechanism as everything else — false positives must be
//! reviewable, false negatives must be visible.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use zamin_protocol::publish::{ReviewEntry, ScanReport, SecretFinding, SecretSeverity};

use crate::publish::selection::ResolvedFile;

/// Files above this are counted as skipped, not scanned: a multi-gigabit
/// jar is not a config file, and line-scanning one is a waste of the
/// operator's time. Skips are counted, never silent.
pub const LINE_SCAN_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Context the caller computes once per scan — things core cannot know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanContext {
    /// True when the server is running with DiscordSRV loaded. The
    /// founder's point: a loaded DiscordSRV's config is LIKELY to carry
    /// a live bot token, so it is treated as especially suspicious even
    /// when no pattern matched.
    pub discord_srv_active: bool,
}

/// Run the scan over an already-resolved selection. Infallible by
/// design: a file that cannot be read (or is too large) is counted in
/// `files_skipped` — the packaging stage will surface any real read
/// failure loudly when it re-reads everything.
pub fn scan(
    files: &BTreeMap<String, ResolvedFile>,
    ctx: &ScanContext,
    reviews: &BTreeSet<ReviewEntry>,
) -> ScanReport {
    let mut report = ScanReport::default();
    for file in files.values() {
        if file.size > LINE_SCAN_MAX_BYTES {
            report.files_skipped += 1;
            continue;
        }
        let bytes = match std::fs::read(&file.abs) {
            Ok(bytes) => bytes,
            Err(_) => {
                report.files_skipped += 1;
                continue;
            }
        };
        report.files_scanned += 1;
        scan_file(&file.path, &bytes, ctx, &mut report.findings);
    }
    for finding in &mut report.findings {
        finding.reviewed = reviews.contains(&ReviewEntry {
            file: finding.file.clone(),
            kind: finding.kind.clone(),
        });
    }
    report
}

/// Findings that would refuse an execute right now: unreviewed and
/// above `low`. High-entropy noise stays informational; everything else
/// blocks until an operator reviews it or overrides explicitly.
pub fn blocking(report: &ScanReport) -> Vec<&SecretFinding> {
    report
        .findings
        .iter()
        .filter(|f| !f.reviewed && f.severity != SecretSeverity::Low)
        .collect()
}

fn is_discord_srv_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.starts_with("plugins/discordsrv/")
}

fn push(findings: &mut Vec<SecretFinding>, finding: SecretFinding) {
    // One finding per (file, line, kind): the same detector meeting the
    // same line twice says it once.
    if !findings
        .iter()
        .any(|f| f.file == finding.file && f.line == finding.line && f.kind == finding.kind)
    {
        findings.push(finding);
    }
}

fn scan_file(rel: &str, bytes: &[u8], ctx: &ScanContext, findings: &mut Vec<SecretFinding>) {
    // File-level detectors first.
    if let Some(f) = sensitive_filename(rel) {
        push(findings, f);
    }
    if ctx.discord_srv_active && is_discord_srv_path(rel) && has_config_name(rel) {
        push(
            findings,
            SecretFinding {
                file: rel.to_owned(),
                line: 0,
                kind: "discord-srv-config".to_owned(),
                severity: SecretSeverity::Critical,
                excerpt: format!(
                    "{rel} — DiscordSRV is loaded and its configuration commonly contains a live bot token"
                ),
                detector: "discord-srv-advisory".to_owned(),
                reviewed: false,
            },
        );
    }

    let text = String::from_utf8_lossy(bytes);
    // Word-level detectors share one tokenization so a word flagged by a
    // precise detector is not re-flagged as generic entropy.
    for (idx, line) in text.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let tokens = words(line);
        let mut flagged: Vec<String> = Vec::new();

        if line.contains("PRIVATE KEY-----") {
            push(
                findings,
                SecretFinding {
                    file: rel.to_owned(),
                    line: line_no,
                    kind: "private-key".to_owned(),
                    severity: SecretSeverity::Critical,
                    excerpt: "a PEM private key header".to_owned(),
                    detector: "token-pattern".to_owned(),
                    reviewed: false,
                },
            );
        }
        if let Some(host) = webhook_host(line) {
            push(
                findings,
                SecretFinding {
                    file: rel.to_owned(),
                    line: line_no,
                    kind: "webhook-url".to_owned(),
                    severity: SecretSeverity::Critical,
                    excerpt: format!("a {host} webhook URL"),
                    detector: "token-pattern".to_owned(),
                    reviewed: false,
                },
            );
        }

        for word in &tokens {
            if let Some((kind, severity, excerpt)) = token_word(word, rel, ctx) {
                flagged.push(word.clone());
                push(
                    findings,
                    SecretFinding {
                        file: rel.to_owned(),
                        line: line_no,
                        kind: kind.to_owned(),
                        severity,
                        excerpt,
                        detector: "token-pattern".to_owned(),
                        reviewed: false,
                    },
                );
            }
        }

        if let Some(f) = config_key(line, line_no, rel, ctx) {
            push(findings, f);
        }

        for word in &tokens {
            if flagged.iter().any(|w| w == word) {
                continue;
            }
            if let Some(excerpt) = high_entropy(word) {
                push(
                    findings,
                    SecretFinding {
                        file: rel.to_owned(),
                        line: line_no,
                        kind: "high-entropy-string".to_owned(),
                        severity: SecretSeverity::Low,
                        excerpt,
                        detector: "high-entropy".to_owned(),
                        reviewed: false,
                    },
                );
            }
        }
    }
}

fn has_config_name(rel: &str) -> bool {
    Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().contains("config"))
        .unwrap_or(false)
}

/// Known sensitive filenames (§46's "known sensitive filenames" list).
fn sensitive_filename(rel: &str) -> Option<SecretFinding> {
    let name = Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())?
        .to_ascii_lowercase();
    let hit = name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".jks")
        || name.ends_with(".keystore")
        || name == "credentials.json"
        || name == "secrets.json"
        || name == "id_rsa"
        || name == "id_dsa"
        || name == "id_ecdsa"
        || name == "id_ed25519"
        || (name.starts_with("service-account") && name.ends_with(".json"));
    if hit {
        Some(SecretFinding {
            file: rel.to_owned(),
            line: 0,
            kind: "sensitive-filename".to_owned(),
            severity: SecretSeverity::High,
            excerpt: format!("{rel} — a filename that conventionally carries credentials"),
            detector: "sensitive-filename".to_owned(),
            reviewed: false,
        })
    } else {
        None
    }
}

fn webhook_host(line: &str) -> Option<&'static str> {
    if line.contains("discord.com/api/webhooks/") || line.contains("discordapp.com/api/webhooks/") {
        Some("Discord")
    } else if line.contains("hooks.slack.com/services/") {
        Some("Slack")
    } else {
        None
    }
}

/// Split a line into candidate tokens. Deliberately split on everything
/// that is never part of a credential: whitespace, quotes, brackets,
/// YAML/properties punctuation. Dots and letters/digits survive, which
/// is what the token shapes need.
fn words(line: &str) -> Vec<String> {
    line.split([
        ' ', '\t', '"', '\'', ',', ';', ':', '=', '(', ')', '[', ']', '{', '}', '<', '>', '!',
    ])
    .filter(|w| !w.is_empty())
    .map(str::to_owned)
    .collect()
}

fn is_base64url(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn redact(word: &str) -> String {
    let n = word.chars().count();
    let head: String = word.chars().take(4).collect();
    format!("{head}…[redacted] (len {n})")
}

/// The precise token shapes one candidate word can carry. Returns
/// (kind, severity, redacted excerpt).
fn token_word(
    word: &str,
    rel: &str,
    _ctx: &ScanContext,
) -> Option<(&'static str, SecretSeverity, String)> {
    // Discord bot token: three dot-separated base64url segments,
    // long / short / long.
    if word.matches('.').count() == 2 {
        let parts: Vec<&str> = word.split('.').collect();
        let (a, b, c) = (parts[0], parts[1], parts[2]);
        let shaped = (20..=64).contains(&a.len())
            && (5..=20).contains(&b.len())
            && (20..=96).contains(&c.len())
            && is_base64url(a)
            && is_base64url(b)
            && is_base64url(c);
        if shaped {
            let inside_discord_srv = is_discord_srv_path(rel);
            let severity = if inside_discord_srv {
                SecretSeverity::Critical
            } else {
                SecretSeverity::High
            };
            return Some(("discord-bot-token", severity, redact(word)));
        }
        // JWT: eyJ…, three base64url segments.
        if word.starts_with("eyJ")
            && is_base64url(parts[0])
            && is_base64url(parts[1])
            && is_base64url(parts[2])
        {
            return Some(("jwt", SecretSeverity::High, redact(word)));
        }
    }
    // AWS access key id: AKIA/ASIA + 16 uppercase alphanumerics.
    if (word.starts_with("AKIA") || word.starts_with("ASIA"))
        && word.len() == 20
        && word[4..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Some(("aws-access-key", SecretSeverity::Critical, redact(word)));
    }
    // GitHub: classic ghp_ (36 alnum) or fine-grained github_pat_.
    if (word.starts_with("ghp_")
        && word.len() == 40
        && word[4..].bytes().all(|b| b.is_ascii_alphanumeric()))
        || (word.starts_with("github_pat_") && word.len() >= 40)
    {
        return Some(("github-token", SecretSeverity::Critical, redact(word)));
    }
    // Slack: xox{b,p,a,r,s}-…
    if word.len() >= 10
        && ["xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-"]
            .iter()
            .any(|p| word.starts_with(p))
    {
        return Some(("slack-token", SecretSeverity::Critical, redact(word)));
    }
    // Stripe live secret keys.
    if (word.starts_with("sk_live_") || word.starts_with("rk_live_"))
        && word.len() >= 24
        && word[8..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Some(("stripe-secret-key", SecretSeverity::Critical, redact(word)));
    }
    // Google API keys.
    if word.starts_with("AIza") && word.len() == 39 && is_base64url(&word[4..]) {
        return Some(("google-api-key", SecretSeverity::High, redact(word)));
    }
    None
}

/// §46's configuration-key heuristic: a key NAME that says "secret" with
/// a value that looks real. Template references (`${...}`), booleans,
/// numbers, and obvious placeholders are skipped; keys that merely NAME
/// a file (`token-file:`) are skipped too.
fn config_key(line: &str, line_no: u32, rel: &str, ctx: &ScanContext) -> Option<SecretFinding> {
    let (key_raw, value_raw) = split_key_value(line)?;
    let key_trimmed = key_raw.trim().trim_matches('"').trim_matches('\'');
    if key_trimmed.is_empty() {
        return None;
    }
    let lower = key_trimmed.to_ascii_lowercase();
    let squashed = lower.replace(['-', '_', ' '], "");
    let says_secret = [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "accesskey",
        "authkey",
        "credential",
        "privatekey",
    ]
    .iter()
    .any(|k| squashed.contains(k));
    if !says_secret {
        return None;
    }
    // Keys that point at where a secret lives rather than the secret.
    if [
        "path", "file", "dir", "folder", "url", "enabled", "name", "command", "id",
    ]
    .iter()
    .any(|suffix| squashed.ends_with(suffix))
    {
        return None;
    }
    let value = value_raw.trim();
    let value = strip_comment(value)
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if value.is_empty()
        || value == "null"
        || value == "~"
        || value.contains("${")
        || value.contains("%(")
        || value.starts_with('<')
        || value.starts_with('$')
        || value.parse::<bool>().is_ok()
        || value.parse::<f64>().is_ok()
        || is_placeholder(value)
    {
        return None;
    }
    let bot_tokenish = squashed.contains("bottoken");
    let discord_srv = is_discord_srv_path(rel);
    let severity = if bot_tokenish || (discord_srv && squashed.contains("token")) {
        SecretSeverity::Critical
    } else {
        SecretSeverity::Medium
    };
    let mut finding = SecretFinding {
        file: rel.to_owned(),
        line: line_no,
        kind: "config-secret-key".to_owned(),
        severity,
        excerpt: format!("key `{key_trimmed}`"),
        detector: "config-key".to_owned(),
        reviewed: false,
    };
    if ctx.discord_srv_active && discord_srv {
        finding.excerpt = format!("key `{key_trimmed}` (DiscordSRV is loaded)");
    }
    Some(finding)
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':');
    let eq = line.find('=');
    match (colon, eq) {
        (Some(c), Some(e)) => {
            if c <= e {
                Some((&line[..c], &line[c + 1..]))
            } else {
                Some((&line[..e], &line[e + 1..]))
            }
        }
        (Some(c), None) => Some((&line[..c], &line[c + 1..])),
        (None, Some(e)) => Some((&line[..e], &line[e + 1..])),
        (None, None) => None,
    }
}

fn strip_comment(value: &str) -> &str {
    // A `#` only starts a comment when the value does not open with a
    // quote (quoted values may contain hashes; unquoted YAML values
    // may not).
    if value.starts_with('"') || value.starts_with('\'') {
        value
    } else if let Some(idx) = value.find(" #") {
        &value[..idx]
    } else {
        value
    }
}

fn is_placeholder(value: &str) -> bool {
    let v = value.to_ascii_lowercase().replace(['-', '_', ' '], "");
    [
        "changeme",
        "changemeplease",
        "example",
        "placeholder",
        "dummy",
        "todo",
        "insert",
        "yourtoken",
        "yoursecret",
        "yourpassword",
        "xxxxxx",
        "secret",
        "token",
        "password",
        "none",
        "noneed",
    ]
    .iter()
    .any(|p| v == *p || v.starts_with("your") || v.starts_with("insert"))
}

/// Generic high-entropy detection (§46): long printable runs with real
/// character-class spread. Deliberately `low` severity — it is the
/// noisiest detector and must not block on its own.
fn high_entropy(word: &str) -> Option<String> {
    if word.len() < 32 || word.len() > 256 {
        return None;
    }
    if !word
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/'))
    {
        return None;
    }
    let mut classes = 0;
    if word.bytes().any(|b| b.is_ascii_lowercase()) {
        classes += 1;
    }
    if word.bytes().any(|b| b.is_ascii_uppercase()) {
        classes += 1;
    }
    if word.bytes().any(|b| b.is_ascii_digit()) {
        classes += 1;
    }
    if word
        .bytes()
        .any(|b| matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/'))
    {
        classes += 1;
    }
    if classes < 3 {
        return None;
    }
    let mut freq = [0u32; 256];
    for b in word.bytes() {
        freq[b as usize] += 1;
    }
    let len = word.len() as f64;
    let entropy: f64 = freq
        .iter()
        .filter(|&&f| f > 0)
        .map(|&f| {
            let p = f as f64 / len;
            -p * p.log2()
        })
        .sum();
    if entropy >= 4.5 {
        Some(redact(word))
    } else {
        None
    }
}
