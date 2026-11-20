//! Pure rendering of the taskbar usage widget: row selection, a layout computed
//! from the user's config and the measured text, and a premultiplied RGBA
//! pixmap. No Win32 here; the native window pushes this pixmap to the screen.
//! Text baselines and bar rects are snapped to whole device pixels so the
//! widget stays crisp at 100/125/150% scaling.

use fontdue::Font;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

use super::config::{parse_hex, Agent, AgentSlot, Layout, Look, Show, Theme, WidgetConfig};
use super::geometry::MAX_WIDTH;
use crate::models::{ProviderUsage, UsageWindow};
use crate::services::usage::{Thresholds, STALE_MS};

/// Space at the left and right edges, logical px.
const MARGIN: f32 = 4.0;
/// Between two columns of a row.
const COL_GAP: f32 = 6.0;
/// What the leftover width gives a bar, logical px.
const BAR_MIN: f32 = 24.0;
const BAR_MAX: f32 = 120.0;
/// The proportions of the original 11 px widget: an 18 px row and a 6 px bar.
const BASE_FONT: f32 = 11.0;
const BASE_ROW: f32 = 18.0;
const BASE_BAR_H: f32 = 6.0;
const SEPARATOR: &str = "\u{b7}";
const STALE_ALPHA: f32 = 0.6;
/// Countdowns sit back from the percentages: the same colour at this share of
/// the row's alpha.
const COUNTDOWN_ALPHA: f32 = 0.7;
const MAX_SCALE: f32 = 4.0;
const WARN_RGB: [u8; 3] = [0xE8, 0xA3, 0x3D];
const CRITICAL_RGB: [u8; 3] = [0xE5, 0x48, 0x4D];

/// One line of the widget.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetRow {
	pub agent: Agent,
	pub five_hour: Option<f64>,
	pub weekly: Option<f64>,
	/// Whole minutes until each window resets; `None` without a future reset.
	pub five_hour_reset_min: Option<i64>,
	pub weekly_reset_min: Option<i64>,
	pub stale: bool,
}

/// The visible agents in the config's order, each with its chosen profile or,
/// without one (or when it is gone), the agent's default. An agent without
/// data is absent.
pub fn rows_from(usage: &[ProviderUsage], now_ms: i64, agents: &[AgentSlot]) -> Vec<WidgetRow> {
	agents
		.iter()
		.filter(|slot| slot.visible)
		.filter_map(|slot| {
			let of_agent = || usage.iter().filter(|u| u.agent == slot.agent.id());
			let chosen =
				slot.profile_id.as_deref().and_then(|id| of_agent().find(|u| u.profile_id == id));
			let u = chosen.or_else(|| of_agent().find(|u| u.is_default))?;
			let reset_min = |w: &Option<UsageWindow>| {
				let left = w.as_ref()?.resets_at? - now_ms;
				(left > 0).then_some(left / 60_000)
			};
			Some(WidgetRow {
				agent: slot.agent,
				five_hour: u.five_hour.as_ref().map(|w| w.used_percent),
				weekly: u.weekly.as_ref().map(|w| w.used_percent),
				five_hour_reset_min: reset_min(&u.five_hour),
				weekly_reset_min: reset_min(&u.weekly),
				stale: now_ms - u.updated_at > STALE_MS,
			})
		})
		.collect()
}

/// Segoe UI on Windows; nothing elsewhere (the widget is Windows only).
pub fn load_system_font() -> Option<Font> {
	#[cfg(windows)]
	{
		let windir = std::env::var_os("WINDIR")?;
		let path = std::path::Path::new(&windir).join("Fonts").join("segoeui.ttf");
		let bytes = std::fs::read(path).ok()?;
		Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
	}
	#[cfg(not(windows))]
	{
		None
	}
}

/// Whether the text is drawn for a light taskbar, from the config's theme and
/// what the system says.
pub fn light_taskbar(theme: Theme, system_light: bool) -> bool {
	match theme {
		Theme::Auto => system_light,
		Theme::Light => true,
		Theme::Dark => false,
	}
}

/// One thing in a row, in drawing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Item {
	Label,
	FiveBar,
	FivePct,
	FiveReset,
	WeekBar,
	WeekPct,
	WeekReset,
}

impl Item {
	fn is_bar(self) -> bool {
		matches!(self, Item::FiveBar | Item::WeekBar)
	}
}

/// What goes first when the width runs out. The label and the 5h percentage
/// are never dropped for lack of room.
const DROP_ORDER: [Item; 5] =
	[Item::WeekReset, Item::FiveReset, Item::WeekBar, Item::WeekPct, Item::FiveBar];

fn wanted(show: &Show) -> Vec<Item> {
	[
		(Item::Label, true),
		(Item::FiveBar, show.five_hour_bar),
		(Item::FivePct, show.five_hour_percent),
		(Item::FiveReset, show.five_hour_reset),
		(Item::WeekBar, show.weekly_bar),
		(Item::WeekPct, show.weekly_percent),
		(Item::WeekReset, show.weekly_reset),
	]
	.into_iter()
	.filter_map(|(item, on)| on.then_some(item))
	.collect()
}

fn drop_one(items: &mut Vec<Item>) -> bool {
	let Some(i) = DROP_ORDER.iter().find_map(|d| items.iter().position(|x| x == d)) else {
		return false;
	};
	items.remove(i);
	true
}

fn advance(font: &Font, text: &str, px: f32) -> f32 {
	text.chars().map(|c| font.metrics(c, px).advance_width).sum()
}

/// How wide each item can be. Texts are measured at their worst case
/// (`100%`, `23h 59m`, `w 100%`, `6d 23h`), not at the numbers on show, so the
/// widget does not shift as they change and the rows' columns line up.
struct Metrics {
	label: f32,
	five_pct: f32,
	five_reset: f32,
	week_pct: f32,
	week_reset: f32,
	/// Between two agents on one row: a gap, the dot, a gap.
	sep: f32,
	/// Agents sharing a row: every visible one when inline, otherwise one.
	segs: usize,
}

