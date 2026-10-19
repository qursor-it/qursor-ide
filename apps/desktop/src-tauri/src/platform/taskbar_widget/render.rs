//! Pure rendering of the taskbar usage widget: row selection and a premultiplied
//! RGBA pixmap. No Win32 here; the native window pushes this pixmap to the
//! screen. Text baselines and bar rects are snapped to whole device pixels so
//! the widget stays crisp at 100/125/150% scaling.

use fontdue::{Font, FontSettings};
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

use crate::models::ProviderUsage;
use crate::services::usage::{CRITICAL, STALE_MS, WARN};

const WIDTH: f32 = 150.0;
const ROW_H: f32 = 18.0;
const ROWS: usize = 2;
const LETTER_X: f32 = 4.0;
const BAR_X: f32 = 18.0;
const BAR_W: f32 = 52.0;
const BAR_H: f32 = 6.0;
const PCT_X: f32 = 76.0;
const WEEK_X: f32 = 106.0;
const FONT_PX: f32 = 11.0;
const STALE_ALPHA: f32 = 0.6;
const MAX_SCALE: f32 = 4.0;

/// One line of the widget.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetRow {
	pub letter: char,
	pub five_hour: Option<f64>,
	pub weekly: Option<f64>,
	pub stale: bool,
}

/// Default profile per agent, Claude then Codex. An agent without data is absent.
pub fn rows_from(usage: &[ProviderUsage], now_ms: i64) -> Vec<WidgetRow> {
	[("claude", 'C'), ("codex", 'X')]
		.iter()
		.filter_map(|(agent, letter)| {
			let u = usage.iter().find(|u| u.agent == *agent && u.is_default)?;
			Some(WidgetRow {
				letter: *letter,
				five_hour: u.five_hour.as_ref().map(|w| w.used_percent),
				weekly: u.weekly.as_ref().map(|w| w.used_percent),
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
		Font::from_bytes(bytes, FontSettings::default()).ok()
	}
	#[cfg(not(windows))]
	{
		None
	}
}

/// Draw the rows into a transparent `150x36` (logical) pixmap.
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
	}
	pm
}

/// NaN reads as 0; everything else clamps to 0..=100.
fn sane(p: f64) -> f64 {
	if p.is_nan() {
		0.0
	} else {
		p.clamp(0.0, 100.0)
	}
}

fn pct_label(v: Option<f64>, prefix: &str) -> String {
	match v {
		Some(p) => format!("{prefix}{}%", sane(p).round() as u32),
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
	let color = if p >= CRITICAL {
		rgba([0xE5, 0x48, 0x4D], alpha)
	} else if p >= WARN {
		rgba([0xE8, 0xA3, 0x3D], alpha)
	} else {
		rgba(text, 0.7 * alpha)
	};
	fill_rect(pm, x, y, fw, bh, color);
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
	use super::*;
	use crate::models::{UsageSource, UsageWindow};

	fn font() -> Font {
		let p =
			concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fonts/LiberationSans-Regular.ttf");
		Font::from_bytes(std::fs::read(p).unwrap(), FontSettings::default()).unwrap()
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
			WidgetRow { letter: 'C', five_hour: Some(62.0), weekly: Some(31.0), stale: false },
			WidgetRow { letter: 'X', five_hour: Some(92.0), weekly: Some(16.0), stale: false },
		]
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
		assert_eq!((pm.width(), pm.height()), (225, 54));
		let pm = render(&sample(), &font(), 1.0, false);
		assert_eq!((pm.width(), pm.height()), (150, 36));
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
		let row = WidgetRow { letter: 'C', five_hour: Some(pct), weekly: None, stale };
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
		rows.push(WidgetRow {
			letter: 'Z',
			five_hour: Some(95.0),
			weekly: Some(95.0),
			stale: false,
		});
		let a = render(&rows, &font(), 1.0, false);
		let b = render(&rows[..2], &font(), 1.0, false);
		assert_eq!((a.width(), a.height()), (150, 36));
		assert_eq!(a.data(), b.data());
	}

	#[test]
	fn out_of_range_and_nan_inputs_clamp_without_panicking() {
		for v in [f64::NAN, -5.0, 150.0, f64::INFINITY] {
			let row = WidgetRow { letter: 'C', five_hour: Some(v), weekly: Some(v), stale: false };
			let pm = render(&[row], &font(), 1.0, false);
			assert_eq!(pm.width(), 150);
		}
		assert_eq!(bar_px(150.0, false, false), [0xE5, 0x48, 0x4D, 255]);
		// -5 and NaN fill nothing: the pixel is the track (25% alpha).
		assert!(near(bar_px(-5.0, false, false), [64, 64, 64, 64]));
		assert!(near(bar_px(f64::NAN, false, false), [64, 64, 64, 64]));
		let huge = render(&sample(), &font(), 1000.0, false);
		assert_eq!(huge.width(), 600);
	}

	#[test]
	fn text_draws_some_pixels() {
		let pm = render(&sample(), &font(), 1.0, false);
		let ink = (76..100).any(|x| (0..18).any(|y| pm.pixel(x, y).unwrap().alpha() > 0));
		assert!(ink);
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
