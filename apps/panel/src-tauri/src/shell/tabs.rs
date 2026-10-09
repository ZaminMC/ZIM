// The authoritative tab model — a source port of Chromium's TabStripModel
// semantics (chrome/browser/ui/tabs/tab_strip_model.{h,cc}), stripped of
// WebContents: a ZIM tab's "content" is a typed Destination.
//
// Ported policies, each with its upstream citation:
// - insertion funnels through ONE entry point that enforces the pinned
//   block invariant (InsertWebContentsAt's doc: the index changes only to
//   keep "all pinned tabs occur before non-pinned tabs");
// - foreground tabs inherit the opener of the previously active tab
//   (AppendWebContents doc);
// - SetTabPinned relocates the tab to the block edge and returns the
//   resulting index;
// - DetachedTab is a first-class struct meant for re-insertion into
//   another strip (DetachWebContentsAtForInsertion);
// - closed tabs stack TabRestoreService-style with their strip position;
// - groups are contiguous by construction; visual data rides the group
//   record (tab_group_visual_data.h).
//
// Documented divergences (porting spec §2): opener affinity drives only
// insertion position; a move that would strand a group member ungroups
// it; closing the strip's last tab opens a fresh New tab (upstream
// closes the window — a desktop panel that quits on the last Ctrl+W
// would be hostile).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type TabId = u32;
pub type GroupId = u32;

/// The closed set of places a tab can show (§58) — the Rust twin of
/// state/destinations.ts. `zim://` URLs canonicalize through
/// [`Destination::url`] / [`Destination::parse`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Destination {
    New,
    Servers,
    Server { server_id: String },
    Console { server_id: String },
    Settings,
    Jobs,
    Audit,
    About,
    Feedback,
    Extensions,
    Downloads,
    Missing { url: String },
}

impl Destination {
    /// destinationUrl() — state/destinations.ts.
    pub fn url(&self) -> String {
        match self {
            Destination::New => "zim://new".into(),
            Destination::Servers => "zim://servers/".into(),
            Destination::Settings => "zim://settings/".into(),
            Destination::Jobs => "zim://jobs/".into(),
            Destination::Audit => "zim://audit/".into(),
            Destination::About => "zim://about/".into(),
            Destination::Feedback => "zim://feedback/".into(),
            Destination::Extensions => "zim://extensions/".into(),
            Destination::Downloads => "zim://downloads/".into(),
            Destination::Server { server_id } => format!("zim://server/{server_id}"),
            Destination::Console { server_id } => format!("zim://console/{server_id}"),
            Destination::Missing { url } => url.clone(),
        }
    }

    /// The inverse of url() — the address bar and the session file both
    /// round-trip through this. Unknown internal pages are honest
    /// Missing destinations (§58), never silent searches.
    pub fn parse(text: &str) -> Destination {
        let Some(rest) = text.strip_prefix("zim://") else {
            return Destination::Missing {
                url: text.to_owned(),
            };
        };
        let (page, arg) = match rest.split_once('/') {
            Some((page, arg)) => (page, arg.trim_end_matches('/')),
            None => (rest, ""),
        };
        match page {
            "new" => Destination::New,
            "servers" => Destination::Servers,
            "settings" => Destination::Settings,
            "jobs" => Destination::Jobs,
            "audit" => Destination::Audit,
            "about" => Destination::About,
            "feedback" => Destination::Feedback,
            "extensions" => Destination::Extensions,
            "downloads" => Destination::Downloads,
            "server" if !arg.is_empty() => Destination::Server {
                server_id: arg.into(),
            },
            "console" if !arg.is_empty() => Destination::Console {
                server_id: arg.into(),
            },
            _ => Destination::Missing {
                url: text.to_owned(),
            },
        }
    }

