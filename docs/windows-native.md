# Native Windows build

This fork adds an x86_64 Windows desktop build of factorai. It runs directly on Windows and does not require WSL. The existing macOS, Linux, and WSL 2 distribution paths remain available.

## Install the desktop app

Run the generated `factorai_0.55.0_x64-setup.exe` and follow the setup wizard. The installed application runs directly on Windows with WebView2. The installer can download WebView2 when needed. Install the native Claude Code or Codex CLI separately to start agent sessions; Git is needed for command-line worktree operations, and SOPS is optional for encrypted files.

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

`dev:windows` combines the Windows configuration with the existing development configuration, so the development app keeps its separate `dev.factorai-dev` identifier.

Build the NSIS setup executable with:

```powershell
$env:CARGO_BUILD_JOBS = '2'
pnpm build:windows
```

The output is under the workspace `target\release\bundle\nsis` directory. The command keeps the factorai product name and existing application identity. Its Windows configuration selects NSIS and disables updater artifacts, the upstream update endpoint, and the upstream signing key. Local Windows builds therefore need to be updated by installing a newer setup executable.

Run `pnpm windows:check` to verify the native bundle defaults before packaging.

## Optional fork updater configuration

A fork release can add its own update feed explicitly with a Tauri configuration overlay. Keep the overlay local unless the fork intentionally maintains its own updater identity and feed:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "pubkey": "YOUR_FORK_PUBLIC_KEY",
      "endpoints": ["https://your-fork.example/latest.json"]
    }
  }
}
```

Save the overlay as `apps/desktop/src-tauri/tauri.windows.updater.conf.json`, then build from the repository root:

```powershell
pnpm --filter @factorai/desktop run build:windows -- --config src-tauri/tauri.windows.updater.conf.json
```

Supply the matching private signing key through the [Tauri updater environment variables](https://v2.tauri.app/plugin/updater/#signing-your-updates); do not put the private key in the config file or repository. Tauri merges additional configuration with the project defaults using JSON Merge Patch, as described in its [configuration files guide](https://v2.tauri.app/develop/configuration-files/).

The Windows workflow checks and builds the NSIS artifact. It does not publish a release.
