# ADR-0024 — The development channel: a public releases repo, signed installers, and the update lane

**Status:** Accepted · **Date:** 2026-10-08

## Context

The panel is a desktop app whose code repo is private — and the founder
asked for a development build anyone can install, with an update check
and automatic updates built in. Three facts shape the decision:

- **A private repo cannot carry a public channel.** Release assets and
  their URLs need anonymous reads; everything inside
  `ZaminMC/ZaminPanel` needs auth. The updater's endpoint is fetched by
  users who do not exist yet.
- **The updater must not trust its channel.** A build that replaces
  itself arrives over the network; without signatures, a compromised
  mirror is a compromised machine.
- **A dev channel without version discipline breaks silently.** The
  updater compares versions and refuses to downgrade; a channel that
  re-publishes `0.1.0` forever would tell every install "you are up to
  date" forever.

## Decision

### The channel lives in a separate public repo

`ZaminMC/ZaminPanel-Releases` holds exactly what the name says: the
NSIS installer, the AppImage, the portable archives, and the updater's
`latest.json` — published under one fixed `dev` tag that every release
run deletes and re-creates (assets, notes, and manifest replaced). The
code repo stays private; the workflow that publishes rides the
`Release (dev)` dispatch in `.github/workflows/release-dev.yml` and
authenticates to the channel repo with the `RELEASES_TOKEN` secret.

The fixed tag is deliberate: the updater endpoint is a constant —
`https://github.com/ZaminMC/ZaminPanel-Releases/releases/download/dev/latest.json`
— baked into `tauri.conf.json`, so installs never reconfigure and the
check is one stable URL. "Latest" release aliases would not work here
(GitHub excludes pre-releases from them, and a dev channel is nothing
but pre-releases).

### Every artifact is signed; the manifest carries the signatures

The release lane generates the updater keypair once (`tauri signer
generate`); the private key lives only in the code repo's Actions
secrets (`TAURI_SIGNING_PRIVATE_KEY` + password) and nowhere else —
not in the channel, not in any bundle. Both lanes build with
`createUpdaterArtifacts: true`, so the bundler emits `<installer>.sig`
minisign signatures next to each installer. The publish job runs
`scripts/packaging/make-updater-json.mjs`, which refuses to emit a
manifest entry whose signature is missing or empty — an unsigned
installer must never be offered. The public key is committed in
`tauri.conf.json`; installs verify the download against it before
applying.

### The version is the run number

A dev build's version is `0.1.<run number>`, stamped into
`tauri.conf.json` at build time (never committed). Every run is
therefore strictly greater than the previous one, and the updater's
"offer only newer versions" rule works unchanged. Nothing else about
the version scheme changes: tagged source releases (`v*`) keep the
bundle lane's explicit version.

### The panel decides; the plugins perform

The webview owns the taxonomy (the house rule — the host is a bridge,
ADR-0003). `integration/updater.ts` wraps `tauri-plugin-updater` and
`tauri-plugin-process` behind a Result-answering backend (never
throws); `state/updates.ts` is the decision machine with an injected
backend, so the whole lane is testable in a plain browser and degrades
to an honest "unavailable" in dev. Two preferences govern:

- **Check automatically** (default on): boot + every six hours, and
  the boot check is the lane's first duty — the store, the chrome
  notice, and the Settings rows ride the entry chunk (the budget raise
  is written down in PERFORMANCE-BUDGETS.md).
- **Install automatically** (default on): an offered update downloads
  and installs by itself; **the restart never is** — killing the
  operator's session is not "automatic", it is a takeover (§82). The
  "ready" state says "restart to switch to it" and waits.

"Available" when auto-install is off renders in the Settings updates
rows, not the chrome — a decision the operator must make belongs where
the decision lives. The chrome notice speaks only for what must be
acted on: a running install, a pending restart, a failure (§81 — a
human sentence carrying the technical message, retry and dismiss as
verbs).

## Consequences

- Users install from
  `https://github.com/ZaminMC/ZaminPanel-Releases/releases` (dev tag,
  pre-release flagged) and updates arrive without touching GitHub.
- The release lane reuses the bundle lane's build steps but skips its
  host-crate gates: `bundle.yml` still runs clippy + host tests on the
  same sources, and the release lane's job is packaging, not proving.
- The channel repo grows one asset set per run; old run's assets are
  replaced, never accumulated.
- Rotating the signing key means: new keypair, new `pubkey` commit, a
  release run to re-sign — installs built with the old public key
  update one last time from a manifest signed by the old key, then a
  fresh install is required. The docs say so; the workflow does not
  pretend to solve key rotation for deployed installs.
