# ADR-0085 — The Windows installer removes the pre-rename factorai install

Date: 2026-10-20
Status: Accepted for this local fork. Follows ADR-0075 (qursor-ide is the native product brand) and ADR-0074 (Windows NSIS defaults).

## Context

ADR-0075 renamed the product from `factorai` to `qursor-ide` and kept the identifier `dev.factorai`. Tauri's NSIS installer derives the install directory, the uninstall registry key and the shortcut names from the product name, so the renamed installer does not update a `factorai` install: it installs beside it, in `%LOCALAPPDATA%\qursor-ide`.

The two installs' shortcuts both carry the AppUserModelID `dev.factorai`, and Windows keeps one Start entry per ID. On a machine that had a `factorai` build, Start kept the old one: searching "qursor" found nothing, and searching "factorai" or using the old shortcut opened the frozen `factorai.exe` from before the rename, so every update looked like it had not landed.

## Decision

Ship an NSIS pre-install hook, `apps/desktop/src-tauri/windows/hooks.nsh`, wired through `bundle.windows.nsis.installerHooks` in `tauri.windows.conf.json`. Before installing, when `%LOCALAPPDATA%\factorai\factorai.exe` exists and is not the install target, the hook does the following:

- deletes that executable;
- if that worked, deletes the old `uninstall.exe` and `setup-in-distro.sh`;
- removes the directory only if it is now empty;
- deletes the `factorai.lnk` Start menu and desktop shortcuts;
- deletes the `HKCU\…\Uninstall\factorai` and `HKCU\Software\factorai\factorai` keys.

If `factorai.exe` is running, its file is locked and the hook leaves everything in place.

The hook does not run the old uninstaller. That uninstaller offers to delete the app data, and the app data lives under `dev.factorai`, which the renamed product shares. `scripts/windows-native.mjs` fails the configuration check if the hook is unwired.

## Consequences

Installing any build from this one on removes the stale install, and Start lists `qursor-ide` for `dev.factorai`.

The hook does not touch a taskbar pin. A pin made while the old entry won the ID has to be removed and pinned again from Start.

The hook names fixed paths of the pre-rename layout. It is a no-op on machines that never had a `factorai` build, and it can go once no such install is expected.
