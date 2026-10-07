# ADR-0013 — Software catalog: the second family (Fabric), a two-arm `SoftwareSource`

**Status:** Accepted · **Date:** 2026-10-07

## Context

The architecture review is explicit about how the software catalog grows: Paper/Purpur/Folia in V1 are **data** (catalog rows plus a URL-speaking client), never an inheritance hierarchy, and "a tiny `SoftwareSource`" is introduced **only when the second family actually lands** (§17.8). The demand is now real, and it is internal to the product rather than speculative: the Plugins tab (ADR-0012) already serves the mods family — a `mods/` directory means fabric, quilt, forge, neoforge loaders in `plugins.search` — while the New Server flow could not create the server those mods run on. An operator with a Fabric box registers it by hand or manages it with another tool; the catalog's zero-manual-JAR promise stops at the Paper family's edge.

The candidates for a second family were Fabric and Forge. Their upstream shapes decide the question more than taste does:

- **FabricMC meta API v2** is keyless and public (the ADR-0012 rule), serves version lists for game/loader/installer, and — decisively — publishes a **ready-to-run server launcher jar** at one stable URL (`/v2/versions/loader/{game}/{loader}/{installer}/server/jar`). No installer step, no build matrix, no process execution at create time. It fits the existing creation job unchanged.
- **Forge** distributes an installer that must be executed to produce a runnable server; the daemon would need a new job kind that runs upstream Java code before a server directory exists. That is a real feature with its own security and UX questions, and pretending it is "just another row" would hide them. Forge stays out until there is demand for the installer job itself.

## Decision

### A two-arm enum, not a trait zoo

`zamin_core::software::SoftwareSource` is an enum with exactly two arms — `Fill` and `FabricMeta` — and each catalog row carries its arm. The daemon dispatches per family at the three catalog touchpoints (`catalog.versions`, `catalog.builds`, `server.create`). A third family edits the enum and adds a client module; nothing else moves. If a third family never lands, the enum costs two arms — cheaper than any abstraction that generalizes a case that does not exist.

### Fabric speaks meta v2; the base URL is a daemon flag

`FabricMetaClient` mirrors `FillClient` and `ModrinthClient`: typed errors, honest request identification, and the base URL a parameter — tests point it at a local mock, air-gapped installs at a mirror (`--fabric-url`, default `https://meta.fabricmc.net`). The version lists pass through **upstream's own newest-first order** with their `stable` flags intact; unlike the Fill client there is no local re-sort, because the API answers in an array whose order is meaningful.

### Loaders are the family's "builds"; the wire stays honest

The protocol's `catalog.builds` returns build ids because Paper publishes numeric builds. Fabric has no such numbers, and fabricating stable ids would be a lie with a type. Instead:

- `catalog.list` entries carry `source` (`"fill"` | `"fabric-meta"`) so clients know which creation dialect applies without hard-coding catalog ids.
- For a fabric entry, `catalog.builds` returns an **empty** `builds` array and the **stable** loader versions, newest first, in the additive `loaders` field. Unstable loaders exist upstream but are not offered as defaults; an explicit pin can name one and the daemon resolves it.
- `server.create` gains an additive `loader` parameter (the pin; omit = newest stable). A numeric `build` on a fabric request is a typed `PROTOCOL_INVALID_REQUEST` — the wrong family's dialect — and unknown game versions, loaders, or installers are `CATALOG_NOT_FOUND` at resolve time, before any job exists, exactly the Fill family's fast-rejection rule.

All of this is additive under the protocol's stability rules (§10.1): no version bump, old clients keep parsing.

### Integrity: Fabric publishes no checksums, and the difference is stated

PaperMC publishes a sha256 per build; Modrinth publishes sha512. Fabric's meta API publishes **no checksum** for the launcher jar. The honest handling is not to invent one and not to pretend the download is verified: the shared downloader always hashes the stream, so a fabric creation records and reports the sha256 of what actually arrived (provenance in the job outcome), while the published-digest verification the Paper family enjoys remains its own. TLS protects the transport for both; only the Fill family gets pre-public verification. This asymmetry is documented here because a silent difference in a security property is worse than either property.

### Creation is the same job it always was

Template stamp → download the launcher jar into place as `server.jar` (byte progress, cancellable, all-or-nothing cleanup on failure) → per-server config (`mcVersion` = the game version, `javaMajorRequired` from the local table, port, optional `javaPath`) → register last. Fabric publishes no Java requirements, so the same local table the Fill family falls back to decides. The loader and installer versions ride the job's progress messages; no config field exists for them because nothing consumes one — a consumer (say, a future "update the loader" flow) earns the field when it arrives, the same rule ADR-0012 applied to the overwrite discipline.

## Consequences

- New Server offers Fabric alongside Paper, Purpur, and Folia; a fabric server lands with `mods/` already part of the layout expectations the Plugins tab derives — the two catalog surfaces finally describe one product.
- The panel swaps the build select for a loader select when the chosen entry speaks `fabric-meta`, preselecting the newest stable loader; the submit gate accepts either family's picker.
- `zamind` grows one mirror flag (`--fabric-url`); the conformance fixtures gain the two-source `catalog.list` and the loaders-shaped `catalog.builds` so both wire shapes are pinned.
- Forge — and any family whose creation needs to run upstream code — waits for an installer-job decision of its own; the enum's third arm is not free and should not be pretended cheap.
