//! Pure rendering of the taskbar usage widget: row selection and a premultiplied
//! RGBA pixmap. No Win32 here; the native window pushes this pixmap to the
//! screen. Text baselines and bar rects are snapped to whole device pixels so
//! the widget stays crisp at 100/125/150% scaling.

use fontdue::Font;
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

use crate::models::{ProviderUsage, UsageWindow};
use crate::services::usage::{CRITICAL, STALE_MS, WARN};

/// Logical width: the worst case row, `100%  23h 59m  w 100%  6d 23h`, with
/// about 6 px between columns and `LETTER_X` of margin at the right edge.
const WIDTH: f32 = 244.0;
const ROW_H: f32 = 18.0;
const ROWS: usize = 2;
const LETTER_X: f32 = 4.0;
const BAR_X: f32 = 18.0;
const BAR_W: f32 = 52.0;
const BAR_H: f32 = 6.0;
const PCT_X: f32 = 76.0;
const RESET_5H_X: f32 = 111.0;
const WEEK_X: f32 = 160.0;
const RESET_WK_X: f32 = 206.0;
const FONT_PX: f32 = 11.0;
const STALE_ALPHA: f32 = 0.6;
/// Countdowns sit back from the percentages: the same colour at this share of
/// the row's alpha.
const COUNTDOWN_ALPHA: f32 = 0.7;
const MAX_SCALE: f32 = 4.0;

/// One line of the widget.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetRow {
	pub letter: char,
	pub five_hour: Option<f64>,
	pub weekly: Option<f64>,
	/// Whole minutes until each window resets; `None` without a future reset.
	pub five_hour_reset_min: Option<i64>,
	pub weekly_reset_min: Option<i64>,
	pub stale: bool,
}

