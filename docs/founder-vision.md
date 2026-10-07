# Founder Vision — the UI and UX

> Provenance: supplied by the founder as the product's source of truth
> (uploaded into the repository as docs/ZaminPanel-Development.zip, file
> `Founder Vision the ui and ux.txt`, and committed here verbatim).
> Scoping note that travels with it, in the founder's own words:
> "From this, explicitly ignore the AI part, but keep a room for it."
> ADR-0015 records how the shell implements this document.


From this, explicitly ignore the AI part, but keep a room for it.

ZaminPanel — Founder Vision & Complete Implementation Prompt

0. READ THIS FIRST

You are implementing ZaminPanel, a desktop application for managing Minecraft servers.

Do not reinterpret ZaminPanel as a conventional Minecraft server control panel.

Do not build:

- a Pterodactyl clone
- a generic hosting dashboard
- a sidebar-first admin panel
- a collection of independent management pages
- a web dashboard where servers are merely cards
- a desktop wrapper around an existing panel
- an AI chatbot bolted onto a server manager

The central product idea is:

«ZaminPanel is a browser for Minecraft servers.»

The user should be able to install ZaminPanel, double-click a supported Minecraft server JAR, and have that server open inside ZaminPanel.

The mental model must be:

Chrome
 ├── Tabs
 ├── Address bar
 ├── Back / Forward
 ├── Reload
 ├── New tab
 ├── Tab groups
 ├── Windows
 ├── Bookmarks
 ├── Extensions
 └── Pages

ZaminPanel
 ├── Tabs             → Minecraft servers / internal pages / Dutchmen chats
 ├── Address bar      → server join addresses / ZaminPanel internal URLs
 ├── Back / Forward   → navigation inside supported tab content
 ├── Reload           → reload current server/page state
 ├── New tab          → Dutchmen-centric new tab
 ├── Tab groups       → server organization
 ├── Windows          → independent ZaminPanel windows
 ├── Bookmarks        → servers, Dutchmen chats, internal pages
 ├── Extensions       → ZaminPanel addons/community integrations
 └── Pages             → servers and internal ZaminPanel pages

The browser metaphor is not decorative.

It is the actual product architecture and UX model.

---

1. EXISTING PROJECT ARCHITECTURE

The repository already has an architecture and documentation defining:

ZaminPanel
     │
     │ Zamin Protocol
     ▼
   zamind
     │
     ▼
 ZaminCore

ZaminCLI also communicates through the same core/protocol architecture.

The daemon/core owns deterministic server-management operations.

Dutchmen must NOT become a second backend.

The intended architecture is:

                    ┌──────────────────────┐
                    │      ZaminPanel      │
                    │   React + Tauri      │
                    └──────────┬───────────┘
                               │
                        Zamin Protocol
                               │
                    ┌──────────▼───────────┐
                    │        zamind         │
                    │   ZaminCore engine   │
                    └───────┬───────┬───────┘
                            │       │
                         Servers  System

Dutchmen should integrate with this architecture rather than bypass it.

Preferred conceptual architecture:

ZaminPanel / Minecraft Bridge
            │
            ▼
     Dutchmen Agent Runtime
            │
            ├── Context
            ├── Tool Registry
            ├── Planner
            ├── Permissions
            ├── Confirmation
            ├── Job Manager
            ├── Verification
            ├── Audit
            │
            ▼
       ZaminCore / zamind
            │
            ▼
      deterministic tools

The AI model is replaceable.

The ZaminCore tool/runtime architecture is not.

Do not put unrestricted AI logic inside the daemon.

AI failure must never be able to crash or corrupt the server-management daemon.

---

2. FIRST PRINCIPLE: ZAMINPANEL IS A BROWSER

The application should visually and behaviorally feel like a polished modern browser.

Think:

- Chrome-level familiarity
- Safari-level visual polish
- browser tab behavior
- browser address-bar behavior
- browser window behavior
- browser extension concepts
- browser-style internal URLs
- browser-like navigation

But do not simply copy Chrome's visual design.

ZaminPanel should have its own visual identity.

The user should be able to understand the interface immediately because the browser metaphor is familiar.

---

3. INSTALLATION EXPERIENCE

The installer should install ZaminPanel normally.

After installation:

Desktop
└── ZaminPanel shortcut

The user does NOT need to manually configure:

start.bat
java -jar ...

for normal ZaminPanel operation.

A supported Minecraft server JAR should be associated with ZaminPanel.

Supported server software includes:

- Paper
- Purpur
- Spigot
- Folia
- other explicitly supported Minecraft server JAR formats

The exact detection mechanism must be reliable.

Do not identify a server merely because a file is called:

server.jar
paper.jar
minecraft.jar

A real compatibility/detection layer must determine whether the JAR is supported.

---

4. DOUBLE-CLICKING A SERVER JAR

This is one of the defining ZaminPanel experiences.

Example:

paper-26.2-129.jar

User double-clicks it.

Instead of Windows launching a generic Java console window, ZaminPanel should open.

Conceptually:

User double-clicks Paper JAR
             ↓
       ZaminPanel opens
             ↓
      server is recognized
             ↓
      server becomes a tab
             ↓
      server process is managed
             ↓
      server page is displayed

The implementation must account for:

- file association
- multiple server JARs
- existing ZaminPanel instance
- multiple ZaminPanel windows
- already-running servers
- duplicate opening
- unsupported JARs
- corrupted/incomplete JARs
- server directories
- process adoption
- PID reuse
- safe process identity

Do not compromise the existing process-safety architecture.

---

