# ADR-0034 — The user-controlled storage root, and the creation contract

**Status:** Accepted · **Date:** 2026-10-11

## Context

Two demands landed together and share one theme: the product must
follow the operator's explicit choices instead of platform defaults.

1. **Where data lives.** The daemon's data root was the OS's Known
   Folders default — `%LOCALAPPDATA%\ZIM` on Windows, which means the
   C: drive on the classic small-SSD-plus-large-D: machine. A
   `--data-dir` flag existed, but nothing a user could reach ever passed
   it, and no setting could move the bulk after install.
2. **What a creation promises.** The New Server flow sent its request
   and then reported a message and a percent. The controls stayed
   editable while the download ran (an operator could "change" software
   that was already downloading), the Java auto-selection never named
   the runtime it would use, the EULA acceptance had no opt-in, and no
   surface distinguished the daemon's real lifecycle steps from a
   generic spinner. The raw material for all of it existed — byte-level
   progress callbacks, the jobs event stream, `java.list` — but the
   wire had no vocabulary for a *stage*, so clients could only guess.

## Decision

### The storage redirect (the pointer outside the data dir)

A pointer file — one absolute path, atomically written — lives at the
platform's **config** address (`platform::paths::storage_redirect_file`,
the previously-declared-but-unused `config_dir()`), never inside the
data dir: moving the data must never move the pointer that says where
the data is. The daemon resolves it exactly once, in `main.rs`, before
the engine exists. Precedence: an explicit `--data-dir` flag beats the
redirect (tests and launchers keep their authority); an unusable
redirect (empty, relative) is logged and ignored — the platform default
runs, the daemon still boots. A `--storage-redirect <file>` flag pins
the pointer's own address so integration tests isolate it exactly the
way `--catalog-url` isolates the network.

Two verbs (protocol §7h): `storage.get` answers the honest map — the
running root, the platform default, the pointer's own path, a pending
root when a stored redirect is not yet live, and the real `instances/`
`cache/` `java/` directories. `storage.set` validates loudly (absolute,
a directory or creatable, writable — the same probe the server
preflight uses), persists atomically, and answers the whole contract:
`restartRequired: true` (always, today) and
`serversInCurrentRoot` — because **the daemon never moves data**.
Changing the root is a first-run operation done honestly: a fresh
install points at `D:\ZIM` before any server exists; an existing
install that moves is told, with a count, what stayed behind. Silent
multi-gigabyte migration by a settings dialog is a footgun this ADR
deliberately refuses. `storage.set` is audited; the CLI and the panel's
Settings page both speak the verbs.

### The creation contract (the staged dialect + the explicit choices)

- **`server.create` gains `autoEula`** (additive, tri-state absence).
  `true` writes `eula.txt` with `eula=true` after the template stamp —
  before the first boot can ask. It is the operator's explicit
  per-creation acceptance, never a global default the daemon invents:
  the file itself says who accepted and how. Absent keeps the stamped
  `eula=false` and the typed `NEEDS_EULA` dialog. The panel remembers
  the checkbox (panel-local storage) but always sends the value
  explicitly — the daemon never guesses.
- **`JobProgress` gains `stage`** (additive). `server.create` speaks
  `preparing | files | eula | downloading | verifying | config |
  registering` through `JobCtl::progress_stage`; every other job keeps
  the plain verb and the field stays absent. A UI's stage checklist
  now derives from wire STATE instead of pattern-matching English
  sentences — the one authoritative lifecycle the frontend renders,
  never a second guess.
- **The modal locks.** Once the request is away every control freezes
  (software, version, build/loader, Java, port, id, name, mode doors);
  the request itself is the immutable snapshot, built once from the
  controls' values at submit time. The daemon's job closure already
  worked from moved values — the UI now tells the same truth.
- **The progress panel reports only measured facts.** Bytes done/total
  from the download subsystem's own callbacks; speed as the slope of
  the last ~3 s of REAL byte events; ETA from that slope and the
  remaining bytes; the artifact's name from the daemon's own message.
  A stall is an honest null (the ETA disappears), an artifact that
  publishes no size gets no fabricated fraction, and the quick stages
  show an empty track rather than a fake ramp. No timer drives
  progress anywhere.
- **Java auto-selection names its answer.** When the field is
  "automatic", the modal resolves the requirement against the
  machine's own `java.list` — the same candidates the daemon's
  `select_java` walks — and shows "Java 21 — selected automatically
  (vendor)" before and during creation, or the existing fetch
  affordance when nothing satisfies it.
- **The creation block.** A Minecraft-style block cracks through ten
  destroy stages mapped from REAL state: byte percent drives the long
  downloading phase, the walked checklist covers the quick ones.
  Cracks never heal (a running max), a size-less download holds its
  entry crack, success resolves through one exit beat paced by the
  job's own terminal state, and failure stops where it stopped — no
  perpetual "breaking". Reduced motion keeps the floor the motion
  tokens already own.
- **Cancellation is a first-class verb in the flow.** While the job
  runs, the modal's Cancel becomes "Cancel creation" — the daemon's
  real `jobs.cancel`, with the same typed-failure surfacing as every
  other outcome.

## Consequences

- The storage root is a settings-page row, a CLI verb, and a startup
  resolution — one pointer file, read once, never a scattered path
  convention. Existing tests are unaffected (they pass `--data-dir`,
  which wins by design); new storage tests pin the wire contract with
  an isolated pointer.
- The panel's Settings Storage section and the New Server modal are
  the two new surfaces; the total-CSS budget line is renegotiated in
  writing (the modal's stylesheet is lazy-chunk CSS that loads when
  the operator opens New Server, never at boot).
- Future artifact kinds (plugins, modpacks) ride the same staged
  dialect when they earn it; nothing in the cache or downloader
  changed — they were already right.
- Migration of an existing install's data remains deliberately
  unimplemented: an explicit copy flow would need drain/verify/swap
  semantics of its own and is not silently approximated here.
