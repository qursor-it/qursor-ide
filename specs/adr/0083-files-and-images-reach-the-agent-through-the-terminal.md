# ADR-0083 — Files and images reach the agent through the terminal, not a composer

Date: 2026-10-15
Status: Accepted for this local fork. Builds on ADR-0002 (the agent is an embedded PTY) and ADR-0016 (native drag-drop is the window's, HTML5 is dead on macOS). Arises from F5 (the terminal).

## Context

The conversation with an agent is Claude Code's own TUI in an xterm.js terminal (ADR-0002). Giving it a file or an image meant typing a path by hand, or leaving the app: dropping a file on the window did nothing, and pasting a screenshot did nothing either.

- **A drop never reached the page.** Tauri reports every drag session on the window as handled (ADR-0016), so the renderer sees no `drop` on macOS, and the path is only available through Tauri's own `onDragDropEvent`.
- **A pasted image was swallowed.** Claude Code does not read images from the terminal's paste. It reads them from the OS clipboard when it receives its own image-paste key, `Alt+V` on Windows and `Ctrl+V` on macOS and Linux. The paste chord in the webview fires a `paste` event that xterm turns into a text paste, and with only an image on the clipboard the text is empty, so nothing arrived. xterm 5.5 itself writes `0x16` on `Ctrl+V` and cancels the keydown, so on Windows and Linux the browser never fires `paste` for that chord at all; `Cmd+V` on macOS is left alone and does.

## Decision

- **No composer.** An input box above or beside the terminal was considered and not taken. It would be a second input in front of Claude's own, and would lose what the TUI gives for free: `/` commands and `@file` suggestions, the prompt history, permission prompts and their keys, and every mode the agent switches the prompt into. Claude Code's prompt stays the input, and the app only helps things reach it.
- **Drop is Tauri's native event, not HTML5.** `getCurrentWebview().onDragDropEvent`, listened for once, by the first terminal built, and for the app's lifetime like the pool. Nothing else in the app listens, so nothing is displaced. The position is physical pixels; dividing by `devicePixelRatio` gives CSS pixels at any zoom, because the webview zoom is folded into it, and `document.elementFromPoint` plus the pooled hosts say which terminal it is. The paths are pasted through `term.paste`, quoted the way a real terminal does (`lib/droppedPaths.ts`): backslash-escaped on POSIX as macOS Terminal and iTerm do, double-quoted on Windows, space-separated, one trailing space. Claude Code attaches an image path as `[Image #N]` by itself.
- **The target wears an outline while a drag is over it**: 1px `primary`, 4px outside the box, set as a `data-drop-target` attribute only when the terminal under the pointer changes, so `over` costs no render (`DESIGN.md` § Terminal Well).
- **Paste is bridged, for Claude only.** A capture-phase `paste` listener on the terminal's host, for a session whose agent is Claude, sends the platform's image-paste key through `term.input` when the clipboard has an `image/*` item and no text (`lib/claudeImagePaste.ts`). A paste with text, Codex and the footer shells are untouched. On Windows, `Ctrl+V` in Claude's terminal is returned to the browser instead of being written as `0x16`, so it fires the `paste`; Linux's `Ctrl+V` is already Claude's own key and `Alt+V` on Windows already reaches the PTY as `ESC v`, so neither is sent twice.

## Consequences

- Files and screenshots reach Claude with the gesture people already know, and the TUI is still the only input.
- `Ctrl+V` in Claude's terminal on Windows now pastes text, which it did not before, as in Windows Terminal. Codex and shells keep `0x16`.
- The listener is registered on first terminal construction and never torn down, so a second consumer of the window's native drop has to share it or replace it with a dispatcher.
- Neither half can be exercised against a real webview from the browser lane. The Playwright specs emit the drag events through the mock bridge and dispatch a synthetic `paste`; what the native event reports for position on each platform is covered by reasoning (physical pixels, divided by the ratio), not by a test.
- Pasting an image in a terminal whose agent record has not learned its name yet counts as Claude, the default.
