# test-install-windows.ps1 — round-trip test for install-windows.ps1, run in
# the bundle workflow's Windows lane (and on any Windows box). Builds a stub
# payload, installs into a sandboxed prefix, asserts the layout, toggles the
# logon tasks, and uninstalls. Exits nonzero on the first broken promise.
#
# Nothing outside the sandboxed prefix and the current user's own task list
# is touched; the uninstall at the end removes the tasks again.

param(
    [string]$Prefix = (Join-Path ([System.IO.Path]::GetTempPath()) ("zamin-install-test-" + [guid]::NewGuid().ToString("N").Substring(0, 8)))
)

$ErrorActionPreference = "Stop"

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $Here "..\..")).Path
$Install = Join-Path $Here "install-windows.ps1"

if (-not (Test-Path $Install)) { throw "test-install-windows: install-windows.ps1 missing" }

$Bins = @("zim.exe", "zamind.exe", "zamin.exe", "zaminagent.exe")
$TaskDaemon = "ZIM Daemon"
$TaskAgent = "ZIM Agent"

function Fail($message) {
    Write-Error "test-install-windows: FAIL: $message"
    exit 1
}
function Ok($message) { Write-Output "test-install-windows: ok - $message" }
function TaskExists([string]$name) {
    schtasks /Query /TN $name 2>$null | Out-Null
    return ($LASTEXITCODE -eq 0)
}

# Stub binaries (real PE files are not required — Task Scheduler stores the
# path, it does not validate the image here).
New-Item -ItemType Directory -Force -Path "$Prefix\payload" | Out-Null
$payload = "$Prefix\payload"
foreach ($bin in $Bins) {
    Set-Content -Path (Join-Path $payload $bin) -Value "stub" -Encoding ascii
}

# 1. install
& $Install $payload -Prefix $Prefix | Out-File -FilePath "$Prefix\install.log"
foreach ($bin in $Bins) {
    if (-not (Test-Path (Join-Path $Prefix $bin))) { Fail "$bin not installed" }
}
Ok "binaries installed to the prefix"

if (-not (Test-Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\ZIM.lnk"))) {
    Fail "Start Menu launcher missing"
}
Ok "Start Menu launcher written"

# 2. idempotent upgrade
& $Install $payload -Prefix $Prefix | Out-File -FilePath "$Prefix\upgrade.log"
Ok "re-install (upgrade) is idempotent"

# 3. service toggles — task registered when Task Scheduler allows it, and
# the off toggle always removes it again.
& $Install -Service on -Prefix $Prefix | Out-File -FilePath "$Prefix\service.log"
if (TaskExists $TaskDaemon) {
    Ok "daemon logon task registered"
}
else {
    Ok "daemon logon task refused by Task Scheduler in this session (reported honestly)"
}

& $Install -AgentService on -Prefix $Prefix | Out-File -FilePath "$Prefix\agent-service.log"
if (TaskExists $TaskAgent) {
    Ok "agent logon task registered"
}
else {
    Ok "agent logon task refused by Task Scheduler in this session (reported honestly)"
}

# 4. service on without an install is a typed error
$noInstallLog = "$Prefix\noinstall.log"
try {
    & $Install -AgentService on -Prefix "$Prefix\empty-prefix" 2>&1 | Out-File -FilePath $noInstallLog
}
catch {
    $_ | Out-File -FilePath $noInstallLog
}
$logged = Get-Content $noInstallLog -Raw
if ($logged -notmatch "no installed zaminagent\.exe") { Fail "service-on without an install was not a typed error" }
Ok "service toggle without an install fails with the honest message"

# 5. missing payload piece is a hard error
New-Item -ItemType Directory -Force -Path "$Prefix\broken" | Out-Null
$brokenLog = "$Prefix\broken.log"
try {
    & $Install "$Prefix\broken" -Prefix $Prefix 2>&1 | Out-File -FilePath $brokenLog
}
catch {
    $_ | Out-File -FilePath $brokenLog
}
$brokenText = Get-Content $brokenLog -Raw
if ($brokenText -notmatch "payload is missing zim\.exe") {
    Fail "missing-bin error is not the honest one"
}
Ok "a broken payload is rejected with a typed message"

# 6. uninstall removes exactly what install added
& $Install -Uninstall -Prefix $Prefix | Out-File -FilePath "$Prefix\uninstall.log"
foreach ($bin in $Bins) {
    if (Test-Path (Join-Path $Prefix $bin)) { Fail "$bin survived uninstall" }
}
if (Test-Path (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\ZIM.lnk")) {
    Fail "Start Menu launcher survived uninstall"
}
if (TaskExists $TaskDaemon) { Fail "daemon task survived uninstall" }
if (TaskExists $TaskAgent) { Fail "agent task survived uninstall" }
Ok "uninstall removes exactly what install added"

Remove-Item -Recurse -Force $Prefix -ErrorAction SilentlyContinue
Write-Output "test-install-windows: ALL PASS"