5. APPLICATION WINDOW

The application should have a browser-style top chrome.

Conceptually:

┌───────────────────────────────────────────────────────────────┐
│ tab  tab  tab  +                              window controls │
├───────────────────────────────────────────────────────────────┤
│ ←  →  ⟳   [ address / search / internal URL ]       ⋮        │
├───────────────────────────────────────────────────────────────┤
│ bookmark bar                                                  │
├───────────────────────────────────────────────────────────────┤
│                                                               │
│                         CONTENT                               │
│                                                               │
└───────────────────────────────────────────────────────────────┘

The top browser chrome should be compact.

Do not waste huge vertical space on navigation.

---

6. TAB SYSTEM

Tabs are first-class application objects.

A tab can represent:

1. a Minecraft server
2. a ZaminPanel internal page
3. a Dutchmen conversation

There are three primary tab types.

---

6.1 ZaminPanel settings tab

Internal URL:

zaminpanel://settings/

This is an actual internal ZaminPanel page.

---

6.2 New tab

New tab has an empty browser-style page.

The address bar is empty.

The center of the page contains a large search/chat input.

However:

«This is NOT a web search box.»

It is the Dutchmen input.

Example:

┌─────────────────────────────────────────────┐
│                                             │
│                                             │
│              What can I help with?          │
│                                             │
│   [ What are the active servers?        ]   │
│                                             │
│                                             │
└─────────────────────────────────────────────┘

The new tab should feel like a blank browser page.

Dutchmen is the primary action.

---

6.3 Server tab

The address bar normally displays the server's joinable address.

Examples:

0:25565
192.168.1.50:25565
example.com:25565

The server's address is not merely decorative.

It identifies the server tab.

If a locally-running server exists but its tab is currently closed, the user should be able to enter its join address in the address bar and open the corresponding server page.

There should also be an easy way to discover currently active servers.

For example:

Active servers
├── localhost:25565
├── 192.168.1.50:25566
└── ...

---

7. ADDRESS BAR

The address bar must understand multiple things.

It should support:

Server addresses

0:25565
localhost:25565
192.168.1.50:25565
example.com:25565

Internal ZaminPanel URLs

zaminpanel://settings/

Dutchmen conversations

After beginning a Dutchmen conversation:

dutchmen:(chat-ID)

Example:

dutchmen:(a8f72c)

The address bar should reflect the current destination.

---

8. DUTCHMEN NEW-TAB TRANSITION

This transition is extremely important.

Do not make it a boring:

input
↓
chat page

transition.

The interaction should feel like a polished Apple/Chrome-level interface transition.

Initial state:

                 [ Ask Dutchmen... ]

User types:

What are the active servers?

and submits.

The input should:

1. transition downward
2. resize/reposition
3. become the chat composer
4. reveal the conversation area
5. introduce a left-side conversation/sidebar structure
6. move the original user message into the conversation
7. animate Dutchmen into the conversation
8. preserve visual continuity between the new-tab input and the resulting chat UI

The animation should feel physically coherent and intentional.

No cheap fade-to-page.

No generic loading spinner.

No abrupt layout replacement.

The feeling should be:

«“The new-tab Dutchmen interface transformed into a conversation.”»

The animation system must remain performant and interruptible.

---

9. DUTCHMEN CHAT

Dutchmen is a first-class ZaminPanel feature.

It should support multi-turn conversations.

Example:

User:
What are the active servers?

Dutchmen:
I found 3 active servers:
...

Then:

User:
Start a profiler in all of them for 1 hour.

Dutchmen should understand that:

"them"

refers to the previously identified active servers.

It should maintain conversation state.

---

10. DUTCHMEN IS AN AGENT, NOT A CHATBOT

Dutchmen must be capable of executing real ZaminPanel operations.

The model should never directly receive unrestricted operating-system access.

Instead, it operates through typed tools.

Conceptually:

User
 ↓
Dutchmen model
 ↓
Agent runtime
 ↓
typed tool call
 ↓
ZaminCore
 ↓
result
 ↓
Dutchmen
 ↓
next action

Example:

!dutchmen replace scoreboard title with &c&lSurvival

Dutchmen should be able to:

inspect installed plugins
        ↓
identify scoreboard-capable plugin
        ↓
inspect configuration
        ↓
find scoreboard title
        ↓
modify correct configuration
        ↓
validate
        ↓
reload plugin
        ↓
verify

The model must not simply hallucinate:

/plugin reload whatever

It needs actual environment information.

---

11. DUTCHMEN TOOL SYSTEM

Create a typed tool registry.

Potential tool categories:

server.*
plugin.*
config.*
world.*
player.*
process.*
filesystem.*
network.*
performance.*
schedule.*
package.*
publish.*
limbo.*
minecraft.*
job.*

Examples:

server.list
server.status
server.start
server.stop
server.restart

plugin.list
plugin.inspect
plugin.search
plugin.install
plugin.update
plugin.reload
plugin.uninstall

config.read
config.search
config.patch
config.validate
config.reload

world.region.replace
world.schematic.download
world.schematic.validate
world.schematic.load
world.schematic.paste
world.schematic.rotate

player.current_context
player.list
player.send_message
player.move_to_limbo
player.return_from_limbo

performance.start_profile
performance.stop_profile
performance.report

job.create
job.status
job.cancel

publish.preview
publish.scan
publish.execute

Every tool must have:

- typed input schema
- typed output schema
- permissions
- preconditions
- postconditions
- error types
- audit information
- confirmation requirements where applicable

---

