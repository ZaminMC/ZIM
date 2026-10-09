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
; The cost is honest and stated: zamind owns the Minecraft server
; processes (its Job Object closes with it), so an update stops
; running servers. The alternative — a silently stuck installer — is
; worse. The daemon restarts on the relaunched panel's first transport
; failure (daemon_ensure), and servers restart from the panel.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Stopping ZIM processes so the files can be replaced…"
  nsExec::ExecToLog 'taskkill /F /IM zim.exe /T'
  nsExec::ExecToLog 'taskkill /F /IM zamind.exe /T'
  nsExec::ExecToLog 'taskkill /F /IM zamin.exe /T'
  ; Give the OS a beat to release the handles taskkill just tore away
  ; (a race here re-creates the very error this hook exists for).
  Sleep 750
!macroend
