!ifndef HWND_BROADCAST
  !define HWND_BROADCAST 0xFFFF
!endif
!ifndef WM_SETTINGCHANGE
  !define WM_SETTINGCHANGE 0x001A
!endif

!macro NSIS_HOOK_POSTINSTALL
  ; rakc.exe is bundled as a resource and lands in $INSTDIR.
  ; Add $INSTDIR to the user PATH so `rakc` works in cmd.
  ReadRegStr $0 HKCU "Environment" "PATH"
  StrCpy $3 "$INSTDIR"
  ${StrLoc} $2 $0 $3 "<"
  StrCmp $2 "" 0 rakc_path_skip_add
    StrCmp $0 "" 0 rakc_path_append
      StrCpy $0 "$3"
      Goto rakc_path_write
    rakc_path_append:
      StrCpy $0 "$0;$3"
    rakc_path_write:
      WriteRegExpandStr HKCU "Environment" "PATH" "$0"
      SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  rakc_path_skip_add:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; PATH entry intentionally left in place (pointing at a removed dir is harmless).
!macroend