impl Metrics {
	fn new(font: &Font, cfg: &WidgetConfig, px: f32) -> Self {
		let visible = cfg.visible_agents().count();
		let shown: Vec<Agent> = if visible == 0 {
			cfg.agents.iter().map(|s| s.agent).collect()
		} else {
			cfg.visible_agents().map(|s| s.agent).collect()
		};
		let label =
			shown.iter().map(|a| advance(font, &cfg.labels.text(*a), px)).fold(0.0, f32::max);
		Self {
			label,
			five_pct: advance(font, "100%", px),
			five_reset: advance(font, "23h 59m", px),
			week_pct: advance(font, "w 100%", px),
			week_reset: advance(font, "6d 23h", px),
			sep: COL_GAP * 2.0 + advance(font, SEPARATOR, px),
			segs: if cfg.layout == Layout::Inline { visible.max(1) } else { 1 },
		}
	}

	fn item_w(&self, item: Item, bar_w: f32) -> f32 {
		match item {
			Item::Label => self.label,
			Item::FivePct => self.five_pct,
			Item::FiveReset => self.five_reset,
			Item::WeekPct => self.week_pct,
			Item::WeekReset => self.week_reset,
			Item::FiveBar | Item::WeekBar => bar_w,
		}
	}

	/// One agent's items with gaps between.
	fn content(&self, items: &[Item], bar_w: f32) -> f32 {
		let sum: f32 = items.iter().map(|i| self.item_w(*i, bar_w)).sum();
		sum + COL_GAP * items.len().saturating_sub(1) as f32
	}

	fn total(&self, items: &[Item], bar_w: f32) -> f32 {
		let segs = self.segs as f32;
		MARGIN * 2.0 + segs * self.content(items, bar_w) + (segs - 1.0) * self.sep
	}
}

/// Where an item sits within its agent's span, logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
	pub item: Item,
	pub x: f32,
	pub w: f32,
}

/// The widget's columns for a width.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
	pub placed: Vec<Placed>,
	/// One agent's span, and the space between two on one row.
	pub seg_w: f32,
	pub sep_w: f32,
	pub bar_w: f32,
}

impl Plan {
	/// Left edge of the `i`-th agent on a row, logical px.
	fn origin(&self, i: usize) -> f32 {
		MARGIN + i as f32 * (self.seg_w + self.sep_w)
	}
}

/// Which items fit in `width` logical px at font size `px`, and where. What
/// does not fit is dropped in `DROP_ORDER`; what is left over is shared by the
/// bars (24..=120), and without bars the row is left-aligned.
pub(crate) fn plan(font: &Font, cfg: &WidgetConfig, px: f32, width: f32) -> Plan {
	let m = Metrics::new(font, cfg, px);
	let mut items = wanted(&cfg.show);
	while m.total(&items, BAR_MIN) > width && drop_one(&mut items) {}
	let bars = items.iter().filter(|i| i.is_bar()).count() * m.segs;
	let bar_w = if bars == 0 {
		0.0
	} else {
		((width - m.total(&items, 0.0)) / bars as f32).clamp(BAR_MIN, BAR_MAX).floor()
	};
	let mut x = 0.0;
	let placed = items
		.iter()
		.map(|&item| {
			let w = m.item_w(item, bar_w);
			let p = Placed { item, x, w };
			x += w + COL_GAP;
			p
		})
		.collect();
	Plan { placed, seg_w: m.content(&items, bar_w), sep_w: m.sep, bar_w }
}

fn lines(cfg: &WidgetConfig) -> usize {
	match cfg.layout {
		Layout::Inline => 1,
		Layout::Rows => cfg.visible_agents().count().max(1),
	}
}

fn row_height(px: f32) -> f32 {
	px * BASE_ROW / BASE_FONT
}

/// The font size in use: the config's, and no larger than lets every line fit
/// in `max_h` logical px (the taskbar's client height).
pub(crate) fn fitted_px(cfg: &WidgetConfig, max_h: f32) -> f32 {
	let px = cfg.font_px;
	let h = lines(cfg) as f32 * row_height(px);
	if max_h.is_finite() && max_h > 0.0 && h > max_h {
		px * max_h / h
	} else {
		px
	}
}

/// The least width that still shows the label and the 5h percentage (when it
/// is on), logical px.
pub fn min_width(font: &Font, cfg: &WidgetConfig) -> f32 {
	let m = Metrics::new(font, cfg, cfg.font_px);
	let mut least = vec![Item::Label];
	if cfg.show.five_hour_percent {
		least.push(Item::FivePct);
	}
	m.total(&least, 0.0).ceil()
}

/// The width actually drawn: the config's, within the least and the most.
pub fn effective_width(font: &Font, cfg: &WidgetConfig) -> f32 {
	cfg.width.min(MAX_WIDTH).max(min_width(font, cfg))
}

/// The colours a row is drawn in.
struct Ink {
	text: [u8; 3],
	normal: Option<[u8; 3]>,
	warn: [u8; 3],
	critical: [u8; 3],
	thresholds: Thresholds,
}

impl Ink {
	fn new(look: &Look, light_taskbar: bool) -> Self {
		let c = &look.config.colors;
		Self {
			text: if light_taskbar { [0, 0, 0] } else { [255, 255, 255] },
			normal: c.normal.as_deref().and_then(parse_hex),
			warn: parse_hex(&c.warn).unwrap_or(WARN_RGB),
			critical: parse_hex(&c.critical).unwrap_or(CRITICAL_RGB),
			thresholds: look.thresholds,
		}
	}
}

