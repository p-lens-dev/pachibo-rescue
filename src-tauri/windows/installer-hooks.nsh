; Hooks run by the NSIS installer Tauri generates.
;
; The bundled adb leaves its server running in the background, holding
; binaries\adb.exe and its DLLs open: the app stops it on exit, but an adb
; run by hand, or left by an app that did not exit cleanly, keeps going, and
; the installer then cannot replace or remove those files ("Error opening file
; for writing"). So before installing over them, or removing them, the
; installed adb is asked to stop its server.

!macro PACHIBO_STOP_ADB
  ${If} ${FileExists} "$INSTDIR\binaries\adb.exe"
    nsExec::Exec '"$INSTDIR\binaries\adb.exe" kill-server'
    Pop $0
    Sleep 1000
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PACHIBO_STOP_ADB
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PACHIBO_STOP_ADB
!macroend
