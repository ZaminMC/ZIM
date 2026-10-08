# Windows Manual Validation Checklist — Browser Shell (P0.2)

Purpose: the human gate for P0.2. Green tests prove behavior; **this proves
the UI is a browser.** Build: latest `v0.2.0` pre-release from
ZaminMC/ZaminPanel-Releases (signed NSIS setup / portable / latest.json).
Run on Windows 10/11, 100% scaling first, then 125%/150%.

Method: open the app and **use it as a user for 15 minutes**. For each row:
PASS / FAIL+describe. A FAIL on any row 1–24 keeps P0.2 open.

## Chrome & window

| # | Item | What "browser-grade" looks like |
|---|---|---|
| 1 | Title bar | Native-feel drag area, no dashboard header; double-click maximizes |
| 2 | Tab dimensions | ~34 DIP tall tabs; standard ~256 DIP wide incl. corner extensions; overlap matches Chromium |
| 3 | Tab shape | Folder-tab silhouette: 10 top / 12 bottom corner radii, separators 2×20 between inactive tabs |
| 4 | Active tab | Expands to fill, connected to toolbar (no gap), distinct bg |
| 5 | Inactive tabs | Dimmer, shrink under pressure (min 16 interior), separators fade |
| 6 | Pinned tabs | Icon-only fixed width (24+insets); pin/unpin via context menu; survive restart |
| 7 | Tab groups | Named+colored group chip/underline; membership survives reorder & restart |
| 8 | Group collapse | Collapse/expand keeps order, animation is compositor-only, no layout jank |
| 9 | Tab drag (reorder) | Starts after ~10 DIP; neighbors reflow with the width law; no flicker |
| 10 | Tab tear-off | Below/above strip ~15 DIP detaches into a new window that keeps dragging |
| 11 | New window | Real second window; tabs move between windows via drag/context menu |
| 12 | Window resize/maximize | Tab strip reflows (min-width law); maximized keeps hit-targets at screen edge |

## Address bar / bookmarks

| # | Item | Standard |
|---|---|---|
| 13 | Address bar | 34 DIP tall, focus ring, selects-all on focus, Esc reverts, `zamin://` and server refs resolve like URLs |
| 14 | Omnibox behavior | Typing classifies URL-vs-search; Enter opens; Alt+Enter opens new tab; keywords don't corrupt state |
| 15 | Bookmark bar | 40009 toggles; folders/menus work; bookmarks persist and reorder |
| 16 | Context menus | Tab menu (pin/group/close others/…), page menu, link menu — browser verbs, not dashboard verbs |

## Behavior & polish

| # | Item | Standard |
|---|---|---|
| 17 | Browser controls | New-tab button, per-tab close with hover state, tab context menu complete |
| 18 | Keyboard | Ctrl+T/W/Tab/Shift+Tab/L/1–8/9, Ctrl+Shift+T reopen, F5/Ctrl+R, F6 — per ADR-0032 |
| 19 | Dark theme | All chrome follows system + in-app toggle without contrast regressions |
| 20 | Hover states | Tab hover card/highlight, close-button hover, omnibox decorations — subtle, 100–200 ms |
| 21 | Focus states | Visible focus rings (tab, omnibox, bar items); F6 roving focus across chrome regions |
| 22 | Animations | Tab open/close/move use the three-duration motion law (ADR-0030); nothing animates layout twice |
| 23 | Crash boundary | Force a tab crash — other tabs and chrome stay alive; recovery is a page, not a modal |
| 24 | **The look test** | Open the app cold. Does it read as **"this is a browser"** in ≤3 seconds? Or as a Minecraft panel with a browser-themed header? If the latter — P0.2 has failed regardless of rows 1–23 |

## After the pass

- File every FAIL as an issue tagged `p0.2-shell` with screenshot + scaling.
- PASS on 1–24 closes the manual gate of P0.2; Phase-1 shell restructure
  (ADR-0033) continues under feature freeze until it re-runs this list.
