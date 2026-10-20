; NSIS installer hooks for the Windows bundle (tauri.windows.conf.json).
;
; Builds before the rebrand (ADR-0075) installed as product "factorai" in
; %LOCALAPPDATA%\factorai, with the same identifier, dev.factorai. The renamed
; installer cannot update that install: it lands beside it in
; %LOCALAPPDATA%\qursor-ide. Both Start menu shortcuts then carry the
; AppUserModelID dev.factorai, Windows keeps one app per ID, and search and the
; old shortcut keep opening the stale factorai.exe. Remove that install's files,
; shortcuts and registry keys before installing. App data lives under the
; identifier (dev.factorai), not here, and is left alone.

!define QURSOR_LEGACY_DIR "$LOCALAPPDATA\factorai"

!macro QURSOR_REMOVE_LEGACY_FACTORAI_INSTALL
  ${If} "$INSTDIR" != "${QURSOR_LEGACY_DIR}"
  ${AndIf} ${FileExists} "${QURSOR_LEGACY_DIR}\factorai.exe"
    ClearErrors
    ; A running factorai.exe is locked: leave the old install as it is.
    Delete "${QURSOR_LEGACY_DIR}\factorai.exe"
    ${If} ${Errors}
      DetailPrint "factorai.exe is running; the pre-rename install in ${QURSOR_LEGACY_DIR} was left in place"
    ${Else}
      Delete "${QURSOR_LEGACY_DIR}\uninstall.exe"
      Delete "${QURSOR_LEGACY_DIR}\setup-in-distro.sh"
      ; Only when empty: anything else in there is not ours to delete.
      RMDir "${QURSOR_LEGACY_DIR}"
      Delete "$SMPROGRAMS\factorai.lnk"
      Delete "$DESKTOP\factorai.lnk"
      DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\factorai"
      DeleteRegKey HKCU "Software\factorai\factorai"
      DetailPrint "Removed the pre-rename factorai install from ${QURSOR_LEGACY_DIR}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro QURSOR_REMOVE_LEGACY_FACTORAI_INSTALL
!macroend