    /// destinationLabel() — the strip's resting title.
    pub fn label(&self) -> String {
        match self {
            Destination::New => "New tab".into(),
            Destination::Servers => "Fleet".into(),
            Destination::Server { server_id } => format!("Server {server_id}"),
            Destination::Console { server_id } => format!("Console {server_id}"),
            Destination::Settings => "Settings".into(),
            Destination::Jobs => "Jobs".into(),
            Destination::Audit => "Audit".into(),
            Destination::About => "About".into(),
            Destination::Feedback => "Feedback".into(),
            Destination::Extensions => "Extensions".into(),
            Destination::Downloads => "Downloads".into(),
            Destination::Missing { url } => format!("No page at {url}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub history: Vec<Destination>,
    pub history_index: usize,
    pub pinned: bool,
    pub group: Option<GroupId>,
    /// The tab this one was opened from (foreground-append inheritance).
    pub opener: Option<TabId>,
    pub muted: bool,
    /// §60: bumped on demand; the content view keys off it and rebuilds.
    /// Never touches the server process.
    pub reload: u32,
}

impl Tab {
    pub fn destination(&self) -> &Destination {
        self.history
            .get(self.history_index)
            .unwrap_or(&Destination::New)
    }

    fn fresh(id: TabId, destination: Destination, opener: Option<TabId>) -> Tab {
        Tab {
            id,
            history: vec![destination],
            history_index: 0,
            pinned: false,
            group: None,
            opener,
            muted: false,
            reload: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub label: String,
    /// Index into the frame's six-color palette.
    pub color: u8,
    pub collapsed: bool,
}

/// TabRestoreService-style entry: what a closed tab remembers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClosedTab {
    pub history: Vec<Destination>,
    pub history_index: usize,
    pub pinned: bool,
    pub index: usize,
}

/// The strip — one window's ordered tabs, selection, groups, and the
/// closed-tab memory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Strip {
    pub tabs: Vec<Tab>,
    pub active: Option<TabId>,
    pub groups: HashMap<GroupId, Group>,
    pub next_tab: TabId,
    pub next_group: GroupId,
    pub closed: Vec<ClosedTab>,
}

pub const MAX_CLOSED: usize = 25;
pub const GROUP_COLORS: u32 = 6;

impl Default for Strip {
    fn default() -> Self {
        Strip::new()
    }
}

impl Strip {
    pub fn new() -> Strip {
        let mut strip = Strip {
            tabs: Vec::new(),
            active: None,
            groups: HashMap::new(),
            next_tab: 1,
            next_group: 1,
            closed: Vec::new(),
        };
        strip.append(Destination::New, true);
        strip
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// The edge of the pinned block: pinning inserts at/below it,
    /// unpinning at/above it.
    fn block_edge(&self, pinned: bool) -> usize {
        if pinned {
            self.tabs
                .iter()
                .position(|t| !t.pinned)
                .unwrap_or(self.tabs.len())
        } else {
            self.tabs
                .iter()
                .rposition(|t| t.pinned)
                .map_or(0, |i| i + 1)
        }
    }

    /// InsertWebContentsAt — the ONE entry point. The index is honored
    /// except where it would break the pinned block; foreground tabs are
    /// selected and inherit the previously active tab as opener. Fresh
    /// tabs are unpinned — pinning goes through [`Strip::set_pinned`],
    /// exactly as upstream separates the two.
    pub fn insert(&mut self, index: usize, destination: Destination, foreground: bool) -> TabId {
        let id = self.next_tab;
        self.next_tab += 1;
        let opener = if foreground { self.active } else { None };
        let mut index = index.min(self.tabs.len());
        index = index.max(self.block_edge(false));
        self.tabs.insert(index, Tab::fresh(id, destination, opener));
        if foreground || self.active.is_none() {
            self.active = Some(id);
        }
        id
    }

    /// AppendWebContents.
    pub fn append(&mut self, destination: Destination, foreground: bool) -> TabId {
        self.insert(self.tabs.len(), destination, foreground)
    }

    /// SetTabPinned — relocate to the block edge, return the new index.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) -> Option<usize> {
        let index = self.index_of(id)?;
        if self.tabs[index].pinned == pinned {
            return Some(index);
        }
        let mut tab = self.tabs.remove(index);
        tab.pinned = pinned;
        // Group membership cannot survive relocation across the block.
        tab.group = None;
        let target = if pinned {
            self.tabs
                .iter()
                .position(|t| !t.pinned)
                .unwrap_or(self.tabs.len())
        } else {
            self.tabs
                .iter()
                .rposition(|t| t.pinned)
                .map_or(0, |i| i + 1)
        };
        self.tabs.insert(target, tab);
        Some(target)
    }

    /// MoveWebContentsAt — with the pinned invariant re-checked and the
    /// documented contiguity divergence (a move that would strand a group
    /// member ungroups it).
    pub fn move_to(&mut self, id: TabId, to: usize) -> Option<usize> {
        let index = self.index_of(id)?;
        if self.tabs.is_empty() {
            return None;
        }
        let to = to.min(self.tabs.len().saturating_sub(1));
        if index == to {
            return Some(index);
        }
        let group = self.tabs[index].group;
        let tab = self.tabs.remove(index);
        let mut target = to.min(self.tabs.len());
        let edge = self.block_edge(tab.pinned);
        target = if tab.pinned {
            target.min(edge)
        } else {
            target.max(edge)
        };
        self.tabs.insert(target, tab);
        if let Some(group) = group {
            if !self.group_contiguous(group) {
                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                    t.group = None;
                }
            }
        }
        Some(target)
    }

    /// MoveTabNext / MoveTabPrevious.
    pub fn move_relative(&mut self, id: TabId, step: isize) -> Option<usize> {
        let index = self.index_of(id)? as isize;
        let to = (index + step).clamp(0, self.tabs.len() as isize - 1) as usize;
        self.move_to(id, to)
    }

    /// SelectTabAt.
    pub fn select(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.active = Some(id);
        true
    }

    /// SelectNextTab (cyclic).
    pub fn select_next(&mut self) {
        self.select_relative(1);
    }

    /// SelectPreviousTab (cyclic).
    pub fn select_previous(&mut self) {
        self.select_relative(-1);
    }

    fn select_relative(&mut self, step: isize) {
        let Some(active) = self.active else { return };
        let Some(index) = self.index_of(active) else {
            return;
        };
        let len = self.tabs.len() as isize;
        if len < 2 {
            return;
        }
        let next = (index as isize + step).rem_euclid(len) as usize;
        self.active = Some(self.tabs[next].id);
    }

    pub fn select_index(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get(index) {
            self.active = Some(tab.id);
        }
    }

    /// CloseWebContentsAt — records a ClosedTab and repairs the
    /// selection (the neighbor to the right, else the one to the left).
    pub fn close(&mut self, id: TabId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let tab = self.tabs.remove(index);
        if let Some(group) = tab.group {
            self.ungroup_all(&[group]);
        }
        self.closed.push(ClosedTab {
            history: tab.history,
            history_index: tab.history_index,
            pinned: tab.pinned,
            index,
        });
        self.closed.truncate(MAX_CLOSED);
        if self.tabs.is_empty() {
            // See the module doc: a fresh New tab, not a dead window.
            self.append(Destination::New, true);
            return true;
        }
        if self.active == Some(id) {
            let next = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.get(index.saturating_sub(1)));
            self.active = next.map(|t| t.id);
        }
        true
    }

    /// IDC_RESTORE_TAB — pop the newest closed tab back to its remembered
    /// position, pinned-ness intact; select it.
    pub fn reopen_closed(&mut self) -> bool {
        let Some(entry) = self.closed.pop() else {
            return false;
        };
        let id = self.next_tab;
        self.next_tab += 1;
        let index = entry.index.min(self.tabs.len());
        let edge = self.block_edge(entry.pinned);
        let index = if entry.pinned {
            index.min(edge)
        } else {
            index.max(edge)
        };
        self.tabs.insert(
            index,
            Tab {
                id,
                history: entry.history,
                history_index: entry.history_index,
                pinned: entry.pinned,
                group: None,
                opener: None,
                muted: false,
                reload: 0,
            },
        );
        self.active = Some(id);
        true
    }

    /// IDC_DUPLICATE_TAB — a second tab, isolated view, same destination
    /// (§48/§65: the server process is never duplicated).
    pub fn duplicate(&mut self, id: TabId) -> Option<TabId> {
        let index = self.index_of(id)?;
        let source = self.tabs[index].clone();
        let new_id = self.next_tab;
        self.next_tab += 1;
        self.tabs.insert(
            index + 1,
            Tab {
                id: new_id,
                history: source.history.clone(),
                history_index: source.history_index,
                pinned: source.pinned,
                group: None,
                opener: source.opener,
                muted: source.muted,
                reload: 0,
            },
        );
        self.active = Some(new_id);
        Some(new_id)
    }

    /// DetachWebContentsAtForInsertion — the tab leaves the strip as a
    /// self-contained DetachedTab; another strip or window re-inserts it.
    pub fn detach(&mut self, id: TabId) -> Option<(Tab, usize)> {
        let index = self.index_of(id)?;
        let tab = self.tabs.remove(index);
        if let Some(group) = tab.group {
            self.ungroup_all(&[group]);
        }
        if self.active == Some(id) {
            let next = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.get(index.saturating_sub(1)));
            self.active = next.map(|t| t.id);
        }
        if self.tabs.is_empty() {
            self.append(Destination::New, false);
        }
        Some((tab, index))
    }

