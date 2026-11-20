# ADR-0113 — The taskbar widget is moved and sized in the taskbar, and configured from a file Rust owns

Date: 2026-11-21
Status: Accepted. Feature F34. Amends ADR-0086 (the taskbar widget).

## Context

ADR-0086 put the usage widget in the Windows taskbar at a fixed place (8 px left
of the notification area), a fixed size (244×36 logical px) and a fixed content
(two rows, Claude then Codex, the default profile of each). People want to put
it elsewhere in the taskbar, make it wider or narrower, and choose what it
shows and how it looks. The colour thresholds (70% amber, 90% red) were
constants in Rust and in the renderer.

The widget is a native layered window reparented into Explorer's taskbar, on a
thread whose input queue is coupled to Explorer's. Whatever moves it has to run
on that thread and never make other code wait on it.

## Decision

- **It moves only inside the taskbar.** A press on the widget takes the
  capture; past the system drag distance (`SM_CXDRAG`/`SM_CYDRAG`) it is a move,
  otherwise a click. It moves horizontally, stays vertically centred, and is
  clamped between the taskbar's left edge and the notification area. Dragging
  it out of the taskbar does not detach it into a floating window: that would
  be a second mode with its own z-order, full-screen and multi-monitor rules,
  for a need nobody stated.
- **The place is a gap from the notification area**, in logical px
  (`gap`, default 8). The notification area widens and narrows with its icons;
  a gap follows it, an absolute x would not. Logical px survive a DPI change.
  The widget still reserves no room: dragged over the task buttons, it covers
  them, as it could before on a crowded taskbar.
- **Resizing changes the width, not the scale.** Within 6 logical px of either
  edge the cursor is `IDC_SIZEWE` and dragging changes the width (and the place,
  for the left edge), between what the label and the 5h percentage need and
  600 px or the taskbar's width. The layout is computed, not fixed: when the
  content does not fit it drops, in order, the weekly countdown, the 5h
  countdown, the weekly bar, the weekly percentage and the 5h bar; spare width
  goes to the bars (24 to 120 px). Columns use worst-case text widths (`100%`,
  `23h 59m`, `6d 23h`), so they line up across rows and do not jump as numbers
  change. Text size is a separate setting (8–20 px), clamped so the rows fit the
  taskbar's height.
- **The configuration is a JSON file Rust owns**, `<data_dir>/taskbar_widget.json`
  (`platform/taskbar_widget/config.rs`, mirrored as `TaskbarWidgetConfig`):
  place and width, which elements show, which agents in which order and with
  which profile, one row per agent or one row for all, letter or name labels,
  text size, theme, background opacity, colours and the click action. Not the
  `settings` table, whose values are scalar strings; not the renderer's
  `prefsStore`, because Rust draws the widget at boot before any WebView, and the
  widget thread itself writes the place after a drag. Every field has a default
  that is ADR-0086's widget, so a missing file or field changes nothing. Written
  through a temp file and a rename. `get_taskbar_widget_config` /
  `set_taskbar_widget_config` apply it live.
- **The config lock is never held across anything that waits on the widget
  thread.** Settings and the widget thread share a `ConfigStore` whose mutex is
  held for a clone or a small file write. A drag writes only the place and the
  width (`set_geometry`), so a setting saved meanwhile is not undone. Saving from
  Settings reads the current place first, so it does not undo a drag either.
- **The thresholds are global usage settings, the colours are the widget's.**
  `usage.warn_percent` and `usage.critical_percent` (whole numbers 1–100, warn
  below critical, otherwise 70/90) colour both the status bar and the widget:
  the same 85% must not be amber in one place and grey in the other. The
  status bar keeps its design-system tokens (`DESIGN.md` § Colors); only the
  widget, drawn outside the app's theme, takes custom colours.
- **The click is a choice**: bring the window forward (the default), bring it
  forward and open the status bar's details (event `usage:open-details`, ignored
  when the status bar is hidden), or nothing.

## Consequences

- The real drag, resize and cursor are Win32 behaviour inside Explorer and are
  not covered by tests; the geometry (`geometry.rs`) and the layout
  (`render.rs` `plan`) are pure functions with unit tests.
- The tray fallback uses the thresholds (in the default colours) and the theme,
  not the layout or the custom colours.
- Still not built: a right-click menu on the widget, secondary taskbars, a
  floating widget outside the taskbar.