12. NEVER GIVE DUTCHMEN AN ARBITRARY SHELL TOOL

Do NOT expose:

shell.execute(command)

as a general-purpose AI tool.

Do not allow the model to invent arbitrary:

PowerShell
cmd
bash
Java commands
filesystem commands

The model must interact with deterministic ZaminCore capabilities.

This is critical for:

- security
- reliability
- reproducibility
- user trust
- cross-platform support
- auditability

---

13. CONTEXT MODEL

Do not dump the entire server filesystem, logs, configs and world into the model context.

Create a compact environment model.

Example:

Server: Survival
Platform: Paper 26.2
Status: Running
Address: 0:25565

Plugins:
  TAB
  WorldEdit
  EssentialsX
  DiscordSRV

Worlds:
  world
  world_nether
  world_the_end

Players:
  Zamin
  Steve

Resources:
  RAM: 4.1 / 8 GB
  CPU: 31%

Dutchmen should use tools to retrieve deeper information.

---

14. CONTEXT-AWARE TOOL EXPOSURE

Do not expose 100+ tools to a small local model on every request.

Tools should be dynamically exposed based on intent.

Example:

User:

replace the scoreboard title

Relevant tools:

plugin.search
plugin.inspect
config.search
config.read
config.patch
config.validate
plugin.reload

Do not give it every filesystem, network, publishing and process tool unnecessarily.

For:

load this schematic

expose:

download.asset
world.schematic.validate
world.schematic.load
world.schematic.paste
world.schematic.rotate

This reduces context usage and improves model reliability.

---

15. DUTCHMEN LONG-RUNNING JOBS

Dutchmen must not remain inside a single inference operation for long-running work.

Example:

Start a profiler in all active servers for 1 hour.

Dutchmen should create jobs.

Conceptually:

job.create
{
    type: performance_profile,
    servers: active_servers,
    duration: 1h
}

Then the system handles the hour-long operation.

When complete:

job.completed
        ↓
structured performance results
        ↓
Dutchmen interprets results
        ↓
practical recommendations

Dutchmen should be able to say things such as:

The main performance cost appears to be:
1. ...
2. ...
3. ...

I would recommend:
...

Recommendations must be based on actual captured data.

---

16. DUTCHMEN DIRECT OPENING LINKS

Dutchmen should be able to provide clickable destinations.

Examples:

Open Survival

Open TAB configuration

Open server files

Open Dutchmen conversation

The result should use ZaminPanel navigation rather than merely printing text.

Example:

[ Open Survival ]
[ Open TAB configuration ]

Clicking should navigate/open the relevant tab.

---

17. SERVER CREATION THROUGH DUTCHMEN

Dutchmen should support requests such as:

Create a new Purpur server with the latest version.

The agent should be capable of:

resolve latest compatible Purpur version
        ↓
create server directory
        ↓
download server software
        ↓
initialize server
        ↓
configure required files
        ↓
register server with ZaminCore
        ↓
start if requested
        ↓
open server in a new ZaminPanel tab

All operations must be deterministic underneath.

---

18. SERVER RESTART + LIMBO

Dutchmen should be able to perform maintenance operations that require a restart.

Example:

Install TAB.

If installation requires a restart and a configured limbo/proxy environment exists:

players detected
      ↓
move players to Limbo
      ↓
stop server safely
      ↓
install/update plugin
      ↓
start server
      ↓
verify plugin loaded
      ↓
return players

The entire workflow should be represented as a job.

The UI should show progress.

Example:

Installing TAB...

✓ Found compatible version
✓ Downloaded plugin
✓ Players moved to Limbo
✓ Server stopped
✓ Plugin installed
✓ Server started
✓ TAB loaded
✓ Players returned

---

19. MINECRAFT BRIDGE

ZaminPanel should have a Minecraft-side bridge plugin.

The bridge allows:

Minecraft
   ↓
Zamin bridge
   ↓
Dutchmen / ZaminCore

It can provide authenticated context such as:

current server
current player
UUID
world
coordinates
permissions
selected item
relevant server state

This allows:

!dutchmen replace all cobblestone in this world
in a 50x50 radius with diamond

to resolve:

world = current world
center = current player location
radius = 50
from = minecraft:cobblestone
to = minecraft:diamond

The model does not need to guess the player's location.

---

20. WORLD EDIT OPERATIONS

Do not generate thousands of Minecraft commands.

Create deterministic region operations.

Example:

world.region.replace(
    world=current_world,
    center=current_player_location,
    radius=50,
    from=minecraft:cobblestone,
    to=minecraft:diamond
)

If WorldEdit is available, use the appropriate WorldEdit integration.

---

21. SCHEMATIC WORKFLOW

Dutchmen should support:

!dutchmen download and load this schematic
[DIRECT-DOWNLOAD-URL]

Then:

place it at my location

Then:

!rotate it

The system should retain the object/context.

Workflow:

download asset
        ↓
validate file
        ↓
validate schematic format
        ↓
load into WorldEdit/integration
        ↓
resolve player location
        ↓
paste
        ↓
verify

For:

!rotate it

the agent should understand what "it" refers to from conversation state.

---

22. NEW TAB SERVER DISCOVERY

The new-tab Dutchmen interface should support:

What are active servers?

Dutchmen should query ZaminCore.

Do not make Dutchmen scrape the UI.

Return structured server information.

Example:

Active Servers

Survival
0:25565
Online
12 players

Creative
0:25566
Online
3 players

Development
0:25567
Starting

Each server should have an opening action.

---

23. SERVER PAGE

The server page should be a polished, dark-themed management environment.

