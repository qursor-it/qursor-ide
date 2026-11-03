# ADR-0087 — The fork updates only from its own Windows feed

Date: 2026-11-02
Status: Accepted for this local fork. Follows ADR-0074 (native Windows NSIS defaults) and replaces the upstream release channels of ADR-0064.

## Context

Upstream (Nightbr/factorai) ships macOS and Linux builds through alpha and stable channels with its own signing key. This fork ships only a native Windows NSIS build. An updater that polled the upstream feed would replace the fork with upstream.

## Decision

- The only update feed is this fork's signed Windows feed, `https://github.com/qursor-it/qursor-ide/releases/latest/download/latest.json`, set in `tauri.windows.conf.json` with the fork's own public key.
- A `v*` tag push runs `windows-release.yml`, which builds the signed NSIS installer and publishes it with `latest.json`. `tauri.windows.release.conf.json` turns updater artifacts on for that workflow only.
- macOS and Linux are source builds with no updater: the base `tauri.conf.json` has no endpoint, no key and no updater artifacts. `check_update` takes its endpoints only from the build's configuration and refuses an empty, non-HTTPS or upstream feed.
- Upstream's alpha and stable channels and their workflows (`alpha`, `promote`, `release`, `release-guard`, `pages`) are removed. `scripts/windows-native.mjs` enforces the feed, the real public key and the empty base endpoints.

## Consequences

- The Update channel selector in Settings no longer changes anything. Removing it is left for a follow-up.
- v0.1 installs have no updater and must install the next release by hand once.
- The private signing key lives in the repository secrets. Losing it strands every install, because none can verify a release signed by a new key.
