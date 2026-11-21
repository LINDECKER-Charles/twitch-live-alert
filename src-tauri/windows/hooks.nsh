; Preserve the login preference during an upgrade.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TwitchLiveAlert"
    ${If} $0 == '"$INSTDIR\twitch-live-alert-desktop.exe" --background'
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TwitchLiveAlert"
    ${EndIf}
  ${EndIf}
!macroend