It should feel like a professional application rather than a website dashboard.

The server page contains a compact sidebar:

Console
Files
Schedules
Network
Startup
Settings

The selected section is visually obvious but compact.

---

24. SERVER HEADER

At the top of the server content:

Server Name

● Online

Status colors:

green  = online
yellow = starting
blue   = stopped
red    = stopped by crash

Below the server name/status:

IP 0:25565
|
RAM 4.1 GB / 8 GB
|
Storage 12.4 GB / 50 GB
|
Uptime 0h, 4m 32s

At the right:

[ Stop ] [ Restart ] [ Publish ]

Stop:

- dark/black
- subtle gray border
- rounded
- white text

Restart:

- blue
- white text

Publish:

- primary publish action
- state-sensitive appearance

Do not make the buttons oversized.

---

25. CONSOLE

Console is the default server page.

Structure:

┌─────────────────────────────────────────────────────────────┐
│ Server Name                                  Stop Restart   │
│ IP | RAM | Storage | Uptime                       Publish   │
├───────────────┬─────────────────────────────────────────────┤
│ Console       │                                             │
│ Files         │                  Console                    │
│ Schedules     │                                             │
│ Network       │                                             │
│ Startup       │                                             │
│ Settings      │                                             │
│               │                                             │
│               │                                             │
│               │                                             │
├───────────────┴─────────────────────────────────────────────┤
│ command input                                      [send]    │
├─────────────────────────────────────────────────────────────┤
│ CPU chart                                                   │
│ RAM chart                                                   │
│ Players chart                                               │
└─────────────────────────────────────────────────────────────┘

---

26. CONSOLE INPUT

At the bottom of the console:

[ Type a Minecraft command...                         ] [Send]

Sending a command should use the proper server command mechanism.

Do not require users to manually open a terminal.

---

27. OPEN CONSOLE IN NEW TAB

The console should have an icon allowing:

Open in new tab

This creates a dedicated console-only tab.

The dedicated console view should be optimized for large console output.

It should still behave as a normal ZaminPanel tab.

---

28. CONSOLE COPY CONTROL

Add a copy button.

Clicking the button once should perform the currently selected copy action.

Possible selection modes:

All errors
All warnings
All

When:

All errors

is selected:

copy button → red

When:

All warnings

is selected:

copy button → yellow

When:

All

is selected:

copy button → white/default

Right-clicking the copy button should allow changing the selection mode.

Do not create an ugly modal if a polished contextual menu is sufficient.

---

29. CONSOLE FILTERS

Provide a horizontal control row:

Show all | Info | Warnings | Errors

These filters should be visually integrated with the console.

Filtering should not destroy the underlying log stream.

The log system should remain structured.

---

30. CONSOLE PERFORMANCE

The console must handle very high-volume Minecraft logs.

Existing project performance requirements must be preserved.

Do not render every line as an independent expensive React tree if that causes performance degradation.

Use appropriate:

- virtualization
- batching
- incremental rendering
- bounded UI buffers
- event cursors
- replay support

The console UI must not become the bottleneck.

---

31. SERVER METRIC CHARTS

Below the console, show:

CPU
current / allowed

RAM
current / allowed

Players
current / allowed

Charts should be visually clean and compact.

They should update smoothly without excessive rendering.

The underlying metric/event architecture should remain in ZaminCore.

---

32. FILES

The Files page is the server's filesystem interface.

It should display the server directory as a polished file browser.

Support:

- folders
- files
- sizes
- modification dates
- file type
- search
- create
- rename
- delete
- upload
- download
- move
- copy
- editor

Do not expose filesystem paths outside the server's permitted root.

Preserve all existing filesystem safety requirements:

- path traversal prevention
- symlink safety
- zip-slip protection
- rooted filesystem boundaries
- safe archive extraction
- safe writes
- atomic file operations where appropriate

---

33. FILE EDITOR

The editor has two modes:

Compose
Source

Source displays the actual file.

Example:

server-port: 25565
auto-clear-entity: true

Compose displays friendly controls:

Server Port
[ 25565 ]

Auto Clear Entity
[ ON ]

Compose is NOT a replacement for Source.

Users must always be able to see/edit the real underlying configuration.

Compose should preferably operate against the actual YAML AST rather than maintaining an independent representation that can diverge.

---

34. SPECIALIZED CONFIG EDITORS

Some configurations should receive specialized editors.

Example:

Scoreboard configuration:

┌───────────────────────┬─────────────────────────────┐
│ Configuration         │ Live Scoreboard Preview     │
│                       │                             │
│ Title                 │        Survival             │
│ [ &c&lSurvival ]      │        ----------------     │
│                       │        Players: 12          │
│ Lines                 │        Money: $100          │
│ ...                   │                             │
└───────────────────────┴─────────────────────────────┘

Other specialized editors may include:

- chat
- tablist
- server list
- scoreboard
- plugin-specific GUIs

Do not hardcode every plugin.

Create an extensible editor API.

---

35. RELOAD ACTION

Configuration editors should provide a built-in reload action where supported.

Example:

[ Save ] [ Save & Reload ]

The user should not have to type:

/plugin reload ...

manually.

The system should know how the plugin/server should be reloaded.

If reload is unsafe or unavailable, explain that and offer the correct supported action.

---

36. SCHEDULES

Schedules should support both basic and advanced scheduling.

Examples:

Restart every day at 04:00

Run command every 30 minutes

Publish automatically every Friday at 18:00

Install/update plugin at maintenance time

Remove plugin at specified time

Schedules should support multiple action types.