/// Draw the rows into a transparent pixmap `effective_width` wide and one row
/// per line tall. `max_h` is the taskbar's client height in logical px.
pub fn render(
	rows: &[WidgetRow],
	font: &Font,
	scale: f32,
	light_taskbar: bool,
	look: &Look,
	max_h: f32,
) -> Pixmap {
	let cfg = &look.config;
	let scale = if scale.is_finite() { scale.clamp(0.5, MAX_SCALE) } else { 1.0 };
	let px = fitted_px(cfg, max_h);
	let width = effective_width(font, cfg);
	let n = lines(cfg);
	let row_h = (row_height(px) * scale).round().max(1.0);
	let w = (width * scale).round().max(1.0) as u32;
	let h = (row_h * n as f32) as u32;
	// Scale is clamped to 0.5..=4, so the size is non-zero and small.
	let mut pm = Pixmap::new(w, h).expect("non-zero pixmap size");
	let ink = Ink::new(look, light_taskbar);
	if cfg.background_opacity > 0.0 {
		let behind = ink.text.map(|c| 255 - c);
		fill_rounded(&mut pm, 4.0 * scale, rgba(behind, cfg.background_opacity));
	}
	let plan = plan(font, cfg, px, width);

	let groups: Vec<&[WidgetRow]> = match cfg.layout {
		Layout::Inline => vec![rows],
		Layout::Rows => rows.iter().take(n).map(std::slice::from_ref).collect(),
	};
	for (line, group) in groups.iter().enumerate() {
		let top = line as f32 * row_h;
		for (i, row) in group.iter().enumerate() {
			let at = Frame { x0: plan.origin(i), top, row_h, scale, px };
			if i > 0 {
				let x = ((at.x0 - plan.sep_w + COL_GAP) * scale).round();
				let baseline = top + (row_h / 2.0 + px * scale * 0.36).round();
				draw_text(
					&mut pm,
					font,
					px * scale,
					SEPARATOR,
					x,
					baseline,
					ink.text,
					COUNTDOWN_ALPHA,
				);
			}
			draw_row(&mut pm, font, row, cfg, &plan, &ink, &at);
		}
	}
	pm
}

/// Where one row is drawn: its left edge in logical px, the rest in device px.
struct Frame {
	x0: f32,
	top: f32,
	row_h: f32,
	scale: f32,
	/// Logical font size.
	px: f32,
}

fn draw_row(
	pm: &mut Pixmap,
	font: &Font,
	row: &WidgetRow,
	cfg: &WidgetConfig,
	plan: &Plan,
	ink: &Ink,
	at: &Frame,
) {
	let alpha = if row.stale { STALE_ALPHA } else { 1.0 };
	let dim = alpha * COUNTDOWN_ALPHA;
	let px = at.px * at.scale;
	let baseline = at.top + (at.row_h / 2.0 + px * 0.36).round();
	let bar_h = (BASE_BAR_H * at.px / BASE_FONT * at.scale).round().max(1.0);
	for p in &plan.placed {
		let x = ((at.x0 + p.x) * at.scale).round();
		let mut text = |s: &str, a: f32| draw_text(pm, font, px, s, x, baseline, ink.text, a);
		match p.item {
			Item::Label => text(&cfg.labels.text(row.agent), alpha),
			Item::FivePct => text(&pct_label(row.five_hour, ""), alpha),
			Item::WeekPct => text(&pct_label(row.weekly, "w "), alpha),
			Item::FiveReset => {
				if let Some(min) = row.five_hour_reset_min {
					text(&countdown(min), dim);
				}
			}
			Item::WeekReset => {
				if let Some(min) = row.weekly_reset_min {
					text(&countdown(min), dim);
				}
			}
			Item::FiveBar | Item::WeekBar => {
				let pct = if p.item == Item::FiveBar { row.five_hour } else { row.weekly };
				let w = (p.w * at.scale).round();
				let bar = Bar { x, w, top: at.top, row_h: at.row_h, h: bar_h };
				draw_bar(pm, pct, &bar, ink, alpha);
			}
		}
	}
}

/// NaN reads as 0; everything else clamps to 0..=100.
pub(crate) fn sane(p: f64) -> f64 {
	if p.is_nan() {
		0.0
	} else {
		p.clamp(0.0, 100.0)
	}
}

/// The whole percent shown for `p`: floored, so a label never reaches a band
/// threshold before the colour does — 89.9 reads "89%" in amber.
pub(crate) fn whole_pct(p: f64) -> u32 {
	sane(p).floor() as u32
}

/// Time until a reset: `<1m`, `45m`, `2h 14m`, `3d 4h`. The same words as the
/// status bar's `formatCountdown`.
pub(crate) fn countdown(min: i64) -> String {
	let min = min.max(0);
	if min < 1 {
		"<1m".into()
	} else if min < 60 {
		format!("{min}m")
	} else if min < 24 * 60 {
		format!("{}h {}m", min / 60, min % 60)
	} else {
		format!("{}d {}h", min / (24 * 60), min / 60 % 24)
	}
}

fn pct_label(v: Option<f64>, prefix: &str) -> String {
	match v {
		Some(p) => format!("{prefix}{}%", whole_pct(p)),
		None => format!("{prefix}--"),
	}
}

/// A bar's rect in device px: `top` and `row_h` are its row's.
struct Bar {
	x: f32,
	w: f32,
	top: f32,
	row_h: f32,
	h: f32,
}

