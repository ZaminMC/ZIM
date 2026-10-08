# CHROMIUM-PORTS — the attribution ledger

ZaminPanel's browser shell (ADR-0033) is **Chromium-derived by behavior, not
by copy**: upstream constants, state machines, and invariants are re-expressed
in this project's own Rust/TypeScript, named in ZaminPanel's own vocabulary
(the **frame**, the **strip**, **destinations**), and cited back to the source
of truth in comments. Nothing upstream ships verbatim; no Chromium assets,
icons, or trademarks appear anywhere.

Upstream code is BSD-3-Clause (Google). The full license text is preserved in
`docs/browser-shell/chromium-ref/lic/LICENSE`, and the referenced upstream
headers/spec files are mirrored under `docs/browser-shell/chromium-ref/` for
auditability.

## Ported artifacts (Phase 1)

| ZaminPanel artifact | Upstream source of truth | What ports, what adapts |
| --- | --- | --- |
| `apps/panel/src-tauri/src/shell/tabs.rs` | `chrome/browser/ui/tabs/tab_strip_model.{h,cc}` | Insert/selection semantics, the pinned-block invariant, foreground-opener inheritance, `DetachWebContentsAtForInsertion`-style detached tabs, the closed-tab memory (TabRestoreService subset). Adapted: destinations replace WebContents; groups are contiguous blocks (drag-formed group membership is a documented divergence). |
| `apps/panel/src-tauri/src/shell/layout.rs` | `chrome/browser/ui/tabs/tab_style.cc`, `ui/views/controls/tabbed_pane/…`, `chrome/browser/ui/layout_constants.cc` | The width law (232 standard, 24 pinned interior, minimums 32), 35+6 strip band, corner radii 10/12, overlap 18, close-button 16. Adapted: overflow shrinks and clamps where upstream scrolls (chevrons reserved for a later phase). |
| `apps/panel/src-tauri/src/shell/host.rs` | `chrome/browser/ui/browser_window/…` (views layer contract) | The frame-band/content split (one webview per tab, chrome-like band above). Adapted: Tauri's multi-webview window stands in for Views; the drag session ports only the detach magnetism (15 DIP, touch 50) and drop-based reorder — tear-off re-embeds the destination rather than moving a live WebContents (documented Phase-1 limit). |
| `apps/panel/src-tauri/src/shell/commands.rs` | `chrome/app/chrome_command_ids.h`, `chrome/browser/ui/browser_command_controller.h` | The command ID space (34014 new tab, 34015 close, 34016/34017 cycle, 34018+ select, 34028 reopen, 39001 focus address, 40009 bookmark bar, …) as ONE table for commands, keys, and tests. Adapted: window-control verbs are numbered locally (marked ⓩ in the table). |
| `apps/panel/src-tauri/src/shell/omnibox.rs` | `components/omnibox/browser/autocomplete_input.{h,cc}` | Fixup + classification: URL vs search vs the panel's own `zaminpanel://` scheme and join addresses. Adapted: the match model is ZaminPanel's three-way `AddressRequest`, not upstream's `AutocompleteMatch`. |
| `apps/panel/src-tauri/src/shell/bookmarks.rs` | `components/bookmarks/browser/bookmark_model.{h,cc}`, `bookmark_node.h` | Permanent nodes (bar / other), stable IDs, codec-style JSON persistence, bar visibility posture. Adapted: destinations, not URLs. |
| `apps/panel/src/frame/` (the frame view) | `chrome/browser/ui/views/frame/browser_view.h`, `tabs/tab_strip.h` | The band renders the model's snapshots: slots from the width law, drag phases fed to the ported controller, Chromium's keyboard contract (ADR-0032). Adapted: React view over Rust state, deliberately dumb geometry. |
| Session (in `host.rs` + `tabs.rs`) | `components/sessions/…`, TabRestoreService | Command-log-shaped persistence of the primary strip + a closed-tab close stack (≤25). Adapted: v1 restores the primary strip only. |

## Doctrine

1. **Behavior over pixels**: what ports is the model's semantics and the
   measured constants; what never ports is branding, assets, or upstream
   UI code verbatim.
2. **Every constant cites its upstream file** at the definition site — a
   reader can always trace a number back to `chromium/chromium`.
3. **No Chrome naming in the product**: the view layer is the *frame*; the
   word "chrome" survives only in upstream file paths and explicit
   upstream-behavior citations, which attribution requires.
4. **The ledger is a release gate**: a release that adds a ported behavior
   without a row here (and a citation in the code) does not ship.
