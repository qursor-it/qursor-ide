//! What the user chose for the taskbar widget, persisted to
//! `<data_dir>/taskbar_widget.json`. Mirrors `@factorai/types`
//! `TaskbarWidgetConfig`.
//!
//! **A file, not settings rows.** The widget is a nested structure (an ordered
//! list of agents, a set of switches, colours) and the `settings` table is
//! scalar strings; the file is read and written by Rust alone, like `usage.json`.
//! Every field defaults, so a missing file, a missing field or a file from an
//! older build is today's widget.

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::agents::{CLAUDE, CODEX};
use crate::services::usage::Thresholds;

pub const DEFAULT_WARN_COLOR: &str = "#E8A33D";
pub const DEFAULT_CRITICAL_COLOR: &str = "#E5484D";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
	Claude,
	Codex,
}

impl Agent {
	/// The id `ProviderUsage.agent` carries.
	pub fn id(self) -> &'static str {
		match self {
			Agent::Claude => CLAUDE,
			Agent::Codex => CODEX,
		}
	}

	pub fn letter(self) -> char {
		match self {
			Agent::Claude => 'C',
			Agent::Codex => 'X',
		}
	}

	pub fn name(self) -> &'static str {
		match self {
			Agent::Claude => "Claude",
			Agent::Codex => "Codex",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Show {
	pub five_hour_bar: bool,
	pub five_hour_percent: bool,
	pub five_hour_reset: bool,
	pub weekly_bar: bool,
	pub weekly_percent: bool,
	pub weekly_reset: bool,
}

impl Default for Show {
	fn default() -> Self {
		Self {
			five_hour_bar: true,
			five_hour_percent: true,
			five_hour_reset: true,
			weekly_bar: false,
			weekly_percent: true,
			weekly_reset: true,
		}
	}
}

/// One agent on the widget. Order in the list is draw order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSlot {
	pub agent: Agent,
	#[serde(default = "yes")]
	pub visible: bool,
	/// `None` is the agent's default profile; an id that no longer exists
	/// falls back to it too.
	#[serde(default)]
	pub profile_id: Option<String>,
}

