# ADR-0096 — Paper (light) is the default theme

Date: 2026-11-13
Status: Accepted. Amends F11 (Appearance) and ADR-0090.

## Context

The theme followed the operating system by default. The owner runs Windows in
dark mode, so the redesign reached them as Slate and read as the dark app it
was meant to replace; the approved mock-ups are Paper.

## Decision

- The default theme preference is `light`. `system` and `dark` stay in Settings
  → Appearance.
- Stored preferences move to version 2: a stored `system` becomes `light` once,
  because until now `system` was the default and says nothing about a choice. A
  stored `dark` or `light` is kept.

## Consequences

- People who had deliberately picked System see light once and can pick System
  again.
- The guide and README screenshots, which emulate a dark OS, now show Paper.
