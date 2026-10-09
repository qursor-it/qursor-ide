# Native Windows smoke validation

`pnpm windows:smoke` drives the real Windows WebView2, Tauri IPC and PowerShell ConPTY paths. It uses no browser automation package, model, or provider CLI. The script first calls the read-only `plugin:app|identifier` command and requires `dev.factorai-windows-smoke`; until that succeeds it creates no fixture, registers no event listener, and sends no mutating command. It uses `__TAURI_INTERNALS__.invoke` and `transformCallback` with `withGlobalTauri` left `false`.

## Launch an isolated smoke app

Use Windows 10 version 1809 or later, x64, and the regular native build prerequisites in [windows-native.md](windows-native.md). Open PowerShell at the repository root and run:

```powershell
$smokeRoot = Join-Path $env:TEMP 'factorai-windows-native-smoke'
$env:CLAUDE_CONFIG_DIR = Join-Path $smokeRoot 'claude'
$env:CODEX_HOME = Join-Path $smokeRoot 'codex'
New-Item -ItemType Directory -Force -Path $env:CLAUDE_CONFIG_DIR | Out-Null
New-Item -ItemType Directory -Force -Path $env:CODEX_HOME | Out-Null
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9230'
Push-Location apps/desktop
pnpm exec tauri dev --config src-tauri/tauri.windows.conf.json --config src-tauri/tauri.dev.conf.json --config src-tauri/tauri.windows.smoke.conf.json
Pop-Location
```

The smoke overlay is last so its isolated identifier wins over the regular and development identifiers. Set both agent data roots before starting Tauri: the app and its indexer then see empty synthetic roots instead of real Claude or Codex profiles. The CDP flag exposes this WebView2 on loopback port `9230` for the duration of the smoke process.

## Run and clean up

Wait for the app window to finish loading. In a second PowerShell at the repository root, run:

```powershell
pnpm windows:smoke
```

A passing report has `fileRoundTrip: true`, `powershellPty: true`, `terminalCleanup: true`, and `remainingTerminals: 0`. The file check adds a temporary project whose directory and filename include spaces and `è`, then reads and writes one synthetic text file through Tauri and reads it back. The PTY check subscribes to `terminal:data`, starts the app's real project shell, sends a PowerShell command, and waits for its executed `Desktop` or `Core` edition plus the fixture directory in the returned output. After killing the PTY, it polls `terminal_list` until the shell and all other terminals have been removed. It does not start Claude or Codex.

The harness unregisters its event listener, kills the shell, removes the temporary project from the smoke app, deletes its temporary fixture directory, and closes the CDP socket. Stop the dev app with Ctrl+C in the launch terminal after the report is printed. The `factorai-windows-native-smoke` directory contains only the empty agent roots used for isolation; remove it after closing the app if desired.

The smoke Tauri overlay is [tauri.windows.smoke.conf.json](../apps/desktop/src-tauri/tauri.windows.smoke.conf.json). The harness checks the app identity before touching its isolated database or filesystem, so pointing port `9230` at another WebView fails closed.

## Local validation — 2026-10-09

The native x86_64 MSVC build was tested with Node 24, Rust stable and WebView2:

- Formatting, text-byte checks, lint, TypeScript, dependency versions and unused-dependency checks passed.
- Frontend and script tests passed (746 frontend tests and 16 script tests); all 323 Playwright tests passed.
- `cargo fmt --check`, Clippy with `-D warnings`, and all Rust targets passed: 521 tests passed. The two upstream live-agent conformance tests remain explicitly ignored and require real agent sessions.
- Official SOPS was exercised against synthetic age keys and encrypted fixtures, including recipient retention, effective Windows DACL retention and protected replacement permissions.
- The real Tauri/WebView2 smoke report returned `fileRoundTrip`, `powershellPty`, and `terminalCleanup` as `true`, with `remainingTerminals: 0` and no cleanup warnings.
- After the smoke run, closing the main window through Tauri exited the native application process normally. The isolated app's sidebar and empty state rendered correctly.

The tests use temporary projects and isolated agent data roots. Live model calls and signed updater delivery were not exercised. The native installer uses manual upgrades until the fork supplies its own signing key and update feed.
