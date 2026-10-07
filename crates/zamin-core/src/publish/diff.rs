//! The §42 diff: the current selection against the previous
//! publication, computed from byte digests. Mtimes never lie here —
//! they cannot; they are not consulted. The output is sorted by path
//! (BTreeMap iteration order) so the listing is stable across previews.

use std::collections::BTreeMap;

use zamin_protocol::publish::{DiffCounts, FileDiffEntry, FileDiffStatus};

use crate::publish::selection::ResolvedFile;
use crate::publish::state::PublicationFile;

/// Walk both maps in path order and classify every path either side
/// knows about. `removed` rows keep the size the file had when it was
/// published and carry no digest (there is nothing on disk to digest).
pub fn diff_publication(
    previous: &BTreeMap<String, PublicationFile>,
    current: &BTreeMap<String, ResolvedFile>,
) -> (Vec<FileDiffEntry>, DiffCounts) {
    let mut files = Vec::new();
    let mut counts = DiffCounts::default();
    let mut prev_it = previous.iter();
    let mut cur_it = current.iter();
    let mut prev = prev_it.next();
    let mut cur = cur_it.next();

    loop {
        let (p_entry, c_entry) = choose_next(prev, cur);
        match (p_entry, c_entry) {
            (None, None) => break,
            (Some((path, old)), None) => {
                counts.removed += 1;
                files.push(FileDiffEntry {
                    path: path.clone(),
                    status: FileDiffStatus::Removed,
                    size: Some(old.size),
                    sha512: None,
                });
                prev = prev_it.next();
            }
            (None, Some((path, new))) => {
                counts.added += 1;
                files.push(FileDiffEntry {
                    path: path.clone(),
                    status: FileDiffStatus::Added,
                    size: Some(new.size),
                    sha512: Some(new.sha512.clone()),
                });
                cur = cur_it.next();
            }
            (Some((path, old)), Some((_, new))) => {
                if old.sha512 == new.sha512 {
                    counts.unchanged += 1;
                    files.push(FileDiffEntry {
                        path: path.clone(),
                        status: FileDiffStatus::Unchanged,
                        size: Some(new.size),
                        sha512: Some(new.sha512.clone()),
                    });
                } else {
                    counts.modified += 1;
                    files.push(FileDiffEntry {
                        path: path.clone(),
                        status: FileDiffStatus::Modified,
                        size: Some(new.size),
                        sha512: Some(new.sha512.clone()),
                    });
                }
                prev = prev_it.next();
                cur = cur_it.next();
            }
        }
    }

    counts.changed = counts.added + counts.modified + counts.removed;
    (files, counts)
}

/// The merge step: pick which side advances. `Less` = the previous map's
/// key sorts first (the file was removed); `Greater` = the current one's
/// (the file was added); `Equal` = both advance together.
type PrevEntry<'a> = (&'a String, &'a PublicationFile);
type CurEntry<'b> = (&'b String, &'b ResolvedFile);

fn choose_next<'a, 'b>(
    prev: Option<PrevEntry<'a>>,
    cur: Option<CurEntry<'b>>,
) -> (Option<PrevEntry<'a>>, Option<CurEntry<'b>>) {
    match (prev, cur) {
        (None, None) => (None, None),
        (Some(_), None) => (prev, None),
        (None, Some(_)) => (None, cur),
        (Some((p, _)), Some((c, _))) => match p.as_str().cmp(c.as_str()) {
            std::cmp::Ordering::Less => (prev, None),
            std::cmp::Ordering::Greater => (None, cur),
            std::cmp::Ordering::Equal => (prev, cur),
        },
    }
}
