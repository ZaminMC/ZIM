# make-portable.ps1 — assemble the Windows portable zip (§23 Phase 7).
#
#   ZaminPanel-<version>-windows-x64/
#     zamin-panel.exe  zamind.exe  zamin.exe  zaminagent.exe
#     README.txt
#
# Usage: make-portable.ps1 -BinsDir <dir> -Version <v> -OutDir <dir>
param(
    [Parameter(Mandatory = $true)][string]$BinsDir,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = "Stop"

foreach ($bin in @("zamin-panel.exe", "zamind.exe", "zamin.exe", "zaminagent.exe")) {
    if (-not (Test-Path (Join-Path $BinsDir $bin))) {
        throw "make-portable: bins-dir is missing $bin"
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$root = Join-Path $OutDir "ZaminPanel-$Version-windows-x64"
if (Test-Path $root) { Remove-Item -Recurse -Force $root }
New-Item -ItemType Directory -Force -Path $root | Out-Null

foreach ($bin in @("zamin-panel.exe", "zamind.exe", "zamin.exe", "zaminagent.exe")) {
    Copy-Item (Join-Path $BinsDir $bin) (Join-Path $root $bin)
}

$readme = @"
ZaminPanel - portable (Windows)
===============================

Run in place:  zamin-panel.exe

Contents:
  zamin-panel.exe   the desktop panel
  zamind.exe        the resident daemon (one per user; owns every server)
  zamin.exe         the CLI (zamin list | start | stop | logs -f | attach)
  zaminagent.exe    the remote bridge (TLS + token; see ADR-0011)

Daemon state lives under %LOCALAPPDATA%\ZaminPanel; server roots are the
ones you register. Deleting this folder never touches daemon state or
servers. The NSIS installer is the other way to install (per-user,
no admin).
"@
Set-Content -Path (Join-Path $root "README.txt") -Value $readme -Encoding utf8

$archive = Join-Path $OutDir "ZaminPanel-$Version-windows-x64.zip"
Compress-Archive -Path $root -DestinationPath $archive -Force
Remove-Item -Recurse -Force $root
Write-Output "make-portable: wrote $archive"
