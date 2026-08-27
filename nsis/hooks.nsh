; NSIS installer hooks for Claude Swap Bar.
;
; Hooked in through `bundle.windows.nsis.installerHooks` in tauri.conf.json.

; Tauri's own uninstall section already deletes an HKCU Run value, but it looks
; for one named after PRODUCTNAME ("Claude Swap Bar"). The app writes its own
; under "ClaudeSwapBar" (src/autostart.rs), so without this the value survives
; the uninstall and keeps pointing at a deleted exe at every login, forever.
;
; Guarded on $UpdateMode for the same reason Tauri guards its own: an update
; runs the uninstaller first, and wiping the value there would silently turn
; off a "Start with Windows" the user had chosen.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "ClaudeSwapBar"
  ${EndIf}
!macroend
