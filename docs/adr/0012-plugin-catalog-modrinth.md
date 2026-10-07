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

## Consequences

- The panel gains a Plugins tab per workspace: search, install (with live progress), installed list, delete. No plugin state lives in the daemon beyond the files themselves — the file system is the inventory.
- Update flows are installs with the target already present; the overwrite rule (fresh target or typed refusal) is decided by the downloader's existing discipline and revisited with real demand.
- A second catalog (CurseForge, Hangar) earns its place as another client behind the same engine methods; the protocol's `plugins.*` surface does not change.