The scheduler belongs to ZaminCore.

The UI should be a polished scheduling interface, not raw cron syntax.

Advanced users may still be able to access precise scheduling controls.

---

37. NETWORK

Network management should provide a polished interface for:

- server ports
- allocated ports
- bind addresses
- availability
- conflicts
- proxy relationships
- network configuration

Example:

Minecraft Port
[ 25565 ]

Bind Address
[ 0.0.0.0 ]

Status
● Available

Port conflicts should be detected before startup where possible.

---

38. STARTUP

Startup should manage server launch configuration.

Expose useful controls such as:

- Java executable
- memory allocation
- JVM arguments
- server JAR
- startup arguments
- environment variables where appropriate
- restart behavior
- crash behavior

Do not force normal users to understand JVM command lines.

Advanced users must still be able to see the actual startup configuration.

---

39. SERVER SETTINGS

Server-specific settings should be separate from global ZaminPanel settings.

Examples:

- server name
- icon
- display address
- resource limits
- restart policy
- crash policy
- log retention
- backup behavior
- permissions
- automation
- Dutchmen behavior
- publishing configuration

---

40. PUBLISH FEATURE

Publish is a major feature for server creators.

It allows creators to package and publish their Minecraft server/resource programmatically through an API.

Potential targets include marketplaces such as BuiltByBit where API access is available.

Do not hardcode marketplace-specific behavior into the core.

Create a publishing provider interface.

Example:

Publish
 ├── provider
 ├── title
 ├── description
 ├── included files
 ├── excluded files
 ├── version
 ├── changelog
 └── credentials

---

41. PUBLISH FILE SELECTION

The creator decides exactly what gets published.

They may select:

entire folders
individual files
specific files inside folders

Example:

plugins/
├── TAB/
├── WorldEdit/
├── DiscordSRV/
└── ...

The user can choose which files are included.

Never blindly package the entire server directory.

---

42. PUBLISH DIFF

ZaminPanel should remember the previous publication state.

The Publish button should show when files have changed since the last publication.

For example:

Publish

becomes visually emphasized when changes exist.

A change indicator should show:

12 files changed

and allow inspection.

Example:

Changed files

M  plugins/TAB/config.yml
M  server.properties
A  plugins/example/config.yml
D  old-file.yml

The publish process should be robust against:

- application shutdown
- PC shutdown
- power failure
- interrupted packaging
- interrupted upload
- network failure

Never leave the publication state corrupted.

Use transactional/state-machine semantics.

---

43. DUTCHMEN-GENERATED CHANGELOG

When publishing, Dutchmen can inspect the diff between:

previous publication

and:

current selected publication

and generate a changelog/description.

It must describe actual changes.

It must not invent features.

Example:

Fixed server startup configuration
Updated TAB configuration
Added new spawn schematic
Updated server properties

The user must be able to edit the generated text before publishing.

---

44. PUBLISH SECURITY SCANNING

Publishing must include a security scan before packaging.

This is extremely important.

The system must detect secrets and credentials.

Examples include:

- Discord bot tokens
- API keys
- access tokens
- passwords
- private keys
- database credentials
- webhook secrets
- OAuth credentials
- environment secrets
- cloud credentials

DiscordSRV deserves special treatment because its configuration commonly contains a Discord bot token.

If:

plugins/DiscordSRV/config.yml

contains a bot token, ZaminPanel should identify it.

If DiscordSRV is loaded and functioning, treat the configuration as especially suspicious because it is likely to contain active credentials.

Do not publish it silently.

---

45. PUBLISH SECURITY UI

Example:

Security Check

⚠ Potential secret detected

plugins/DiscordSRV/config.yml

Detected:
Discord bot token

Recommended:
Exclude this file from publication.

Buttons:

[ Exclude File ]
[ Review File ]
[ Publish Anyway ]
[ Cancel ]

"Publish Anyway" should require an explicit confirmation.

For extremely high-risk credentials, consider requiring a stronger confirmation.

Do not merely color the file red and continue automatically.

---

46. SECRET DETECTION

Create an extensible secret scanner.

It should use:

- known token patterns
- provider-specific detectors
- generic high-entropy detection
- configuration-key heuristics
- known sensitive filenames
- known sensitive environment variables

False positives must be reviewable.

Do not claim perfect secret detection.

The UI should clearly say that scanning is a safety mechanism, not a guarantee.

---

47. PUBLISH CREDENTIALS

Marketplace/API credentials must never be stored in plain project configuration.

Use secure platform credential storage where available.

Examples:

Windows Credential Manager

or the appropriate secure OS credential mechanism.

Never put API tokens in:

server files
published packages
logs
AI prompts
Git repositories

---

48. TAB GROUPS

Tabs support groups, like modern browsers.

Right-click tab:

New tab to the right

Add tab to new group

Move tab to new window

────────────────────

Reload
Duplicate
Pin
Mute

────────────────────

Share tab with Dutchmen
    Start new chat with Dutchmen

────────────────────

Show tabs vertically

────────────────────

Close
Close other tabs
Close tabs to the right

The separator lines represent menu dividers.

---

49. TAB GROUPS UX

Groups should be collapsible.

Example:

[ Survival ]
  Survival
  Survival Console
  Dutchmen chat

[ Development ]
  Dev Server
  Test Server

Groups must not feel like folders.

They should behave like browser tab groups.

---

50. MOVE TAB TO NEW WINDOW

Selecting:

Move tab to new window

creates another ZaminPanel window containing that tab.

The underlying server must remain owned by ZaminCore.

