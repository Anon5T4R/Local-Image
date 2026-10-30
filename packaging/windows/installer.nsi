!include "MUI2.nsh"

Name "Local Image"
OutFile "local-image-setup.exe"
InstallDir "$PROGRAMFILES64\Local Image"
RequestExecutionLevel admin

!define VERSION "0.1.0"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

; ESSENCIAL: sem isto as páginas MUI ficam em branco (sem strings de idioma).
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "target\release\local-image.exe"

  ; Atalhos no contexto do USUÁRIO (não do admin elevado).
  SetShellVarContext current

  ; Menu Iniciar
  CreateDirectory "$SMPROGRAMS\Local Image"
  CreateShortCut "$SMPROGRAMS\Local Image\Local Image.lnk" "$INSTDIR\local-image.exe"
  ; Desktop
  CreateShortCut "$DESKTOP\Local Image.lnk" "$INSTDIR\local-image.exe"

  ; Registro de desinstalação (Adicionar/Remover programas)
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "DisplayName" "Local Image"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "Publisher" "Anon5T4R"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "DisplayIcon" "$INSTDIR\local-image.exe"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image" "NoRepair" 1

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  ; Remove atalhos no contexto do usuário
  SetShellVarContext current
  Delete "$SMPROGRAMS\Local Image\Local Image.lnk"
  Delete "$DESKTOP\Local Image.lnk"
  RMDir "$SMPROGRAMS\Local Image"

  ; Remove arquivos
  Delete "$INSTDIR\local-image.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  ; Remove registro
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Local Image"
SectionEnd
