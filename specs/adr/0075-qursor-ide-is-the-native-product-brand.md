# ADR-0075 — Qursor IDE is the native product brand

Date: 2026-10-09
Status: Accepted for this local fork. Supersedes ADR-0074 only where it names the native product as FactorAI; all Windows platform and behavior decisions in ADR-0074 remain in force.

## Context

The owner requests the project be named `qursor-ide` and use Qursor's logo and colors. ADR-0074 described the requested platform port as a FactorAI distribution and explicitly kept Qursor branding out of scope. That naming decision is now superseded; the port itself is not.

The existing Tauri identifier `dev.factorai` also determines the app data location. Changing it would strand the database and settings created by existing FactorAI builds. The repository still uses private `@factorai/*` workspace packages, the Rust library crate `factorai_lib`, and FactorAI-named APIs and environment markers.

## Decision

Use `qursor-ide` as the Tauri product name, Rust package/binary name, and native installer stem. Use **Qursor IDE** as the native window title and bundle display name. Apply Qursor's logo and colors to the app's visual identity while preserving its existing layout, workflows, provider behavior, and native Windows/macOS/Linux functionality.

Keep the release identifier `dev.factorai`, the development identifier `dev.factorai-dev`, and the isolated Windows smoke identifier `dev.factorai-windows-smoke`. This preserves the existing data directory, development separation, and smoke harness boundary. Keep private `@factorai/*` workspace scopes, `factorai_lib`, Rust APIs, MCP names, environment variables, repository and updater URLs, upstream copyright, license, and existing macOS/Linux behavior as they are.

The visual rebrand and native package rename share this decision; changes to the renderer and brand assets can be implemented in their own slices. Historical FactorAI validation remains evidence about the FactorAI package that was tested; it does not establish that the Qursor-branded package has been rebuilt or validated.

## Consequences

Native Windows builds produce `qursor-ide.exe` and, for version 0.55.0 x64, `qursor-ide_0.55.0_x64-setup.exe`. The stable app identifier keeps existing app data available to the renamed product. Internal package and integration names retain their FactorAI spelling until a separate compatibility decision covers them.

The bundle configuration and documentation use Qursor IDE naming, and the app uses Qursor's visual identity. A new Qursor-branded installer or smoke run must be recorded separately from the historical FactorAI validation results.