/// Default profile per agent, Claude then Codex. An agent without data is absent.
pub fn rows_from(usage: &[ProviderUsage], now_ms: i64) -> Vec<WidgetRow> {
	[("claude", 'C'), ("codex", 'X')]
		.iter()
		.filter_map(|(agent, letter)| {
			let u = usage.iter().find(|u| u.agent == *agent && u.is_default)?;
			let reset_min = |w: &Option<UsageWindow>| {
				let left = w.as_ref()?.resets_at? - now_ms;
				(left > 0).then_some(left / 60_000)
			};
			Some(WidgetRow {
				letter: *letter,
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

/// Draw the rows into a transparent `244x36` (logical) pixmap.
pub fn render(rows: &[WidgetRow], font: &Font, scale: f32, light_taskbar: bool) -> Pixmap {
	let scale = if scale.is_finite() { scale.clamp(0.5, MAX_SCALE) } else { 1.0 };
	let w = (WIDTH * scale).round().max(1.0) as u32;
	let h = (ROW_H * ROWS as f32 * scale).round().max(1.0) as u32;
	// Scale is clamped to 0.5..=4, so the size is non-zero and small.
	let mut pm = Pixmap::new(w, h).expect("non-zero pixmap size");
	let text: [u8; 3] = if light_taskbar { [0, 0, 0] } else { [255, 255, 255] };

	for (i, row) in rows.iter().take(ROWS).enumerate() {
		let top = (i as f32 * ROW_H * scale).round();
		let row_h = (ROW_H * scale).round();
		let alpha = if row.stale { STALE_ALPHA } else { 1.0 };
		let px = FONT_PX * scale;
		let baseline = top + (row_h / 2.0 + px * 0.36).round();
		let dim = alpha * COUNTDOWN_ALPHA;

		draw_text(
			&mut pm,
			font,
			px,
			&row.letter.to_string(),
			(LETTER_X * scale).round(),
			baseline,
			text,
			alpha,
		);
		draw_bar(&mut pm, row.five_hour, top, row_h, scale, text, alpha);
		draw_text(
			&mut pm,
			font,
			px,
			&pct_label(row.five_hour, ""),
			(PCT_X * scale).round(),
			baseline,
			text,
			alpha,
		);
		if let Some(min) = row.five_hour_reset_min {
			let x = (RESET_5H_X * scale).round();
			draw_text(&mut pm, font, px, &countdown(min), x, baseline, text, dim);
		}
		draw_text(
			&mut pm,
			font,
			px,
			&pct_label(row.weekly, "w "),
			(WEEK_X * scale).round(),
			baseline,
			text,
			alpha,
		);
		if let Some(min) = row.weekly_reset_min {
			let x = (RESET_WK_X * scale).round();
			draw_text(&mut pm, font, px, &countdown(min), x, baseline, text, dim);
		}
	}
	pm
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
/// threshold (70, 90) before the colour does — 89.9 reads "89%" in amber.
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

fn draw_bar(
	pm: &mut Pixmap,
	pct: Option<f64>,
	top: f32,
	row_h: f32,
	scale: f32,
	text: [u8; 3],
	alpha: f32,
) {
	let x = (BAR_X * scale).round();
	let bw = (BAR_W * scale).round();
	let bh = (BAR_H * scale).round().max(1.0);
	let y = top + ((row_h - bh) / 2.0).round();
	let p = pct.map(sane);
	let fw = p.map_or(0.0, |p| (bw * p as f32 / 100.0).round());
	// The track is drawn only where the fill is not, so the fill keeps its exact alpha.
	fill_rect(pm, x + fw, y, bw - fw, bh, rgba(text, 0.25 * alpha));
	let Some(p) = p else { return };
	let color = match band_rgb(p) {
		Some(c) => rgba(c, alpha),
		None => rgba(text, 0.7 * alpha),
	};
	fill_rect(pm, x, y, fw, bh, color);
}

/// Amber or red past the thresholds; `None` is the neutral band.
fn band_rgb(p: f64) -> Option<[u8; 3]> {
	if p >= CRITICAL {
		Some([0xE5, 0x48, 0x4D])
	} else if p >= WARN {
		Some([0xE8, 0xA3, 0x3D])
	} else {
		None
	}
}

/// The tray fallback's square icon: the percentage (no `%`, it would not fit)
/// in its band colour, the taskbar's text colour when neutral, shrunk until it
/// fits with a pixel to spare on each side.
pub fn render_icon(pct: Option<f64>, font: &Font, size: u32, light_taskbar: bool) -> Pixmap {
	let size = size.clamp(1, 256);
	let mut pm = Pixmap::new(size, size).expect("non-zero pixmap size");
	let s = size as f32;
	let label = pct.map_or_else(|| "--".to_string(), |p| whole_pct(p).to_string());
	let text = if light_taskbar { [0, 0, 0] } else { [255, 255, 255] };
	let color = pct.map(sane).and_then(band_rgb).unwrap_or(text);
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

	fn font() -> Font {
		let p = concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/../../../tests/fixtures/fonts/LiberationSans-Regular.ttf"
		);
		Font::from_bytes(std::fs::read(p).unwrap(), FontSettings::default()).unwrap()
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
				..row('C', Some(62.0), Some(31.0))
			},
			WidgetRow {
				five_hour_reset_min: Some(45),
				weekly_reset_min: Some(6 * 24 * 60 + 23 * 60),
				..row('X', Some(92.0), Some(16.0))
			},
		]
	}

	fn row(letter: char, five_hour: Option<f64>, weekly: Option<f64>) -> WidgetRow {
		WidgetRow {
			letter,
			five_hour,
			weekly,
			five_hour_reset_min: None,
			weekly_reset_min: None,
			stale: false,
		}
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
		let rows = rows_from(&[u.clone()], now);
		assert_eq!((rows[0].five_hour_reset_min, rows[0].weekly_reset_min), (Some(134), Some(0)));
		u.five_hour = at(0);
		u.weekly = at(-60_000);
		let rows = rows_from(&[u.clone()], now);
		assert_eq!((rows[0].five_hour_reset_min, rows[0].weekly_reset_min), (None, None));
		u.five_hour = None;
		assert_eq!(rows_from(&[u], now)[0].five_hour_reset_min, None);
	}

	#[test]
	fn rows_from_picks_default_profiles_in_order() {
		let now = 10_000_000;
		let list = vec![
			usage("codex", "cx", true, 92.0, 16.0, now),
			usage("claude", "other", false, 5.0, 5.0, now),
			usage("claude", "main", true, 62.0, 31.0, now - STALE_MS - 1),
		];
		let rows = rows_from(&list, now);
		assert_eq!(rows.len(), 2);
		assert_eq!((rows[0].letter, rows[0].five_hour, rows[0].stale), ('C', Some(62.0), true));
		assert_eq!((rows[1].letter, rows[1].weekly, rows[1].stale), ('X', Some(16.0), false));
	}

	#[test]
	fn render_has_scaled_size() {
		let pm = render(&sample(), &font(), 1.5, false);
		assert_eq!((pm.width(), pm.height()), (366, 54));
		let pm = render(&sample(), &font(), 1.0, false);
		assert_eq!((pm.width(), pm.height()), (244, 36));
	}

	#[test]
	fn empty_rows_render_transparent() {
		let pm = render(&[], &font(), 1.0, false);
		assert!(pm.data().chunks(4).all(|p| p[3] == 0));
	}

	#[test]
	fn critical_bar_contains_red_pixels() {
		let pm = render(&sample()[1..], &font(), 1.0, false);
		// The critical row is row 0 here: bar x 18..70, 6px tall centred in 18px.
		let px = pm.pixel(30, 9).unwrap();
		assert_eq!((px.red(), px.green(), px.blue(), px.alpha()), (0xE5, 0x48, 0x4D, 255));
	}

	/// Pixel in the middle of the first row's bar, at `fraction` of its width.
	fn bar_px(pct: f64, stale: bool, light: bool) -> [u8; 4] {
		let row = WidgetRow { stale, ..row('C', Some(pct), None) };
		let pm = render(&[row], &font(), 1.0, light);
		let p = pm.pixel(19, 9).unwrap();
		[p.red(), p.green(), p.blue(), p.alpha()]
	}

	fn near(a: [u8; 4], b: [u8; 4]) -> bool {
		a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 1)
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
	fn more_than_two_rows_are_truncated() {
		let mut rows = sample();
		rows.push(row('Z', Some(95.0), Some(95.0)));
		let a = render(&rows, &font(), 1.0, false);
		let b = render(&rows[..2], &font(), 1.0, false);
		assert_eq!((a.width(), a.height()), (244, 36));
		assert_eq!(a.data(), b.data());
	}

	#[test]
	fn out_of_range_and_nan_inputs_clamp_without_panicking() {
		for v in [f64::NAN, -5.0, 150.0, f64::INFINITY] {
			let row = row('C', Some(v), Some(v));
			let pm = render(&[row], &font(), 1.0, false);
			assert_eq!(pm.width(), 244);
		}
		assert_eq!(bar_px(150.0, false, false), [0xE5, 0x48, 0x4D, 255]);
		// -5 and NaN fill nothing: the pixel is the track (25% alpha).
		assert!(near(bar_px(-5.0, false, false), [64, 64, 64, 64]));
		assert!(near(bar_px(f64::NAN, false, false), [64, 64, 64, 64]));
		let huge = render(&sample(), &font(), 1000.0, false);
		assert_eq!(huge.width(), 976);
	}

	#[test]
	fn text_draws_some_pixels() {
		let pm = render(&sample(), &font(), 1.0, false);
		let ink = (76..100).any(|x| (0..18).any(|y| pm.pixel(x, y).unwrap().alpha() > 0));
		assert!(ink);
	}

	fn advance(f: &Font, text: &str, px: f32) -> f32 {
		text.chars().map(|c| f.metrics(c, px).advance_width).sum()
	}

	/// Worst case: `100%`, `23h 59m`, `w 100%`, `6d 23h`.
	fn widest() -> WidgetRow {
		WidgetRow {
			five_hour_reset_min: Some(23 * 60 + 59),
			weekly_reset_min: Some(6 * 24 * 60 + 23 * 60),
			..row('C', Some(100.0), Some(100.0))
		}
	}

	/// Does any pixel of `pm` in columns `x0..x1` carry ink?
	fn ink_in(pm: &Pixmap, x0: u32, x1: u32) -> bool {
		(x0..x1.min(pm.width()))
			.any(|x| (0..pm.height()).any(|y| pm.pixel(x, y).unwrap().alpha() > 0))
	}

	#[test]
	fn the_widest_row_fits_its_columns_with_a_gap() {
		let f = font();
		let columns = [
			(PCT_X, "100%", RESET_5H_X),
			(RESET_5H_X, "23h 59m", WEEK_X),
			(WEEK_X, "w 100%", RESET_WK_X),
			(RESET_WK_X, "6d 23h", WIDTH + 5.0 - LETTER_X),
		];
		for (x, text, next) in columns {
			let end = x + advance(&f, text, FONT_PX);
			assert!(end + 5.0 <= next, "{text:?} ends at {end}, next column at {next}");
		}
		const { assert!(BAR_X + BAR_W + 5.0 <= PCT_X) };
		assert!(LETTER_X + advance(&f, "C", FONT_PX) + 5.0 <= BAR_X);
	}

	#[test]
	fn the_widest_row_is_neither_clipped_nor_overlapping_at_any_scale() {
		for scale in [1.0_f32, 1.25, 1.5, 2.0] {
			let pm = render(&[widest(), widest()], &font(), scale, false);
			let col = |x: f32| (x * scale).round() as u32;
			// Nothing touches the last two columns: the right edge is not clipped.
			assert!(!ink_in(&pm, pm.width() - 2, pm.width()), "ink at the edge, scale {scale}");
			// Each gap between a text and the next column is clear of ink.
			let f = font();
			for (x, text, next) in [
				(PCT_X, "100%", RESET_5H_X),
				(RESET_5H_X, "23h 59m", WEEK_X),
				(WEEK_X, "w 100%", RESET_WK_X),
			] {
				let end = col(x) + advance(&f, text, FONT_PX * scale).ceil() as u32;
				assert!(end < col(next), "{text:?} reaches {end} at scale {scale}");
				assert!(!ink_in(&pm, end + 1, col(next)), "{text:?} bleeds right, scale {scale}");
			}
			// The last countdown does draw, and inside the pixmap.
			assert!(ink_in(&pm, col(RESET_WK_X), col(WIDTH - LETTER_X)));
		}
	}

	#[test]
	fn countdowns_are_drawn_at_70_percent_of_the_row_alpha() {
		let top_alpha = |stale: bool| {
			let row = WidgetRow { stale, ..widest() };
			let pm = render(&[row], &font(), 1.0, false);
			let x0 = RESET_5H_X as u32;
			let xs = x0..WEEK_X as u32;
			xs.flat_map(|x| (0..18).map(move |y| (x, y)))
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
		let pm = render(&[row('C', Some(62.0), Some(31.0))], &font(), 1.0, false);
		assert!(!ink_in(&pm, RESET_5H_X as u32, WEEK_X as u32));
		assert!(!ink_in(&pm, RESET_WK_X as u32, pm.width()));
	}

	fn max_ink(pm: &Pixmap) -> [u8; 4] {
		let p = pm.pixels().iter().max_by_key(|p| p.alpha()).unwrap();
		[p.red(), p.green(), p.blue(), p.alpha()]
	}

	#[test]
	fn icon_is_square_with_the_number_in_its_band_colour() {
		for pct in [Some(5.0), Some(62.0), Some(100.0), None] {
			let pm = render_icon(pct, &font(), 32, false);
			assert_eq!((pm.width(), pm.height()), (32, 32));
			assert!(pm.pixels().iter().any(|p| p.alpha() > 0), "{pct:?} drew nothing");
		}
		assert_eq!(max_ink(&render_icon(Some(95.0), &font(), 32, false)), [0xE5, 0x48, 0x4D, 255]);
		assert_eq!(max_ink(&render_icon(Some(75.0), &font(), 32, false)), [0xE8, 0xA3, 0x3D, 255]);
		assert_eq!(max_ink(&render_icon(Some(10.0), &font(), 32, true)), [0, 0, 0, 255]);
	}

	#[test]
	fn icon_text_stays_inside_the_square() {
		// Three digits are shrunk to fit: the outer columns stay empty.
		let pm = render_icon(Some(100.0), &font(), 32, false);
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
				let pm = render(&rows, &font(), scale, light);
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
