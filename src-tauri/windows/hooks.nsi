; NSIS hooks for the Hermes Assistant installer (tauri.conf.json → bundle.windows.nsis.installerHooks).
;
; One-off migration from v0.1.0 / v0.2.0: those builds were published under the product
; name "Hermes", so NSIS put them in %LOCALAPPDATA%\Hermes — the very folder Hermes Agent
; uses as its home (%LOCALAPPDATA%\hermes, Windows paths are case-insensitive). The product
; is now "Hermes Assistant" (own folder), and the old entry would otherwise stay in
; "Apps & features". Its uninstaller only deletes the files it installed and removes the
; folder non-recursively, so a Hermes Agent living there is left untouched.

!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$LOCALAPPDATA\Hermes\uninstall.exe" 0 +4
    ExecWait '"$LOCALAPPDATA\Hermes\uninstall.exe" /S _?=$LOCALAPPDATA\Hermes'
    Delete "$LOCALAPPDATA\Hermes\uninstall.exe"
    RMDir "$LOCALAPPDATA\Hermes"
!macroend
