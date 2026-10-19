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

; Server mode (docs/SERVER.md): the same installer can also register hermesd as the
; Windows service «Hermes Assistant Server» (gateway watchdog + control API, no window,
; works without a logged-in user). Asked once per user and never during silent or
; passive (auto-update) runs; "Yes" starts `hermesd install` elevated in a console,
; where the service account is chosen.
!include "FileFunc.nsh"

!macro NSIS_HOOK_POSTINSTALL
  IfSilent hermes_server_done
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/P" $R1
  IfErrors 0 hermes_server_done
  ReadRegStr $R2 HKCU "Software\ua.koavart.hermes" "ServerModeAsked"
  StrCmp $R2 "1" hermes_server_done
  IfFileExists "$INSTDIR\resources\server\hermesd.exe" 0 hermes_server_done
  WriteRegStr HKCU "Software\ua.koavart.hermes" "ServerModeAsked" "1"
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Установить Hermes как серверную службу Windows?$\r$\n$\r$\nСлужба работает без окна и без входа пользователя: следит за Hermes Agent и даёт Control API на 127.0.0.1:8645. Нужны права администратора.$\r$\n$\r$\nДля обычного ПК выберите «Нет»." IDNO hermes_server_done
  ExecShell "runas" '"$INSTDIR\resources\server\hermesd.exe"' "install --interactive --pause"
  hermes_server_done:
!macroend
