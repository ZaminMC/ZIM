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
use std::collections::{BTreeSet, HashMap};

pub type TabId = u32;
pub type GroupId = u32;

/// The closed set of places a tab can show (§58) — the Rust twin of
/// state/destinations.ts. `zim://` URLs canonicalize through
/// [`Destination::url`] / [`Destination::parse`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Destination {
    New,
    DevTools,
    Servers,
    Server {
        server_id: String,
    },
    Console {
        server_id: String,
    },
    /// A join address the operator typed (§7): the shell lands it on its
    /// own destination and the Join page consults the daemon — the
    /// registry and a server-list ping — for the verdict. Never a raw
    /// webview navigation; this is a Minecraft server browser.
    Join {
        host: Option<String>,
        port: u16,
    },
    Settings,
    Jobs,
    Audit,
    About,
    Feedback,
    Extensions,
    Downloads,
    Missing {
        url: String,
    },
}

impl Destination {
    /// destinationUrl() — state/destinations.ts.
    pub fn url(&self) -> String {
        match self {
            Destination::New => "zim://new".into(),
            Destination::DevTools => "zim://devtools/".into(),
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
            Destination::Join { host, port } => {
                format!("zim://join/{}:{port}", host.clone().unwrap_or_default())
            }
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
            "devtools" => Destination::DevTools,
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
            // "host:port" — the host side may be empty (a port-only join).
            // Anything unparseable stays an honest Missing.
            "join" if !arg.is_empty() => match arg.rsplit_once(':') {
                Some((host, port)) => port
                    .parse::<u16>()
                    .ok()
                    .map(|port| Destination::Join {
                        host: if host.is_empty() {
                            None
                        } else {
                            Some(host.to_owned())
                        },
                        port,
                    })
                    .unwrap_or_else(|| Destination::Missing {
                        url: text.to_owned(),
                    }),
                None => Destination::Missing {
                    url: text.to_owned(),
                },
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
            Destination::DevTools => "Developer tools".into(),
            Destination::Servers => "Fleet".into(),
            Destination::Server { server_id } => format!("Server {server_id}"),
            Destination::Console { server_id } => format!("Console {server_id}"),
            Destination::Join { host, port } => match host {
                Some(host) => format!("Join {host}:{port}"),
                None => format!("Join port {port}"),
            },
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
    /// The tab's contents zoom factor (Chromium's per-contents zoom).
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// The discovery query waiting for this tab's page (the omnibox's
    /// search dialect). The tab may not HAVE a webview when the query
    /// lands — Alt-Enter's fresh foreground tab boots after the sync —
    /// so the model holds the text and hello delivers it at boot; a live
    /// page also hears the event. A reload re-delivers (the search
    /// survives a reload, as upstream's results page does); a restored
    /// session never resurrects one — the field is in-memory only.
    #[serde(skip)]
    pub pending_query: Option<String>,
}

fn default_zoom() -> f32 {
    1.0
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
            zoom: 1.0,
            pending_query: None,
        }
    }
}

/// Chromium's zoom ladder (components/zoom/zoom_controller.cc's preset
/// levels): zoom moves through these factors, never arbitrary values.
const ZOOM_LADDER: [f32; 16] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// One step along the ladder: +1 zooms in, -1 zooms out, anything else
/// resets. An unknown current factor lands on 1.0's rung first.
pub fn zoom_step(current: f32, direction: i32) -> f32 {
    let rung = ZOOM_LADDER
        .iter()
        .position(|z| (*z - current).abs() < 0.001)
        .unwrap_or(6);
    match direction {
        1 => ZOOM_LADDER[(rung + 1).min(ZOOM_LADDER.len() - 1)],
        -1 => ZOOM_LADDER[rung.saturating_sub(1)],
        _ => 1.0,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub label: String,
    /// Index into Chromium's group color enum (tab_group_color.h,
    /// TabGroupColorId — the wire format values are written to disk, so
    /// the index semantics never change).
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
    /// Chromium's tab multi-selection — tabs::TabStripModelSelectionState's
    /// surviving shape (selected set + anchor), stored by STABLE TAB ID so
    /// moves and inserts never need the IncrementFrom/DecrementFrom index
    /// shuffles upstream needs. THE INVARIANT (upstream CHECKs it —
    /// TabStripModelSelectionState::Valid): the ACTIVE tab is always
    /// selected; an empty selection means no active either.
    #[serde(default)]
    pub selection: BTreeSet<TabId>,
    /// The anchor: the index the extension gestures reach from
    /// (ui::ListSelectionModel's anchor — "the last item the user clicked
    /// on"). Empty when nothing is selected.
    #[serde(default)]
    pub anchor: Option<TabId>,
    pub groups: HashMap<GroupId, Group>,
    pub next_tab: TabId,
    pub next_group: GroupId,
    pub closed: Vec<ClosedTab>,
    /// §54 (ADR-0026): the strip's presentation axis. A per-window pref
    /// persisted with the session; `default` keeps sessions written
    /// before the field existed horizontal. Nothing but the VIEW reads
    /// it — no identity rule, no test seam changes shape.
    #[serde(default)]
    pub vertical: bool,
}

pub const MAX_CLOSED: usize = 25;
/// tab_groups::TabGroupColorId::kNumEntries — grey, blue, red, yellow,
/// green, pink, purple, cyan, orange (components/tab_groups/tab_group_color.h).
/// New group ids cycle the enum; the frame resolves the index through the
/// backported palette (ui/chromium/chromiumTabs.ts).
pub const GROUP_COLORS: u32 = 9;

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
            selection: BTreeSet::new(),
            anchor: None,
            groups: HashMap::new(),
            next_tab: 1,
            next_group: 1,
            closed: Vec::new(),
            vertical: false,
        };
        strip.append(Destination::New, true);
        strip
    }

    /// TabStripModel::SetSelectedTab — the ONE activation law: the
    /// selection collapses to the tab and the anchor rides with it
    /// ("typically there is only one selected item, in which case the
    /// anchor and active correspond" — list_selection_model.h). Every
    /// activation path routes through here; the multi-selection gestures
    /// below are the only writers that bypass it.
    fn activate(&mut self, id: TabId) {
        self.active = Some(id);
        self.selection.clear();
        self.selection.insert(id);
        self.anchor = Some(id);
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
            self.activate(id);
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

    /// The drag drop's reorder (TabDragController's move semantics): the
    /// dragged tab is conceptually lifted OUT of the strip — the
    /// insertion index runs over the REMAINING tabs — and
    /// [`Strip::move_to`] already interprets its target exactly that way
    /// (remove, clamp, insert). On top of the move, the drop policies:
    /// a pinned tab dropped at or after the unpinned block UNPINS
    /// (upstream's drag-out-of-block unpin; the reverse never pins — an
    /// unpinned tab dropped into the pinned block lands at its edge,
    /// move_to's clamp).
    ///
    /// A SELECTED drop rides the block: when the source stands in a
    /// multi-selection, the drop is MoveSelectedTabsTo's law
    /// ([`Strip::move_selected_to`]) — the whole selection lands as one
    /// block (MaybeStartDrag dragged them together, the drop moves them
    /// together) — and the unpin policy rides with the SOURCE: a pinned
    /// source dropped past the block's edge unpins every selected pinned
    /// tab (they crossed the boundary together).
    pub fn reorder_drop(&mut self, id: TabId, index_among_others: usize) -> Option<usize> {
        let block = self.selection.contains(&id) && self.selection.len() > 1;
        let pinned = self
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.pinned)
            .unwrap_or(false);
        if pinned && index_among_others >= self.block_edge(true) {
            if block {
                let pinned_selected: Vec<TabId> = self
                    .tabs
                    .iter()
                    .filter(|t| t.pinned && self.selection.contains(&t.id))
                    .map(|t| t.id)
                    .collect();
                for tab_id in pinned_selected {
                    self.set_pinned(tab_id, false);
                }
            } else {
                self.set_pinned(id, false);
            }
        }
        if block {
            self.move_selected_to(index_among_others)?;
            return self.index_of(id);
        }
        self.move_to(id, index_among_others)
    }

    /// TabStripModel::MoveSelectedTabsTo (tab_strip_model.cc:1079) — the
    /// drag's block move. The selection splits into its pinned and
    /// unpinned classes (strip order, by stable id), and each class
    /// lands CONTIGUOUS at its own clamped destination:
    /// `last_pinned = clamp(index + n_p - 1, n_p - 1, pinned_count - 1)`
    /// puts the pinned class inside the pinned region; the unpinned
    /// class lands from `clamp(index + n_p, pinned_count, count - n_u)`
    /// on. The bounds read the PRE-move geometry (the unittest's own
    /// arithmetic — the 20-case matrix in tab_strip_model_unittest.cc
    /// pins the law verbatim); the insert lands the class in one piece
    /// over the strip with the class lifted out, which is exactly the
    /// net the upstream move chain produces. The group law rides after
    /// each class: a moved tab whose group no longer contiguous
    /// ungroups (the same divergence [`Strip::move_to`] documents; the
    /// unittest's part-group case keeps the group alive with the
    /// stay-put member). The law never unpins — the drop policy above
    /// owns that decision.
    pub fn move_selected_to(&mut self, index: usize) -> Option<()> {
        let pinned_count = self.block_edge(true);
        let pinned_selected: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.pinned && self.selection.contains(&t.id))
            .map(|t| t.id)
            .collect();
        let unpinned_selected: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| !t.pinned && self.selection.contains(&t.id))
            .map(|t| t.id)
            .collect();
        if pinned_selected.is_empty() && unpinned_selected.is_empty() {
            return None;
        }
        let count_before = self.tabs.len() as isize;
        let n_p = pinned_selected.len() as isize;
        let n_u = unpinned_selected.len() as isize;
        if n_p > 0 {
            let last_pinned = (index as isize + n_p - 1).clamp(n_p - 1, pinned_count as isize - 1);
            let dest = (last_pinned - n_p + 1) as usize;
            self.move_block(&pinned_selected, dest);
        }
        if n_u > 0 {
            let first_unpinned =
                (index as isize + n_p).clamp(pinned_count as isize, count_before - n_u);
            self.move_block(&unpinned_selected, first_unpinned as usize);
        }
        Some(())
    }

    /// One class's landing: the class lifts out in strip order, the
    /// remainder closes the gap, the class re-inserts at `dest` in one
    /// piece — then the group law: a moved tab whose group is no longer
    /// contiguous leaves it (the stay-put members keep the group).
    fn move_block(&mut self, ids: &[TabId], dest: usize) {
        let mut block = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == *id) {
                block.push(self.tabs.remove(pos));
            }
        }
        let dest = dest.min(self.tabs.len());
        for (offset, tab) in block.into_iter().enumerate() {
            self.tabs.insert(dest + offset, tab);
        }
        for id in ids {
            let Some(group) = self.tabs.iter().find(|t| t.id == *id).and_then(|t| t.group) else {
                continue;
            };
            if !self.group_contiguous(group) {
                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == *id) {
                    t.group = None;
                }
            }
        }
    }

    /// SelectTabAt — the PLAIN click's law (Tab::OnMousePressed fires it
    /// only when the tab is not already selected): the selection
    /// collapses to the one tab, the anchor rides with it.
    pub fn select(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.activate(id);
        true
    }

    /// Tab::OnMousePressed's ctrl branch → BrowserTabStripController::
    /// ToggleSelected → TabStripModel::{SelectTabAt, DeselectTabAt}.
    /// ADDING joins the selection and moves the anchor; the ACTIVE stays
    /// (documented delta: current main's SelectTabAt also SetActiveTabs
    /// the clicked tab — tab_strip_model.cc:1567, the split refactor's
    /// detail; the model's own unittests treat Select and Activate as
    /// distinct verbs and never pin an activation in the selection
    /// tests, so the classic non-activating toggle stands here, flagged
    /// for the Windows eyeball pass). REMOVING refuses the last
    /// selection ("one tab must be selected" — DeselectTabAt) and
    /// promotes the FIRST SELECTED when the active or anchor leaves.
    pub fn toggle_selection(&mut self, id: TabId) -> bool {
        let Some(_index) = self.index_of(id) else {
            return false;
        };
        if self.selection.contains(&id) {
            if self.selection.len() == 1 {
                return false;
            }
            self.selection.remove(&id);
            if self.active == Some(id) || self.active.is_none() {
                let first = *self.selection.iter().next().expect("non-empty above");
                self.active = Some(first);
            }
            if self.anchor.is_none() || self.anchor == Some(id) {
                self.anchor = self.active;
            }
        } else {
            self.selection.insert(id);
            self.anchor = Some(id);
        }
        true
    }

    /// TabStripModel::ExtendSelectionTo — the shift branch: the range
    /// from the anchor REPLACES the selection; the clicked tab becomes
    /// the active; the anchor stays (the range keeps reaching from the
    /// same origin while shift-walking).
    pub fn extend_selection(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        let Some(anchor) = self.anchor else {
            // "If the anchor is empty, this sets the anchor, selection
            // and active to |index|" (SetSelectionFromAnchorTo's law —
            // the same answer SetSelectedTab gives).
            self.activate(id);
            return true;
        };
        let (start, end) = match (self.index_of(anchor), self.index_of(id)) {
            (Some(a), Some(b)) if a <= b => (a, b),
            (Some(a), Some(b)) => (b, a),
            _ => return false,
        };
        self.selection.clear();
        for tab in &self.tabs[start..=end] {
            self.selection.insert(tab.id);
        }
        self.active = Some(id);
        true
    }

    /// TabStripModel::AddSelectionFromAnchorTo — the shift+ctrl branch:
    /// the range ADDS to the standing selection (the anchor keeps its
    /// origin — list_selection_model.cc: "this does not change the
    /// anchor"), the clicked tab becomes the active.
    pub fn add_selection_from_anchor_to(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        let Some(anchor) = self.anchor else {
            self.activate(id);
            return true;
        };
        let (start, end) = match (self.index_of(anchor), self.index_of(id)) {
            (Some(a), Some(b)) if a <= b => (a, b),
            (Some(a), Some(b)) => (b, a),
            _ => return false,
        };
        for tab in &self.tabs[start..=end] {
            self.selection.insert(tab.id);
        }
        self.active = Some(id);
        true
    }

    /// TabStripModel::GetIndicesForCommand — the context menu's scope
    /// law: a SELECTED context tab commands the whole selection, an
    /// unselected one, itself alone. (Upstream also expands a split;
    /// ZIM has no splits.)
    pub fn indices_for_command(&self, id: TabId) -> Vec<TabId> {
        if self.selection.contains(&id) {
            self.selection.iter().copied().collect()
        } else {
            vec![id]
        }
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
        self.activate(self.tabs[next].id);
    }

    pub fn select_index(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get(index) {
            self.activate(tab.id);
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
        // The removal path's selection maintenance (the close-side law of
        // TabStripModelSelectionState — RemoveTabFromSelection, then the
        // FIRST SELECTED is promoted when the active left; the
        // single-select case empties the set and the neighbor law speaks).
        self.selection.remove(&id);
        if self.active == Some(id) {
            if self.selection.is_empty() {
                // Single-select: the neighbor to the right, else the one
                // to the left, and the selection collapses to it.
                let next = self
                    .tabs
                    .get(index)
                    .or_else(|| self.tabs.get(index.saturating_sub(1)));
                if let Some(next_id) = next.map(|t| t.id) {
                    self.activate(next_id);
                }
            } else {
                // Multi-selection: the FIRST SELECTED survivor is
                // promoted — active and anchor both ride to it, the rest
                // of the selection survives (upstream's removal law).
                let first = *self.selection.iter().next().expect("non-empty above");
                self.active = Some(first);
                self.anchor = Some(first);
            }
        }
        // The anchor rode the closed tab without the active — the active
        // replaces it.
        if self.anchor == Some(id) {
            self.anchor = self.active;
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
                zoom: 1.0,
                pending_query: None,
            },
        );
        self.activate(id);
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
                zoom: source.zoom,
                // A duplicated search tab re-runs the search (upstream's
                // duplicate carries the results URL; ours carries the
                // query that produces it).
                pending_query: source.pending_query.clone(),
            },
        );
        self.active = Some(new_id);
        self.selection.clear();
        self.selection.insert(new_id);
        self.anchor = Some(new_id);
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
        // The same removal maintenance close() runs — the selection loses
        // the tab, the neighbor law speaks when the active left.
        self.selection.remove(&id);
        if self.active == Some(id) {
            let next = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.get(index.saturating_sub(1)));
            match next.map(|t| t.id) {
                Some(_) if !self.selection.is_empty() => {
                    let first = *self.selection.iter().next().expect("non-empty above");
                    self.active = Some(first);
                    self.anchor = Some(first);
                }
                Some(next_id) => {
                    self.activate(next_id);
                }
                None => {
                    self.active = self.selection.iter().next().copied();
                    self.anchor = self.active;
                }
            }
        } else if self.anchor == Some(id) {
            self.anchor = self.active;
        }
        if self.tabs.is_empty() {
            self.append(Destination::New, false);
        }
        Some((tab, index))
    }

    /// The drag block — MaybeStartDrag's law at the model boundary: the
    /// tab under the hand carries its whole selection when it stands in
    /// a multi-selection, itself otherwise. The drop verdict
    /// ([`Strip::reorder_drop`]) and the between-windows / tear-off
    /// detach both speak through this one set, so a drag can never
    /// disagree about WHAT is being carried (the audit's own case: the
    /// old cross-window path detached one tab out of a selected block).
    pub fn drag_block(&self, id: TabId) -> Vec<TabId> {
        if self.selection.contains(&id) && self.selection.len() > 1 {
            self.selection.iter().copied().collect()
        } else {
            vec![id]
        }
    }

    /// Detach the whole block: every id leaves in STRIP ORDER as
    /// DetachedTabs, ready to re-insert as a block in another window or
    /// to boot a tear-off window with. The removal maintenance runs ONCE
    /// for the block — per-tab `detach` calls would fire the neighbor
    /// law and mint the empty-strip New tab between removals (a block
    /// pulled from a 3-tab strip would leave TWO fresh New tabs). Groups
    /// the moving tabs belonged to die on the source — the same law the
    /// single [`Strip::detach`] speaks through `ungroup_all`. An emptied
    /// strip gets its fresh New tab, foreground off: the next
    /// activation belongs to the operator, not the model.
    pub fn detach_block(&mut self, ids: &[TabId]) -> Vec<Tab> {
        let wanted: std::collections::HashSet<TabId> = ids.iter().copied().collect();
        let mut block = Vec::with_capacity(ids.len());
        let mut survivors = Vec::with_capacity(self.tabs.len());
        let mut first_removed_index: Option<usize> = None;
        let mut groups: Vec<GroupId> = Vec::new();
        for (index, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if wanted.contains(&tab.id) {
                if first_removed_index.is_none() {
                    first_removed_index = Some(index);
                }
                if let Some(group) = tab.group {
                    groups.push(group);
                }
                block.push(tab);
            } else {
                survivors.push(tab);
            }
        }
        self.tabs = survivors;
        groups.sort_unstable();
        groups.dedup();
        self.ungroup_all(&groups);
        for id in ids {
            self.selection.remove(id);
        }
        if self.active.is_some_and(|active| wanted.contains(&active)) {
            // The active tab left with the block: the FIRST SELECTED
            // survivor is promoted (close's multi-selection law); with
            // no survivor left, the neighbor where the block stood is
            // ACTIVATED — activate, not a raw active write, so the
            // strip's own invariant (the active tab is always selected)
            // survives the removal. Nothing left at all: the empty-strip
            // New tab below takes the activation through append's rule.
            if let Some(first) = self.selection.iter().next().copied() {
                self.active = Some(first);
                self.anchor = Some(first);
            } else {
                let at = first_removed_index.unwrap_or(0);
                let next = self
                    .tabs
                    .get(at)
                    .or_else(|| self.tabs.get(at.saturating_sub(1)))
                    .map(|t| t.id);
                match next {
                    Some(next_id) => self.activate(next_id),
                    None => {
                        self.active = None;
                        self.anchor = None;
                    }
                }
            }
        }
        if self.anchor.is_some_and(|anchor| wanted.contains(&anchor)) {
            self.anchor = self.active;
        }
        if self.tabs.is_empty() {
            self.append(Destination::New, false);
        }
        block
    }

    /// The moved block's landing selection: the hand's tab activates and
    /// anchors, the rest of the block rides selected — Chromium's moved
    /// tabs arrive in the target window still selected, with the tab the
    /// hand carried as the active one (tab_strip_model's insert law; the
    /// single-tab case collapses to a plain activation).
    pub fn select_block(&mut self, anchor: TabId, ids: &[TabId]) {
        self.activate(anchor);
        for id in ids {
            if self.index_of(*id).is_some() {
                self.selection.insert(*id);
            }
        }
        self.anchor = Some(anchor);
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

    // The group-membership API for the editor + menu verbs.
    /// "Add to existing group" (tab_menu_model.cc's per-group submenu
    /// items): the tab moves adjacent to the group's LAST member and
    /// joins it; a collapsed group expands — a newcomer must be seen
    /// (upstream's add-to-group law).
    pub fn group_add(&mut self, group: GroupId, id: TabId) {
        if !self.groups.contains_key(&group) {
            return;
        }
        let Some(from) = self.index_of(id) else {
            return;
        };
        let mut tab = self.tabs.remove(from);
        tab.group = Some(group);
        tab.pinned = false;
        // Recompute the anchor AFTER the removal — the mover may itself
        // be a member, and its old slot shifted everyone.
        let anchor = self
            .tabs
            .iter()
            .rposition(|t| t.group == Some(group))
            .map(|last| last + 1)
            .unwrap_or_else(|| self.block_edge(false).min(self.tabs.len()));
        self.tabs.insert(anchor.min(self.tabs.len()), tab);
        if let Some(g) = self.groups.get_mut(&group) {
            g.collapsed = false;
        }
    }

    /// "Remove tab from group" (the tab menu's CommandRemoveFromGroup).
    /// The LAST member out destroys the group — upstream groups never
    /// exist empty.
    pub fn group_remove(&mut self, id: TabId) {
        let former = {
            let mut former = None;
            for t in self.tabs.iter_mut() {
                if t.id == id {
                    former = t.group.take();
                    break;
                }
            }
            former
        };
        if let Some(group) = former {
            let still_members = self.tabs.iter().any(|t| t.group == Some(group));
            if !still_members {
                self.groups.remove(&group);
            }
        }
    }

    /// The editor's name field (tab_group_editor_bubble_view.cc's title
    /// controller): the model takes every keystroke. An EMPTY title is
    /// legal — upstream chips render the color alone.
    pub fn group_rename(&mut self, group: GroupId, label: &str) {
        if let Some(g) = self.groups.get_mut(&group) {
            g.label = label.to_owned();
        }
    }

    /// The editor's color grid (color_picker_view.cc): one circle per
    /// TabGroupColorId in the enum's wire order; an out-of-range index
    /// is refused, never wrapped.
    pub fn group_set_color(&mut self, group: GroupId, color: u8) -> bool {
        if (color as u32) >= GROUP_COLORS {
            return false;
        }
        match self.groups.get_mut(&group) {
            Some(g) => {
                g.color = color;
                true
            }
            None => false,
        }
    }

    /// The editor's "Ungroup": every member leaves (their histories and
    /// positions stay), the group dies.
    pub fn group_ungroup(&mut self, group: GroupId) -> bool {
        if !self.groups.contains_key(&group) {
            return false;
        }
        for t in self.tabs.iter_mut() {
            if t.group == Some(group) {
                t.group = None;
            }
        }
        self.groups.remove(&group);
        true
    }

    /// The editor's "New tab in group": a fresh New tab joins the
    /// group's end and takes focus.
    pub fn group_new_tab(&mut self, group: GroupId) -> Option<TabId> {
        if !self.groups.contains_key(&group) {
            return None;
        }
        let id = self.next_tab;
        self.next_tab += 1;
        let mut tab = Tab::fresh(id, Destination::New, None);
        tab.group = Some(group);
        let anchor = self
            .tabs
            .iter()
            .rposition(|t| t.group == Some(group))
            .map(|last| last + 1)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(anchor.min(self.tabs.len()), tab);
        if let Some(g) = self.groups.get_mut(&group) {
            g.collapsed = false;
        }
        self.activate(id);
        Some(id)
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

    /// CloseWebContentsAt's scoped siblings from the tab menu: close
    /// every OTHER tab (IDC close context: "Close other tabs"), or every
    /// tab to the right of this one. Each close rides [`Strip::close`],
    /// so the closed set and selection repair come for free.
    pub fn close_others(&mut self, id: TabId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        let victims: Vec<TabId> = self
            .tabs
            .iter()
            .map(|t| t.id)
            .filter(|t| *t != id)
            .collect();
        for victim in victims {
            self.close(victim);
        }
        self.select(id);
        true
    }

    pub fn close_to_right(&mut self, id: TabId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let victims: Vec<TabId> = self.tabs[index + 1..].iter().map(|t| t.id).collect();
        if victims.is_empty() {
            return false;
        }
        for victim in victims {
            self.close(victim);
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

    // -- the multi-selection laws (Tab::OnMousePressed's dispatch, the
    //    model's SelectTabAt/DeselectTabAt/ExtendSelectionTo/
    //    AddSelectionFromAnchorTo, and GetIndicesForCommand) ------------

    /// A fresh strip with N foreground tabs; returns the ids in order.
    fn strip_with(n: usize) -> (Strip, Vec<TabId>) {
        let mut strip = Strip::new();
        let mut ids = vec![strip.tabs[0].id];
        for _ in 1..n {
            ids.push(strip.append(Destination::New, true));
        }
        (strip, ids)
    }

    #[test]
    fn the_plain_click_collapses_the_selection_to_the_clicked_tab() {
        let (mut strip, ids) = strip_with(5);
        // ctrl-walk: the active gains company.
        assert!(strip.toggle_selection(ids[2]));
        assert_eq!(strip.selection.len(), 2); // the active + the toggled
                                              // A second ctrl-click on the ACTIVE deselects it — the first
                                              // selected is promoted (DeselectTabAt's law, size > 1 branch).
        assert!(strip.toggle_selection(ids[4]));
        assert_eq!(strip.selection, BTreeSet::from([ids[2]]));
        assert_eq!(strip.active, Some(ids[2]));
        // Ctrl-walk two more in, then the plain click on an unselected
        // tab collapses everything to it.
        assert!(strip.toggle_selection(ids[4]));
        assert!(strip.toggle_selection(ids[1]));
        assert_eq!(strip.selection.len(), 3);
        assert!(strip.select(ids[3]));
        assert_eq!(strip.selection, BTreeSet::from([ids[3]]));
        assert_eq!(strip.active, Some(ids[3]));
        assert_eq!(strip.anchor, Some(ids[3]));
    }

    #[test]
    fn the_ctrl_click_toggles_without_touching_the_active() {
        let (mut strip, ids) = strip_with(5);
        assert_eq!(strip.active, Some(ids[4])); // the last append won
        assert!(strip.toggle_selection(ids[1]));
        assert!(strip.toggle_selection(ids[2]));
        assert_eq!(strip.active, Some(ids[4])); // the active NEVER moved
        assert_eq!(strip.selection, BTreeSet::from([ids[4], ids[1], ids[2]]));
        // The anchor rides the last toggled tab (the extension's origin).
        assert_eq!(strip.anchor, Some(ids[2]));
    }

    #[test]
    fn the_ctrl_click_refuses_to_deselect_the_last_tab_standing() {
        let (mut strip, ids) = strip_with(3);
        // The active is the only selected tab — DeselectTabAt's law.
        assert!(!strip.toggle_selection(ids[2]));
        assert_eq!(strip.selection, BTreeSet::from([ids[2]]));
        // With company, the active CAN leave — the first selected is
        // promoted (DeselectTabAt's promotion law).
        assert!(strip.toggle_selection(ids[0]));
        assert!(strip.toggle_selection(ids[2]));
        assert_eq!(strip.active, Some(ids[0]));
        assert_eq!(strip.selection, BTreeSet::from([ids[0]]));
    }

    #[test]
    fn the_shift_click_replaces_the_selection_with_the_anchor_range() {
        let (mut strip, ids) = strip_with(5);
        assert_eq!(strip.active, Some(ids[4]));
        // Shift from the active (the anchor) to an earlier tab.
        assert!(strip.extend_selection(ids[1]));
        assert_eq!(
            strip.selection,
            BTreeSet::from([ids[4], ids[3], ids[2], ids[1]])
        );
        assert_eq!(strip.active, Some(ids[1]));
        // The anchor NEVER moved — a second shift-walk re-derives the
        // range from the same origin (ExtendSelectionTo's law).
        assert_eq!(strip.anchor, Some(ids[4]));
        assert!(strip.extend_selection(ids[3]));
        assert_eq!(strip.selection, BTreeSet::from([ids[4], ids[3]]));
    }

    #[test]
    fn the_shift_ctrl_click_adds_the_range_to_the_standing_selection() {
        let (mut strip, ids) = strip_with(6);
        // The ctrl-click moves the ANCHOR to the clicked tab — the
        // extension's origin is the last tab the user clicked.
        assert!(strip.toggle_selection(ids[0]));
        assert_eq!(strip.anchor, Some(ids[0]));
        assert!(strip.add_selection_from_anchor_to(ids[3]));
        // The range 0..3 joins the standing {ids[5]}.
        assert_eq!(
            strip.selection,
            BTreeSet::from([ids[0], ids[1], ids[2], ids[3], ids[5]])
        );
        assert_eq!(strip.active, Some(ids[3]));
        // The anchor kept its origin (AddSelectionFromAnchorTo "does not
        // change the anchor").
        assert_eq!(strip.anchor, Some(ids[0]));
    }

    #[test]
    fn a_selected_context_tab_commands_the_whole_selection() {
        let (mut strip, ids) = strip_with(4);
        assert!(strip.toggle_selection(ids[1]));
        assert_eq!(
            strip.indices_for_command(ids[3]),
            // selected → the whole selection, ascending (BTreeSet order)
            vec![ids[1], ids[3]]
        );
        assert_eq!(strip.indices_for_command(ids[2]), vec![ids[2]]); // unselected → itself
    }

    #[test]
    fn closing_the_active_of_a_selection_promotes_the_first_selected() {
        let (mut strip, ids) = strip_with(5);
        assert!(strip.toggle_selection(ids[1]));
        assert!(strip.toggle_selection(ids[2])); // selection {4,1,2}, active 4
        assert!(strip.close(ids[4])); // the active dies
        assert_eq!(strip.active, Some(ids[1])); // the FIRST SELECTED survives
        assert_eq!(strip.selection, BTreeSet::from([ids[1], ids[2]]));
        assert_eq!(strip.anchor, Some(ids[1]));
        // The single-select case keeps the neighbor law.
        let (mut strip, ids) = strip_with(3);
        assert!(strip.close(ids[2])); // the active, alone in its selection
        assert_eq!(strip.active, Some(ids[1]));
        assert_eq!(strip.selection, BTreeSet::from([ids[1]]));
    }

    #[test]
    fn the_invariant_survives_every_entry_path() {
        let (mut strip, _ids) = strip_with(4);
        // Append/insert/reopen/duplicate each activate — the selection
        // collapses to the new active every time.
        let dup = strip.duplicate(strip.tabs[0].id).expect("duplicates");
        assert_eq!(strip.selection, BTreeSet::from([dup]));
        strip.select_next();
        assert_eq!(strip.selection.len(), 1);
        assert!(strip.selection.contains(&strip.active.expect("a tab")));
        strip.reopen_closed();
        assert_eq!(strip.selection.len(), 1);
        assert!(strip.selection.contains(&strip.active.expect("a tab")));
        strip.append(Destination::New, true);
        assert_eq!(strip.selection.len(), 1);
        assert!(strip.selection.contains(&strip.active.expect("a tab")));
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
        // The fresh strip's own New tab stays unpinned, so the law under
        // test is the BLOCK's contiguity, not "everything before the new
        // tab is pinned" — an unpinned tab may lawfully sit between the
        // block and an end-append (Chrome appends at the end; the clamp
        // only forbids landing INSIDE the block).
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        strip.set_pinned(a, true);
        strip.set_pinned(b, true);
        let c = strip.append(Destination::About, true);
        let edge = strip.block_edge(false);
        // The pinned block is a contiguous prefix…
        assert!(strip.tabs[..edge].iter().all(|t| t.pinned));
        assert!(!strip.tabs[edge].pinned);
        // …and the insert never lands inside it.
        let index = strip.index_of(c).unwrap();
        assert!(index >= edge);
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
    fn reorder_drop_lifts_the_tab_out_before_inserting() {
        let mut strip = Strip::new();
        let fresh = strip.tabs[0].id;
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        let c = strip.append(Destination::About, true);
        // Drag a past all the others: the insertion index runs over the
        // REMAINING tabs [fresh, b, c], so 3 lands after c.
        strip.reorder_drop(a, 3);
        let order: Vec<TabId> = strip.tabs.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![fresh, b, c, a]);
    }

    #[test]
    fn a_pinned_tab_dropped_beyond_the_block_unpins() {
        let mut strip = Strip::new();
        // The fresh New tab joins the pinned block, so the block edge is
        // exactly where the pinned tabs end.
        let fresh = strip.tabs[0].id;
        strip.set_pinned(fresh, true);
        let a = strip.append(Destination::Servers, true);
        // b exists to make the block's edge honest (its id is never read).
        let _b = strip.append(Destination::Jobs, true);
        strip.set_pinned(a, true);
        // Block [fresh, a], edge 2; dropping a at 2 lands it at/after the
        // unpinned block → unpin, and it rides to the end.
        strip.reorder_drop(a, 2);
        let tab = strip.tabs.iter().find(|t| t.id == a).unwrap();
        assert!(!tab.pinned);
        assert_eq!(strip.tabs.last().unwrap().id, a);
    }

    #[test]
    fn a_pinned_tab_dropped_inside_the_block_stays_pinned() {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let b = strip.append(Destination::Jobs, true);
        // c exists to put the block edge at 2 (its id is never read).
        let _c = strip.append(Destination::About, true);
        strip.set_pinned(a, true);
        strip.set_pinned(b, true);
        // Block [a, b], edge 2. Drop b at 0 among the others → still
        // pinned, reordered within the block.
        strip.reorder_drop(b, 0);
        let tab = strip.tabs.iter().find(|t| t.id == b).unwrap();
        assert!(tab.pinned);
        assert_eq!(strip.tabs[0].id, b);
    }

    // -- MoveSelectedTabsTo: the block move (tab_strip_model.cc:1079).
    // The harness is upstream's own (tab_strip_model_test_utils.cc's
    // PrepareTabstripForSelectionTest): tab_count tabs, the first
    // pinned_count pinned, exactly `selected` selected, the anchor on
    // the first. The expectations are GetTabStripStateString's —
    // creation rank + 'p' — copied VERBATIM from
    // tab_strip_model_unittest.cc's MoveSelectedTabsTo matrix.

    /// The unittest's own harness (see the block comment above). Returns
    /// the tabs in CREATION order — the state string's numbering. Every
    /// append is background (upstream's PrepareTabs adds quiet tabs), and
    /// the boot tab's standing selection is cleared — the harness's
    /// selection is EXACTLY `selected` (SetSelectionFromModel's law).
    fn prepare_for_selection_test(
        strip: &mut Strip,
        tab_count: usize,
        pinned_count: usize,
        selected: &[usize],
    ) -> Vec<TabId> {
        while strip.tabs.len() < tab_count {
            strip.append(Destination::Servers, false);
        }
        let created: Vec<TabId> = strip.tabs.iter().map(|t| t.id).collect();
        for tab_id in created.iter().take(pinned_count) {
            strip.set_pinned(*tab_id, true);
        }
        strip.selection.clear();
        strip.anchor = None;
        for index in selected {
            strip.selection.insert(created[*index]);
        }
        if let Some(first) = selected.first() {
            let id = created[*first];
            strip.active = Some(id);
            strip.anchor = Some(id);
        }
        created
    }

    /// GetTabStripStateString — creation rank + 'p' per tab.
    fn state_string(strip: &Strip, created: &[TabId]) -> String {
        strip
            .tabs
            .iter()
            .map(|t| {
                let rank = created
                    .iter()
                    .position(|&c| c == t.id)
                    .expect("every tab was created");
                if t.pinned {
                    format!("{rank}p")
                } else {
                    format!("{rank}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn move_selected_to_matches_the_unittest_matrix() {
        // (tab_count, pinned_count, selected, target_index, expected)
        // — tab_strip_model_unittest.cc:5543 verbatim.
        let matrix: &[(usize, usize, &[usize], usize, &str)] = &[
            // 1 selected tab.
            (2, 0, &[0], 1, "1 0"),
            (3, 0, &[0], 2, "1 2 0"),
            (3, 0, &[2], 0, "2 0 1"),
            (3, 0, &[2], 1, "0 2 1"),
            (3, 0, &[0, 1], 0, "0 1 2"),
            // 2 selected tabs.
            (6, 0, &[4, 5], 1, "0 4 5 1 2 3"),
            (3, 0, &[0, 1], 1, "2 0 1"),
            (4, 0, &[0, 2], 1, "1 0 2 3"),
            (6, 0, &[0, 1], 3, "2 3 4 0 1 5"),
            // 3 selected tabs.
            (6, 0, &[0, 2, 3], 3, "1 4 5 0 2 3"),
            (7, 0, &[4, 5, 6], 1, "0 4 5 6 1 2 3"),
            (7, 0, &[1, 5, 6], 4, "0 2 3 4 1 5 6"),
            // 5 selected tabs.
            (8, 0, &[0, 2, 3, 6, 7], 3, "1 4 5 0 2 3 6 7"),
            // 7 selected tabs.
            (
                16,
                0,
                &[0, 1, 2, 3, 4, 7, 9],
                8,
                "5 6 8 10 11 12 13 14 0 1 2 3 4 7 9 15",
            ),
            // With pinned tabs.
            (6, 2, &[2, 3], 2, "0p 1p 2 3 4 5"),
            (6, 2, &[0, 4], 3, "1p 0p 2 3 4 5"),
            (6, 3, &[1, 2, 4], 0, "1p 2p 0p 4 3 5"),
            (8, 3, &[1, 3, 4], 4, "0p 2p 1p 5 6 3 4 7"),
            (7, 4, &[2, 3, 4], 3, "0p 1p 2p 3p 5 4 6"),
        ];
        for (i, (tab_count, pinned_count, selected, target, expected)) in matrix.iter().enumerate()
        {
            let mut strip = Strip::new();
            let created =
                prepare_for_selection_test(&mut strip, *tab_count, *pinned_count, selected);
            strip
                .move_selected_to(*target)
                .expect("the selection exists");
            assert_eq!(state_string(&strip, &created), *expected, "case {i}");
        }
    }

    #[test]
    fn move_selected_to_keeps_a_whole_group_that_moves_together() {
        // MoveSelectedTabsToWithEntireGroupSelected, adapted to ZIM's
        // group_create (it gathers members to the unpinned edge — a
        // documented divergence — so the group lives at the edge here):
        // the whole group rides the block intact and stays grouped.
        let mut strip = Strip::new();
        let created = prepare_for_selection_test(&mut strip, 10, 5, &[2, 3, 5, 6]);
        let group = strip
            .group_create(&[created[5], created[6]], "docs")
            .unwrap();
        strip.move_selected_to(3).expect("the selection exists");
        assert_eq!(state_string(&strip, &created), "0p 1p 4p 2p 3p 5 6 7 8 9");
        assert!(strip.groups.contains_key(&group));
        assert!(strip.group_contiguous(group));
    }

    #[test]
    fn move_selected_to_ungroups_only_the_stranded_mover() {
        // MoveSelectedTabsToWithPartGroupSelected, adapted: the mover is
        // a group member whose landing lands an ungrouped tab between it
        // and its stay-put partner — the MOVER ungroups, the group
        // survives with the stayer.
        let mut strip = Strip::new();
        let created = prepare_for_selection_test(&mut strip, 10, 5, &[2, 3, 5]);
        let group = strip
            .group_create(&[created[5], created[6]], "docs")
            .unwrap();
        // An ungrouped stranger slots in BEFORE the group: the group now
        // sits at 6,7 with tab 7 ahead of it — the geometry the strand
        // needs (ZIM's group_create put the group at the edge).
        strip.move_to(created[7], 5);
        strip.move_selected_to(3).expect("the selection exists");
        assert_eq!(state_string(&strip, &created), "0p 1p 4p 2p 3p 5 7 6 8 9");
        let mover = strip.tabs.iter().find(|t| t.id == created[5]).unwrap();
        assert!(!mover.group.is_some());
        let stayer = strip.tabs.iter().find(|t| t.id == created[6]).unwrap();
        assert_eq!(stayer.group, Some(group));
    }

    #[test]
    fn a_selected_drop_rides_the_block() {
        // The drop policy: a source standing in a multi-selection
        // drops the WHOLE selection as one block (MaybeStartDrag
        // dragged them together — the drop moves them together).
        let mut strip = Strip::new();
        let fresh = strip.tabs[0].id;
        let a = strip.append(Destination::Servers, false);
        let b = strip.append(Destination::Jobs, false);
        let c = strip.append(Destination::About, false);
        let d = strip.append(Destination::Settings, false);
        strip.selection.clear();
        strip.anchor = None;
        strip.selection.insert(b);
        strip.selection.insert(d);
        strip.active = Some(d);
        strip.anchor = Some(d);
        // Dropping d at 1 among the others [fresh, a, c] (the boot tab
        // rides at 0) carries b: the block [b, d] lifts out and lands
        // between fresh and a.
        strip.reorder_drop(d, 1).expect("the drop lands");
        let order: Vec<TabId> = strip.tabs.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![fresh, b, d, a, c]);
        assert!(strip.selection.contains(&b) && strip.selection.contains(&d));
    }

    #[test]
    fn a_pinned_source_dropped_past_the_edge_unpins_the_selected_class() {
        // The unpin policy rides with the SOURCE across the whole
        // selected pinned class: they crossed the boundary together.
        let mut strip = Strip::new();
        let fresh = strip.tabs[0].id;
        strip.set_pinned(fresh, true);
        let a = strip.append(Destination::Servers, false);
        // The spacer keeps the block's edge honest (one unpinned tab
        // below the class) — its id is never selected.
        let spacer = strip.append(Destination::Jobs, false);
        let b = strip.append(Destination::About, false);
        strip.set_pinned(a, true);
        strip.set_pinned(b, true);
        // Block [fresh, a, b], edge 3; the selection is {a, b} and the
        // drop of b at 3 lands at the edge → BOTH unpin.
        strip.selection.clear();
        strip.selection.insert(a);
        strip.selection.insert(b);
        strip.reorder_drop(b, 3).expect("the drop lands");
        let a_tab = strip.tabs.iter().find(|t| t.id == a).unwrap();
        let b_tab = strip.tabs.iter().find(|t| t.id == b).unwrap();
        assert!(!a_tab.pinned);
        assert!(!b_tab.pinned);
        // The block lands together — the unpins edge-jumped them, the
        // block move re-gathered them at the drop point.
        let order: Vec<TabId> = strip.tabs.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![fresh, spacer, b, a]);
    }

    #[test]
    fn a_single_selection_drop_stays_the_single_law() {
        // The regression guard: a lone selection drops exactly as the
        // single law always did — the block gate needs company.
        let mut strip = Strip::new();
        let fresh = strip.tabs[0].id;
        let a = strip.append(Destination::Servers, false);
        let b = strip.append(Destination::Jobs, false);
        strip.selection.clear();
        strip.selection.insert(a);
        strip.active = Some(a);
        strip.reorder_drop(a, 2).expect("the drop lands");
        let order: Vec<TabId> = strip.tabs.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![fresh, b, a]);
    }

    #[test]
    fn url_roundtrip() {
        let d = Destination::Server {
            server_id: "abc".into(),
        };
        assert_eq!(Destination::parse(&d.url()), d);
        assert_eq!(
            Destination::parse("zim://join/box.local:25565"),
            Destination::Join {
                host: Some("box.local".into()),
                port: 25565
            }
        );
        assert_eq!(
            Destination::parse("zim://join/:25565"),
            Destination::Join {
                host: None,
                port: 25565
            }
        );
        assert!(matches!(
            Destination::parse("zim://join/nonsense"),
            Destination::Missing { .. }
        ));
        assert_eq!(
            Destination::parse("zim://console/s1"),
            Destination::Console {
                server_id: "s1".into()
            }
        );
    }

    // -- The group editor's verbs (tab_group_editor_bubble_view.cc writes
    //    routed through the command lane) -------------------------------

    fn strip_with_group() -> (Strip, TabId, GroupId) {
        let mut strip = Strip::new();
        let a = strip.append(Destination::Servers, true);
        let group = strip.group_create(&[a], "survival").unwrap();
        (strip, a, group)
    }

    #[test]
    fn group_rename_takes_every_keystroke_and_allows_empty() {
        let (mut strip, _, group) = strip_with_group();
        strip.group_rename(group, "surv");
        assert_eq!(strip.groups[&group].label, "surv");
        // An EMPTY title is legal — upstream chips render the color alone.
        strip.group_rename(group, "");
        assert_eq!(strip.groups[&group].label, "");
    }

    #[test]
    fn group_color_refuses_out_of_range_and_unknown_groups() {
        let (mut strip, _, group) = strip_with_group();
        assert!(strip.group_set_color(group, 8));
        assert_eq!(strip.groups[&group].color, 8);
        assert!(!strip.group_set_color(group, 9)); // kNumEntries = 9
        assert!(!strip.group_set_color(group, 255));
        assert!(!strip.group_set_color(9999, 0));
    }

    #[test]
    fn removing_the_last_member_destroys_the_group() {
        let (mut strip, a, group) = strip_with_group();
        strip.group_remove(a);
        assert!(!strip.groups.contains_key(&group));
        let member = strip.tabs.iter().find(|t| t.id == a).unwrap();
        assert!(member.group.is_none());
    }

    #[test]
    fn removing_a_non_last_member_keeps_the_group() {
        let (mut strip, a, group) = strip_with_group();
        let b = strip.append(Destination::Jobs, true);
        strip.group_add(group, b);
        strip.group_remove(b);
        assert!(strip.groups.contains_key(&group));
        assert!(strip
            .tabs
            .iter()
            .find(|t| t.id == a)
            .unwrap()
            .group
            .is_some());
    }

    #[test]
    fn ungroup_frees_every_member_and_dies() {
        let (mut strip, _a, group) = strip_with_group();
        let b = strip.append(Destination::Jobs, true);
        strip.group_add(group, b);
        assert!(strip.group_ungroup(group));
        assert!(!strip.groups.contains_key(&group));
        assert!(strip.tabs.iter().all(|t| t.group.is_none()));
        // The tabs themselves survive.
        assert_eq!(strip.tabs.len(), 3);
        assert!(!strip.group_ungroup(group));
    }

    #[test]
    fn group_add_lands_after_the_last_member_and_expands() {
        let (mut strip, a, group) = strip_with_group();
        let _b = strip.append(Destination::Jobs, true);
        let c = strip.append(Destination::About, true);
        strip.groups.get_mut(&group).unwrap().collapsed = true;
        strip.group_add(group, c);
        // The mover sits directly after the group's last member (a)…
        let a_index = strip.index_of(a).unwrap();
        assert_eq!(strip.tabs[a_index + 1].id, c);
        assert_eq!(strip.tabs[a_index + 1].group, Some(group));
        // …every other tab stays OUT of the group (the newcomer joined
        // the run, it did not swallow its neighbors)…
        for tab in strip.tabs.iter() {
            if tab.id != c && tab.id != a {
                assert!(tab.group.is_none());
            }
        }
        // …and the group expanded to show the newcomer.
        assert!(!strip.groups[&group].collapsed);
    }

    #[test]
    fn group_new_tab_joins_the_end_and_takes_focus() {
        let (mut strip, a, group) = strip_with_group();
        let fresh = strip.group_new_tab(group).unwrap();
        let a_index = strip.index_of(a).unwrap();
        assert_eq!(strip.tabs[a_index + 1].id, fresh);
        assert_eq!(strip.tabs[a_index + 1].group, Some(group));
        assert_eq!(strip.tabs[a_index + 1].destination(), &Destination::New);
        assert_eq!(strip.active, Some(fresh));
    }

    // -- the drag block laws (MaybeStartDrag's carried set, the
    //    between-windows move and the tear-off's whole-block landing) --

    fn labeled_strip() -> (Strip, Vec<TabId>) {
        let mut strip = Strip::new();
        let mut ids = vec![strip.tabs[0].id];
        for _ in 1..4 {
            let id = strip.append(Destination::Settings, true);
            // A second entry in the history so a moved tab's identity is
            // provable (can_back stays true across a move).
            strip.navigate(id, Destination::About);
            ids.push(id);
        }
        (strip, ids)
    }

    #[test]
    fn the_drag_block_is_the_selection_or_the_one_tab() {
        let (mut strip, ids) = labeled_strip();
        // No multi-selection: the hand's tab is the whole block.
        assert_eq!(strip.drag_block(ids[1]), vec![ids[1]]);
        // ctrl-walk A and B in (D stays the active member): the hand on
        // A carries the whole selection — including the active it did
        // not touch.
        assert!(strip.toggle_selection(ids[0]));
        assert!(strip.toggle_selection(ids[1]));
        let mut block = strip.drag_block(ids[0]);
        block.sort_unstable();
        let mut expected = vec![ids[0], ids[1], ids[3]];
        expected.sort_unstable();
        assert_eq!(block, expected);
    }

    #[test]
    fn detach_block_moves_the_whole_selection_once() {
        let (mut strip, ids) = labeled_strip();
        // The selection becomes A, B, C, D (everything); the hand drags
        // C — the BLOCK is all four.
        assert!(strip.toggle_selection(ids[0]));
        assert!(strip.toggle_selection(ids[1]));
        assert!(strip.toggle_selection(ids[2]));
        let block = strip.detach_block(&strip.drag_block(ids[2]));
        // The BLOCK left — in strip order — not one tab of it.
        assert_eq!(
            block.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![ids[0], ids[1], ids[2], ids[3]]
        );
        // The emptied strip: ONE fresh New tab (the per-tab-detach
        // disease minted a new one per removal — three here).
        assert_eq!(strip.tabs.len(), 1);
        assert_eq!(strip.tabs[0].destination(), &Destination::New);
        assert_eq!(strip.active, Some(strip.tabs[0].id));
        assert!(strip.selection.contains(&strip.tabs[0].id));
    }

    #[test]
    fn detach_block_promotes_a_selected_survivor_and_keeps_strays() {
        let (mut strip, ids) = labeled_strip();
        // A and B join the selection; then the active D steps out (the
        // first selected — A — is promoted). The block is A+B; C and D
        // stay.
        assert!(strip.toggle_selection(ids[0]));
        assert!(strip.toggle_selection(ids[1]));
        assert!(strip.toggle_selection(ids[3]));
        let block = strip.detach_block(&strip.drag_block(ids[0]));
        assert_eq!(
            block.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![ids[0], ids[1]]
        );
        assert_eq!(
            strip.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![ids[2], ids[3]]
        );
        // The active left with the block and no selected survivor
        // stayed — the neighbor where the block stood is promoted (C).
        assert_eq!(strip.active, Some(ids[2]));
        assert_eq!(strip.anchor, Some(ids[2]));
        assert!(strip.selection.contains(&ids[2]));
        // The moved tabs keep their ids and destinations.
        assert_eq!(block[0].id, ids[0]);
        assert_eq!(block[1].id, ids[1]);
    }

    #[test]
    fn select_block_lands_the_moved_block_selected() {
        let (mut source, ids) = labeled_strip();
        // A and B join, D steps out; the hand drags B.
        assert!(source.toggle_selection(ids[0]));
        assert!(source.toggle_selection(ids[1]));
        assert!(source.toggle_selection(ids[3]));
        let block = source.detach_block(&source.drag_block(ids[1]));

        let mut target = Strip::new();
        let target_seed = target.tabs[0].id;
        // The target re-inserts the block; ids mint fresh from ITS counter.
        let mut new_ids = Vec::new();
        for (offset, tab) in block.iter().enumerate() {
            let id = target.insert(offset, tab.destination().clone(), false);
            new_ids.push(id);
        }
        // The hand's tab (ids[1], second in the block) anchors.
        let dragged_new = new_ids[1];
        target.select_block(dragged_new, &new_ids);
        assert_eq!(target.active, Some(dragged_new));
        assert_eq!(target.anchor, Some(dragged_new));
        assert_eq!(target.selection.len(), block.len());
        // The seed tab is deselected but alive; the destinations rode
        // over intact (the seed's New, the moved B's Settings).
        assert!(target.selection.contains(&new_ids[0]));
        assert!(target.selection.contains(&dragged_new));
        assert!(target.tabs.iter().any(|t| t.id == target_seed));
        assert_eq!(
            target
                .tabs
                .iter()
                .find(|t| t.id == new_ids[0])
                .unwrap()
                .destination(),
            &Destination::New
        );
        assert_eq!(
            target
                .tabs
                .iter()
                .find(|t| t.id == new_ids[1])
                .unwrap()
                .destination(),
            &Destination::About
        );
    }
}
