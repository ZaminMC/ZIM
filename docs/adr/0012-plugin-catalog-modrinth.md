# ADR-0012 — Plugin catalog: Modrinth, loaders as data, sha512-pinned installs

**Status:** Accepted · **Date:** 2026-10-07

## Context

The architecture review defers "plugin install/update from catalogs" as safe to defer — deferred, not abandoned. The software catalog (ADR-0010's context; `zamin-core::software`) already proved the shape: rows in a table plus a URL-speaking client, verified downloads, and the New Server flow that needs zero manual jar handling. What must be decided for plugins is which catalog speaks first, where files land, what is verified, and how much the daemon claims to know about a server it did not create.

The constraints that matter:

- A keyless, public API is required — an API key per home-lab install is a support burden, not a feature. CurseForge requires one; Modrinth does not.
- The daemon tracks servers as directories, not as software facts: `server.get` returns `software: None` for registered servers. Anything derived from "what software is this" would be a guess wearing a type.
- Plugin jars land inside a server root that ADR-0009 defends; a filename arriving over HTTPS is as untrusted as a zip entry.
- Installs are long file operations; the daemon has exactly two other such operations (backups, downloads) and both run as cancellable jobs with progress.

## Decision

### Modrinth v2 is the V1 source; the client is a parameter

`zamin-core::plugins::ModrinthClient` mirrors `FillClient`: three concerns (search, versions, download), typed errors, base URL a parameter — tests point it at a local mock, air-gapped installs at a mirror. Search is `GET /v2/search` with a loader facet group; versions are `GET /v2/project/{id}/version`, filtered by loader client-side where the URL stays a plain GET.

### Loaders are data; the target directory comes from the disk

Two tables, not an enum hierarchy: `loaders_for_target` (which Modrinth loader slugs a target accepts) and `target_for_root` (which target a server wants). The target is derived from the filesystem — a `mods/` directory in the server root means mods, anything else is `plugins/`. This is the only honest reading of a server the daemon did not create: the directory that exists is the truth on disk, and a created-but-never-run Paper server simply gets `plugins/` created on first install.

### The daemon never claims to know the server's Minecraft version

Search carries no game-version facet. `plugins.versions` returns each version's `game_versions` so the panel can display and pin; `plugins.install` takes the latest version for the loader unless the caller pins `versionId`. When the panel someday learns the game version (server.properties parsing, ping payloads), this ADR needs no change — the pin already exists.

### Integrity: sha512, verified; filenames: sanitized, always

Modrinth publishes sha1 and sha512; the downloader verifies sha512 — the downloader's checksum parameter becomes an algorithm-carrying value instead of a sha256 string, and the atomic staging discipline (staging file → fsync → rename) is unchanged. Filenames from the API pass `safe_file_name` before touching disk: no path separators, no `..`, no Windows reserved names, no control bytes, no trailing dots/spaces, 255-byte cap. Deletes sanitize the same way and refuse symlinked targets.

### Installs are jobs

`plugins.install` returns a job (`JobKind::PluginInstall`) with byte progress and cancellation, exactly like `server.create` and `java.install`. The daemon's rule stands: every long file operation is a job, visible in `jobs.list`, audited like any mutating command.

### The overwrite rule (amended 2026-10-07): identical bytes short-circuit, different bytes ask

The original decision deferred the overwrite discipline "until real demand". The demand is real — updating a plugin by delete-then-install by hand is the exact manual jar handling this catalog exists to remove — so the rule is now decided, and it keeps three properties the deferred note promised: no shadow state (the disk's checksum decides), no new client obligation, and the shared downloader's atomic discipline untouched.

- A target file whose sha512 matches the published digest means the identical file is already installed: the install short-circuits to success without a download. Re-installing is idempotent, and the proof is the disk's own digest.
- A target file with different content is the operator's decision, not the daemon's: `plugins.install` answers the typed `PLUGIN_EXISTS` (file in context) at resolve time, before any job exists. `replace: true` — the CLI's `--replace`, the panel's explicit Replace button — lands the verified download over the old file atomically; a failed download leaves the old bytes intact because the rename is the last step.
- The rule lives in the shared installer (`install_file`) and rides `DownloadOptions.replace`; the server-jar download keeps its never-overwrite discipline (a jar download races a running server's jar in a way a plugin install never does), and the JDK fetch stays fresh-directory only.

The genuinely different-file update (a version bump that publishes a new jar name) is two ordinary installs plus a delete — the daemon cannot and should not map jar names to projects (the directory is the inventory), so composing those verbs stays client-side, where the version list already carries the file names.

### The update check (added 2026-10-07, the rule's read side)

The overwrite rule made *applying* an update one explicit step; *noticing* one still meant the operator eyeballing Modrinth by hand — the last manual jar handling in the story. The update check (`plugins.updates`) closes it without revisiting a single rule above. It never invents state to remember which project a jar came from: each jar's own sha512 identifies it (the disk's checksum decides, exactly as the overwrite rule words it), Modrinth's version-from-hash endpoint answers which version carries those bytes, and the project's version list — filtered by the same loader rule the install path uses — decides the verdict:

- `up-to-date` — the digest matches the newest installable version's published digest. Nothing else is claimed: a project may publish newer versions for other loader families and the server's directory still hears "up to date".
- `update-available` — a newer (or different-loader) installable version exists. The entry carries the full recipe (`projectId`, both version numbers, the `latestVersionId` pin), so applying it is the overwrite rule's own flow — `plugins.install` with the pin and `replace` — through the CLI, the panel, or any third client. A foreign-family jar (say, a fabric build copied into a paper server) reads `update-available` on purpose: the update *is* the fix, replacing the wrong-family bytes with the proper ones.
- `unmanaged` — the catalog has no file with these bytes (a jar dropped in by hand), or it knows the bytes but publishes nothing installable for this loader family. The operator action is "none through the panel", said plainly instead of inventing a version to click.

The check is an explicit request, not a startup obligation or a background poll: it costs the operator one round trip per recognized jar, network access stays a user decision (the same trade `plugins.search` made on day one), and a server whose target directory does not exist yet answers an empty report.

## Consequences

- The panel gains a Plugins tab per workspace: search, install (with live progress), installed list, delete. No plugin state lives in the daemon beyond the files themselves — the file system is the inventory.
- Update flows are installs with the target already present; the overwrite rule is landed — see "The overwrite rule" above: identical bytes short-circuit, different bytes answer `PLUGIN_EXISTS` and land only behind the explicit `replace`. The update check (`plugins.updates`) tells the operator which rows want that flow; the verdicts are computed from the disk's bytes on demand, never stored.
- A second catalog (CurseForge, Hangar) earns its place as another client behind the same engine methods; the protocol's `plugins.*` surface does not change.
