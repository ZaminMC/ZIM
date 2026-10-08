//! `files.*` — the file manager surface (protocol spec §8). Every path is
//! server-root-relative and POSIX-style; the rooted filesystem (ADR-0009)
//! is the only filesystem the daemon will touch, so absolute paths and
//! `..` are rejected before they reach the disk.
//!
//! Reads and writes are chunked: a client never sends or receives one
//! giant frame. Writes stage into a daemon-held temp file under the
//! server root and finalize with `files.commit` (one atomic rename), so a
//! partially uploaded file is never visible at its target path.

use serde::{Deserialize, Serialize};

/// Cap on `files.list` page size. The full listing is paged through
/// `offset`; `total` tells the client when it has everything.
pub const FILES_LIST_MAX_LIMIT: u32 = 2_000;

/// Default page size for `files.list` — one comfortable screen of a
/// directory.
pub const FILES_LIST_DEFAULT_LIMIT: u32 = 500;

/// Cap on one `files.read` chunk's `maxBytes`. Bounds the response frame;
/// clients larger than this page through with `offset`.
pub const FILES_READ_MAX_BYTES: u32 = 1024 * 1024;

/// Cap on one `files.write` chunk's base64 payload (decoded size). Bounds
/// the request frame.
pub const FILES_WRITE_MAX_CHUNK: u32 = 1024 * 1024;

/// Hard ceiling on one `files.copy`'s total landed bytes. A synchronous
/// method must never be able to fill a disk in one call; trees larger
/// than this belong to the backup/restore jobs. Mirrors the core limit.
pub const FILES_COPY_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Default and cap for one `files.search`'s hit count. The search is a
/// bounded walk, not an index; `truncated` says when the bound cut it.
pub const FILES_SEARCH_DEFAULT_LIMIT: u32 = 100;
pub const FILES_SEARCH_MAX_LIMIT: u32 = 200;

// --- files.list ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesListParams {
    pub server_id: String,
    /// Directory to list, root-relative POSIX style; "" or "." lists the
    /// root.
    #[serde(default)]
    pub path: String,
    /// Entries to skip — clients page forward with `offset + limit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// Entries to return; defaults to 2000, capped there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesEntry {
    pub name: String,
    pub kind: EntryKind,
    /// Bytes, for files. Absent for directories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Last modification, Unix epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,
    /// True for symlinks resolving outside the server root: listed so a
    /// client can show them, but every operation on them is denied
    /// (ADR-0009).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub symlink_outside: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesListResult {
    /// The directory that was listed (normalized, root-relative).
    pub path: String,
    /// Directories first, then files, each alphabetical — the daemon's
    /// canonical order; clients page through it with offset/limit.
    pub entries: Vec<FilesEntry>,
    /// Total entries in the directory, across all pages.
    pub total: u64,
}

// --- files.read ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesReadParams {
    pub server_id: String,
    pub path: String,
    /// Byte offset to read from; 0 starts the file.
    pub offset: u64,
    /// Chunk size; capped at [`FILES_READ_MAX_BYTES`].
    pub max_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesReadResult {
    /// The chunk, base64 (standard alphabet, padded).
    pub data: String,
    /// True when this chunk reaches the end of the file.
    pub eof: bool,
    /// Total file size in bytes, so clients can size progress and skip
    /// pointless final reads.
    pub total_bytes: u64,
}

// --- files.write / files.commit (staged uploads) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesWriteParams {
    pub server_id: String,
    /// The staging handle from the first call's result. Absent on the
    /// first chunk — the daemon creates a fresh staging file and returns
    /// its handle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_id: Option<String>,
    /// Base64 (standard alphabet, padded) chunk of the file content.
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesWriteResult {
    pub staging_id: String,
    /// Bytes staged so far, decoded.
    pub bytes_staged: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesCommitParams {
    pub server_id: String,
    pub staging_id: String,
    /// Where the staged file lands, root-relative. An atomic rename —
    /// readers see the old or the new content, never a partial file.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesCommitResult {
    /// The committed path, root-relative (echoes the target).
    pub path: String,
    /// Committed size in bytes.
    pub size_bytes: u64,
}

// --- files.mkdir / files.rename / files.delete ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesMkdirParams {
    pub server_id: String,
    /// Created with parents (like `mkdir -p`), root-contained.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesRenameParams {
    pub server_id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesDeleteParams {
    pub server_id: String,
    /// A file, or an EMPTY directory — recursive deletion is a job-sized
    /// operation and does not belong in a synchronous method (spec §8).
    pub path: String,
}

// --- files.copy (spec §8b) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesCopyParams {
    pub server_id: String,
    /// The source file or directory, root-relative POSIX style.
    pub from: String,
    /// Where the copy lands, root-relative. Must not already exist —
    /// copies never overwrite (`FS_COPY_TARGET_EXISTS`); the client
    /// offers a fresh name instead.
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesCopyResult {
    /// The copy's landed path (echoes `to`).
    pub path: String,
    /// Files copied (1 for a plain file).
    pub files: u64,
    /// Total bytes landed across every file.
    pub bytes: u64,
}

// --- files.search (spec §8c) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesSearchParams {
    pub server_id: String,
    /// Case-insensitive substring matched against entry names, walking
    /// the whole server root. Empty is rejected — the listing, not the
    /// search, is the way to see a directory.
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesSearchHit {
    /// Root-relative POSIX path of the match.
    pub path: String,
    pub kind: EntryKind,
    /// Bytes, for files. Absent for directories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesSearchResult {
    /// Matches sorted by path; the walk's deterministic order.
    pub hits: Vec<FilesSearchHit>,
    /// True when the walk found more matches (or deeper directories)
    /// than the bounds let it report — the client says so honestly.
    pub truncated: bool,
    /// Entries visited by the walk, so a client can say "searched N
    /// files" instead of guessing at coverage.
    pub scanned: u64,
}
