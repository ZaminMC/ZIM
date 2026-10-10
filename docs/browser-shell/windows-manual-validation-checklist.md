# Windows Manual Validation Checklist — Browser Shell (P0.2)

Purpose: the human gate for P0.2. Green tests prove behavior; **this proves
the UI is a browser.** Build: the latest `vMAJOR.MINOR.PATCH` pre-release from
ZaminMC/ZIM's releases page (signed NSIS setup / portable).
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
| 19 | The official white theme | White surfaces, gray strip (#DEE1E6), hairline borders, one blue (#1A73E8); no dark remnants, no neon accents; the terminal keeps its dark canvas |
| 20 | Hover states | Tab hover card/highlight, close-button hover, omnibox decorations — subtle, 100–200 ms |
| 21 | Focus states | Visible focus rings (tab, omnibox, bar items); F6 roving focus across chrome regions |
| 22 | Animations | Tab open/close/move use the three-duration motion law (ADR-0030); nothing animates layout twice |
| 23 | Crash boundary | Force a tab crash — other tabs and chrome stay alive; recovery is a page, not a modal |
| 24 | **The look test** | Open the app cold. Does it read as **"this is a browser"** in ≤3 seconds? Or as a Minecraft panel with a browser-themed header? If the latter — P0.2 has failed regardless of rows 1–23. **2026-10-10: PASS (local harness, pixel-probed).** Rest state `shots/looktest-rest.png`: strip `#DEE1E6`, toolbar `#FFFFFF`, omnibox field `#F1F3F4`, active tab melts into the toolbar (seam white through y=41–43, probe-verified). Hover `looktest-hover-zoom.png`: opaque `#C3C6CA` chip, close revealed. Group member active `looktest-activemember-zoom.png`: white tab, band pauses around the active curve. Reads as a browser. |

## After the pass

- File every FAIL as an issue tagged `p0.2-shell` with screenshot + scaling.
- PASS on 1–24 closes the manual gate of P0.2; Phase-1 shell restructure
  (ADR-0033) continues under feature freeze until it re-runs this list.


## 2026-10-10 addendum — rows for this pass's fixes

| # | Item | What "fixed" looks like |
|---|---|---|
| 25 | Window drag | The window MOVES from any bare strip area (the ACL now grants start-dragging; before this pass the undecorated window was locked). *Code-verified locally (drag ACL + rail hit-test in shell/host.rs); the physical drag needs the Windows app.* |
| 26 | Responsiveness | Clicking tabs/verbs answers immediately; no disk stall behind every beat (session saves are debounced + atomic now). *Code-verified locally; the feel needs the Windows app.* |
| 27 | Group underline containment | An inactive group member's colored underline never reads as a line crossing into the neighbor tab (22px insets inside the visible span). **2026-10-10: PASS (harness).** `shots/looktest-groupband-zoom.png` / `looktest-activemember-zoom.png`: member underlines weld into one band, the band pauses around the active tab's curve, non-member boundaries keep the 22px containment law. **2026-10-10 (later): the inset law re-derived from upstream** — tab_group_underline.cc's GetStrokeInset is 18 (overlap 18 − adjustment 2 + stroke 2), and the row-30 backport moved the boundary rule to it (members 0, edges 18, active edges −2). The visual claim is unchanged; the numbers now come from the port. |
| 28 | Tray laws | Close parks in the tray with a tooltip; tray Open restores; tray Quit ends the process fully. **2026-10-10: code-verified** (`main.rs`: CloseRequested→prevent+hide on the primary only, `tray-open`/`tray-quit` items, left-click shows, tooltip "ZIM — servers keep running in the background", Quit = `app.exit(0)`; daemon deliberately detached). The icon itself needs the Windows app. |
| 29 | Updater lane | The pill never closes the app on its own; download is automatic (when enabled), apply is only the explicit restart click. *Verified against the v0.4.29 release: the feed answers, the installer builds, the pill renders from `updatePhase` with an explicit restart button only.* **2026-10-10: the file-lock law hardened** — the user's installer screenshot (`Error opening file for writing: …zamind.exe`, 00:44 local) predates the v0.4.29 kill hook by an hour, but the audit of the tauri-bundler v2.9.4 template found the gaps it could have hit: the template's own Restart Manager covers only the main binary, and `installer-hooks.nsh` now kills `zim`/`zamind`/`zamin` with a poll-until-gone loop (taskkill exit-code law, 12-round cap) in both `PREINSTALL` (fires for manual installs *and* the `/UPDATE` launch — one section, both modes) and `PREUNINSTALL`. Needs the next tagged release's installer to prove it on Windows. |
| 30 | Chromium strip backport | The strip is painted by the backported law (`ui/chromium/chromiumTabs.ts`, chromium/src main @ 157.0.8097.0 — see PROVENANCE.md there): the active tab's body is GetPath() as SVG (extensions r12 melting into the toolbar, top arcs shrinking with width), inactive hover is the detached squarcle at the pipeline's 40% blend, group chips are the solid nine-palette pills with max-contrast labels, the group line rides 2 DIP at stroke inset 18, separators are 2×16 hanging marks. **Eyeball pass needed on Windows**: hover fills, chip colors/contrast per palette entry, the line's continuity across a group run, tight-tab fill expansion, and 20+ tab declutter. |
| 31 | Group editor + tab menu backport | The group editor IS Chromium's bubble (tabs/groups/): the chip's RIGHT click opens it (left stays the collapse toggle — tab_group_header_view.cc's OnMouseReleased law), the title field renames live per keystroke, the nine-color radio grid applies instantly (ring in the bubble's own color), and New-tab-in-group / Ungroup / Close group act through the host's new ⓩ verbs (50024–50029). The tab context menu is tab_menu_model.cc's Build() order: new-tab-to-right → group adds (existing groups flattened under the verb) → remove-from-group → reload/duplicate/pin/mute → axis verb → close block. **Eyeball pass needed on Windows**: the editor's popup rect (240×224), the color ring contrast, the collapsed-group expand on add, and the empty-name chip (a bare color dot). |
