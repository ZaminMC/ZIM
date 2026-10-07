# ZaminPanel

A polished desktop and CLI workspace for running, managing, and developing Minecraft servers.

## Workspace layout

| Crate | Role |
|---|---|
| `crates/zamin-protocol` | Zamin Protocol v0: envelopes, typed params/results, error codes, conformance fixtures. Pure data — no I/O. |
| `crates/zamin-ipc` | Framed local transports: Windows named pipe, Unix domain socket. Client and server sides. |
| `crates/zamin-core` | The engine that owns servers: registry, config, filesystem safety, Java discovery, lifecycle, logs. |
| `crates/zamind` | The resident daemon (ADR-0001): supervision actors, event hub, protocol sessions, adoption. |
| `crates/zamin-cli` | `zamin` — the command line client (second protocol client, Phase 2). |
| `crates/zamin-agent` | `zaminagent` — the remote bridge (ADR-0011): TLS + token auth for remote protocol clients, relaying to the local daemon. |
| `crates/zamin-bridge` | The panel host's forwarder: frame coalescing (~50 ms batches), down signals. Testable without a webview. |
| `apps/panel` | The desktop UI (Phase 3): a browser for Minecraft servers (ADR-0015) — tabs, the address bar, typed destinations — TypeScript protocol client + design system, Tauri 2 host. Develops in a plain browser against `apps/panel/dev-bridge.mjs`. |
| `crates/testing/fake-mc-server` | Deterministic Paper mimic used by the whole test matrix — no Java needed. |

## The zamin CLI

The CLI talks to `zamind` over the local transport and never touches
`zamin-core` (ADR-0002: clients know the protocol, not internals).

```
zamin list                          # every server + state
zamin status <id>                   # one server's details
zamin daemon                        # daemon version, server counts
zamin register <id> <dir> [--name]  # register an existing server directory
zamin rename <id> --name "New"      # change the display name
zamin remove <id> [--yes]           # unregister (directory untouched)
zamin start|stop|restart|kill <id>  # lifecycle verbs (protocol §5)
zamin logs <id> [--lines N]         # tail of logs/latest.log (file-backed)
zamin logs <id> -f                  # follow: recent buffer, then live
zamin attach <id>                   # console: logs in, lines to stdin; /quit detaches

zamin plugins search <id> [words]   # the catalog (ADR-0012), loader-faceted
zamin plugins versions <id> <pid>   # the pin list for one project
zamin plugins install <id> <pid> [--version ID] [--wait]   # a job; --wait follows bytes
zamin plugins installed <id>        # the target directory is the inventory
zamin plugins updates <id>          # the update check: the disk's bytes vs the catalog
zamin plugins delete <id> <file> [--yes]

zamin schedules list <id>           # the daemon's clock, per server (ADR-0014)
zamin schedules add <id> --name "nightly" --at 04:30          # daily restart
zamin schedules add <id> --name "backup" --every 3600 --backup # interval backup
zamin schedules add <id> --name "weekend" --weekdays sat,sun --at 09:00 \
      --command "say Restarting soon"
zamin schedules pause|resume|remove <id> <schedule-id>
zamin jobs list                     # installs, backups, downloads — running and finished
zamin jobs get|cancel <job-id>
```

Global flags: `--endpoint <socket|pipe>` (default: the per-user endpoint),
`--json` (pretty JSON of the raw protocol result; errors print the typed
error object and exit 1), `--timeout <secs>`.

Exit codes: `0` success, `1` operation/protocol failure, `2` usage error.

## Software catalog

New Server installs from the catalog with zero manual JAR handling. Two
upstream families speak today ([ADR-0013](docs/adr/0013-software-catalog-second-family-fabric.md)):
Paper, Purpur and Folia through the PaperMC Fill API (builds with
published sha256 digests), and Fabric through the FabricMC meta API
(version lists plus a ready-to-run launcher jar). Catalog entries carry
their `source`, so clients know which dialect applies — a numeric build
or a loader pin. Fabric publishes no checksums; the daemon records the
sha256 of what arrived instead of pretending a published digest was
verified. Forge waits for an installer-job decision of its own. Every
base URL is a daemon flag (`--catalog-url`, `--fabric-url`), so mirrors
and air-gapped installs work exactly like the plugin catalog's
`--modrinth-url`.