The UI window is only a client.

Moving a tab must NOT:

- restart the server
- duplicate the server process
- duplicate daemon ownership
- corrupt the server state

---

51. TAB ISOLATION

Each tab must be isolated.

A broken server page/plugin/editor must not crash:

- another tab
- the entire ZaminPanel process
- zamind
- another server

Conceptually:

ZaminPanel
 ├── Tab A
 │    └── isolated UI state
 │
 ├── Tab B
 │    └── isolated UI state
 │
 ├── Tab C
 │    └── isolated UI state
 │
 └── Tab D
      └── isolated UI state

A server-specific renderer crash should be recoverable.

Do not allow third-party extensions to directly destabilize the core application.

---

52. PINNING

Pinned tabs behave like browser pinned tabs.

They should:

- become compact
- remain at the beginning of the tab strip
- preserve their destination
- be protected from accidental closure where appropriate

---

53. MUTE

Mute is relevant when:

- an extension produces audio
- a server-related media component produces audio
- future addons introduce sound

The tab should show its muted state.

---

54. VERTICAL TABS

Provide:

Show tabs vertically

This should switch the tab UI into a vertical tab layout.

The concept should resemble Discord/browser vertical navigation more than a generic sidebar.

The user's server tabs remain the same tab objects.

Only their presentation changes.

---

55. BOOKMARK BAR

The bookmark bar behaves similarly to a browser bookmark bar.

It can contain:

- server addresses
- server tabs
- Dutchmen conversations
- internal ZaminPanel pages

Examples:

Survival
Development
Dutchmen
Server Settings

Bookmarks should preserve destination identity.

---

56. EXTENSIONS / ADDONS

ZaminPanel should have an extension/addon system.

Extensions may:

- add UI
- add server integrations
- add specialized config editors
- add context-menu actions
- add tools to Dutchmen
- add sidebar pages
- add publishing providers
- add server software support
- add marketplace integrations

Extensions should NOT receive unrestricted access to the machine.

Create a permission model.

---

57. CONTEXT MENU EXTENSIONS

Extensions should be able to add context-menu entries.

Example:

Right click server tab

...
──────────────
My Extension
    Inspect server
    Export configuration

Extensions must declare their permissions.

The extension system must remain isolated.

---

58. INTERNAL URL SYSTEM

Create an internal URL/router system.

Examples:

zaminpanel://settings/
zaminpanel://extensions/
zaminpanel://servers/
dutchmen:(chat-ID)

Potential future internal URLs:

zaminpanel://about/
zaminpanel://downloads/
zaminpanel://jobs/

Do not implement this as arbitrary web navigation.

It is a typed internal navigation system.

---

59. BACK / FORWARD

Back and forward should operate on navigable ZaminPanel destinations.

They should feel browser-like.

Do not invent meaningless navigation history for every UI click.

Only actual destination changes should become history entries.

---

60. RELOAD

Reload should reload the current tab's view/state.

For a server:

Reload

means:

- refresh server state
- reconnect event subscriptions
- refresh relevant data
- preserve server process
- preserve unsaved editor state where appropriate

It must NOT mean:

restart Minecraft

unless explicitly requested by a server-specific action.

---

61. SERVER IDENTITY

The system must distinguish:

tab
server
process
server directory
server address

These are not interchangeable.

A tab is a UI destination.

A server is a managed entity.

A process is a runtime instance.

The existing process identity rules must remain intact.

Use PID + process start marker or the existing documented equivalent.

Never blindly kill a PID just because the number matches.

---

62. CRASH HANDLING

If a server crashes:

status → red

The server tab must remain open.

The user should see:

Server stopped unexpectedly.

Reason:
...

[ Restart ]
[ View logs ]
[ Ask Dutchmen ]

Dutchmen should be able to inspect the crash.

Example:

Why did the server crash?

Dutchmen should inspect:

- recent logs
- crash reports
- plugin state
- Java information
- server version
- relevant configuration

Then explain likely causes.

Do not invent a cause when evidence is insufficient.

---

63. SERVER TAB WHEN STOPPED

A stopped server remains a valid tab.

It should display:

● Stopped

and relevant controls.

The tab must not disappear merely because the process stopped.

---

64. SERVER DISCOVERY

ZaminPanel should maintain a registry of known servers.

The registry can detect:

- running servers
- configured servers
- previously opened servers
- server directories
- supported server JARs

Opening a server address should resolve against the known registry where possible.

---

65. NO DUPLICATE BACKEND

Do not implement server lifecycle logic separately in:

React
Tauri
Dutchmen
CLI

There must be one authoritative implementation.

The existing:

ZaminCore / zamind

architecture remains authoritative.

The UI is a client.

Dutchmen is an agent client/runtime.

CLI is a client.

---

66. AI MODEL IMPLEMENTATION

Do NOT train a foundation model from scratch.

Dutchmen should initially use an existing capable local language model.

The model should be accessed through an isolated model adapter.

For the local implementation, llama.cpp/OpenAI-compatible local inference can be used.

The model should support structured tool/function calling.

Conceptually:

Dutchmen Runtime
       ↓
Model Adapter
       ↓
Local LLM
       ↓
structured tool call
       ↓
Dutchmen Runtime

The model must never directly mutate files or processes.

---

67. MODEL-AGNOSTIC DESIGN

Do not hardcode Dutchmen to one model.

The runtime should allow:

local model
future larger local model
future remote model

without changing the tool layer.

The model adapter should provide:

chat
structured output
tool calls
streaming
token usage
model metadata
cancellation

---

