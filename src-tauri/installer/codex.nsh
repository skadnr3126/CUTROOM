!macro NSIS_HOOK_PREINSTALL
  ${If} ${FileExists} "$INSTDIR\mcp\cutroom-node.exe"
    !insertmacro CheckIfAppIsRunning "$INSTDIR\mcp\cutroom-node.exe" "Cutroom MCP"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ClearErrors
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-codex-mcp' $0
  ${If} ${Errors}
  ${OrIf} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Cutroom could not register its MCP server. Check your Codex config.toml and run this installer again." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  ${If} ${FileExists} "$INSTDIR\mcp\cutroom-node.exe"
    !insertmacro CheckIfAppIsRunning "$INSTDIR\mcp\cutroom-node.exe" "Cutroom MCP"
  ${EndIf}
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-codex-mcp' $0
  ${EndIf}
!macroend