fn yes() -> bool {
	true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
	/// One row per agent.
	#[default]
	Rows,
	/// Every agent on one row, separated by a dot.
	Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Labels {
	/// `C` / `X`.
	#[default]
	Letter,
	/// `Claude` / `Codex`.
	Name,
}

impl Labels {
	pub fn text(self, agent: Agent) -> String {
		match self {
			Labels::Letter => agent.letter().to_string(),
			Labels::Name => agent.name().to_string(),
		}
	}
}

/// Which taskbar the text is drawn for; `auto` reads the system theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
	#[default]
	Auto,
	/// A light taskbar: dark text.
	Light,
	/// A dark taskbar: light text.
	Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Colors {
	/// `None` is the text colour at 70%.
	pub normal: Option<String>,
	pub warn: String,
	pub critical: String,
}

impl Default for Colors {
	fn default() -> Self {
		Self {
			normal: None,
			warn: DEFAULT_WARN_COLOR.into(),
			critical: DEFAULT_CRITICAL_COLOR.into(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Click {
	/// Bring the main window to the front.
	#[default]
	Focus,
	/// Focus, and open the status bar's usage details.
	Details,
	None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WidgetConfig {
	/// Logical px from the widget's right edge to the notification area.
	pub gap: f32,
	/// Logical px.
	pub width: f32,
	pub show: Show,
	pub agents: Vec<AgentSlot>,
	pub layout: Layout,
	pub labels: Labels,
	pub font_px: f32,
	pub theme: Theme,
	/// 0 is no background.
	pub background_opacity: f32,
	pub colors: Colors,
	pub click: Click,
}

impl Default for WidgetConfig {
	fn default() -> Self {
		let slot = |agent| AgentSlot { agent, visible: true, profile_id: None };
		Self {
			gap: 8.0,
			width: 244.0,
			show: Show::default(),
			agents: vec![slot(Agent::Claude), slot(Agent::Codex)],
			layout: Layout::default(),
			labels: Labels::default(),
			font_px: 11.0,
			theme: Theme::default(),
			background_opacity: 0.0,
			colors: Colors::default(),
			click: Click::default(),
		}
	}
}

pub const MIN_FONT_PX: f32 = 8.0;
pub const MAX_FONT_PX: f32 = 20.0;

impl WidgetConfig {
	/// The config with every value brought into its range: the file is edited
	/// by hand and the command takes whatever the renderer sends. A bad colour
	/// is the default one.
	pub fn sanitized(mut self) -> Self {
		let d = Self::default();
		let num = |v: f32, default: f32| if v.is_finite() { v } else { default };
		self.gap = num(self.gap, d.gap).max(0.0);
		self.width = num(self.width, d.width).max(1.0);
		self.font_px = num(self.font_px, d.font_px).clamp(MIN_FONT_PX, MAX_FONT_PX);
		self.background_opacity = num(self.background_opacity, 0.0).clamp(0.0, 1.0);
		if self.colors.normal.as_deref().is_some_and(|c| parse_hex(c).is_none()) {
			self.colors.normal = None;
		}
		if parse_hex(&self.colors.warn).is_none() {
			self.colors.warn = DEFAULT_WARN_COLOR.into();
		}
		if parse_hex(&self.colors.critical).is_none() {
			self.colors.critical = DEFAULT_CRITICAL_COLOR.into();
		}
		self
	}

	/// The agents on show, in draw order.
	pub fn visible_agents(&self) -> impl Iterator<Item = &AgentSlot> {
		self.agents.iter().filter(|s| s.visible)
	}
}

/// `#RRGGBB`.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
	let h = s.strip_prefix('#')?;
	if h.len() != 6 || !h.is_ascii() {
		return None;
	}
	let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
	Some([b(0)?, b(2)?, b(4)?])
}

/// What the widget thread draws from besides the numbers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
	pub config: WidgetConfig,
	pub thresholds: Thresholds,
}

/// The config and the file behind it. Held by `TaskbarWidget`, outside the
/// widget handle, so it outlives the widget being switched off. The lock is
/// only ever held for a clone or a small file write.
pub struct ConfigStore {
	path: PathBuf,
	config: Mutex<WidgetConfig>,
}

impl ConfigStore {
	/// A missing or unreadable file is the defaults.
	pub fn load(path: PathBuf) -> Self {
		let config = std::fs::read(&path)
			.ok()
			.and_then(|b| serde_json::from_slice::<WidgetConfig>(&b).ok())
			.unwrap_or_default()
			.sanitized();
		Self { path, config: Mutex::new(config) }
	}

	pub fn get(&self) -> WidgetConfig {
		self.config.lock().clone()
	}

	/// Keep and persist `config`, sanitised; returns what was kept.
	pub fn set(&self, config: WidgetConfig) -> WidgetConfig {
		let config = config.sanitized();
		let mut slot = self.config.lock();
		*slot = config.clone();
		self.write(&config);
		config
	}

	/// A drag or a resize moved the widget: keep only the geometry, so a
	/// setting changed meanwhile is not undone.
	pub fn set_geometry(&self, gap: f32, width: f32) {
		let mut slot = self.config.lock();
		slot.gap = gap;
		slot.width = width;
		let config = slot.clone().sanitized();
		*slot = config.clone();
		self.write(&config);
	}

	/// Temp file then rename, so a crash mid-write leaves the old file whole.
	fn write(&self, config: &WidgetConfig) {
		let tmp = self.path.with_extension("json.tmp");
		let result = serde_json::to_vec(config)
			.map_err(std::io::Error::other)
			.and_then(|b| std::fs::write(&tmp, b))
			.and_then(|_| std::fs::rename(&tmp, &self.path));
		if let Err(e) = result {
			warn!(error = %e, "could not persist the taskbar widget config");
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn missing_fields_are_the_defaults() {
		let c: WidgetConfig = serde_json::from_str("{}").unwrap();
		assert_eq!(c, WidgetConfig::default());
		let c: WidgetConfig = serde_json::from_str(
			r##"{"width":300,"show":{"weeklyBar":true},"colors":{"warn":"#112233"}}"##,
		)
		.unwrap();
		assert_eq!(c.width, 300.0);
		assert!(c.show.weekly_bar && c.show.five_hour_bar);
		assert_eq!(c.colors.warn, "#112233");
		assert_eq!(c.colors.critical, DEFAULT_CRITICAL_COLOR);
		assert_eq!(c.gap, 8.0);
	}

	#[test]
	fn camel_case_round_trip() {
		let json = serde_json::to_value(WidgetConfig::default()).unwrap();
		assert_eq!(json["fontPx"], 11.0);
		assert_eq!(json["show"]["fiveHourBar"], true);
		assert_eq!(json["agents"][1]["agent"], "codex");
		assert_eq!(json["agents"][0]["profileId"], serde_json::Value::Null);
		assert_eq!(json["click"], "focus");
		let back: WidgetConfig = serde_json::from_value(json).unwrap();
		assert_eq!(back, WidgetConfig::default());
	}

	#[test]
	fn sanitized_clamps_and_replaces_bad_values() {
		let mut c = WidgetConfig {
			gap: -4.0,
			width: f32::NAN,
			font_px: 99.0,
			background_opacity: 3.0,
			..WidgetConfig::default()
		};
		c.colors =
			Colors { normal: Some("red".into()), warn: "#12".into(), critical: "#00ff00".into() };
		let c = c.sanitized();
		assert_eq!(
			(c.gap, c.width, c.font_px, c.background_opacity),
			(0.0, 244.0, MAX_FONT_PX, 1.0)
		);
		assert_eq!(c.colors.normal, None);
		assert_eq!(c.colors.warn, DEFAULT_WARN_COLOR);
		assert_eq!(c.colors.critical, "#00ff00");
	}

	#[test]
	fn hex_colours() {
		assert_eq!(parse_hex("#E8A33D"), Some([0xE8, 0xA3, 0x3D]));
		assert_eq!(parse_hex("E8A33D"), None);
		assert_eq!(parse_hex("#E8A3"), None);
		assert_eq!(parse_hex("#GGGGGG"), None);
	}

	#[test]
	fn store_persists_and_geometry_keeps_the_rest() {
		let dir = tempfile::TempDir::new().unwrap();
		let path = dir.path().join("taskbar_widget.json");
		let store = ConfigStore::load(path.clone());
		assert_eq!(store.get(), WidgetConfig::default());
		store.set(WidgetConfig { font_px: 14.0, ..WidgetConfig::default() });
		store.set_geometry(30.0, 300.0);
		let again = ConfigStore::load(path).get();
		assert_eq!((again.gap, again.width, again.font_px), (30.0, 300.0, 14.0));
	}
}
