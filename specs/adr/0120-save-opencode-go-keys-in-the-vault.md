# ADR-0120 — Save OpenCode Go keys in the vault

## Context

OpenCode Go limits are optional. Configuring the key through the process
environment is inconvenient for users who launch Qursor from their desktop.

## Decision

Settings → Usage accepts the key and saves it in the encrypted vault
introduced by ADR-0106. This extends the vault to one usage credential.
The renderer can submit a replacement, remove it, and read its source.
Only the quota reader decrypts the stored key, for the OpenCode Go HTTPS
usage endpoint, with redirects disabled.

## Consequences

A saved key takes precedence over the existing environment fallback.
If the vault is locked, the saved key cannot serve quota reads. Saving or
removing a key clears the cached Go limits. OpenCode model selection and
sign-in still happen in the CLI.