## Plugin catalog

The panel's Plugins tab installs from Modrinth
([ADR-0012](docs/adr/0012-plugin-catalog-modrinth.md)): search is faceted
by the server's own layout — a `mods/` directory means the
Fabric/Quilt/NeoForge/Forge family, everything else is Bukkit-family
`plugins/`. Installs resolve at request time, stream byte progress as a
job, verify the published sha512, and land atomically in the target
directory; the directory is the inventory. Re-installing the identical
file short-circuits on the sha512; an update that carries different
content answers the typed `PLUGIN_EXISTS` and lands only behind
`--replace` (the panel asks the same way). The catalog base URL is a
daemon flag (`--modrinth-url`), so mirrors and air-gapped installs work
exactly like the software catalog's `--catalog-url`. **Check updates** (`plugins.updates`) asks the catalog which of the
installed jars have newer versions: each jar's own sha512 identifies it
— no shadow state — and the verdicts read `up to date`, `update
available` (carrying the pin that applies it through `--replace` plus
`--retire`, which removes the old jar once the new bytes verify — an
applied update leaves one jar, not two), or
`unmanaged` (bytes the catalog does not publish). The CLI reaches the
same surface over SSH — `zamin plugins search/versions/install/installed/
updates/delete` plus `zamin jobs` for the long-running operations.

## Schedules

