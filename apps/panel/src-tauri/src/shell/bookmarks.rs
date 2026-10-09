// The bookmarks model — a structural port of components/bookmarks/
// browser/bookmark_model.h, scoped to what ZIM bookmarks today:
// permanent nodes are the BAR and OTHER; nodes are id-keyed; persistence
// is a JSON codec file; edits are observable through the snapshot lane.
// No folders yet (documented in the porting spec §6).

use serde::{Deserialize, Serialize};

use crate::shell::tabs::Destination;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub destination: Destination,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bookmarks {
    pub items: Vec<Bookmark>,
    /// The IDC_SHOW_BOOKMARK_BAR posture — persists with the model.
    pub bar_visible: bool,
}

impl Bookmarks {
    /// BookmarkStorage: load the codec file (absent/corrupt ⇒ empty —
    /// a damaged file must never take the browser down).
    pub fn load(path: &std::path::Path) -> Bookmarks {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// BookmarkStorage: save the codec file (best-effort — a read-only
    /// home must not kill the browser; the next successful edit retries).
    pub fn save(&self, path: &std::path::Path) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, bytes);
        }
    }

    /// A fresh id-keyed node (BookmarkModel's UUID index; this build
    /// derives ids from time+counter without an external dependency —
    /// documented in the porting spec §6).
    fn fresh_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("bm-{stamp:x}-{seq:x}")
    }

    /// The star verb (IDC_BOOKMARK_THIS_TAB semantics in ZIM:
    /// toggle — a second press removes, matching the star's contract).
    pub fn toggle(&mut self, destination: Destination, title: String) {
        if let Some(at) = self.items.iter().position(|b| b.destination == destination) {
            self.items.remove(at);
        } else {
            self.items.push(Bookmark {
                id: Self::fresh_id(),
                title,
                destination,
            });
        }
    }

    /// isBookmarked — the star's resting state. Wired with the omnibox
    /// star's filled posture (a later phase); the ported API keeps it.
    #[allow(dead_code)]
    pub fn is_bookmarked(&self, destination: &Destination) -> bool {
        self.items.iter().any(|b| &b.destination == destination)
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|b| b.id != id);
    }

    pub fn toggle_bar(&mut self) {
        self.bar_visible = !self.bar_visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_bookmarks_unbookmarks() {
        let mut marks = Bookmarks::default();
        let d = Destination::Server {
            server_id: "s1".into(),
        };
        assert!(!marks.is_bookmarked(&d));
        marks.toggle(d.clone(), "Server s1".into());
        assert!(marks.is_bookmarked(&d));
        marks.toggle(d.clone(), "Server s1".into());
        assert!(marks.items.is_empty());
    }

    #[test]
    fn bar_visibility_persists_through_the_codec() {
        let mut marks = Bookmarks {
            bar_visible: true,
            ..Bookmarks::default()
        };
        marks.toggle(Destination::Jobs, "Jobs".into());
        let path = std::env::temp_dir().join(format!("zamin-bm-{}.json", std::process::id()));
        marks.save(&path);
        let loaded = Bookmarks::load(&path);
        assert!(loaded.bar_visible);
        assert_eq!(loaded.items.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_codec_loads_empty_not_dead() {
        let path = std::env::temp_dir().join(format!("zamin-bm-bad-{}.json", std::process::id()));
        std::fs::write(&path, b"{ not json").unwrap();
        let loaded = Bookmarks::load(&path);
        assert!(loaded.items.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
