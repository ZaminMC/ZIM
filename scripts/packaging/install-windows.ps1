# install-windows.ps1 — per-user install for ZaminPanel: the Windows mirror
# of install-linux.sh. No admin: everything lands in the user's profile,
# and the "service" is a per-user Task Scheduler logon task (the Windows
# shape of the systemd user unit — ADR-0008's service row, ADR-0011).
#
# Payload layout (exactly what the portable archive contains):
#   zamin-panel.exe  zamind.exe  zamin.exe  zaminagent.exe
#
# Usage:
#   install-windows.ps1 <payload-dir>            install (idempotent)
#   install-windows.ps1 -Service on|off          toggle the daemon logon task
#   install-windows.ps1 -AgentService on|off     toggle the agent logon task
#                                                (the headless-box half of
#                                                ADR-0011)
#   install-windows.ps1 -Uninstall               remove everything installed
#   install-windows.ps1 -Prefix DIR              install root (default:
#                                                %LOCALAPPDATA%\Programs\ZaminPanel)
#
# Installed pieces:
#   $Prefix\{zamin-panel,zamind,zamin,zaminagent}.exe
#   Start Menu\Programs\ZaminPanel.lnk
#   Task Scheduler: "ZaminPanel Daemon" (only via -Service on)
#   Task Scheduler: "ZaminPanel Agent" (only via -AgentService on)
#
# Never touched: daemon-owned state (%LOCALAPPDATA%\ZaminPanel, server roots).

param(
    [Parameter(Position = 0)] [string]$Payload,
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA "Programs\ZaminPanel"),
    [string]$Service,
    [string]$AgentService,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

$Bins = @("zamin-panel.exe", "zamind.exe", "zamin.exe", "zaminagent.exe")
$TaskDaemon = "ZaminPanel Daemon"
$TaskAgent = "ZaminPanel Agent"
$Shortcut = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\ZaminPanel.lnk"

function Info($message) { Write-Output "install-windows: $message" }
function Die($message) { throw "install-windows: $message" }

# The Windows service story: a per-user logon task. Creating one for the
# current user needs no elevation on a normal desktop; where Task Scheduler
# refuses (hardened boxes, some CI sandboxes), the report is honest and the
# files are still managed.
function Register-LogonTask([string]$name, [string]$exePath) {
    schtasks /Create /TN $name /TR "`"$exePath`"" /SC ONLOGON /RL LIMITED /F 2>$null
    if ($LASTEXITCODE -ne 0) {
        Info "warning: task '$name' could not be registered here —"
        Info "warning: Task Scheduler refused (restricted session?). Register it"
        Info "warning: from a normal desktop session: schtasks /Create /TN '$name' /TR '`"$exePath`"' /SC ONLOGON /RL LIMITED /F"
        return
    }
    Info "service ON: task '$name' starts $exePath at logon"
}

function Remove-LogonTask([string]$name) {
    schtasks /End /TN $name 2>$null | Out-Null
    schtasks /Delete /TN $name /F 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Info "service OFF: task '$name' removed"
    }
}

if ($Uninstall) {
    Remove-LogonTask $TaskDaemon
    Remove-LogonTask $TaskAgent
    foreach ($bin in $Bins) {
        $path = Join-Path $Prefix $bin
        if (Test-Path $path) { Remove-Item -Force $path }
    }
    if (Test-Path $Shortcut) { Remove-Item -Force $Shortcut }
    Info "uninstalled from $Prefix (daemon data was not touched)"
    exit 0
}

if ($Service -or $AgentService) {
    foreach ($pair in @(@($Service, $TaskDaemon, "zamind.exe"), @($AgentService, $TaskAgent, "zaminagent.exe"))) {
        $value, $taskName, $bin = $pair
        if (-not $value) { continue }
        if ($value -notin @("on", "off")) { Die "-Service/-AgentService need on|off" }
        $exePath = Join-Path $Prefix $bin
        if ($value -eq "on") {
            if (-not (Test-Path $exePath)) {
                Die "no installed $bin at $exePath — install first"
            }
            Register-LogonTask $taskName $exePath
        }
        else {
            Remove-LogonTask $taskName
        }
    }
    exit 0
}

if (-not $Payload) { Die "usage: install-windows.ps1 <payload-dir> (see the header comment)" }
if (-not (Test-Path $Payload -PathType Container)) { Die "payload directory not found: $Payload" }
foreach ($bin in $Bins) {
    if (-not (Test-Path (Join-Path $Payload $bin))) {
        Die "payload is missing $bin"
    }
}

New-Item -ItemType Directory -Force -Path $Prefix | Out-Null

# Binaries: copy (not move) so a deleted archive never breaks the install.
foreach ($bin in $Bins) {
    Copy-Item -Force (Join-Path $Payload $bin) (Join-Path $Prefix $bin)
}

# Start Menu launcher for the desktop panel.
if (Test-Path (Join-Path $Payload "zamin-panel.exe")) {
    $shell = New-Object -ComObject WScript.Shell
    $lnk = $shell.CreateShortcut($Shortcut)
    $lnk.TargetPath = Join-Path $Prefix "zamin-panel.exe"
    $lnk.WorkingDirectory = $Prefix
    $lnk.Save()
}

Info "installed: $Prefix\{zamin-panel,zamind,zamin,zaminagent}.exe"
Info "launcher:  $Shortcut"
Info "daemon service stays off until: install-windows.ps1 -Service on"
Info "remote agent service stays off until: install-windows.ps1 -AgentService on"