    /// IDC_ADD_NEW_TAB_TO_GROUP — creating a group compacts its members
    /// contiguously (documented divergence from Chrome's drag-formed
    /// groups).
    pub fn group_create(&mut self, ids: &[TabId], label: &str) -> Option<GroupId> {
        let mut members: Vec<Tab> = Vec::new();
        for id in ids {
            if let Some(index) = self.index_of(*id) {
                members.push(self.tabs.remove(index));
            }
        }
        if members.is_empty() {
            return None;
        }
        let group_id = self.next_group;
        self.next_group += 1;
        let color = (group_id % GROUP_COLORS) as u8;
        self.groups.insert(
            group_id,
            Group {
                id: group_id,
                label: label.to_owned(),
                color,
                collapsed: false,
            },
        );
        let start = self.block_edge(false).min(self.tabs.len());
        // The insert position walks the members — the zip carries the
        // counter instead of a mutable shadow (clippy's counter law).
        for (target, mut member) in (start..).zip(members) {
            member.group = Some(group_id);
            self.tabs.insert(target.min(self.tabs.len()), member);
        }
        Some(group_id)
    }

    // The group-membership API for the menu verbs (a later phase wires
    // group_add to the "move to group" verb; group_remove to ungroup).
    #[allow(dead_code)]
    pub fn group_add(&mut self, group: GroupId, id: TabId) {
        let anchor = self.tabs.iter().position(|t| t.group == Some(group));
        let Some(anchor) = anchor else { return };
        if self.move_to(id, anchor).is_some() {
            if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                t.group = Some(group);
            }
        }
    }

    #[allow(dead_code)]
    pub fn group_remove(&mut self, id: TabId) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
            t.group = None;
        }
    }

    pub fn group_toggle_collapsed(&mut self, group: GroupId) {
        if let Some(g) = self.groups.get_mut(&group) {
            g.collapsed = !g.collapsed;
        }
    }

    /// IDC_CLOSE_TAB_GROUP — close the members left to right; each
    /// remembers its position (v1 divergence: Chrome restores a closed
    /// group as one entry; here every tab is individually reopenable).
    pub fn group_close(&mut self, group: GroupId) -> bool {
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.group == Some(group))
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            return false;
        }
        for id in ids {
            self.close(id);
        }
        true
    }

    fn ungroup_all(&mut self, groups: &[GroupId]) {
        for t in self.tabs.iter_mut() {
            if t.group.is_some_and(|g| groups.contains(&g)) {
                t.group = None;
            }
        }
        let live: Vec<GroupId> = self.tabs.iter().filter_map(|t| t.group).collect();
        self.groups.retain(|g, _| live.contains(g));
    }

    fn group_contiguous(&self, group: GroupId) -> bool {
        let mut first: Option<usize> = None;
        let mut last = 0;
        let mut count = 0;
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.group == Some(group) {
                if first.is_none() {
                    first = Some(i);
                }
                last = i;
                count += 1;
            }
        }
        match first {
            Some(first) => last - first == count - 1,
            None => true,
        }
    }

    /// Navigate the tab: truncate the forward history, push the
    /// destination (§59 per-tab history).
    pub fn navigate(&mut self, id: TabId, destination: Destination) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        tab.history.truncate(tab.history_index + 1);
        tab.history.push(destination);
        tab.history_index = tab.history.len() - 1;
        true
    }

    pub fn back(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        if tab.history_index == 0 {
            return false;
        }
        tab.history_index -= 1;
        true
    }

    pub fn forward(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        if tab.history_index + 1 >= tab.history.len() {
            return false;
        }
        tab.history_index += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_strip_opens_a_new_tab() {
        let strip = Strip::new();
        assert_eq!(strip.tabs.len(), 1);
        assert_eq!(strip.tabs[0].destination(), &Destination::New);
    }

    #[test]
    fn foreground_tab_inherits_the_opener_of_the_active_tab() {
        let mut strip = Strip::new();
        let first = strip.tabs[0].id;
        let second = strip.append(Destination::Settings, true);
        let tab = strip.tabs.iter().find(|t| t.id == second).unwrap();
        assert_eq!(tab.opener, Some(first));
    }

    #[test]
    fn pinned_block_invariant_holds_on_insert() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        strip.set_pinned(a, true);
        strip.set_pinned(b, true);
        // A foreground append lands after the pinned block.
        let c = strip.append(Destination::About, true);
        let index = strip.index_of(c).unwrap();
        assert!(strip.tabs[..index].iter().all(|t| t.pinned));
        assert!(!strip.tabs[index].pinned);
    }

    #[test]
    fn set_pinned_relocates_to_the_block_edge() {
        let mut strip = Strip::new();
        let _a = strip.append(Destination::Servers, true);
        let _b = strip.append(Destination::Jobs, true);
        let c = strip.append(Destination::About, true);
        let result = strip.set_pinned(c, true).unwrap();
        assert_eq!(strip.tabs[result].id, c);
        assert_eq!(strip.tabs.iter().filter(|t| t.pinned).count(), 1);
    }

    #[test]
    fn closing_the_active_tab_selects_the_neighbor() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        strip.select(b);
        strip.close(b);
        assert_eq!(strip.active, Some(a));
    }

    #[test]
    fn last_tab_close_leaves_a_fresh_new_tab() {
        let mut strip = Strip::new();
        let a = strip.tabs[0].id;
        strip.close(a);
        assert_eq!(strip.tabs.len(), 1);
        assert_eq!(strip.tabs[0].destination(), &Destination::New);
        assert_eq!(strip.closed.len(), 1);
    }

    #[test]
    fn reopen_puts_the_tab_back_where_it_was() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        strip.select(a);
        strip.close(a);
        let before = strip.tabs.len();
        assert!(strip.reopen_closed());
        assert_eq!(strip.tabs.len(), before + 1);
        assert_eq!(
            strip.active,
            Some(strip.tabs[before.min(strip.tabs.len() - 1)].id)
        );
    }

    #[test]
    fn duplicate_clones_the_view_never_the_process() {
        let mut strip = Strip::new();
        let a = strip.append(
            Destination::Server {
                server_id: "s1".into(),
            },
            true,
        );
        strip.navigate(
            a,
            Destination::Console {
                server_id: "s1".into(),
            },
        );
        let dup = strip.duplicate(a).unwrap();
        let tab = strip.tabs.iter().find(|t| t.id == dup).unwrap();
        assert_eq!(
            tab.destination(),
            &Destination::Console {
                server_id: "s1".into()
            }
        );
        assert_eq!(tab.history.len(), 2);
    }

    #[test]
    fn groups_are_contiguous_and_close_together() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        let c = strip.append(Destination::About, true);
        let group = strip.group_create(&[a, b, c], "docs").unwrap();
        assert!(strip.group_contiguous(group));
        let _ = strip.group_close(group);
        assert!(strip.tabs.iter().all(|t| t.group.is_none()));
        assert!(strip.reopen_closed());
    }

    #[test]
    fn navigation_truncates_forward_history() {
        let mut strip = Strip::new();
        let a = strip.tabs[0].id;
        strip.navigate(a, Destination::Servers);
        strip.navigate(a, Destination::Settings);
        assert!(strip.back(a));
        assert!(strip.forward(a));
        strip.back(a);
        strip.navigate(a, Destination::Jobs);
        let tab = strip.tabs.iter().find(|t| t.id == a).unwrap();
        assert_eq!(tab.history.len(), 3); // New → Jobs; the forward arm is gone
    }

    #[test]
    fn detach_returns_a_reinsertable_tab() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let (tab, _index) = strip.detach(a).unwrap();
        assert_eq!(tab.destination(), &Destination::Servers);
        assert!(!strip.tabs.is_empty()); // the strip repaired itself
    }

    #[test]
    fn url_roundtrip() {
        let d = Destination::Server {
            server_id: "abc".into(),
        };
        assert_eq!(Destination::parse(&d.url()), d);
        assert_eq!(
            Destination::parse("zim://nonsense/x"),
            Destination::Missing {
                url: "zim://nonsense/x".into()
            }
        );
        assert_eq!(
            Destination::parse("zim://console/s1"),
            Destination::Console {
                server_id: "s1".into()
            }
        );
    }
}
