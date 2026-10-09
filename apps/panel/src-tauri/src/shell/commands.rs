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
/// IDC_MOVE_TAB_TO_NEW_WINDOW 34056.
pub const MOVE_TAB_TO_NEW_WINDOW: u32 = 34056;
/// IDC_ADD_NEW_TAB_TO_GROUP 34100.
pub const ADD_NEW_TAB_TO_GROUP: u32 = 34100;
/// IDC_CLOSE_TAB_GROUP 34104.
pub const CLOSE_TAB_GROUP: u32 = 34104;
/// IDC_BOOKMARK_THIS_TAB 35000.
pub const BOOKMARK_THIS_TAB: u32 = 35000;
/// IDC_BOOKMARK_ALL_TABS 35001 (reserved — not wired this phase).
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

/// Window-control verbs for the frame's caption buttons — no upstream
/// command IDs (Views owns them natively); ZIM-local numbering.
pub const WINDOW_MINIMIZE: u32 = 50010;
pub const WINDOW_TOGGLE_MAXIMIZE: u32 = 50011;
pub const WINDOW_CLOSE: u32 = 50012;

/// The command palette lives in the ACTIVE TAB's webview, but Ctrl+K
/// can land while the frame holds focus — the frame can only reach the
/// palette through the host (a CustomEvent never crosses webviews).
pub const TOGGLE_PALETTE: u32 = 50013;
