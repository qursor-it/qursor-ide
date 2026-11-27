//! The part of the usage monitor the terminal manager needs: the status-line
//! helper a Claude session runs, and the thresholds setting it is stored beside.

pub mod child_guard;
pub mod statusline;

/// Bar colour thresholds, percent used: amber from `warn`, red from `critical`.
/// The user's two settings; the defaults are what the colours were before they
/// could be changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
	pub warn: f64,
	pub critical: f64,
}

impl Default for Thresholds {
	fn default() -> Self {
		Self { warn: 70.0, critical: 90.0 }
	}
}

impl Thresholds {
	/// From the two persisted strings. They are text in SQLite, so anything can be
	/// there: a value that is not a whole number in 1..=100, or a warn that does
	/// not sit below the critical, reads as the defaults.
	pub fn parse(warn: Option<&str>, critical: Option<&str>) -> Self {
		let d = Self::default();
		let one = |v: Option<&str>, default: f64| {
			v.and_then(|s| s.trim().parse::<u32>().ok())
				.filter(|n| (1..=100).contains(n))
				.map_or(default, f64::from)
		};
		let t = Self { warn: one(warn, d.warn), critical: one(critical, d.critical) };
		if t.warn < t.critical {
			t
		} else {
			d
		}
	}
}
/// Wall-clock milliseconds.
pub fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn thresholds_default_when_unset_invalid_or_inverted() {
		let d = Thresholds::default();
		let t = |warn, critical| Thresholds { warn, critical };
		assert_eq!(Thresholds::parse(None, None), d);
		assert_eq!(Thresholds::parse(Some("50"), Some("80")), t(50.0, 80.0));
		assert_eq!(Thresholds::parse(Some("80"), Some("80")), d);
		assert_eq!(Thresholds::parse(Some("95"), Some("90")), d);
		assert_eq!(Thresholds::parse(Some("abc"), Some("95")), t(70.0, 95.0));
		assert_eq!(Thresholds::parse(Some("0"), Some("101")), d);
		assert_eq!(Thresholds::parse(Some("-5"), Some("50")), d);
	}
}
