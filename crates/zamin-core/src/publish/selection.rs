//! The founder's §41 file selection: rules, validation, and the
//! containment-checked walk that resolves them against a server root.
//!
//! Matching is byte-exact and case-sensitive (a server root is not a
//! case-insensitive promises API; an operator who wrote `Plugins/`
//! meant `Plugins/`). Excludes win over includes. Symlinks are skipped
//! on purpose: a publish must never follow a link out of the root, and
//! a link inside the root is someone's machine-local convenience, not
//! portable package content. A file that vanishes mid-walk is skipped
//! — it is "removed" as of now, which is exactly what the diff should
//! say; the packaging stage re-reads every resolved file and fails
//! loudly if one disappears between resolve and read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha512};
use zamin_protocol::publish::{PublishSelection, SelectionRule};

use crate::error::CoreError;

/// Wire caps so a hand-typed config cannot grow a novel.
pub const MAX_RULES: usize = 64;
pub const MAX_RULE_CHARS: usize = 512;

/// Safety limits for one publish walk (mirrors the archive trap list's
/// spirit, not its numbers: a publish packages a selection, not a root).
pub const MAX_ENTRIES: u64 = 20_000;
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFile {
    /// Server-root relative, `/`-separated.
    pub path: String,
    pub abs: PathBuf,
    pub sha512: String,
    pub size: u64,
}

/// Limits handed in by the caller (tests use small ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveLimits {
    pub max_entries: u64,
    pub max_total_bytes: u64,
}

impl Default for ResolveLimits {
    fn default() -> Self {
        ResolveLimits {
            max_entries: MAX_ENTRIES,
            max_total_bytes: MAX_TOTAL_BYTES,
        }
    }
}

fn invalid(reason: impl Into<String>) -> CoreError {
    CoreError::InvalidPublishConfig {
        reason: reason.into(),
    }
}

/// Validate any operator-supplied root-relative path (review keys, file
/// rules): relative, `/`-separated, no `..`, no backslashes, capped.
/// Returns the normalized payload (trailing slashes trimmed).
pub fn validate_relative_path(raw: &str) -> Result<String, CoreError> {
    validate_rule_payload(raw, "path")
}

/// Validate one rule's path-ish payload: relative, `/`-separated, no
/// `..`, no backslashes, no empty segments, capped. Returns the
/// normalized payload (trailing slashes trimmed on folder rules).
fn validate_rule_payload(raw: &str, what: &str) -> Result<String, CoreError> {
    if raw.is_empty() {
        return Err(invalid(format!("{what} rules need a path")));
    }
    if raw.len() > MAX_RULE_CHARS {
        return Err(invalid(format!(
            "{what} rule is over {MAX_RULE_CHARS} characters"
        )));
    }
    if raw.starts_with('/') {
        return Err(invalid(format!(
            "{what} rule {raw:?} is absolute; publish rules are server-root relative"
        )));
    }
    if raw.contains('\\') {
        return Err(invalid(format!(
            "{what} rule {raw:?} uses a backslash; use `/` separators"
        )));
    }
    let trimmed = raw.trim_end_matches('/');
    for segment in trimmed.split('/') {
        if segment.is_empty() {
            return Err(invalid(format!(
                "{what} rule {raw:?} has an empty path segment"
            )));
        }
        if segment == ".." {
            return Err(invalid(format!(
                "{what} rule {raw:?} escapes the server root"
            )));
        }
    }
    Ok(trimmed.to_owned())
}

