# Qursor IDE for native Windows

This fork adds an x86_64 Windows desktop build of Qursor IDE. It runs directly on Windows and does not require WSL. The existing macOS, Linux, and WSL 2 distribution paths remain available.

## Install the desktop app

Run the generated `qursor-ide_0.56.0_x64-setup.exe` and follow the setup wizard. The installed application runs directly on Windows with WebView2. The installer can download WebView2 when needed. Install the native Claude Code or Codex CLI separately to start agent sessions; Git is needed for command-line worktree operations, and SOPS is optional for encrypted files.

## Build requirements

- Windows 10 version 1809 (October 2018 Update) or later, x86_64
- Node.js 24 or newer and pnpm 10 or newer
- Rust stable with the `x86_64-pc-windows-msvc` target
- Microsoft C++ Build Tools with **Desktop development with C++** selected
- Microsoft Edge WebView2 Evergreen Runtime

The ConPTY API used by the native PowerShell shell requires Windows 10 version 1809 or later ([Microsoft's `CreatePseudoConsole` requirements](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole)). Tauri's [Windows prerequisites](https://v2.tauri.app/start/prerequisites/) cover the C++ toolchain and WebView2. WebView2 is preinstalled on Windows 11 and on most eligible, updated Windows 10 devices, but can be absent on clean Windows 10, LTSC, or Server installs; verify it before launch and install the [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) if needed. The NSIS installer uses Tauri's default bootstrapper mode, which downloads the runtime when absent and needs an internet connection ([Tauri installer options](https://v2.tauri.app/distribute/windows-installer/)). Windows ARM64 is not included in this target.

## Develop and build

From the repository root, install dependencies and start the Windows development build:

```powershell
pnpm install
pnpm dev:windows
```

`dev:windows` combines the Windows configuration with the existing development configuration, so the development app keeps its separate `it.qursor.ide-dev` identifier. The release identifier is `it.qursor.ide`; data from `dev.factorai` builds is not migrated (ADR-0088).

Build the NSIS setup executable with:

```powershell
$env:CARGO_BUILD_JOBS = '2'
pnpm build:windows
```

The output is under the workspace `target\release\bundle\nsis` directory. The product and executable use the `qursor-ide` name, and version 0.56.0 produces `qursor-ide_0.56.0_x64-setup.exe`. The application identifier is `it.qursor.ide`, which sets the data location; data from `dev.factorai` builds is not migrated (ADR-0088). The Windows configuration selects NSIS and points the updater at this fork's own signed feed (see below); it never polls the upstream project. Local builds do not produce updater artifacts, so a local build is updated by installing a newer setup executable.

Run `pnpm windows:check` to verify the native bundle defaults before packaging.

## Fork updater and releases

Installed Windows builds poll `https://github.com/qursor-it/qursor-ide/releases/latest/download/latest.json` (`tauri.windows.conf.json`, ADR-0087). Pushing a `v*` tag runs `.github/workflows/windows-release.yml`, which checks the tag equals `v` + the `tauri.conf.json` version, builds the signed NSIS installer with `tauri.windows.release.conf.json` (the only place `createUpdaterArtifacts` is on) and publishes it with `latest.json` to this repository's releases.

One-time setup:

1. Generate the signing key: `pnpm tauri signer generate -w ~/.tauri/qursor-updater.key`.
2. Paste the public key into `plugins.updater.pubkey` in `apps/desktop/src-tauri/tauri.windows.conf.json`, replacing `REPLACE_WITH_QURSOR_UPDATER_PUBKEY`. `pnpm windows:check` fails until this is done.
3. Add the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (the key file contents) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Keep the private key safe: if it is lost, no installed build can verify a new release, and every install has to be reinstalled by hand.

macOS and Linux are source builds only. Their base `tauri.conf.json` has no updater endpoint or key, so the updater is off and never polls upstream.

The Windows workflow checks and builds the NSIS artifact. It does not publish a release.