fn draw_bar(pm: &mut Pixmap, pct: Option<f64>, bar: &Bar, ink: &Ink, alpha: f32) {
	let y = bar.top + ((bar.row_h - bar.h) / 2.0).round();
	let p = pct.map(sane);
	let fw = p.map_or(0.0, |p| (bar.w * p as f32 / 100.0).round());
	// The track is drawn only where the fill is not, so the fill keeps its exact alpha.
	fill_rect(pm, bar.x + fw, y, bar.w - fw, bar.h, rgba(ink.text, 0.25 * alpha));
	let Some(p) = p else { return };
	let color = match band(p, ink.thresholds) {
		Band::Critical => rgba(ink.critical, alpha),
		Band::Warn => rgba(ink.warn, alpha),
		Band::Normal => match ink.normal {
			Some(c) => rgba(c, alpha),
			None => rgba(ink.text, 0.7 * alpha),
		},
	};
	fill_rect(pm, bar.x, y, fw, bar.h, color);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Band {
	Normal,
	Warn,
	Critical,
}

/// Amber or red from the thresholds; `Normal` below them.
pub(crate) fn band(p: f64, t: Thresholds) -> Band {
	if p >= t.critical {
		Band::Critical
	} else if p >= t.warn {
		Band::Warn
	} else {
		Band::Normal
	}
}

/// The tray fallback's square icon: the percentage (no `%`, it would not fit)
/// in its band colour, the taskbar's text colour when neutral, shrunk until it
/// fits with a pixel to spare on each side.
pub fn render_icon(
	pct: Option<f64>,
	font: &Font,
	size: u32,
	light_taskbar: bool,
	thresholds: Thresholds,
) -> Pixmap {
	let size = size.clamp(1, 256);
	let mut pm = Pixmap::new(size, size).expect("non-zero pixmap size");
	let s = size as f32;
	let label = pct.map_or_else(|| "--".to_string(), |p| whole_pct(p).to_string());
	let text = if light_taskbar { [0, 0, 0] } else { [255, 255, 255] };
	let color = match pct.map(sane).map(|p| band(p, thresholds)) {
		Some(Band::Critical) => CRITICAL_RGB,
		Some(Band::Warn) => WARN_RGB,
		_ => text,
	};
	let width = |px: f32| label.chars().map(|c| font.metrics(c, px).advance_width).sum::<f32>();
	let mut px = s * 0.75;
	while px > 4.0 && width(px) > s - 3.0 {
		px -= 0.5;
	}
	let x = ((s - width(px)) / 2.0).round();
	// Digits are about 0.7 em tall in Segoe UI and Liberation Sans.
	let baseline = ((s + px * 0.7) / 2.0).round();
	draw_text(&mut pm, font, px, &label, x, baseline, color, 1.0);
	pm
}

fn rgba(c: [u8; 3], a: f32) -> Color {
	Color::from_rgba8(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Color) {
	let Some(rect) = Rect::from_xywh(x, y, w, h) else { return };
	let mut paint = Paint::default();
	paint.set_color(color);
	paint.anti_alias = false;
	pm.fill_rect(rect, &paint, Transform::identity(), None);
}

/// The whole pixmap as a rounded rectangle of corner radius `r`.
fn fill_rounded(pm: &mut Pixmap, r: f32, color: Color) {
	let (w, h) = (pm.width() as f32, pm.height() as f32);
	let r = r.min(w / 2.0).min(h / 2.0);
	let mut pb = PathBuilder::new();
	pb.move_to(r, 0.0);
	pb.line_to(w - r, 0.0);
	pb.quad_to(w, 0.0, w, r);
	pb.line_to(w, h - r);
	pb.quad_to(w, h, w - r, h);
	pb.line_to(r, h);
	pb.quad_to(0.0, h, 0.0, h - r);
	pb.line_to(0.0, r);
	pb.quad_to(0.0, 0.0, r, 0.0);
	pb.close();
	let Some(path) = pb.finish() else { return };
	let mut paint = Paint::default();
	paint.set_color(color);
	paint.anti_alias = true;
	pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// Blend glyph coverage masks in `color` (premultiplied) with a snapped baseline.
#[allow(clippy::too_many_arguments)] // flat drawing helper; a struct would only rename the arguments
fn draw_text(
	pm: &mut Pixmap,
	font: &Font,
	px: f32,
	s: &str,
	x: f32,
	baseline: f32,
	color: [u8; 3],
	alpha: f32,
) {
	let (pw, ph) = (pm.width() as i32, pm.height() as i32);
	let data = pm.data_mut();
	let mut pen = x;
	for ch in s.chars() {
		let (m, cov) = font.rasterize(ch, px);
		let gx = pen.round() as i32 + m.xmin;
		let gy = baseline as i32 - m.height as i32 - m.ymin;
		for row in 0..m.height {
			for col in 0..m.width {
				let (dx, dy) = (gx + col as i32, gy + row as i32);
				if dx < 0 || dy < 0 || dx >= pw || dy >= ph {
					continue;
				}
				let a = cov[row * m.width + col] as f32 / 255.0 * alpha;
				if a <= 0.0 {
					continue;
				}
				let i = ((dy * pw + dx) * 4) as usize;
				let inv = 1.0 - a;
				for (c, src) in color.iter().enumerate() {
					data[i + c] = (*src as f32 * a + data[i + c] as f32 * inv).round() as u8;
				}
				data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv).round() as u8;
			}
		}
		pen += m.advance_width;
	}
}

#[cfg(test)]
mod tests {
	use fontdue::FontSettings;

	use super::*;
	use crate::models::UsageSource;
	use crate::platform::taskbar_widget::config::{Colors, Labels};

	fn font() -> Font {
		let p = concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/../../../tests/fixtures/fonts/LiberationSans-Regular.ttf"
		);
		Font::from_bytes(std::fs::read(p).unwrap(), FontSettings::default()).unwrap()
	}

	const NO_LIMIT: f32 = f32::INFINITY;

	fn draw_with(rows: &[WidgetRow], scale: f32, light: bool, look: &Look) -> Pixmap {
		render(rows, &font(), scale, light, look, NO_LIMIT)
	}

	/// The widget as it was before it was configurable.
	fn draw(rows: &[WidgetRow], scale: f32, light: bool) -> Pixmap {
		draw_with(rows, scale, light, &Look::default())
	}

	fn look_with(edit: impl FnOnce(&mut WidgetConfig)) -> Look {
		let mut look = Look::default();
		edit(&mut look.config);
		look
	}

	#[test]
	fn labels_floor_so_they_agree_with_the_band() {
		assert_eq!(pct_label(Some(89.9), ""), "89%");
		assert_eq!(pct_label(Some(69.99), "w "), "w 69%");
		assert_eq!(pct_label(Some(90.0), ""), "90%");
		assert_eq!(pct_label(Some(150.0), ""), "100%");
		assert_eq!(pct_label(None, "w "), "w --");
	}

	fn usage(agent: &str, id: &str, default: bool, five: f64, week: f64, at: i64) -> ProviderUsage {
		let win = |p| Some(UsageWindow { used_percent: p, resets_at: None });
		ProviderUsage {
			agent: agent.into(),
			profile_id: id.into(),
			profile_name: id.into(),
			is_default: default,
			five_hour: win(five),
			weekly: win(week),
			per_model: vec![],
			source: UsageSource::StatusLine,
			updated_at: at,
		}
	}

	fn sample() -> Vec<WidgetRow> {
		vec![
			WidgetRow {
				five_hour_reset_min: Some(134),
				weekly_reset_min: Some(3 * 24 * 60 + 4 * 60),
				..row(Agent::Claude, Some(62.0), Some(31.0))
			},
			WidgetRow {
				five_hour_reset_min: Some(45),
				weekly_reset_min: Some(6 * 24 * 60 + 23 * 60),
				..row(Agent::Codex, Some(92.0), Some(16.0))
			},
		]
	}

	fn row(agent: Agent, five_hour: Option<f64>, weekly: Option<f64>) -> WidgetRow {
		WidgetRow {
			agent,
			five_hour,
			weekly,
			five_hour_reset_min: None,
			weekly_reset_min: None,
			stale: false,
		}
	}

	fn slots() -> Vec<AgentSlot> {
		WidgetConfig::default().agents
	}

	#[test]
	fn countdown_boundaries() {
		assert_eq!(countdown(-3), "<1m");
		assert_eq!(countdown(0), "<1m");
		assert_eq!(countdown(1), "1m");
		assert_eq!(countdown(45), "45m");
		assert_eq!(countdown(59), "59m");
		assert_eq!(countdown(60), "1h 0m");
		assert_eq!(countdown(180), "3h 0m");
		assert_eq!(countdown(134), "2h 14m");
		assert_eq!(countdown(23 * 60 + 59), "23h 59m");
		assert_eq!(countdown(24 * 60), "1d 0h");
		assert_eq!(countdown(3 * 24 * 60 + 4 * 60 + 30), "3d 4h");
		assert_eq!(countdown(6 * 24 * 60 + 23 * 60 + 59), "6d 23h");
	}

	#[test]
	fn rows_from_counts_down_to_future_resets_only() {
		let now = 10_000_000;
		let mut u = usage("claude", "main", true, 62.0, 31.0, now);
		let at = |ms: i64| Some(UsageWindow { used_percent: 1.0, resets_at: Some(now + ms) });
		u.five_hour = at(134 * 60_000 + 59_999);
		u.weekly = at(30_000);
		let rows = rows_from(&[u.clone()], now, &slots());
		assert_eq!((rows[0].five_hour_reset_min, rows[0].weekly_reset_min), (Some(134), Some(0)));
		u.five_hour = at(0);
		u.weekly = at(-60_000);
		let rows = rows_from(&[u.clone()], now, &slots());
		assert_eq!((rows[0].five_hour_reset_min, rows[0].weekly_reset_min), (None, None));
		u.five_hour = None;
		assert_eq!(rows_from(&[u], now, &slots())[0].five_hour_reset_min, None);
	}

	#[test]
	fn rows_from_picks_default_profiles_in_order() {
		let now = 10_000_000;
		let list = vec![
			usage("codex", "cx", true, 92.0, 16.0, now),
			usage("claude", "other", false, 5.0, 5.0, now),
			usage("claude", "main", true, 62.0, 31.0, now - STALE_MS - 1),
		];
		let rows = rows_from(&list, now, &slots());
		assert_eq!(rows.len(), 2);
		assert_eq!(
			(rows[0].agent, rows[0].five_hour, rows[0].stale),
			(Agent::Claude, Some(62.0), true)
		);
		assert_eq!(
			(rows[1].agent, rows[1].weekly, rows[1].stale),
			(Agent::Codex, Some(16.0), false)
		);
	}

	#[test]
	fn rows_from_follows_the_chosen_profile_order_and_visibility() {
		let now = 10_000_000;
		let list = vec![
			usage("claude", "main", true, 62.0, 31.0, now),
			usage("claude", "other", false, 5.0, 5.0, now),
			usage("codex", "cx", true, 92.0, 16.0, now),
		];
		let slot = |agent, visible, profile: Option<&str>| AgentSlot {
			agent,
			visible,
			profile_id: profile.map(Into::into),
		};
		// Codex first, Claude on its non-default profile.
		let rows = rows_from(
			&list,
			now,
			&[slot(Agent::Codex, true, None), slot(Agent::Claude, true, Some("other"))],
		);
		assert_eq!(rows.iter().map(|r| r.agent).collect::<Vec<_>>(), [Agent::Codex, Agent::Claude]);
		assert_eq!(rows[1].five_hour, Some(5.0));
		// A profile that is gone, or of another agent, falls back to the default.
		for gone in ["deleted", "cx"] {
			let rows = rows_from(&list, now, &[slot(Agent::Claude, true, Some(gone))]);
			assert_eq!(rows[0].five_hour, Some(62.0));
		}
		// Hidden agents are left out.
		let rows = rows_from(
			&list,
			now,
			&[slot(Agent::Claude, false, None), slot(Agent::Codex, true, None)],
		);
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].agent, Agent::Codex);
	}

	#[test]
	fn render_has_scaled_size() {
		let pm = draw(&sample(), 1.5, false);
		assert_eq!((pm.width(), pm.height()), (366, 54));
		let pm = draw(&sample(), 1.0, false);
		assert_eq!((pm.width(), pm.height()), (244, 36));
	}

	#[test]
	fn empty_rows_render_transparent() {
		let pm = draw(&[], 1.0, false);
		assert!(pm.data().chunks(4).all(|p| p[3] == 0));
	}

	fn plan_for(cfg: &WidgetConfig) -> Plan {
		plan(&font(), cfg, cfg.font_px, effective_width(&font(), cfg))
	}

	/// Pixel in the first row of the bar `item`, one pixel in from its left end.
	fn bar_pixel(
		look: &Look,
		item: Item,
		five: f64,
		week: f64,
		stale: bool,
		light: bool,
	) -> [u8; 4] {
		let r = WidgetRow { stale, ..row(Agent::Claude, Some(five), Some(week)) };
		let pm = draw_with(&[r], 1.0, light, look);
		let p = plan_for(&look.config);
		let bar = p.placed.iter().find(|b| b.item == item).expect("bar is on the widget");
		let c = pm.pixel((MARGIN + bar.x).round() as u32 + 1, 9).unwrap();
		[c.red(), c.green(), c.blue(), c.alpha()]
	}

	fn bar_px(pct: f64, stale: bool, light: bool) -> [u8; 4] {
		bar_pixel(&Look::default(), Item::FiveBar, pct, 0.0, stale, light)
	}

	fn near(a: [u8; 4], b: [u8; 4]) -> bool {
		a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 1)
	}

	#[test]
	fn critical_bar_contains_red_pixels() {
		assert_eq!(bar_px(92.0, false, false), [0xE5, 0x48, 0x4D, 255]);
	}

	#[test]
	fn warn_bar_is_amber() {
		assert_eq!(bar_px(75.0, false, false), [0xE8, 0xA3, 0x3D, 255]);
	}

	#[test]
	fn stale_critical_bar_is_premultiplied_at_60_percent() {
		let want =
			[(0xE5 as f32 * 0.6) as u8, (0x48 as f32 * 0.6) as u8, (0x4D as f32 * 0.6) as u8, 153];
		assert!(near(bar_px(95.0, true, false), want), "{:?}", bar_px(95.0, true, false));
	}

	#[test]
	fn neutral_fill_is_exactly_70_percent_text_colour() {
		// White at 70% alpha, premultiplied: 255 * 0.7 = 178.5.
		assert!(near(bar_px(50.0, false, false), [179, 179, 179, 179]));
		// Black at 70% alpha: colour 0, alpha 179.
		assert!(near(bar_px(50.0, false, true), [0, 0, 0, 179]));
	}

	#[test]
	fn threshold_boundaries() {
		let neutral = [179, 179, 179, 179];
		assert!(near(bar_px(69.9, false, false), neutral));
		assert_eq!(bar_px(70.0, false, false), [0xE8, 0xA3, 0x3D, 255]);
		assert_eq!(bar_px(89.9, false, false), [0xE8, 0xA3, 0x3D, 255]);
		assert_eq!(bar_px(90.0, false, false), [0xE5, 0x48, 0x4D, 255]);
	}

	#[test]
	fn custom_thresholds_move_the_bands() {
		let look =
			Look { thresholds: Thresholds { warn: 50.0, critical: 80.0 }, ..Look::default() };
		let at = |pct| bar_pixel(&look, Item::FiveBar, pct, 0.0, false, false);
		assert!(near(at(49.0), [179, 179, 179, 179]));
		assert_eq!(at(50.0), [0xE8, 0xA3, 0x3D, 255]);
		assert_eq!(at(79.0), [0xE8, 0xA3, 0x3D, 255]);
		assert_eq!(at(80.0), [0xE5, 0x48, 0x4D, 255]);
		assert_eq!(band(95.0, look.thresholds), Band::Critical);
		assert_eq!(band(10.0, look.thresholds), Band::Normal);
	}

	#[test]
	fn custom_colours_replace_the_bands() {
		let look = look_with(|c| {
			c.colors = Colors {
				normal: Some("#445566".into()),
				warn: "#112233".into(),
				critical: "#aabbcc".into(),
			};
		});
		let at = |pct| bar_pixel(&look, Item::FiveBar, pct, 0.0, false, false);
		assert_eq!(at(10.0), [0x44, 0x55, 0x66, 255]);
		assert_eq!(at(75.0), [0x11, 0x22, 0x33, 255]);
		assert_eq!(at(95.0), [0xAA, 0xBB, 0xCC, 255]);
	}

	#[test]
	fn out_of_range_and_nan_inputs_clamp_without_panicking() {
		for v in [f64::NAN, -5.0, 150.0, f64::INFINITY] {
			let row = row(Agent::Claude, Some(v), Some(v));
			let pm = draw(&[row], 1.0, false);
			assert_eq!(pm.width(), 244);
		}
		assert_eq!(bar_px(150.0, false, false), [0xE5, 0x48, 0x4D, 255]);
		// -5 and NaN fill nothing: the pixel is the track (25% alpha).
		assert!(near(bar_px(-5.0, false, false), [64, 64, 64, 64]));
		assert!(near(bar_px(f64::NAN, false, false), [64, 64, 64, 64]));
		let huge = draw(&sample(), 1000.0, false);
		assert_eq!(huge.width(), 976);
	}

	#[test]
	fn text_draws_some_pixels() {
		let pm = draw(&sample(), 1.0, false);
		assert!(ink_in(&pm, 0, 18));
	}

	fn advance_of(f: &Font, text: &str, px: f32) -> f32 {
		text.chars().map(|c| f.metrics(c, px).advance_width).sum()
	}

	/// Worst case: `100%`, `23h 59m`, `w 100%`, `6d 23h`.
	fn widest() -> WidgetRow {
		WidgetRow {
			five_hour_reset_min: Some(23 * 60 + 59),
			weekly_reset_min: Some(6 * 24 * 60 + 23 * 60),
			..row(Agent::Claude, Some(100.0), Some(100.0))
		}
	}

	/// Does any pixel of `pm` in columns `x0..x1` carry ink?
	fn ink_in(pm: &Pixmap, x0: u32, x1: u32) -> bool {
		(x0..x1.min(pm.width()))
			.any(|x| (0..pm.height()).any(|y| pm.pixel(x, y).unwrap().alpha() > 0))
	}

	fn items(p: &Plan) -> Vec<Item> {
		p.placed.iter().map(|b| b.item).collect()
	}

	fn all_on() -> WidgetConfig {
		look_with(|c| c.show.weekly_bar = true).config
	}

	#[test]
	fn defaults_reproduce_the_original_columns() {
		let cfg = WidgetConfig::default();
		let p = plan_for(&cfg);
		assert_eq!(
			items(&p),
			[
				Item::Label,
				Item::FiveBar,
				Item::FivePct,
				Item::FiveReset,
				Item::WeekPct,
				Item::WeekReset
			]
		);
		// Roughly the old 52 px bar, and the last countdown ends at the margin.
		assert!((40.0..=70.0).contains(&p.bar_w), "{}", p.bar_w);
		let last = p.placed.last().unwrap();
		let end = MARGIN + last.x + last.w;
		assert!((end - (244.0 - MARGIN)).abs() <= 1.0, "ends at {end}");
		let bar = p.placed.iter().find(|b| b.item == Item::FiveBar).unwrap();
		assert!((MARGIN + bar.x - 18.0).abs() <= 2.0, "bar at {}", MARGIN + bar.x);
	}

	#[test]
	fn narrowing_drops_in_the_documented_order() {
		let mut cfg = all_on();
		let mut previous = items(&plan(&font(), &cfg, 11.0, 600.0)).len();
		assert_eq!(previous, 7);
		for w in (20..600).rev() {
			cfg.width = w as f32;
			let p = plan(&font(), &cfg, 11.0, w as f32);
			let kept = items(&p);
			assert!(kept.len() <= previous, "width {w} grew");
			previous = kept.len();
			let dropped = 7 - kept.len();
			// What is gone is always a prefix of the drop order.
			for (i, item) in DROP_ORDER.iter().enumerate() {
				assert_eq!(kept.contains(item), i >= dropped, "width {w}, {item:?}");
			}
			assert!(kept.contains(&Item::Label) && kept.contains(&Item::FivePct));
		}
		// Down at the floor only the label and the 5h percentage are left.
		assert_eq!(previous, 2);
	}

	#[test]
	fn bars_share_the_leftover_between_24_and_120() {
		let mut cfg = WidgetConfig::default();
		let bar_at = |cfg: &WidgetConfig, w: f32| plan(&font(), cfg, 11.0, w).bar_w;
		assert!(bar_at(&cfg, 300.0) > bar_at(&cfg, 244.0));
		assert_eq!(bar_at(&cfg, 600.0), 120.0);
		// Whenever the bar is on show it is at least 24.
		for w in 100..600 {
			let p = plan(&font(), &cfg, 11.0, w as f32);
			if items(&p).contains(&Item::FiveBar) {
				assert!((24.0..=120.0).contains(&p.bar_w), "width {w}: {}", p.bar_w);
			}
		}
		// Two bars split what one had.
		cfg.show.weekly_bar = true;
		assert!(bar_at(&cfg, 400.0) < bar_at(&WidgetConfig::default(), 400.0));
	}

	#[test]
	fn without_bars_the_row_is_left_aligned_whatever_the_width() {
		let mut cfg = WidgetConfig::default();
		cfg.show.five_hour_bar = false;
		let narrow = plan(&font(), &cfg, 11.0, 244.0);
		let wide = plan(&font(), &cfg, 11.0, 500.0);
		assert_eq!(narrow.placed, wide.placed);
		assert_eq!(narrow.placed[0].x, 0.0);
	}

	#[test]
	fn the_weekly_bar_fills_with_the_weekly_percentage() {
		let look = look_with(|c| {
			c.show.weekly_bar = true;
			c.width = 400.0;
		});
		assert_eq!(
			bar_pixel(&look, Item::WeekBar, 10.0, 95.0, false, false),
			[0xE5, 0x48, 0x4D, 255]
		);
		assert!(near(
			bar_pixel(&look, Item::FiveBar, 10.0, 95.0, false, false),
			[179, 179, 179, 179]
		));
	}

	#[test]
	fn names_replace_letters_and_widen_the_label() {
		let letters = Metrics::new(&font(), &WidgetConfig::default(), 11.0).label;
		let cfg = WidgetConfig { labels: Labels::Name, ..WidgetConfig::default() };
		let names = Metrics::new(&font(), &cfg, 11.0).label;
		assert!(names > letters * 3.0, "{letters} -> {names}");
		assert!(min_width(&font(), &cfg) > min_width(&font(), &WidgetConfig::default()));
	}

	#[test]
	fn inline_puts_every_agent_on_one_row() {
		let look = look_with(|c| {
			c.layout = Layout::Inline;
			c.width = 420.0;
		});
		let pm = draw_with(&sample(), 1.0, false, &look);
		assert_eq!((pm.width(), pm.height()), (420, 18));
		let p = plan_for(&look.config);
		let second = (p.origin(1) * 1.0) as u32;
		assert!(p.origin(1) > p.origin(0) + p.seg_w);
		assert!(ink_in(&pm, second, pm.width()), "second agent drawn");
		// The dot sits in the gap between the two.
		let dot = (p.origin(1) - p.sep_w) as u32;
		assert!(ink_in(&pm, dot + 5, second - 4));
		// One agent alone has no dot.
		let alone = draw_with(&sample()[..1], 1.0, false, &look);
		assert!(!ink_in(&alone, (p.origin(0) + p.seg_w) as u32 + 2, alone.width()));
	}

	#[test]
	fn font_is_clamped_so_the_lines_fit_the_taskbar() {
		let cfg = WidgetConfig::default();
		assert_eq!(fitted_px(&cfg, 48.0), 11.0);
		assert_eq!(fitted_px(&cfg, NO_LIMIT), 11.0);
		let px = fitted_px(&cfg, 30.0);
		assert!(px < 11.0 && 2.0 * row_height(px) <= 30.0 + 0.01, "{px}");
		let pm = render(&sample(), &font(), 1.0, false, &Look::default(), 30.0);
		assert!(pm.height() <= 30, "{}", pm.height());
		// Bigger text means taller rows, within the room.
		let big = look_with(|c| c.font_px = 16.0);
		let pm = render(&sample(), &font(), 1.0, false, &big, NO_LIMIT);
		assert_eq!(pm.height(), 2 * (16.0_f32 * 18.0 / 11.0).round() as u32);
		let pm = render(&sample(), &font(), 1.0, false, &big, 48.0);
		assert!(pm.height() <= 48);
	}

	#[test]
	fn width_never_goes_under_the_label_and_percentage() {
		let mut cfg = WidgetConfig::default();
		let min = min_width(&font(), &cfg);
		assert!(min > 2.0 * MARGIN);
		cfg.width = 10.0;
		assert_eq!(effective_width(&font(), &cfg), min);
		assert_eq!(items(&plan_for(&cfg)), [Item::Label, Item::FivePct]);
		cfg.width = 5000.0;
		assert_eq!(effective_width(&font(), &cfg), MAX_WIDTH);
		let pm = draw_with(&sample(), 1.0, false, &Look { config: cfg, ..Look::default() });
		assert_eq!(pm.width(), 600);
	}

	#[test]
	fn background_is_the_opposite_of_the_text_at_that_opacity() {
		let look = look_with(|c| c.background_opacity = 0.5);
		let at = |light| {
			let pm = draw_with(&[], 1.0, light, &look);
			let p = pm.pixel(pm.width() - 3, pm.height() / 2).unwrap();
			let corner = pm.pixel(0, 0).unwrap().alpha();
			([p.red(), p.green(), p.blue(), p.alpha()], corner)
		};
		// Light text on a dark pad, and dark text on a light one.
		let (dark, corner) = at(false);
		assert!(near(dark, [0, 0, 0, 128]), "{dark:?}");
		assert!(corner < 128, "the corner is rounded");
		assert!(near(at(true).0, [128, 128, 128, 128]));
	}

	#[test]
	fn the_widest_row_is_neither_clipped_nor_overlapping_at_any_scale() {
		let cfg = all_on();
		let look = Look { config: WidgetConfig { width: 420.0, ..cfg.clone() }, ..Look::default() };
		let texts = [
			(Item::Label, "C"),
			(Item::FivePct, "100%"),
			(Item::FiveReset, "23h 59m"),
			(Item::WeekPct, "w 100%"),
			(Item::WeekReset, "6d 23h"),
		];
		for scale in [1.0_f32, 1.25, 1.5, 2.0] {
			let pm = draw_with(&[widest(), widest()], scale, false, &look);
			let p = plan_for(&look.config);
			let col = |x: f32| ((MARGIN + x) * scale).round() as u32;
			// Nothing touches the last columns: the right edge is not clipped.
			assert!(!ink_in(&pm, pm.width() - 2, pm.width()), "ink at the edge, scale {scale}");
			let f = font();
			for pair in p.placed.windows(2) {
				let Some((_, text)) = texts.iter().find(|(i, _)| *i == pair[0].item) else {
					continue;
				};
				let end = col(pair[0].x) + advance_of(&f, text, 11.0 * scale).ceil() as u32;
				assert!(end < col(pair[1].x), "{text:?} reaches {end} at scale {scale}");
			}
			// The last countdown does draw, and inside the pixmap.
			let last = p.placed.last().unwrap();
			assert!(ink_in(&pm, col(last.x), col(last.x + last.w)));
		}
	}

	#[test]
	fn countdowns_are_drawn_at_70_percent_of_the_row_alpha() {
		let look = Look::default();
		let p = plan_for(&look.config);
		let reset = p.placed.iter().find(|b| b.item == Item::FiveReset).unwrap();
		let (x0, x1) = ((MARGIN + reset.x).round() as u32, (MARGIN + reset.x + reset.w) as u32);
		let top_alpha = |stale: bool| {
			let pm = draw_with(&[WidgetRow { stale, ..widest() }], 1.0, false, &look);
			(x0..x1)
				.flat_map(|x| (0..18).map(move |y| (x, y)))
				.map(|(x, y)| pm.pixel(x, y).unwrap().alpha())
				.max()
				.unwrap()
		};
		// Never above 0.7 * 255 = 178.5, and the strongest pixel of a 11 px
		// glyph is close to full coverage.
		let (fresh, stale) = (top_alpha(false), top_alpha(true));
		assert!((140..=179).contains(&fresh), "{fresh}");
		// Stale multiplies by 0.6.
		assert!((stale as f32 - fresh as f32 * 0.6).abs() <= 2.0, "{fresh} -> {stale}");
	}

	#[test]
	fn rows_without_a_reset_draw_no_countdown() {
		let look = Look::default();
		let p = plan_for(&look.config);
		let pm = draw(&[row(Agent::Claude, Some(62.0), Some(31.0))], 1.0, false);
		for b in p.placed.iter().filter(|b| matches!(b.item, Item::FiveReset | Item::WeekReset)) {
			let x = (MARGIN + b.x).round() as u32;
			assert!(!ink_in(&pm, x, x + b.w as u32), "{:?}", b.item);
		}
	}

	#[test]
	fn more_rows_than_lines_are_truncated() {
		let mut rows = sample();
		rows.push(row(Agent::Claude, Some(95.0), Some(95.0)));
		let a = draw(&rows, 1.0, false);
		let b = draw(&rows[..2], 1.0, false);
		assert_eq!((a.width(), a.height()), (244, 36));
		assert_eq!(a.data(), b.data());
	}

	#[test]
	fn theme_picks_the_text_colour() {
		assert!(light_taskbar(Theme::Light, false));
		assert!(!light_taskbar(Theme::Dark, true));
		assert!(light_taskbar(Theme::Auto, true));
		assert!(!light_taskbar(Theme::Auto, false));
	}

	fn max_ink(pm: &Pixmap) -> [u8; 4] {
		let p = pm.pixels().iter().max_by_key(|p| p.alpha()).unwrap();
		[p.red(), p.green(), p.blue(), p.alpha()]
	}

	#[test]
	fn icon_is_square_with_the_number_in_its_band_colour() {
		let t = Thresholds::default();
		for pct in [Some(5.0), Some(62.0), Some(100.0), None] {
			let pm = render_icon(pct, &font(), 32, false, t);
			assert_eq!((pm.width(), pm.height()), (32, 32));
			assert!(pm.pixels().iter().any(|p| p.alpha() > 0), "{pct:?} drew nothing");
		}
		assert_eq!(
			max_ink(&render_icon(Some(95.0), &font(), 32, false, t)),
			[0xE5, 0x48, 0x4D, 255]
		);
		assert_eq!(
			max_ink(&render_icon(Some(75.0), &font(), 32, false, t)),
			[0xE8, 0xA3, 0x3D, 255]
		);
		assert_eq!(max_ink(&render_icon(Some(10.0), &font(), 32, true, t)), [0, 0, 0, 255]);
		// The user's thresholds apply to the icon too.
		let low = Thresholds { warn: 5.0, critical: 20.0 };
		assert_eq!(
			max_ink(&render_icon(Some(10.0), &font(), 32, false, low)),
			[0xE8, 0xA3, 0x3D, 255]
		);
	}

	#[test]
	fn icon_text_stays_inside_the_square() {
		// Three digits are shrunk to fit: the outer columns stay empty.
		let pm = render_icon(Some(100.0), &font(), 32, false, Thresholds::default());
		for y in 0..32 {
			assert_eq!(pm.pixel(0, y).unwrap().alpha(), 0);
			assert_eq!(pm.pixel(31, y).unwrap().alpha(), 0);
		}
	}

	#[test]
	#[ignore = "writes preview PNGs for visual review"]
	fn write_previews() {
		let dir = concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/../../../.superpowers/sdd/2026-10-19-usage-monitor"
		);
		let mut stale = sample();
		stale[1].stale = true;
		for (scale, light) in [(1.0, false), (1.0, true), (1.5, false), (1.5, true)] {
			for (name, rows) in [("", sample()), ("-stale", stale.clone())] {
				let pm = draw(&rows, scale, light);
				let mut bg = Pixmap::new(pm.width(), pm.height()).unwrap();
				bg.fill(if light {
					Color::from_rgba8(243, 243, 243, 255)
				} else {
					Color::from_rgba8(32, 32, 32, 255)
				});
				bg.draw_pixmap(
					0,
					0,
					pm.as_ref(),
					&tiny_skia::PixmapPaint::default(),
					Transform::identity(),
					None,
				);
				let theme = if light { "light" } else { "dark" };
				bg.save_png(format!("{dir}/task-9-widget-{scale}x-{theme}{name}.png")).unwrap();
			}
		}
	}
}
