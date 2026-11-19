# ADR-0104 — SSH secrets live in a local encrypted vault

**Date.** 2026-11-19
**Status.** Accepted. Amends the credentials line of `PRODUCT.md` § Constraints
("the app never reads, stores or sends a token") by narrowing its scope. Arises
from F44.

## Context

The owner asked for what MobaXterm does: saved servers whose passwords and
private keys the app keeps, so a tab opens without typing anything. Until now
the app held no secret at all — Claude's login lives in the CLI's own config
directory (ADR-0036), GitHub's in `gh` (ADR-0077).

## Decision

1. **The app may store SSH credentials, and only those**: passwords, private
   keys, key passphrases. Still never a Claude, Codex, GitHub or cloud token.
2. **One encrypted file**, `secrets.vault` in the app's data directory, separate
   from the SQLite database. Each entry is AES-256-GCM with its id as associated
   data. Written atomically, owner-only on Unix.
3. **The key** is random and kept in the OS keychain by default (Windows
   Credential Manager, macOS Keychain, Secret Service). A file rather than one
   keychain item per secret because a Windows credential holds 2560 bytes and an
   RSA-4096 key does not fit. Optionally, the key is derived with Argon2id from a
   **master password** that is never stored; the vault is then locked at every
   start, and the keychain item is deleted.
4. **No secret crosses the bridge to the renderer, the logs, an event or an
   agent tool.** The renderer is told whether a server *has* a secret. Only the
   SSH connect path decrypts one, into a buffer zeroed on drop.

## Consequences

- Without a master password, anything running as the OS user can ask the
  keychain for the key. That is the keychain's own boundary, and the master
  password is the answer for whoever needs more.
- Where no keychain exists (a headless Linux session), the vault says so and
  offers a master password instead of storing anything unprotected.
- An exported vault (moving to another machine) must be encrypted with a
  passphrase the human chooses; export is not built yet.
