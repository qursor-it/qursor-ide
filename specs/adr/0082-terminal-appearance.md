# ADR-0082 — Terminal appearance: a bundled mono face, a dark ANSI palette, and four terminal settings

Date: 2026-10-15
Status: Accepted for this local fork. Amends Q8 (`07-open-questions.md`) on the dark palette. Arises from F11 (settings). Builds on ADR-0002 (the agent is an embedded PTY) and ADR-0013 (where preferences live).

## Context

The conversation with an agent is Claude Code's own TUI in an xterm.js terminal (ADR-0002). The app does not draw that UI; it can only dress it — the face, the palette, the spacing, the cursor. Three things made it look worse than it had to:

- **The mono face was never shipped.** Every stack began with `"JetBrains Mono"`, and nothing registered it: no `@font-face`, no package. On a machine without the font installed, the terminal, Monaco and every SHA and chip fell back to Consolas, Menlo or Courier.
- **Dark used xterm's default ANSI colours** (Q8 left them on purpose). They are saturated primaries tuned for a black ground, and they clash with the Terminal Well and with the status and git hues around it.
- **Everything was a literal:** `fontSize: 13`, xterm's default line height, a block cursor, 8px of padding. `DESIGN.md` said terminal mono is 12/16px while the code drew 13px.

## Decision

- **Bundle JetBrains Mono** as `@fontsource-variable/jetbrains-mono` (OFL), imported once in `main.tsx`. Its family is `"JetBrains Mono Variable"`; `lib/monoFont.ts` holds the one stack (`MONO_STACK`) that the terminal, the two Monaco viewers and `--font-family-mono` use, with the static name kept second for a machine that has it installed. The face is loaded up front and every pooled terminal re-measures when it lands, so the first session of a launch does not keep the fallback's metrics.
- **Dark gets its own ANSI table** (`DARK_ANSI` in `components/terminal/themes.ts`), written in oklch and converted like the tokens. Hues follow `DESIGN.md`'s: green with `working` and `git-added`, amber with `git-modified`, red with `destructive`, blue with the primary family, cyan with `background`. Every colour holds 4.5:1 on the well except `black` and `brightBlack`, which hold 3:1 so dim still reads as dim. `themes.test.ts` pins both floors, and the light table's (whose four lighter brights are 3:1, not the 4.5:1 its old comment claimed).
- **Four renderer-only preferences** in `prefsStore` (ADR-0013 — Rust never reads them): `terminalFont` (a preset id: JetBrains Mono, Cascadia Code, Fira Code, SF Mono / Menlo, Consolas, system monospace; each a full fallback stack), `terminalFontSize` (10–20, default 13 — what every terminal drew before), `terminalLineHeight` (1.0–1.6, default 1.0), `terminalCursor` (bar, block, underline; default bar, outlined when unfocused). `terminalOptions()` maps them to xterm options and clamps whatever localStorage holds.
- **They apply live to every pooled terminal on Save** — agent and footer shells share the pool — and each is refitted, so the PTY gets its new grid. A font change refits again once `document.fonts.ready` resolves.
- **A Terminal section in Settings**, after Appearance: four `Select`s and a CSS preview on the well in the draft's face, size and line height, with a box-drawn frame in it.
- **16px of side padding and 12px top and bottom** on the agent terminal and the footer shells, up from 8px. The last-column slack rule in the desktop stylesheet needs at least 8px of it.

### Why the line height defaults to 1.0

xterm 5.5's DOM renderer — macOS, Linux, and Windows whenever WebGL is off — draws box-drawing characters as font glyphs, so a line height above 1.0 opens a gap between rows in every vertical stroke. Claude's input box is drawn with `╭─╮│╰╯`. Measured on 2026-10-15 under the DOM renderer at 3x: continuous at 1.0, visibly broken at 1.15. The setting stays, with a hint, for anybody who prefers the air to the frame.

## Consequences

- Q8's "no theme picker, no custom themes" stands: there are still two palettes, following the app's theme. Only the dark one stopped being xterm's.
- Shell output (`git status`, `ls`, test runners) changes colour in dark. Claude Code draws most of its own UI in 24-bit colour and is affected only where it uses the ANSI slots; its `dark-ansi` theme (`/theme`) uses them throughout and so inherits this palette.
- The bundle carries the variable woff2 files (Latin and the other subsets fontsource ships). Monaco and every `font-mono` surface now render the real face everywhere, not only where it was installed.
- The terminal's size is a user setting and so sits outside `DESIGN.md`'s Two Sizes Rule, which governs chrome; the rule is amended to say so.
- The footer shells lose 8px of height to the padding.
- A chat view over the JSONL transcript, or a composer above the PTY, was considered and not taken: either is a second UI beside Claude's own, and this change keeps ADR-0002's single surface.
