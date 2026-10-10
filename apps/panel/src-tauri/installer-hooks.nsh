; ZIM's NSIS installer hooks (bundle > windows > nsis > installerHooks).
;
; THE FILE-LOCK LAW: the installer cannot replace an executable that a
; running process holds open. The panel's own update path exits ZIM
; before the installer runs, but the resident daemon (zamind) and the
; CLI sidecar (zamin) deliberately outlive the panel — and the user's
; own screenshot hit exactly this: "Error opening file for writing:
; F:\Zamin\Zamin Instances Manager\zamind.exe", the update stuck
; mid-install with Abort/Retry/Ignore. A stale or manually-opened
; panel would hold its own lock too. So every ZIM process dies before
; the first file is extracted.
;
; THE HOOK-COVERAGE LAW (2026-10-10, verified against the tauri-bundler
; v2.9.4 template): the template has ONE install section that runs in both
; modes — a double-clicked install and the in-app updater's /UPDATE
; launch (the path the user's "ZIM 0.4.26 is downloading" pill takes) —
; so NSIS_HOOK_PREINSTALL fires for both. The template's own
; CheckIfAppIsRunning uses the Restart Manager on the MAIN binary only;
; zamind and zamin deliberately outlive the panel and are invisible to
; it. NSIS_HOOK_PREUNINSTALL belongs to the same law: uninstalling from
; Windows Settings while servers run must not meet a locked file. Two
; entry points, one stop-first law. (There is no NSIS_HOOK_PREUPDATE in
; any 2.x template — update mode reuses the install section.)
;
; THE POLL LAW: one taskkill shot is a race, not a guarantee. taskkill's
; exit code is the stable signal — 0 means it terminated something (so
; the image existed and may have just respawned children), 128 means
; nothing matched (the image is gone). The loop kills until the OS
; answers 128, so a slow tree teardown cannot slip past a single shot;
; a hard round cap keeps a permission failure from hanging the installer
; (denied kills answer 1 and exit immediately — the honest failure is
; the OS's own Retry dialog, not a silent hang).
;
; The cost is honest and stated: zamind owns the Minecraft server
; processes (its Job Object closes with it), so an update stops
; running servers. The alternative — a silently stuck installer — is
; worse. The daemon restarts on the relaunched panel's first transport
; failure (daemon_ensure), and servers restart from the panel.

; The template includes this file after MUI2, but the poll loop's
; ${Do}/${Loop} live in LogicLib — a guarded include keeps the hooks
; standing alone if the template's include order ever shifts.
!ifndef LOGICLIB
  !include "LogicLib.nsh"
!endif

!macro ZIM_KILL_IMAGE IMAGE
  StrCpy $1 0
  ${Do}
    nsExec::ExecToLog 'taskkill /F /IM "${IMAGE}" /T'
    Pop $0
    ; 128 = nothing matched (gone); "error" or 1 = denied/failed — both
    ; mean further rounds change nothing.
    ${If} $0 != 0
      ${ExitDo}
    ${EndIf}
    IntOp $1 $1 + 1
    ${If} $1 >= 12
      ; Twelve successful kill rounds (~6s) and the image keeps coming
      ; back: stop, let the file dialog tell the truth.
      ${ExitDo}
    ${EndIf}
    Sleep 500
  ${Loop}
!macroend

!macro ZIM_STOP_ALL
  DetailPrint "Stopping ZIM processes so the files can be replaced…"
  !insertmacro ZIM_KILL_IMAGE "zim.exe"
  !insertmacro ZIM_KILL_IMAGE "zamind.exe"
  !insertmacro ZIM_KILL_IMAGE "zamin.exe"
  ; One settle beat after the last kill: termination itself is
  ; synchronous, but the kernel's handle release (what actually
  ; unlocks the .exe) can lag a beat behind it.
  Sleep 750
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro ZIM_STOP_ALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro ZIM_STOP_ALL
!macroend
