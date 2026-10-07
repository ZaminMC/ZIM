//! Publish (founder vision §40–47, §74, ADR-0017): the pieces of a
//! publication that must be exactly right and testable without a live
//! daemon. This module is deliberately daemon-free — callers pass
//! explicit paths so the data-dir layout stays a daemon concern, like
//! `backup` before it.
//!
//! - **Selection** — the founder's §41 file-picking rules (folders,
//!   files, globs) resolved against a server root. Excludes win. An
//!   empty include list selects nothing: the whole server directory is
//!   never blindly packaged, structurally.
//! - **State** — the previous publication's record (what actually went
//!   out, file digests included) and the false-positive reviews. Both
//!   are versioned, atomically written, and loud on corruption — a torn
//!   write leaves the old record, never a half one (§42's "never leave
//!   the publication state corrupted").
//! - **Diff** — the §42 M / A / D inspection of the current selection
//!   against that record, computed from byte digests, never from mtimes.
//! - **Secrets** — the §44/§46 scanner: extensible detectors, redacted
//!   excerpts, honest limits, reviewable false positives.
//! - **Package** — the deterministic zip with an embedded self-describing
//!   manifest, built in staging and committed by rename.
//! - **Provider** — the §40 provider interface plus the two honest
//!   built-ins (archive-only, local folder). Marketplace behavior is
//!   never hardcoded; it arrives as new providers.
//! - **Credentials** — §47: the daemon never stores credentials; they
//!   ride the environment channel (the OS keyring is the desktop host's
//!   reserved room).
//!
//! The AI part of the founder's publish vision (§43's Dutchmen-generated
//! changelog) is ignored by standing scope: the changelog is a plain
//! string the operator edits. The room is named in ADR-0017 and stays
//! empty until real demand names it.

pub mod credentials;
pub mod diff;
pub mod package;
pub mod provider;
pub mod secrets;
pub mod selection;
pub mod state;

#[cfg(test)]
mod tests;