/// Validate a whole selection: cap on rule count, every payload sane,
/// globs well-formed (`**` only as a whole segment).
pub fn validate_selection(sel: &PublishSelection) -> Result<(), CoreError> {
    let total = sel.includes.len() + sel.excludes.len();
    if total > MAX_RULES {
        return Err(invalid(format!(
            "the selection carries {total} rules; the cap is {MAX_RULES}"
        )));
    }
    for rule in sel.includes.iter().chain(sel.excludes.iter()) {
        match rule {
            SelectionRule::Folder { path } => {
                validate_rule_payload(path, "folder")?;
            }
            SelectionRule::File { path } => {
                let normalized = validate_rule_payload(path, "file")?;
                if normalized.ends_with('/') {
                    return Err(invalid(format!("file rule {normalized:?} names a folder")));
                }
            }
            SelectionRule::Glob { pattern } => {
                let normalized = validate_rule_payload(pattern, "glob")?;
                for segment in normalized.split('/') {
                    if segment.contains("**") && segment != "**" {
                        return Err(invalid(format!(
                            "glob {normalized:?} mixes `**` with other characters in one segment"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Does one rule match one root-relative path?
pub fn rule_matches(rule: &SelectionRule, rel: &str) -> bool {
    match rule {
        SelectionRule::Folder { path } => rel.starts_with(&format!("{path}/")),
        SelectionRule::File { path } => rel == path,
        SelectionRule::Glob { pattern } => glob_match(pattern, rel),
    }
}

/// Does the selection (includes minus excludes) pick `rel`? An empty
/// include list picks nothing — §41's structural rule.
pub fn selects(sel: &PublishSelection, rel: &str) -> bool {
    sel.includes.iter().any(|r| rule_matches(r, rel))
        && !sel.excludes.iter().any(|r| rule_matches(r, rel))
}

/// `*` and `?` within one path segment; no `/` inside a segment, so a
/// plain two-pointer with star-backtracking is exact.
fn segment_match(pattern: &str, candidate: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    let (mut pi, mut ci) = (0usize, 0usize);
    let (mut star, mut star_ci) = (None::<usize>, 0usize);
    while ci < c.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == c[ci]) {
            pi += 1;
            ci += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_ci = ci;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            star_ci += 1;
            ci = star_ci;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Glob over `/`-separated paths: `**` spans whole segments (including
/// zero), `*` / `?` stay inside one segment.
pub fn glob_match(pattern: &str, candidate: &str) -> bool {
    let mut p_segs: Vec<&str> = pattern.split('/').collect();
    let mut c_segs: Vec<&str> = candidate.split('/').collect();
    // A pattern may not start or end with an empty segment (validated),
    // but defensively pop one empty tail from trailing-slash input.
    while p_segs.last() == Some(&"") {
        p_segs.pop();
    }
    while c_segs.last() == Some(&"") {
        c_segs.pop();
    }
    glob_segments(&p_segs, &c_segs)
}

fn glob_segments(p: &[&str], c: &[&str]) -> bool {
    match p.split_first() {
        None => c.is_empty(),
        Some((&"**", rest)) => {
            // `**` eats zero or more segments.
            if glob_segments(rest, c) {
                return true;
            }
            for skip in 1..=c.len() {
                if glob_segments(rest, &c[skip..]) {
                    return true;
                }
            }
            false
        }
        Some((p_head, p_rest)) => match c.split_first() {
            None => false,
            Some((c_head, c_rest)) => {
                segment_match(p_head, c_head) && glob_segments(p_rest, c_rest)
            }
        },
    }
}

fn hash_file(path: &Path) -> Result<(String, u64), CoreError> {
    let mut file = std::fs::File::open(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha512::new();
    let mut size = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Resolve the selection against `root`: a deterministic, sorted walk
/// that hashes every selected file. Refuses to even start when the
/// selection itself is invalid.
pub fn resolve_selection(
    root: &Path,
    sel: &PublishSelection,
    limits: ResolveLimits,
) -> Result<BTreeMap<String, ResolvedFile>, CoreError> {
    validate_selection(sel)?;
    let mut out = BTreeMap::new();
    let mut entries_seen = 0u64;
    let mut total_bytes = 0u64;
    walk(
        root,
        PathBuf::new(),
        sel,
        limits,
        &mut entries_seen,
        &mut total_bytes,
        &mut out,
    )?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    root: &Path,
    rel_dir: PathBuf,
    sel: &PublishSelection,
    limits: ResolveLimits,
    entries_seen: &mut u64,
    total_bytes: &mut u64,
    out: &mut BTreeMap<String, ResolvedFile>,
) -> Result<(), CoreError> {
    let abs_dir = if rel_dir.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(&rel_dir)
    };
    let mut read = std::fs::read_dir(&abs_dir).map_err(|source| CoreError::Io {
        path: abs_dir.clone(),
        source,
    })?;
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    for entry in &mut read {
        let entry = entry.map_err(|source| CoreError::Io {
            path: abs_dir.clone(),
            source,
        })?;
        names.push(entry.file_name());
    }
    names.sort();
    for name in names {
        *entries_seen += 1;
        if *entries_seen > limits.max_entries {
            return Err(CoreError::PublishTooLarge {
                found: *entries_seen,
                entries: *entries_seen,
                max_bytes: limits.max_total_bytes,
                max_entries: limits.max_entries,
            });
        }
        let rel = if rel_dir.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel_dir.join(&name)
        };
        let abs = root.join(&rel);
        // Symlink check FIRST: a link never gets followed or packaged.
        let meta = std::fs::symlink_metadata(&abs).map_err(|source| CoreError::Io {
            path: abs.clone(),
            source,
        })?;
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(root, rel, sel, limits, entries_seen, total_bytes, out)?;
            continue;
        }
        if !meta.is_file() {
            continue; // sockets, fifos, devices: not package content.
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if !selects(sel, &rel_str) {
            continue;
        }
        let size = meta.len();
        *total_bytes += size;
        if *total_bytes > limits.max_total_bytes {
            return Err(CoreError::PublishTooLarge {
                found: *total_bytes,
                entries: *entries_seen,
                max_bytes: limits.max_total_bytes,
                max_entries: limits.max_entries,
            });
        }
        // A file that vanished between readdir and hash is "removed as
        // of now" — the diff's business, not an error.
        if let Ok((sha, size)) = hash_file(&abs) {
            out.insert(
                rel_str.clone(),
                ResolvedFile {
                    path: rel_str,
                    abs,
                    sha512: sha,
                    size,
                },
            );
        }
    }
    Ok(())
}
