; Migrate the published Assistant installers without touching either AppData tree.
!define PP_LEGACY_PRODUCT "PenguinPal Assistant"
!define PP_LEGACY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PP_LEGACY_PRODUCT}"
!define PP_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
Var PPLegacyDir
Var PPMsiCode
Var PPMsiVerb
Var PPAutoStart

!macro PPRequireStopped
  nsis_tauri_utils::FindProcessCurrentUser "penguin-pal.exe"
  Pop $0
  ${If} $0 = 0
    SetErrorLevel 2
    MessageBox MB_OK|MB_ICONEXCLAMATION "请先从系统托盘退出 PenguinPal，再重新运行安装包。 / Please exit PenguinPal from the system tray first." /SD IDOK
    Abort
  ${EndIf}
!macroend

; Only migrate a Run value targeting a known installation, never arbitrary args.
!macro PPCaptureAutoStart NAME DIRECTORY
  ReadRegStr $0 HKCU "${PP_RUN_KEY}" "${NAME}"
  StrCpy $1 '$\"${DIRECTORY}\penguin-pal.exe$\"'
  StrLen $2 $1
  StrCpy $3 $0 $2
  ${If} $0 == '${DIRECTORY}\penguin-pal.exe'
  ${OrIf} $3 == $1
    StrCpy $PPAutoStart 1
  ${EndIf}
!macroend

!macro PPRemoveShortcut LINK
  !insertmacro IsShortcutTarget "${LINK}" "$PPLegacyDir\penguin-pal.exe"
  Pop $0
  ${If} $0 = 1
    Delete "${LINK}"
  ${EndIf}
!macroend

!macro PPRemoveLegacyNsis VIEW
  SetRegView ${VIEW}
  ReadRegStr $0 HKCU "${PP_LEGACY_KEY}" "DisplayName"
  ReadRegStr $1 HKCU "${PP_LEGACY_KEY}" "Publisher"
  ${If} $0 != "${PP_LEGACY_PRODUCT}"
  ${OrIf} $1 != "penguinpal"
    Goto pp_legacy_done_${VIEW}
  ${EndIf}
  ReadRegStr $0 HKCU "${PP_LEGACY_KEY}" "InstallLocation"
  nsis_tauri_utils::StrReplace $0 '$\"' ""
  Pop $PPLegacyDir
  ReadRegStr $0 HKCU "${PP_LEGACY_KEY}" "UninstallString"
  ${If} $PPLegacyDir == ""
    Goto pp_legacy_invalid_${VIEW}
  ${EndIf}
  ${If} $0 != '$\"$PPLegacyDir\uninstall.exe$\"'
  ${AndIf} $0 != '$PPLegacyDir\uninstall.exe'
    Goto pp_legacy_invalid_${VIEW}
  ${EndIf}
  ${IfNot} ${FileExists} "$PPLegacyDir\uninstall.exe"
    Goto pp_legacy_invalid_${VIEW}
  ${EndIf}
  !insertmacro PPCaptureAutoStart "${PP_LEGACY_PRODUCT}" "$PPLegacyDir"
  ; /UPDATE explicitly disables removal of AppData, even in silent mode.
  ClearErrors
  ExecWait '$\"$PPLegacyDir\uninstall.exe$\" /S /UPDATE _?=$PPLegacyDir' $0
  ${If} ${Errors}
    Goto pp_legacy_invalid_${VIEW}
  ${EndIf}
  ReadRegStr $1 HKCU "${PP_LEGACY_KEY}" "DisplayName"
  ${If} $0 != 0
  ${OrIf} $1 != ""
    Goto pp_legacy_invalid_${VIEW}
  ${EndIf}
  ; /UPDATE preserves old shortcuts. Remove only links targeting that old EXE.
  !insertmacro PPRemoveShortcut "$DESKTOP\${PP_LEGACY_PRODUCT}.lnk"
  !insertmacro PPRemoveShortcut "$SMPROGRAMS\${PP_LEGACY_PRODUCT}.lnk"
  !insertmacro PPRemoveShortcut "$SMPROGRAMS\${PP_LEGACY_PRODUCT}\${PP_LEGACY_PRODUCT}.lnk"
  RMDir "$SMPROGRAMS\${PP_LEGACY_PRODUCT}"
  ; Never recursively delete the old install directory or user data.
  Delete "$PPLegacyDir\uninstall.exe"
  RMDir "$PPLegacyDir"
  DeleteRegKey HKCU "Software\penguinpal\${PP_LEGACY_PRODUCT}"
  Goto pp_legacy_done_${VIEW}
  pp_legacy_invalid_${VIEW}:
    SetErrorLevel 3
    MessageBox MB_OK|MB_ICONSTOP "旧版 Assistant 未能安全卸载。请修复或卸载旧版后重试；不要勾选删除用户数据。 / Repair or uninstall the old Assistant, keeping user data, then retry." /SD IDOK
    Abort
  pp_legacy_done_${VIEW}:
!macroend

!macro PPRemoveLegacyMsi
  ; Use the original WiX UpgradeCode, not registry-provided shell commands.
  pp_msi_next:
  System::Call 'msi::MsiEnumRelatedProductsW(w "{7DA8386F-4E1C-5F1F-80F1-31DA2975A295}", i 0, i 0, w .r0) i.r1'
  ${If} $1 = 259 ; ERROR_NO_MORE_ITEMS
    Goto pp_msi_done
  ${EndIf}
  ${If} $1 != 0
    Goto pp_msi_failed
  ${EndIf}
  StrCpy $PPMsiCode $0
  System::Call 'msi::MsiGetProductInfoW(w "$PPMsiCode", w "AssignmentType", w .r0, *i ${NSIS_MAX_STRLEN} r2) i.r1'
  ${If} $1 != 0
    Goto pp_msi_failed
  ${EndIf}
  StrCpy $PPMsiVerb "runas"
  ${If} $0 == "0" ; Per-user MSI does not require elevation.
    StrCpy $PPMsiVerb "open"
  ${EndIf}
  System::Call 'msi::MsiGetProductInfoW(w "$PPMsiCode", w "InstallLocation", w .r0, *i ${NSIS_MAX_STRLEN} r2) i.r1'
  StrCpy $PPLegacyDir $0
  ${If} $1 != 0
    StrCpy $PPLegacyDir ""
  ${EndIf}
  ; MSI directory properties conventionally end with a backslash.
  StrCpy $0 $PPLegacyDir 1 -1
  ${If} $0 == "\"
    StrCpy $PPLegacyDir $PPLegacyDir -1
  ${EndIf}
  !insertmacro PPCaptureAutoStart "${PP_LEGACY_PRODUCT}" "$PPLegacyDir"
  ClearErrors
  ${If} $PPMsiVerb == "open"
    ExecWait '$\"$SYSDIR\msiexec.exe$\" /x $PPMsiCode /passive /norestart' $0
    ${If} ${Errors}
      Goto pp_msi_failed
    ${EndIf}
    ${If} $0 = 3010
      SetRebootFlag true
    ${ElseIf} $0 != 0
      Goto pp_msi_failed
    ${EndIf}
  ${Else}
    ; ExecShellWait has no exit-code output. Verify removal via MSI below.
    ExecShellWait "runas" "$SYSDIR\msiexec.exe" '/x $PPMsiCode /passive /norestart' SW_SHOWNORMAL
    ${If} ${Errors}
      Goto pp_msi_failed
    ${EndIf}
    ${If} ${FileExists} "$PPLegacyDir\penguin-pal.exe"
      SetRebootFlag true
    ${EndIf}
  ${EndIf}
  System::Call 'msi::MsiQueryProductStateW(w "$PPMsiCode") i.r0'
  ${If} $0 != -1 ; INSTALLSTATE_UNKNOWN means the product is uninstalled.
    Goto pp_msi_failed
  ${EndIf}
  Goto pp_msi_next
  pp_msi_failed:
    SetErrorLevel 4
    MessageBox MB_OK|MB_ICONSTOP "旧版 MSI 卸载未完成或管理员授权已取消。用户数据保留，请重试安装。 / MSI removal failed or elevation was cancelled. User data is kept; retry setup." /SD IDOK
    Abort
  pp_msi_done:
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PPRequireStopped
  StrCpy $PPAutoStart 0
  ReadRegStr $PPLegacyDir HKCU "${MANUPRODUCTKEY}" ""
  !insertmacro PPCaptureAutoStart "PenguinPal Companion" "$PPLegacyDir"
  !insertmacro PPCaptureAutoStart "${PRODUCTNAME}" "$PPLegacyDir"
  !insertmacro PPRemoveLegacyMsi
  !insertmacro PPRemoveLegacyNsis 32
  !insertmacro PPRemoveLegacyNsis 64
  ; The legacy uninstaller may remove the selected directory.
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $PPAutoStart = 1
    WriteRegStr HKCU "${PP_RUN_KEY}" "${PRODUCTNAME}" '$\"$INSTDIR\penguin-pal.exe$\"'
    DeleteRegValue HKCU "${PP_RUN_KEY}" "${PP_LEGACY_PRODUCT}"
    DeleteRegValue HKCU "${PP_RUN_KEY}" "PenguinPal Companion"
  ${EndIf}
!macroend
