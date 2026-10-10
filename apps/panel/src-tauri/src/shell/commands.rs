// The browser command registry — Chromium's ID space (chrome/app/
// chrome_command_ids.h), mirrored so the keyboard contract (ADR-0032),
// the frame layer, and the tests cite ONE table. IDs marked ⓩ are
// ZIM-local because modern Chromium has no single IDC for the
// verb; the local numbering keeps clear of upstream's blocks.

/// IDC_RELOAD 33002 (33007 = bypassing cache, reserved).
pub const RELOAD: u32 = 33002;
/// IDC_RELOAD_BYPASSING_CACHE 33007.
pub const RELOAD_BYPASSING_CACHE: u32 = 33007;
/// IDC_NEW_TAB 34014.
pub const NEW_TAB: u32 = 34014;
/// IDC_CLOSE_TAB 34015.
pub const CLOSE_TAB: u32 = 34015;
/// IDC_SELECT_NEXT_TAB 34016.
pub const SELECT_NEXT_TAB: u32 = 34016;
/// IDC_SELECT_PREVIOUS_TAB 34017.
pub const SELECT_PREVIOUS_TAB: u32 = 34017;
/// IDC_SELECT_TAB_0 .. IDC_SELECT_TAB_7 = 34018 .. 34025.
pub const SELECT_TAB_0: u32 = 34018;
/// IDC_DUPLICATE_TAB 34027.
pub const DUPLICATE_TAB: u32 = 34027;
/// IDC_RESTORE_TAB 34028.
pub const RESTORE_TAB: u32 = 34028;
/// IDC_MOVE_TAB_NEXT 34032.
pub const MOVE_TAB_NEXT: u32 = 34032;
/// IDC_MOVE_TAB_PREVIOUS 34033.
pub const MOVE_TAB_PREVIOUS: u32 = 34033;
/// IDC_MOVE_TAB_TO_NEW_WINDOW 34056 (reserved — not wired this phase).
#[allow(dead_code)]
pub const MOVE_TAB_TO_NEW_WINDOW: u32 = 34056;
/// IDC_ADD_NEW_TAB_TO_GROUP 34100.
pub const ADD_NEW_TAB_TO_GROUP: u32 = 34100;
/// IDC_CLOSE_TAB_GROUP 34104.
pub const CLOSE_TAB_GROUP: u32 = 34104;
/// IDC_BOOKMARK_THIS_TAB 35000.
pub const BOOKMARK_THIS_TAB: u32 = 35000;
/// IDC_BOOKMARK_ALL_TABS 35001 (reserved — not wired this phase).
#[allow(dead_code)]
pub const BOOKMARK_ALL_TABS: u32 = 35001;
/// IDC_FOCUS_LOCATION 39001.
pub const FOCUS_LOCATION: u32 = 39001;
/// IDC_SHOW_BOOKMARK_BAR 40009.
pub const SHOW_BOOKMARK_BAR: u32 = 40009;

/// ⓩ Local: toggle the active tab's pinned state (modern Chromium pins
/// through the tab context menu; there is no single IDC).
pub const TOGGLE_PINNED: u32 = 50001;
/// ⓩ Local: select the LAST tab (upstream reserves the same block).
pub const SELECT_LAST_TAB: u32 = 50002;
/// ⓩ Local: toggle the active tab's mute posture (§53).
pub const TOGGLE_MUTE: u32 = 50003;
/// ⓩ Local: navigate the ACTIVE tab to an explicit destination (the
/// bookmarks bar's verb; upstream routes through the navigator).
pub const NAVIGATE_ACTIVE: u32 = 50004;
/// ⓩ Local: toggle the active tab's group collapse.
pub const TOGGLE_GROUP_COLLAPSE: u32 = 50005;
/// ⓩ Local: navigate the active tab back (upstream routes through the
/// navigator; this build numbers the frame verbs locally).
pub const NAV_BACK: u32 = 50006;
/// ⓩ Local: navigate the active tab forward.
pub const NAV_FORWARD: u32 = 50007;

/// ⓩ Local: the group editor's name field (upstream's bubble writes the
/// visual data straight from the title controller — no browser IDC; this
/// build numbers the frame verbs locally). An EMPTY name is legal.
pub const RENAME_GROUP: u32 = 50024;
/// ⓩ Local: the editor's color grid (tabs/groups/color_picker_view.cc —
/// one circle per TabGroupColorId in the enum's wire order).
pub const SET_GROUP_COLOR: u32 = 50025;
/// ⓩ Local: "Remove tab from group" (tab_menu_model.cc's
/// CommandRemoveFromGroup; no browser IDC exists for it).
pub const REMOVE_TAB_FROM_GROUP: u32 = 50026;
/// ⓩ Local: the editor's "Ungroup" — every member leaves, the group dies.
pub const UNGROUP_GROUP: u32 = 50027;
/// ⓩ Local: the editor's "New tab in group".
pub const NEW_TAB_IN_GROUP: u32 = 50028;
/// ⓩ Local: the tab menu's "Add to existing group" item, one per group.
pub const ADD_TAB_TO_EXISTING_GROUP: u32 = 50029;

/// ⓩ Local: the multi-selection gestures (Tab::OnMousePressed's
/// modifier branches — tab.cc:726-764 — routed through the command lane
/// because the frame webview holds no model). The dispatch order is the
/// press law's own: shift+ctrl > shift > ctrl.
pub const TOGGLE_TAB_SELECTION: u32 = 50034;
pub const EXTEND_TAB_SELECTION: u32 = 50035;
pub const ADD_SELECTION_FROM_ANCHOR_TO: u32 = 50036;
/// ⓩ Local: the tab MENU's close — the selection's scope law
/// (GetIndicesForCommand) decides whether one tab or every selected tab
/// dies; the close BOX stays CLOSE_TAB (one tab, never the selection).
pub const CLOSE_SELECTED_TABS: u32 = 50037;

/// Window-control verbs for the frame's caption buttons — no upstream
/// command IDs (Views owns them natively); ZIM-local numbering.
pub const WINDOW_MINIMIZE: u32 = 50010;
pub const WINDOW_TOGGLE_MAXIMIZE: u32 = 50011;
pub const WINDOW_CLOSE: u32 = 50012;

/// The command palette lives in the ACTIVE TAB's webview, but Ctrl+K
/// can land while the frame holds focus — the frame can only reach the
/// palette through the host (a CustomEvent never crosses webviews).
pub const TOGGLE_PALETTE: u32 = 50013;

/// ⓩ Local: contents zoom for the ACTIVE tab (Chromium routes zoom
/// through the zoom controller with its preset ladder; the app menu's
/// zoom row and the keyboard contract both land here).
pub const ZOOM_IN: u32 = 50015;
pub const ZOOM_OUT: u32 = 50016;
pub const ZOOM_RESET: u32 = 50017;

/// ⓩ Local: open a new top-level browser window (the app menu's
/// "New window"; upstream owns IDC_NEW_WINDOW 34000 — kept clear of the
/// ported block).
pub const NEW_WINDOW: u32 = 50018;

/// ⓩ Local: the tab menu's scoped closes (Chromium's close-context
/// pair; no single IDC in the ported block).
pub const CLOSE_OTHER_TABS: u32 = 50019;
pub const CLOSE_TABS_TO_THE_RIGHT: u32 = 50020;

/// ⓩ Local: open the ACTIVE tab's developer tools (Chromium routes
/// through IDC_DEV_TOOLS; the devtools feature gates the release build).
pub const DEV_TOOLS: u32 = 50021;

/// ⓩ Local: open the application's log folder (the daemon's audit log
/// and the panel's own state). Upstream owns no IDC for this.
pub const OPEN_LOGS: u32 = 50022;

/// ⓩ Local: the strip's presentation axis (§54, ADR-0026) — the same
/// tab objects render as a left rail or the horizontal band. No
/// upstream IDC (Chromium has no shipped vertical-strip verb).
pub const TOGGLE_VERTICAL_STRIP: u32 = 50023;
