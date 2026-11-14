Unicode True
!include "MUI2.nsh"

!ifndef APP_VERSION
  !define APP_VERSION "0.3.1"
!endif

Name "Antigravity Quota Monitor ${APP_VERSION}"
OutFile "dist\antigravity-quota-monitor-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\AntigravityQuotaMonitor"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ICON "icons\icon.ico"
!define MUI_UNICON "icons\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\antigravity-quota-monitor.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Antigravity Quota Monitor Taskbar Widget"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  nsExec::ExecToStack 'taskkill /F /IM antigravity-quota-monitor.exe'
  SetOutPath "$INSTDIR"

  File "target\release\antigravity-quota-monitor.exe"
  File "LICENSE"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Start Menu shortcut
  CreateShortcut "$SMPROGRAMS\Antigravity Quota Monitor.lnk" "$INSTDIR\antigravity-quota-monitor.exe"

  ; Auto-start on Windows login (Current User)
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AntigravityQuotaMonitor" '"$INSTDIR\antigravity-quota-monitor.exe"'

  ; Windows Installed Apps / Add or Remove Programs entry
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "DisplayName" "Antigravity Quota Monitor"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "Publisher" "sidequest-chain"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "DisplayIcon" "$INSTDIR\antigravity-quota-monitor.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::ExecToStack 'taskkill /F /IM antigravity-quota-monitor.exe'

  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AntigravityQuotaMonitor"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AntigravityQuotaMonitor"

  Delete "$SMPROGRAMS\Antigravity Quota Monitor.lnk"
  Delete "$INSTDIR\antigravity-quota-monitor.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd

