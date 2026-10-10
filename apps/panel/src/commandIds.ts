// The browser command registry's view-side mirror — ONE table for every
// webview that dispatches commands (the frame, the popup overlay). The
// authoritative registry is src-tauri/src/shell/commands.rs; the IDs here
// must move when it moves (the shell tests pin the pairs).

export const CMD = {
  RELOAD: 33002,
  NEW_TAB: 34014,
  CLOSE_TAB: 34015,
  SELECT_NEXT_TAB: 34016,
  SELECT_PREVIOUS_TAB: 34017,
  SELECT_TAB_0: 34018,
  DUPLICATE_TAB: 34027,
  RESTORE_TAB: 34028,
  ADD_NEW_TAB_TO_GROUP: 34100,
  CLOSE_TAB_GROUP: 34104,
  BOOKMARK_THIS_TAB: 35000,
  FOCUS_LOCATION: 39001,
  SHOW_BOOKMARK_BAR: 40009,
  TOGGLE_PINNED: 50001,
  SELECT_LAST_TAB: 50002,
  TOGGLE_MUTE: 50003,
  NAVIGATE_ACTIVE: 50004,
  TOGGLE_GROUP_COLLAPSE: 50005,
  NAV_BACK: 50006,
  NAV_FORWARD: 50007,
  WINDOW_MINIMIZE: 50010,
  WINDOW_TOGGLE_MAXIMIZE: 50011,
  WINDOW_CLOSE: 50012,
  TOGGLE_PALETTE: 50013,
  ZOOM_IN: 50015,
  ZOOM_OUT: 50016,
  ZOOM_RESET: 50017,
  NEW_WINDOW: 50018,
  CLOSE_OTHER_TABS: 50019,
  CLOSE_TABS_TO_THE_RIGHT: 50020,
  DEV_TOOLS: 50021,
  OPEN_LOGS: 50022,
  TOGGLE_VERTICAL_STRIP: 50023,
  // The group editor (tab_group_editor_bubble_view.cc's writes, routed
  // through the command lane — no upstream browser IDCs exist for them).
  RENAME_GROUP: 50024,
  SET_GROUP_COLOR: 50025,
  REMOVE_TAB_FROM_GROUP: 50026,
  UNGROUP_GROUP: 50027,
  NEW_TAB_IN_GROUP: 50028,
  ADD_TAB_TO_EXISTING_GROUP: 50029,
} as const;