The daemon runs the clock ([ADR-0014](docs/adr/0014-schedules-daemon-runs-the-clock.md)).
A schedule is a named rule per server — a **when** (a fixed interval, a daily
time, or weekdays plus a time, all on the daemon's own clock) and a **then**
(a restart, a backup, or one console line). The daemon re-reads every store
every 15 seconds and fires what is due through its ordinary paths, so a
scheduled restart looks exactly like an operator's in the event stream,
jobs, and audit. Two rules keep it honest: a schedule never switches a
stopped server on, and missed firings are skipped, never replayed — a week
of downtime is not a firing storm at boot. `lastFiredMs` on the record is
the clock's only memory; the panel's Schedules tab and
`zamin schedules list/add/pause/resume/remove` are the two ways to author it.

## Players

The Players tab asks the server itself — Server List Ping plus the log's
join/leave roster, no plugins required. Selecting a name opens the
moderation cluster: **kick**, **op**, **deop**, **whitelist add**, and
**ban** (the lasting verb confirms first). There is deliberately no
moderation protocol: the panel composes ordinary console lines over the
same stdin path the Console uses, so the server executes, the answer
lands in the log, and events, jobs, and the audit tell the same story
they tell for a typed command. The composer guards the vanilla username
charset, verbs disable while the server is not running, and the notes
never claim an outcome they cannot know.

## The browser shell

ZaminPanel is a browser for Minecraft servers
([ADR-0015](docs/adr/0015-browser-shell.md), the
[founder vision](docs/founder-vision.md) made real). The sidebar is gone:
a tab strip wears every open destination — a server tab's favicon is its
state dot, live from the event stream — and a tool bar carries back,
forward, reload, and the address bar. Destinations are a closed type,
never strings: `zaminpanel://servers/` (the fleet), `zaminpanel://new`
(discovery), `zaminpanel://settings/`, and one tab per server. Identity
discipline holds everywhere: navigating to an open destination focuses
its tab, never duplicates it.

The address bar speaks three dialects: internal `zaminpanel://` URLs (an
unknown page renders an honest "No such page"), join addresses resolved
by port with host agreement — `localhost:25565`, `0:25565`,
`box.example.com:25565`; a miss says so and navigates nowhere — and free
text, which is a discovery query over the registry, executed live on the
new tab. Back/forward ride per-tab destination history; reload rebuilds
the view (state, subscriptions) and never touches the server process.
The keyboard is browser-honest: Ctrl+T/W/L/R, Alt+arrows, Ctrl+Tab, F6,
and Ctrl+K keeps the command palette. Dutchmen — the AI part of the
vision — is a documented reservation: the new tab's room is reserved,
the dialect will grow, nothing pretends.

## Development loop

```
cargo build --workspace  # once: links every binary the tests spawn
cargo test --workspace    # unit + conformance + lifecycle + e2e (no Java needed)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Panel (apps/panel) has its own gates — `npm test`, `npm run typecheck`,
`npm run lint` — and a browser smoke loop described in
[apps/panel/README.md](apps/panel/README.md).

`cargo build --workspace` produces every binary the tests spawn (`zamind`,
`zamin`, `fake-mc-server`); run it rather than per-package tests so the
integration harnesses find their binaries.

## Documentation

- [Architecture review](docs/architecture/ARCHITECTURE-REVIEW.md) — decisions and implementation order (§23)
- [Protocol v0](docs/architecture/protocol-v0.md) — the client boundary
- [ADRs](docs/adr/) — accepted decisions 0001–0014
- [Style guide](docs/development/STYLE-GUIDE.md) · [Testing](docs/development/TESTING.md) · [Glossary](docs/development/GLOSSARY.md)

## Install (Phase 7)

Windows ships as a per-user NSIS installer or a portable zip; Linux as an
AppImage or a portable tar.gz that doubles as the installer payload. Every
layout carries all four binaries — `zamin-panel`, `zamind`, `zamin`,
`zaminagent` — and the panel brings the daemon up on first contact
(ADR-0010).

Linux, no root, XDG everywhere:

```sh
# AppImage: run directly, or integrate it:
./ZaminPanel_0.1.0_x86_64.AppImage

# Portable tree: run in place…
tar xf ZaminPanel-0.1.0-linux-x86_64.tar.gz && ./zaminpanel-0.1.0/bin/zamin-panel
# …or install it (bins + launcher entry + icons; --uninstall reverses;
# --autostart on starts the panel at login):
./zaminpanel-0.1.0/install-linux.sh ./zaminpanel-0.1.0
```

Notifications follow one rule — crash and job completion, only while the
window is hidden or blurred — and the Ctrl+K palette offers "Start with the
system" where the host can deliver it. Bundles are built and tested by
[.github/workflows/bundle.yml](.github/workflows/bundle.yml) on both OS
lanes; the remaining §23 proof (clean VM installs) is manual by design.

## Remote (Phase 8)

A headless box runs the same binaries as a desktop. On the box: install
from the portable payload, keep the daemon alive with the service unit,
and expose the agent:

```sh
# read what the agent printed at startup (also in journalctl --user):
#   certificate fingerprint (pin this on remote clients): ab:cd:…
#   token file "/home/you/.local/share/zaminpanel/agent/token"

P=./zaminpanel-0.1.0/install-linux.sh
$P ./zaminpanel-0.1.0         # bins + launcher + icons (no root)
$P --service on               # zamind as a systemd user unit
$P --agent-service on         # zaminagent as a systemd user unit
```

Windows boxes mirror it: `install-windows.ps1 payload -Service on` and
`-AgentService on` create per-user Task Scheduler logon tasks.

On the desktop, the panel's footer has a connection chip (default
"Local"): add the box — address `host:port`, the token from its token
file, and the fingerprint the agent printed (the pin is the server
identity; the token is the credential, sent as `daemon.hello` `auth`
inside TLS). Everything works as if the server were local: fleet,
lifecycle, console, files, backups. The shipped app dials the agent
from Rust — the Tauri host opens the pinned TLS relay itself, so the
chip behaves identically in the installed panel and in the browser dev
loop (the Node bridge relays the same way). Schedules live on the box:
the daemon fires them from its own clock, so a nightly restart happens
whether or not any panel is watching. See
[ADR-0011](docs/adr/0011-remote-transport-agent-tls-auth.md) for the
threat model — no CA, no trust store; a pinned fingerprint or an explicit,
discouraged skip-verify.
