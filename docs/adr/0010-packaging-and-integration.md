# ADR-0010 — Packaging & integration

**Status:** Accepted · **Date:** 2026-10-06

## Context

Phase 7 (§23) delivers "installer + portable (Win), AppImage + tar.gz (Linux), `.desktop`, notifications, autostart" with the proof of done "real installs on clean Win11 + Ubuntu/Arch VMs". Decisions made now shape what those VM installs actually are: which binaries ship, who starts the daemon, how deep desktop integration goes, and what notifications owe the operator.

## Decision

### Everything ships together; the panel brings the daemon

One artifact set carries all three binaries: the panel host, `zamind`, and `zamin`. Tauri's `externalBin` (sidecars) installs them next to `zamin-panel` in every layout — NSIS install dir, AppImage `usr/bin`, portable tree. The host resolves the daemon by sibling lookup, then `PATH` (development).

`daemon_ensure` (host command) makes ARCH-REVIEW §1.2 concrete: on connection refusal the webview asks the host to probe the per-user endpoint, spawn the sibling daemon, and wait for bind before the client's retry schedule continues. Single-instance races remain the daemon's job — `IpcServer::bind` answers `AlreadyRunning` and the loser exits(1), so a simultaneous spawn from two panels is a race we are allowed to lose. The daemon is spawned detached (no kill-on-close on either platform); a watcher thread reaps it if it exits early.

The installed binary is named `zamin-panel` (`mainBinaryName`), matching sibling resolution and the install script — not the product name's spelling.

### Windows: NSIS, per-user

`installMode: "currentUser"` — no UAC, per-user install, matching the per-user daemon model (ADR-0001). The portable zip (three exes + README) is the no-installer alternative. macOS (`app`/`dmg`) is removed from bundle targets until macOS is actually a supported platform — a bundle we cannot test is a claim, not a deliverable.

### Linux: AppImage + portable first, no-root XDG install

Per ARCH-REVIEW §12.2: AppImage + portable tar.gz; `.deb`/`.rpm` once stable; never a distro-ifdef. The portable archive is a runnable tree (`bin/` + `share/`) and simultaneously the payload `install-linux.sh` consumes: bins to `$prefix/bin`, desktop entry with `Exec` rewritten, hicolor icons, idempotent upgrades, clean uninstall, `--autostart on|off`. No root anywhere; XDG everywhere.

### Autostart: the boring mechanisms

Linux: an XDG autostart entry in `$XDG_CONFIG_HOME/autostart` (honored by GNOME and KDE). Windows: the `HKCU\...\Run` key. Both trivially removable, both offered through the palette command "Start with the system", which is hidden honestly in a plain browser. AppImage-aware: the entry's `Exec` is `$APPIMAGE`, not the dying `/tmp` mount. The systemd user unit / Task Scheduler task variants belong to Phase 8 (services), where the daemon gets a service story of its own.

### Notifications: the taxonomy lives in the webview

The host registers `tauri-plugin-notification` and delegates every decision to the webview (`integration/notifications.ts`), where the rule is operator empathy: notify only what the operator must know *while not looking* — a server crash and a job completion — and only while the window is hidden or blurred. The focused window already owns the in-app surfaces (crash card, progress chips). The decision matrix is pure and unit-tested; delivery never throws and stays silent in a plain browser.

### What the CI proves and what only a VM can

The `bundle.yml` workflow proves, on both runner OSes: packaging-script gates (shellcheck, `desktop-file-validate`), the install-script round trip, the host crate's clippy + tests (the main CI cannot run them — the host is workspace-excluded), and the actual bundling. The §23 proof — clean Win11 / Ubuntu / Arch VM installs — stays a manual step; the workflow exists so that step exercises artifacts that already built and tested green, never surprises.

## Consequences

- The daemon's default endpoint and data dirs (ADR-0007/0008) are the installed defaults — no flags needed at first run.
- Upgrading an installed copy re-runs the same steps (idempotent); uninstall never touches daemon state or server roots.
- Windows autostart writes the registry through `winreg` inside the host crate — the one place OS-conditional code lives outside the workspace seam, as documented in ADR-0008's host carve-out.
- Deferred: `.deb`/`.rpm`, Flatpak, macOS bundles, auto-update channels.