68. SMALL MODEL OPTIMIZATION

Dutchmen must be designed so that a relatively small local model can accomplish complex tasks.

Do this through:

- strong tool schemas
- compact environment snapshots
- dynamic tool exposure
- deterministic resolvers
- structured results
- clear tool descriptions
- explicit preconditions
- explicit postconditions
- verification
- limited ambiguity

Do NOT attempt to solve the problem simply by stuffing enormous context into the model.

---

69. PLUGIN CAPABILITY RESOLUTION

Example:

replace scoreboard title with &c&lSurvival

There may be multiple plugins capable of scoreboard functionality.

Dutchmen needs a capability index.

Conceptually:

TAB
 ├── tablist
 ├── scoreboard
 └── nametags

Plugin X
 └── scoreboard

The system should identify which installed component actually owns the relevant scoreboard.

Dutchmen should then inspect that plugin's configuration.

This should be partly deterministic rather than requiring the model to guess.

---

70. VERIFICATION

Every meaningful Dutchmen operation should have verification.

Example:

config.patch

must be followed by:

config.validate

and potentially:

plugin.reload

and:

plugin.state

Then Dutchmen reports success.

Never say:

Done!

when the operation merely requested a change.

Success means the expected postcondition was verified.

---

71. CONFIRMATION MODEL

Not every action should require confirmation.

Suggested categories:

Read-only

Automatically allowed:

server.status
plugin.list
config.read
performance.report

Low-risk mutation

May be automatic depending on settings:

config.patch
plugin.reload

High-impact mutation

Require confirmation:

server.restart
plugin.uninstall
world.region.replace
large-scale world modification

Dangerous external operations

Require strong confirmation:

publishing
arbitrary external downloads
credential-sensitive operations
destructive filesystem operations

Permissions must be configurable.

---

72. AUDIT LOG

Every Dutchmen operation should produce an audit trail.

Example:

Dutchmen

User request:
Replace scoreboard title with &c&lSurvival

Actions:
✓ Identified TAB as scoreboard provider
✓ Read TAB configuration
✓ Modified scoreboard title
✓ Validated configuration
✓ Reloaded TAB
✓ Verified scoreboard state

The user should be able to inspect what Dutchmen actually did.

---

73. JOBS

Long-running operations should use jobs.

Jobs need:

job ID
state
progress
started time
updated time
result
error
cancellation

States:

queued
running
waiting
completed
failed
cancelled

Jobs must survive UI reconnection.

---

74. SERVER PUBLISH JOB

Publishing should use a job state machine.

Example:

Preparing
Scanning
Packaging
Uploading
Waiting for provider
Completed

If the application closes, the underlying state should remain recoverable.

---

75. DESIGN LANGUAGE

The UI should be:

- dark by default
- compact
- premium
- clean
- restrained
- polished
- highly responsive

Avoid:

- giant cards
- excessive rounded rectangles
- dashboard-template aesthetics
- excessive gradients
- unnecessary glass effects
- giant headings
- excessive empty space
- generic SaaS styling

The interface should feel like an actual desktop application.

Animations should be:

- fast
- intentional
- physically coherent
- subtle where appropriate
- expressive during major transitions

Especially polish:

- tab opening
- tab closing
- tab moving
- tab grouping
- new-tab → Dutchmen transition
- server startup state
- server crash state
- sidebar transitions
- command execution
- publish progress
- Dutchmen tool execution

---

76. RESPONSIVENESS

The interface must respond immediately to user actions.

Do not block the UI while:

- reading large logs
- scanning files
- starting servers
- stopping servers
- installing plugins
- publishing
- running AI inference
- profiling

Use asynchronous/event-driven architecture.

---

77. SERVER LOG PIPELINE

Preserve the existing performance requirements.

The backend must support high-throughput logs.

The UI must consume logs through bounded/subscription mechanisms.

Do not send the entire historical console on every update.

Use:

snapshot
+
cursor
+
incremental events
+
replay

where appropriate.

---

78. PROTOCOL

Preserve the existing Zamin Protocol architecture.

Protocol features should include the already-defined concepts:

- request IDs
- typed errors
- subscriptions
- cursors
- replay
- snapshots
- jobs
- version negotiation
- tolerant readers
- idempotency where applicable

Do not bypass the protocol for convenience.

---

79. SECURITY

Treat every server directory as untrusted input.

Protect against:

- path traversal
- symlink escape
- malicious archives
- zip-slip
- malicious plugin files
- arbitrary downloads
- credential exposure
- PID reuse
- command injection
- extension privilege abuse

Never let Dutchmen transform a natural-language request into arbitrary OS execution.

---

80. EXTENSION SECURITY

Extensions should have explicit permissions.

Possible permissions:

server.read
server.control
filesystem.read
filesystem.write
network
minecraft.bridge
dutchmen.tools
ui.tabs
ui.context_menu
publishing
credentials

Sensitive permissions should require user approval.

---

81. ERROR UX

Errors should be human-readable.

Bad:

ERR_CORE_0x1928A

Good:

TAB could not be installed.

The download completed, but the server rejected the plugin because
the installed server version is incompatible.

[ View details ]

Technical details should remain available.

---

82. NO FAKE FUNCTIONALITY

Do not create UI buttons that only visually work.

For every implemented feature:

UI
 ↓
protocol
 ↓
backend
 ↓
actual operation
 ↓
result
 ↓
UI

If something cannot be implemented yet, represent it as unavailable rather than pretending.

---

83. IMPLEMENTATION PROCESS

Before writing large amounts of code:

1. Inspect the repository.
2. Read the existing project documentation.
3. Identify current architecture.
4. Identify existing components.
5. Identify existing protocol definitions.
6. Identify what already exists.
7. Do not duplicate existing functionality.
8. Determine the smallest architecture changes needed.
9. Produce a concrete implementation plan.
10. Implement incrementally.

Do not perform a giant unrelated refactor.

Preserve working functionality.

---

84. FIRST IMPLEMENTATION TARGET

Build the browser shell first.

The first usable vertical slice should be:

ZaminPanel opens
        ↓
browser-style window
        ↓
tabs
        ↓
address bar
        ↓
new tab
        ↓
Dutchmen input
        ↓
server discovery
        ↓
open server tab
        ↓
server status
        ↓
console

This must work end-to-end.

---

85. SECOND VERTICAL SLICE

Implement:

server lifecycle
start
stop
restart
status
crash detection
console
metrics

through the existing ZaminCore/zamind architecture.

---

86. THIRD VERTICAL SLICE

Implement:

Files
Source editor
Compose editor

Then add specialized config editor infrastructure.

---

87. FOURTH VERTICAL SLICE

Implement Dutchmen:

chat
context
tool registry
tool calls
verification
audit
jobs

Start with simple operations:

What servers are running?
What plugins are installed?
What is this server's status?
Open Survival.

Then move into mutations.

---

88. FIFTH VERTICAL SLICE

Implement advanced Dutchmen operations:

plugin installation
configuration modification
reload
WorldEdit
schematics
profiling
multi-server operations
restart workflows
Limbo

---

89. SIXTH VERTICAL SLICE

Implement publishing:

selection
diff
security scan
changelog generation
package creation
provider API
upload
recovery

---

90. TESTING

Test the actual behavior, not only UI snapshots.

Tests must cover:

Tabs

- open
- close
- duplicate
- pin
- group
- move window
- reopen
- crash isolation

Servers

- start
- stop
- restart
- crash
- adoption
- duplicate start
- PID reuse
- stale process
- server disappearance

Files

- traversal
- symlink escape
- zip-slip
- atomic writes
- invalid YAML

Dutchmen

- tool selection
- malformed tool calls
- missing context
- ambiguous requests
- failed tool calls
- retries
- cancellation
- verification
- multi-turn references

Jobs

- restart
- reconnect
- cancellation
- failure
- completion

Publishing

- changed files
- unchanged files
- excluded files
- secret detection
- interrupted packaging
- interrupted upload
- recovery

---

91. CRITICAL DESIGN TEST

At every architectural decision, ask:

«Does this make ZaminPanel feel more like a browser for Minecraft infrastructure, or more like another server control panel?»

If the answer is the latter, reconsider the design.

---

92. THE FINAL EXPERIENCE

The intended user experience should eventually feel like this:

The user installs ZaminPanel.

They double-click:

paper-26.2-129.jar

ZaminPanel opens.

A new tab appears:

[ Survival ● ]

The address bar says:

0:25565

The server starts.

The user sees:

Survival
● Online

IP 0:25565 | RAM 4.1/8 GB | Storage 12.4/50 GB | Uptime 0h 4m

[ Stop ] [ Restart ] [ Publish ]

Console

They open another tab.

The new tab says:

What can Dutchmen do for you?

They type:

What are the active servers?

The new-tab interface beautifully transforms into Dutchmen chat.

Dutchmen replies:

3 servers are currently active.

Survival       0:25565
Creative       0:25566
Development    0:25567

Each result can be opened.

The address bar becomes:

dutchmen:(a8f72c)

The user says:

Start a profiler in all active servers for one hour.
Then tell me what is eating performance.

Dutchmen creates the jobs.

An hour later:

Profiling complete.

Survival:
...
Creative:
...
Development:
...

The largest performance issue across the network is...

The user then types:

Install TAB on Survival.

Dutchmen:

✓ Found compatible TAB
✓ Downloaded
✓ Installation requires restart

Players are currently online.

[ Move players to Limbo and restart ]
[ Cancel ]

User approves.

✓ Players moved to Limbo
✓ Survival stopped
✓ TAB installed
✓ Survival started
✓ TAB loaded
✓ Players returned

Then:

!dutchmen replace scoreboard title with &c&lSurvival

Dutchmen understands the current server and resolves the actual scoreboard provider.

Then:

!dutchmen download and load this schematic
https://...

It downloads, validates, loads and places it at the player's location.

Then:

!rotate it

It understands what "it" refers to.

The user never had to open:

plugins/
server.properties
start.bat
cmd.exe
PowerShell

unless they explicitly wanted to.

That is the point of ZaminPanel.

---

93. NON-NEGOTIABLE PRODUCT IDENTITY

Do not lose these principles while implementing:

ZaminPanel is a browser.
Servers are tabs.
Server addresses are destinations.
New Tab is Dutchmen.
Dutchmen is an agent, not a chatbot.
Internal pages have ZaminPanel URLs.
Chats have Dutchmen URLs.
Extensions behave like browser extensions.
Bookmarks can point to servers and conversations.
Tabs can be grouped.
Tabs can move between windows.
Every tab is isolated.
ZaminCore is the authoritative backend.
The AI never directly controls the OS.
Deterministic tools perform real operations.
Every meaningful AI operation is verified.
Long operations are jobs.
The UI must be genuinely polished.

Most importantly:

«Do not redesign this into something that looks more conventional simply because conventional Minecraft panels are easier to implement.»

The unusual browser model is the product.

Implement the architecture that makes that model real.