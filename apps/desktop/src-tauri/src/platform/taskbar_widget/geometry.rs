//! The arithmetic of dragging and resizing the widget, apart from Win32 so it
//! is testable anywhere. Everything is in logical px, and a position is the
//! `gap` (from the widget's right edge to the notification area), which is what
//! survives a change of taskbar width or scale.

/// How close to the left or right edge a press must be to resize, logical px.
pub const EDGE_PX: f32 = 6.0;
/// The widest the widget can be made, logical px.
pub const MAX_WIDTH: f32 = 600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
	Move,
	ResizeLeft,
	ResizeRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
	pub gap: f32,
	pub width: f32,
}

/// What a drag may do: the width range, and the room between the taskbar's
/// left edge and the notification area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
	pub min_width: f32,
	pub max_width: f32,
	pub avail: f32,
}

impl Bounds {
	/// `max_width` is the smaller of `MAX_WIDTH` and the room, and never under `min_width`.
	pub fn new(min_width: f32, avail: f32) -> Self {
		let max_width = MAX_WIDTH.min(avail).max(min_width);
		Self { min_width, max_width, avail }
	}
}

/// Which gesture a press at client `x` starts, in device px.
pub fn mode_at(x: i32, width_px: i32, edge_px: i32) -> Mode {
	if x < edge_px {
		Mode::ResizeLeft
	} else if x >= width_px - edge_px {
		Mode::ResizeRight
	} else {
		Mode::Move
	}
}

/// A gesture has begun once the cursor is `threshold` px away on either axis
/// (the system's drag size), so a click stays a click.
pub fn passed_threshold(dx: i32, dy: i32, threshold: i32) -> bool {
	dx.abs() >= threshold || dy.abs() >= threshold
}

/// The widget after the cursor moved `dx` logical px to the right of where the
/// press landed. A widget that cannot fit is held at `min_width` flush against
/// the notification area.
pub fn apply(mode: Mode, start: Geo, dx: f32, b: Bounds) -> Geo {
	let room = |gap: f32| (b.avail - gap).max(b.min_width);
	match mode {
		// The right edge follows the cursor: less gap when it moves right.
		Mode::Move => {
			let width = start.width.clamp(b.min_width, b.max_width);
			Geo { gap: (start.gap - dx).clamp(0.0, (b.avail - width).max(0.0)), width }
		}
		// The right edge stays; the left edge follows the cursor.
		Mode::ResizeLeft => {
			let width = (start.width - dx).clamp(b.min_width, b.max_width.min(room(start.gap)));
			Geo { gap: start.gap, width }
		}
		// The left edge stays, so the gap takes up what the width gains.
		Mode::ResizeRight => {
			let hi = b.max_width.min(start.width + start.gap).max(b.min_width);
			let width = (start.width + dx).clamp(b.min_width, hi);
			Geo { gap: (start.gap - (width - start.width)).max(0.0), width }
		}
	}
}

/// Whole logical px: a stored position that is not a fraction.
pub fn snap(g: Geo) -> Geo {
	Geo { gap: g.gap.round(), width: g.width.round() }
}

/// Logical px from device px at `dpi`.
pub fn to_logical(px: i32, dpi: u32) -> f32 {
	px as f32 * 96.0 / dpi.max(1) as f32
}

#[cfg(test)]
mod tests {
	use super::*;

	const B: Bounds = Bounds { min_width: 40.0, max_width: 600.0, avail: 900.0 };
	const START: Geo = Geo { gap: 8.0, width: 244.0 };

	#[test]
	fn edges_resize_and_the_middle_moves() {
		assert_eq!(mode_at(0, 300, 6), Mode::ResizeLeft);
		assert_eq!(mode_at(5, 300, 6), Mode::ResizeLeft);
		assert_eq!(mode_at(6, 300, 6), Mode::Move);
		assert_eq!(mode_at(293, 300, 6), Mode::Move);
		assert_eq!(mode_at(294, 300, 6), Mode::ResizeRight);
	}

	#[test]
	fn a_click_is_not_a_drag() {
		assert!(!passed_threshold(3, -3, 4));
		assert!(passed_threshold(-4, 0, 4));
		assert!(passed_threshold(0, 5, 4));
	}

	#[test]
	fn move_follows_the_cursor_and_stays_on_the_taskbar() {
		assert_eq!(apply(Mode::Move, START, 10.0, B).gap, 0.0);
		assert_eq!(apply(Mode::Move, START, -50.0, B).gap, 58.0);
		// Clamped at the notification area and at the taskbar's left edge.
		assert_eq!(apply(Mode::Move, START, 500.0, B).gap, 0.0);
		assert_eq!(apply(Mode::Move, START, -5000.0, B).gap, 900.0 - 244.0);
		assert_eq!(apply(Mode::Move, START, -50.0, B).width, 244.0);
	}

	#[test]
	fn right_edge_resize_keeps_the_left_edge_still() {
		let start = Geo { gap: 100.0, width: 244.0 };
		let g = apply(Mode::ResizeRight, start, 30.0, B);
		assert_eq!((g.gap, g.width), (70.0, 274.0));
		// Left edge = avail - gap - width stays where it was.
		assert_eq!(B.avail - g.gap - g.width, B.avail - start.gap - start.width);
		// Cannot grow past the notification area.
		let g = apply(Mode::ResizeRight, start, 400.0, B);
		assert_eq!((g.gap, g.width), (0.0, 344.0));
	}

	#[test]
	fn left_edge_resize_keeps_the_right_edge_still() {
		let g = apply(Mode::ResizeLeft, START, -56.0, B);
		assert_eq!((g.gap, g.width), (8.0, 300.0));
		let g = apply(Mode::ResizeLeft, START, 1000.0, B);
		assert_eq!(g.width, 40.0);
		let g = apply(Mode::ResizeLeft, START, -5000.0, B);
		assert_eq!(g.width, 600.0);
		// Never past the taskbar's left edge.
		let tight = Bounds { avail: 300.0, ..B };
		assert_eq!(apply(Mode::ResizeLeft, START, -5000.0, tight).width, 292.0);
	}

	#[test]
	fn resize_respects_the_width_limits() {
		assert_eq!(apply(Mode::ResizeRight, START, -1000.0, B).width, 40.0);
		let wide = Geo { gap: 400.0, width: 244.0 };
		assert_eq!(apply(Mode::ResizeRight, wide, 1000.0, B).width, 600.0);
	}

	#[test]
	fn bounds_never_invert() {
		assert_eq!(Bounds::new(60.0, 1920.0).max_width, 600.0);
		assert_eq!(Bounds::new(60.0, 400.0).max_width, 400.0);
		assert_eq!(Bounds::new(60.0, 20.0).max_width, 60.0);
		// A taskbar too narrow for the widget holds it flush at its minimum.
		let g = apply(Mode::Move, START, -10.0, Bounds::new(60.0, 20.0));
		assert_eq!((g.gap, g.width), (0.0, 60.0));
	}

	#[test]
	fn snapping_and_scaling() {
		let g = snap(Geo { gap: 7.6, width: 243.4 });
		assert_eq!((g.gap, g.width), (8.0, 243.0));
		assert_eq!(to_logical(12, 144), 8.0);
		assert_eq!(to_logical(12, 0), 12.0 * 96.0);
	}
}
