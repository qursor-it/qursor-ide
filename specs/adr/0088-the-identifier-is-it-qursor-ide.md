# ADR-0088 — The identifier is it.qursor.ide

Date: 2026-11-02
Status: Accepted for this local fork. Supersedes ADR-0075 only where it keeps the identifiers `dev.factorai`, `dev.factorai-dev` and `dev.factorai-windows-smoke`.

## Context

ADR-0075 kept the Tauri identifier `dev.factorai` so that installs from before the rename kept their data. The fork is now its own product with its own update feed (ADR-0087). An upstream identifier makes it share the app data directory and the Windows AppUserModelID with an upstream factorai install, and Tauri derives the NSIS publisher from the identifier's second segment, so the installer showed "factorai".

## Decision

- The identifier is `it.qursor.ide`; a dev build uses `it.qursor.ide-dev` and the Windows smoke harness `it.qursor.ide-windows-smoke`. `scripts/windows-native.mjs` enforces it.
- The bundle publisher is set explicitly to `Qursor`, so Add/Remove Programs and the installer's registry key read `Software\Qursor\qursor-ide`.
- There is no automatic migration. The only known install is migrated by hand: with the app closed, copy `%APPDATA%\dev.factorai` to `%APPDATA%\it.qursor.ide`.
- The installer's preinstall hook deletes the orphaned `HKCU\Software\factorai\qursor-ide` key, written by installs made under the old identifier.

## Consequences

- A `dev.factorai` install's data is not seen by the new build unless it is copied.
- The old WebView cache is not carried over, so UI state such as open panels resets.
- The taskbar pin may need re-pinning, because the AppUserModelID changes.
- The old `%APPDATA%\dev.factorai` stays in place as a backup.
- The database file keeps the name `factorai.db`.
