# ADR-0017 — Publish: the sixth slice, done honestly (founder §40–47, §74, §89)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder's sixth vertical slice (§89) names the publish story: *selection, diff, security scan, changelog generation, package creation, provider API, upload, recovery*. The §40–47 sections carry the rules; §74 carries the job shape. Standing scope (the founder's own scoping note and every session since) ignores the AI part and keeps its rooms — so **§43's Dutchmen-generated changelog does not ship**: the changelog is a plain, operator-edited string, and the room is named (`Generate with Dutchmen — reserved` in the panel; no wire method) until real demand names it.

Three founder rules shape everything else. §40: *"Do not hardcode marketplace-specific behavior into the core. Create a publishing provider interface."* §41: *"Never blindly package the entire server directory."* §47: *"Marketplace/API credentials must never be stored in plain project configuration … never put API tokens in server files, published packages, logs, AI prompts, Git repositories."*

## Decision

### The §41 selection is structural, not advisory

A publish packages a **selection**: include rules (a whole `folder:`, one exact `file:`, or a `glob:` with `*`/`?` inside a segment and `**` spanning segments) minus exclude rules — excludes win. An empty include list selects **nothing**; `publish.execute` refuses with `PUBLISH_NOTHING_SELECTED` rather than "conveniently" packaging the root. Resolution is a containment-checked, sorted walk that hashes every selected file (sha512); symlinks are skipped on purpose — a publish never follows a link out of the root, and a link inside it is machine-local convenience, not portable content. A file that vanishes mid-walk is "removed as of now" (the diff's business); the packaging stage re-reads everything and fails loudly if the world moved between resolve and read. Matching is byte-exact and case-sensitive — an operator who wrote `Plugins/` meant `Plugins/`.

### The §42 diff is digest-merge, and the record commits last

The publication record (`publication.json`, under the server's daemon metadata — **never inside the server root**, so a publish cannot publish its own record) stores what actually went out: per-file sha512 + size, package digest, provider receipt. The preview's M/A/D inspection is a merge over that map against the fresh resolve — mtimes are never consulted, so the diff cannot lie. Writes go through the atomic discipline; a torn write leaves the **old** record, never a half one, and a corrupt record is a loud typed error — a silent "never published" would make the next diff say "everything added". The record commits **only after the provider answers**: a crash mid-upload leaves the previous record and a stray package, which the next publish overwrites. That is §42's "never leave the publication state corrupted", read as a state machine rather than a promise.

### The §44/§46 scanner is layered, extensible, and honest about its limits

Detectors are additive vocabulary, not a monolith: token-pattern shapes (Discord bot tokens — three base64url dot-separated segments — AWS, GitHub, Slack, Stripe, Google, JWT, webhook URLs, PEM headers), the §46 configuration-key heuristic (a key that *says* secret with a value that looks real; template references, booleans, pointers like `token-file:`, and obvious placeholders are skipped), generic high-entropy (deliberately `low` severity — the noisiest detector must not block), sensitive filenames, and the DiscordSRV advisory. DiscordSRV gets the founder's special treatment two ways: any bot-token detection inside its directory escalates to critical, and the advisory fires when the daemon can honestly say the plugin is *loaded and functioning* — the server Running **and** the jar present in `plugins/`. Excerpts are **redacted everywhere** (a key name, a masked preview with the length): a finding is evidence of a pattern, not a copy of the secret, so it can ride the wire, the panel, the CLI, and the logs without §47 becoming a lie.

Blocking is a policy, stated once in core: an **unreviewed finding above `low`** refuses the automatic path. §46's "false positives must be reviewable" is a first-class verb — `publish.review.set` persists the `(file, kind)` pair; reviewed findings stop blocking but stay visible. §45's "Publish Anyway" is the explicit override: `confirmUnsafe: true` on the wire, a two-step confirmation in the panel (arm, then a typed second click), a flag in the CLI. The UI says what §46 demands: the scan is a safety mechanism, not a guarantee.

### The §40 provider interface, with honest built-ins

`PublishProvider` is the seam: id, display name, credential needs, settings validation (unknown keys are refused — a typo in `outDir` must not silently mean "no destination"), and `upload`. Nothing marketplace-specific lives in the pipeline; BuiltByBit and friends arrive as new implementations. Two built-ins make the story real today without inventing an API: **archive** (the package is built and kept; the receipt says no upload happened) and **local-dir** (the package and its receipt land in an operator-chosen absolute folder — a watched/synced folder makes this an actual distribution channel). §74's stages ride the ordinary job system: preparing → scanning → packaging → uploading, as typed progress events, with "waiting for the provider" folded into upload for the built-ins (a marketplace provider's real wait is its own implementation detail). The gate and the provider are checked **before** the job exists (the java.install convention); the job re-runs resolve and scan on fresh disk so a publication is never packaged from stale knowledge.

### §47 is a contract the daemon keeps by having nowhere to break it

The publish config, the record, the receipts, and the audit carry **no credential field**. A provider that needs one reads `ZAMIN_PUBLISH_CREDENTIAL_<ID>` from its environment at execute time; a missing one is a typed `AUTH_REQUIRED` naming the variable. The OS's secure storage (Windows Credential Manager and friends) is the desktop host's reserved room: it puts the secret into that channel for the daemon it spawns. The daemon never persists what it receives.

## Consequences

- Wire: `publish.config.get/set`, `publish.providers.list`, `publish.preview`, `publish.execute` (a job), `publish.state`, `publish.review.set`; codes `PUBLISH_NOT_CONFIGURED`, `PUBLISH_NOTHING_SELECTED`, `PUBLISH_SELECTION_TOO_LARGE`, `PUBLISH_SECRETS_DETECTED`, `PUBLISH_UPLOAD_FAILED`; `JobKind::publish.execute`.
- Packages are deterministic (fixed timestamps, path order): identical content packages byte-identically, so the package digest is a real identity, and the embedded `zamin-publish.json` manifest makes every artifact self-describing.
- Caps (20k entries / 2 GiB) and cancellation are enforced in the packaging core, with the staging file cleaned on a cancelled pack.
- The §43 changelog room is the only slice item not built — reserved, named in the UI, absent from the wire, per standing scope. Marketplace providers are the same story by design: the interface is the room.
- Still open from the founder's machinery, deliberately: drag reorder and real window movement (§50), Mute, vertical tabs, Forge (demand-gated), the SQLite log index (deferred).
