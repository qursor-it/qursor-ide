//! The Win32 half of the taskbar widget (ADR-0084). One named thread owns a
//! hidden listener window (timers, `TaskbarCreated`), the layered surface
//! reparented into `Shell_TrayWnd`, the WinEvent hook and the message loop.
//!
//! Cross-process `SetParent` attaches this thread's input queue to Explorer's,
//! so this thread can hang when Explorer does. Nothing else ever sends it a
//! message or waits on it: the handle queues on a channel and posts `WM_APP`
//! to the thread (`Shared::wake`), and the state below is thread-local.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use fontdue::Font;
use tiny_skia::{Pixmap, PremultipliedColorU8};
use tracing::warn;
use windows_sys::w;
use windows_sys::Win32::Foundation::{
	ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
	CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, MapWindowPoints,
	ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
	BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::{
	GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
	CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW, FindWindowW,
	GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW, GetWindowRect,
	GetWindowThreadProcessId, IsChild, IsWindowVisible, KillTimer, PeekMessageW, PostQuitMessage,
	PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW, SetParent, SetTimer,
	SetWindowLongPtrW, SetWindowPos, TranslateMessage, UpdateLayeredWindow,
	EVENT_OBJECT_LOCATIONCHANGE, GWL_EXSTYLE, GWL_STYLE, HWND_TOP, MA_NOACTIVATE, MSG,
	OBJID_WINDOW, PM_NOREMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW, ULW_ALPHA, WINEVENT_OUTOFCONTEXT,
	WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_TIMER, WM_USER, WNDCLASSEXW, WS_CHILD,
	WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOPARENTNOTIFY, WS_EX_TOOLWINDOW,
	WS_POPUP,
};

use super::tray::TrayFallback;
use super::{focus_main, render, rows_from, Host, Msg, WidgetRow};
use crate::models::ProviderUsage;
use crate::services::usage::now_ms;
use crate::services::usage::store::apply_resets;

/// Theme re-read, stale/reset ageing, and re-embed after an Explorer restart.
const TICK_TIMER: usize = 1;
const TICK_MS: u32 = 60_000;
/// Throttles taskbar location changes: one reposition per 50 ms at most.
const REPOSITION_TIMER: usize = 2;
const REPOSITION_MS: u32 = 50;
/// The check that the widget really is under the taskbar and visible.
const VERIFY_TIMER: usize = 3;
const VERIFY_MS: u32 = 2_000;
/// Logical pixels between the widget and the notification area.
const GAP: f32 = 8.0;
/// Set (to anything) to skip embedding and go straight to the tray icon: the
/// way to see the fallback on a machine where embedding works.
const FORCE_TRAY_ENV: &str = "FACTORAI_TASKBAR_WIDGET_FORCE_TRAY";

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// What the handle and the widget thread share. Atomics only: no lock.
pub(crate) struct Shared {
	thread_id: u32,
	/// A wake-up is already queued, so a burst of updates posts once.
	wake_pending: AtomicBool,
	/// The embedded surface, or 0. For the embed test.
	surface: AtomicIsize,
}

impl Shared {
	pub(crate) fn new(thread_id: u32) -> Self {
		Self { thread_id, wake_pending: AtomicBool::new(false), surface: AtomicIsize::new(0) }
	}

	/// `PostThreadMessageW` never waits for the target thread. A failed post
	/// (thread gone, queue full) clears the flag so the next call tries again.
	pub(crate) fn wake(&self) {
		if self.wake_pending.swap(true, Ordering::AcqRel) {
			return;
		}
		// SAFETY: plain value arguments; an unknown thread id just fails.
		let ok = unsafe { PostThreadMessageW(self.thread_id, WM_APP, 0, 0) };
		if ok == 0 {
			self.wake_pending.store(false, Ordering::Release);
		}
	}

	#[cfg(test)]
	fn surface(&self) -> HWND {
		self.surface.load(Ordering::Acquire) as HWND
	}
}

/// Start the `taskbar-widget` thread. Waits only for the thread to create its
/// message queue, which happens before it touches Explorer.
pub(super) fn spawn(host: Host, initial: Vec<ProviderUsage>) -> Option<(Sender<Msg>, Arc<Shared>)> {
	let (tx, rx) = mpsc::channel();
	// Rendezvous: the send completes only into a waiting `recv`, so a thread
	// whose starter already gave up knows it and never embeds.
	let (ready_tx, ready_rx) = mpsc::sync_channel(0);
	let spawned = std::thread::Builder::new().name("taskbar-widget".into()).spawn(move || {
		let mut msg = MSG::default();
		// SAFETY: plain calls on this thread. The peek creates the message
		// queue, so a post that follows the ready signal cannot be lost.
		let thread_id = unsafe {
			SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
			PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
			GetCurrentThreadId()
		};
		let shared = Arc::new(Shared::new(thread_id));
		if ready_tx.send(shared.clone()).is_err() {
			return;
		}
		run(host, rx, shared, initial);
	});
	if let Err(e) = spawned {
		warn!(error = %e, "could not start the taskbar widget thread");
		return None;
	}
	let shared = ready_rx.recv_timeout(Duration::from_secs(2)).ok()?;
	Some((tx, shared))
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
	Embedded,
	Tray,
}

struct Widget {
	host: Host,
	rx: Receiver<Msg>,
	shared: Arc<Shared>,
	font: Option<Font>,
	usage: Vec<ProviderUsage>,
	light: bool,
	mode: Mode,
	/// Hidden top-level window: owns the timers and hears `TaskbarCreated`,
	/// which a child of Explorer's (dead) taskbar would not.
	listener: HWND,
	/// The layered child, or null while Explorer is restarting.
	surface: HWND,
	taskbar: HWND,
	hook: HWINEVENTHOOK,
	/// What is on screen, so a tick that changes nothing pushes nothing.
	drawn: Option<(Vec<WidgetRow>, u32, bool)>,
	size: (i32, i32),
	tray: Option<TrayFallback>,
	/// The reposition timer is armed: a throttle, so a steady stream of
	/// location events cannot keep pushing it back.
	reposition_pending: bool,
}

thread_local! {
	static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

/// Run `f` on the widget unless it is already borrowed — that is, unless this
/// is a message sent to us synchronously from inside one of our own Win32
/// calls, which has nothing to add.
fn with_widget(f: impl FnOnce(&mut Widget)) {
	WIDGET.with(|w| {
		if let Ok(mut w) = w.try_borrow_mut() {
			if let Some(w) = w.as_mut() {
				f(w);
			}
		}
	});
}

fn run(host: Host, rx: Receiver<Msg>, shared: Arc<Shared>, initial: Vec<ProviderUsage>) {
	// Without the listener there are no timers and no `TaskbarCreated`, so no
	// embedding; the tray, which is the contract, still works.
	let listener = create_listener().unwrap_or(null_mut());
	let mut w = Widget {
		host,
		rx,
		shared,
		font: super::load_system_font(),
		usage: initial,
		light: light_theme(),
		mode: Mode::Embedded,
		listener,
		surface: null_mut(),
		taskbar: null_mut(),
		hook: null_mut(),
		drawn: None,
		size: (0, 0),
		tray: None,
		reposition_pending: false,
	};
	if listener.is_null() {
		w.fall_back("could not create the widget's window");
	} else if w.font.is_none() {
		w.fall_back("no system font");
	} else if std::env::var_os(FORCE_TRAY_ENV).is_some() {
		w.fall_back(&format!("{FORCE_TRAY_ENV} is set"));
	} else if let Err(why) = w.embed() {
		w.fall_back(why);
	}
	if !listener.is_null() {
		// SAFETY: our own window, on its own thread.
		unsafe { SetTimer(listener, TICK_TIMER, TICK_MS, None) };
	}
	WIDGET.with(|cell| *cell.borrow_mut() = Some(w));

	let mut msg = MSG::default();
	loop {
		// SAFETY: standard message loop on the thread that owns the windows.
		let got = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
		if got == 0 || got == -1 {
			break;
		}
		if msg.hwnd.is_null() && msg.message == WM_APP {
			let mut stop = false;
			with_widget(|w| stop = !w.drain());
			if stop {
				break;
			}
			continue;
		}
		// SAFETY: `msg` was filled by `GetMessageW`.
		unsafe {
			TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	}

	if let Some(mut w) = WIDGET.with(|cell| cell.borrow_mut().take()) {
		w.teardown();
	}
}

impl Widget {
	/// Apply what the handle queued. `false` on `Stop`.
	fn drain(&mut self) -> bool {
		// Cleared first: a send after this drain must post again.
		self.shared.wake_pending.store(false, Ordering::Release);
		let mut changed = false;
		while let Ok(msg) = self.rx.try_recv() {
			match msg {
				Msg::Update(usage) => {
					self.usage = usage;
					changed = true;
				}
				Msg::Stop => return false,
			}
		}
		if changed {
			self.refresh();
		}
		true
	}

	/// Create the surface and move it into the taskbar.
	fn embed(&mut self) -> Result<(), &'static str> {
		// SAFETY: every handle below is checked before use; the surface is ours.
		unsafe {
			let taskbar = FindWindowW(w!("Shell_TrayWnd"), null());
			if taskbar.is_null() {
				return Err("Shell_TrayWnd not found");
			}
			let surface = CreateWindowExW(
				WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOPARENTNOTIFY,
				CLASS,
				null(),
				WS_POPUP,
				0,
				0,
				1,
				1,
				null_mut(),
				null_mut(),
				GetModuleHandleW(null()),
				null(),
			);
			if surface.is_null() {
				return Err("could not create the widget window");
			}
			// A window becomes a child by style first, then by parent.
			let style = GetWindowLongPtrW(surface, GWL_STYLE) as u32;
			let style = (style & !WS_POPUP) | WS_CHILD | WS_CLIPSIBLINGS;
			SetWindowLongPtrW(surface, GWL_STYLE, style as isize);
			if SetParent(surface, taskbar).is_null() {
				DestroyWindow(surface);
				return Err("SetParent into the taskbar failed");
			}
			// Windows 11 can leave a freshly reparented layered window under the
			// taskbar's composition visual; toggling the layered bit rebinds it
			// (the same fix CodeZeno's monitor applies once per reparent).
			let ex = GetWindowLongPtrW(surface, GWL_EXSTYLE);
			SetWindowLongPtrW(surface, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
			SetWindowLongPtrW(surface, GWL_EXSTYLE, ex);

			self.surface = surface;
			self.taskbar = taskbar;
			self.drawn = None;
			self.shared.surface.store(surface as isize, Ordering::Release);

			let mut pid = 0;
			let tid = GetWindowThreadProcessId(taskbar, &mut pid);
			self.hook = SetWinEventHook(
				EVENT_OBJECT_LOCATIONCHANGE,
				EVENT_OBJECT_LOCATIONCHANGE,
				null_mut(),
				Some(on_location_change),
				pid,
				tid,
				WINEVENT_OUTOFCONTEXT,
			);
			SetTimer(self.listener, VERIFY_TIMER, VERIFY_MS, None);
		}
		self.refresh();
		// A layered child the system would not draw (no Windows 8+ manifest,
		// or a change in a later Windows) is on screen as an empty rectangle.
		if self.drawn.is_none() {
			self.unembed();
			return Err("UpdateLayeredWindow failed on the embedded window");
		}
		Ok(())
	}

	/// Drop the surface and its hook. Safe to call with neither.
	fn unembed(&mut self) {
		// SAFETY: our own hook, timer and window; null handles are skipped.
		unsafe {
			KillTimer(self.listener, VERIFY_TIMER);
			KillTimer(self.listener, REPOSITION_TIMER);
			if !self.hook.is_null() {
				UnhookWinEvent(self.hook);
			}
			if !self.surface.is_null() {
				DestroyWindow(self.surface);
			}
		}
		self.hook = null_mut();
		self.surface = null_mut();
		self.reposition_pending = false;
		self.drawn = None;
		self.shared.surface.store(0, Ordering::Release);
	}

	/// Embedding is not working: log once, and show a tray icon instead.
	fn fall_back(&mut self, why: &str) {
		self.unembed();
		self.mode = Mode::Tray;
		warn!(reason = why, "taskbar widget: cannot embed in the taskbar, using a tray icon");
		self.tray = self.host.app.clone().map(TrayFallback::start);
		self.refresh();
	}

	fn rows(&self) -> Vec<WidgetRow> {
		let now = now_ms();
		rows_from(&apply_resets(self.usage.clone(), now), now)
	}

	/// Re-render if anything visible changed, then place the surface.
	fn refresh(&mut self) {
		let rows = self.rows();
		match self.mode {
			Mode::Tray => {
				let key = (rows, 0, self.light);
				if self.drawn.as_ref() != Some(&key) {
					if let Some(tray) = &self.tray {
						tray.show(&key.0, self.font.as_ref(), self.light);
					}
					self.drawn = Some(key);
				}
			}
			Mode::Embedded => {
				let (Some(font), false) = (&self.font, self.surface.is_null()) else { return };
				// SAFETY: a window handle; a dead one returns 0.
				let dpi = match unsafe { GetDpiForWindow(self.taskbar) } {
					0 => 96,
					d => d,
				};
				let key = (rows, dpi, self.light);
				if self.drawn.as_ref() != Some(&key) {
					let pm = render(&key.0, font, dpi as f32 / 96.0, self.light);
					// SAFETY: our own layered window.
					if unsafe { push(self.surface, &pm) } {
						self.size = (pm.width() as i32, pm.height() as i32);
						self.drawn = Some(key);
					}
				}
				self.place(dpi);
			}
		}
	}

	/// Just left of the notification area, vertically centred, in the taskbar's
	/// client coordinates.
	fn place(&self, dpi: u32) {
		let (w, h) = self.size;
		// SAFETY: plain geometry queries; out-params are local.
		unsafe {
			let mut bar = RECT::default();
			GetClientRect(self.taskbar, &mut bar);
			let notify = FindWindowExW(self.taskbar, null_mut(), w!("TrayNotifyWnd"), null());
			let right = if notify.is_null() {
				bar.right
			} else {
				let mut r = RECT::default();
				GetWindowRect(notify, &mut r);
				let mut pts = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
				MapWindowPoints(null_mut(), self.taskbar, pts.as_mut_ptr(), 2);
				pts[0].x
			};
			let gap = (GAP * dpi as f32 / 96.0).round() as i32;
			let x = right - w - gap;
			let y = bar.top + (bar.bottom - bar.top - h) / 2;
			SetWindowPos(self.surface, HWND_TOP, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
		}
	}

	fn on_timer(&mut self, id: usize) {
		match id {
			TICK_TIMER => {
				// A wake-up lost to a race still lands within a minute.
				if !self.drain() {
					// SAFETY: ends this thread's message loop.
					unsafe { PostQuitMessage(0) };
					return;
				}
				self.light = light_theme();
				// Explorer restarted and `TaskbarCreated` never reached us.
				// SAFETY: a lookup by class name.
				let bar_is_back = unsafe { !FindWindowW(w!("Shell_TrayWnd"), null()).is_null() };
				if self.mode == Mode::Embedded && self.surface.is_null() && bar_is_back {
					if let Err(why) = self.embed() {
						self.fall_back(why);
					}
				}
				self.refresh();
			}
			REPOSITION_TIMER => {
				// SAFETY: our own timer.
				unsafe { KillTimer(self.listener, REPOSITION_TIMER) };
				self.reposition_pending = false;
				self.refresh();
			}
			VERIFY_TIMER => {
				// SAFETY: our own timer; the queries take any handle.
				let lost = unsafe {
					KillTimer(self.listener, VERIFY_TIMER);
					!self.surface.is_null()
						&& (GetParent(self.surface) != self.taskbar
							|| IsWindowVisible(self.surface) == 0)
				};
				if lost {
					self.fall_back("not visible under the taskbar 2 s after embedding");
				}
			}
			_ => {}
		}
	}

	/// Explorer is back: the old surface died with its parent.
	fn on_taskbar_created(&mut self) {
		if self.mode != Mode::Embedded {
			return;
		}
		self.unembed();
		if let Err(why) = self.embed() {
			self.fall_back(why);
		}
	}

	fn teardown(&mut self) {
		self.unembed();
		// SAFETY: our own window and timer.
		if !self.listener.is_null() {
			unsafe {
				KillTimer(self.listener, TICK_TIMER);
				DestroyWindow(self.listener);
			}
		}
		// Dropping the fallback removes the tray icon.
		self.tray = None;
	}
}

const CLASS: *const u16 = w!("FactoraiTaskbarWidget");

fn create_listener() -> Option<HWND> {
	static REGISTERED: OnceLock<bool> = OnceLock::new();
	// SAFETY: the class struct and names are static; the window is ours.
	unsafe {
		let instance = GetModuleHandleW(null());
		let ok = *REGISTERED.get_or_init(|| {
			let class = WNDCLASSEXW {
				cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
				lpfnWndProc: Some(wndproc),
				hInstance: instance,
				lpszClassName: CLASS,
				..Default::default()
			};
			RegisterClassExW(&class) != 0
		});
		if !ok {
			return None;
		}
		TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
		let hwnd = CreateWindowExW(
			WS_EX_TOOLWINDOW,
			CLASS,
			null(),
			WS_POPUP,
			0,
			0,
			0,
			0,
			null_mut(),
			null_mut(),
			instance,
			null(),
		);
		(!hwnd.is_null()).then_some(hwnd)
	}
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
	match msg {
		WM_TIMER => {
			with_widget(|w| w.on_timer(wp));
			return 0;
		}
		WM_LBUTTONUP => {
			with_widget(|w| {
				if hwnd == w.surface {
					if let Some(app) = &w.host.app {
						focus_main(app);
					}
				}
			});
			return 0;
		}
		WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
		WM_DESTROY => with_widget(|w| {
			// Explorer went away and took the surface with it.
			if hwnd == w.surface {
				w.surface = null_mut();
				w.unembed();
			}
		}),
		m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
			with_widget(|w| w.on_taskbar_created());
			return 0;
		}
		_ => {}
	}
	// SAFETY: forwarding the arguments we were given.
	unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// A move of the taskbar or anything in it: reposition at most every 50 ms.
unsafe extern "system" fn on_location_change(
	_hook: HWINEVENTHOOK,
	_event: u32,
	hwnd: HWND,
	id_object: i32,
	_id_child: i32,
	_thread: u32,
	_time: u32,
) {
	if id_object != OBJID_WINDOW {
		return;
	}
	with_widget(|w| {
		if w.surface.is_null() || hwnd == w.surface {
			return;
		}
		// SAFETY: window-tree queries and our own timer.
		unsafe {
			let ours = hwnd == w.taskbar || IsChild(w.taskbar, hwnd) != 0;
			if ours && !w.reposition_pending {
				SetTimer(w.listener, REPOSITION_TIMER, REPOSITION_MS, None);
				w.reposition_pending = true;
			}
		}
	});
}

/// `HKCU\…\Themes\Personalize\SystemUsesLightTheme`: what the taskbar follows.
fn light_theme() -> bool {
	let mut value = 0u32;
	let mut len = std::mem::size_of::<u32>() as u32;
	// SAFETY: static NUL-terminated names; `value` holds a DWORD.
	let status = unsafe {
		RegGetValueW(
			HKEY_CURRENT_USER,
			w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
			w!("SystemUsesLightTheme"),
			RRF_RT_REG_DWORD,
			null_mut(),
			(&mut value as *mut u32).cast(),
			&mut len,
		)
	};
	status == ERROR_SUCCESS && value == 1
}

/// tiny-skia's premultiplied RGBA as a DIB's premultiplied BGRA. A fully
/// transparent pixel gets alpha 1: a layered window lets clicks through
/// alpha-0 pixels, and the gaps between the letters should still be the widget.
fn bgra(p: PremultipliedColorU8) -> u32 {
	if p.alpha() == 0 {
		return 0x0100_0000;
	}
	(p.alpha() as u32) << 24 | (p.red() as u32) << 16 | (p.green() as u32) << 8 | p.blue() as u32
}

/// Push the pixmap to the layered window, resizing it to the pixmap.
///
/// # Safety
/// `hwnd` must be a layered window owned by this thread.
unsafe fn push(hwnd: HWND, pm: &Pixmap) -> bool {
	let (w, h) = (pm.width() as i32, pm.height() as i32);
	// SAFETY: every GDI object created here is released before returning, and
	// the DIB holds exactly `w * h` 32-bit pixels.
	unsafe {
		let screen = GetDC(null_mut());
		let mem = CreateCompatibleDC(screen);
		let info = BITMAPINFO {
			bmiHeader: BITMAPINFOHEADER {
				biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
				biWidth: w,
				// Negative: top-down rows, as tiny-skia stores them.
				biHeight: -h,
				biPlanes: 1,
				biBitCount: 32,
				biCompression: BI_RGB,
				..Default::default()
			},
			..Default::default()
		};
		let mut bits = null_mut();
		let bitmap = CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
		if bitmap.is_null() || bits.is_null() {
			DeleteDC(mem);
			ReleaseDC(null_mut(), screen);
			return false;
		}
		let old = SelectObject(mem, bitmap);
		let dst = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
		for (d, p) in dst.iter_mut().zip(pm.pixels()) {
			*d = bgra(*p);
		}
		let size = SIZE { cx: w, cy: h };
		let origin = POINT { x: 0, y: 0 };
		let blend = BLENDFUNCTION {
			BlendOp: AC_SRC_OVER as u8,
			BlendFlags: 0,
			SourceConstantAlpha: 255,
			AlphaFormat: AC_SRC_ALPHA as u8,
		};
		let ok =
			UpdateLayeredWindow(hwnd, screen, null(), &size, mem, &origin, 0, &blend, ULW_ALPHA);
		SelectObject(mem, old);
		DeleteObject(bitmap);
		DeleteDC(mem);
		ReleaseDC(null_mut(), screen);
		ok != 0
	}
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use super::*;
	use crate::models::{UsageSource, UsageWindow};

	#[test]
	fn bgra_swaps_channels_and_keeps_transparent_pixels_clickable() {
		let p = PremultipliedColorU8::from_rgba(10, 20, 30, 40).unwrap();
		assert_eq!(bgra(p), 0x280A_141E);
		assert_eq!(bgra(PremultipliedColorU8::TRANSPARENT), 0x0100_0000);
	}

	/// The thread on the other end has a queue and never reads it — the shape
	/// of a widget thread stuck behind a hung Explorer.
	#[test]
	fn waking_a_thread_that_never_pumps_returns_at_once() {
		let (tx, rx) = mpsc::channel();
		let stuck = std::thread::spawn(move || {
			let mut msg = MSG::default();
			// SAFETY: creates this thread's queue.
			unsafe { PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE) };
			// SAFETY: plain call.
			tx.send(unsafe { GetCurrentThreadId() }).unwrap();
			std::thread::sleep(Duration::from_secs(2));
		});
		let shared = Shared::new(rx.recv().unwrap());
		let started = Instant::now();
		for _ in 0..1000 {
			shared.wake();
		}
		assert!(started.elapsed() < Duration::from_millis(500));
		assert!(shared.wake_pending.load(Ordering::Acquire));
		stuck.join().unwrap();
	}

	fn usage(agent: &str, five: f64, week: f64) -> ProviderUsage {
		let win = |p| Some(UsageWindow { used_percent: p, resets_at: None });
		ProviderUsage {
			agent: agent.into(),
			profile_id: agent.into(),
			profile_name: agent.into(),
			is_default: true,
			five_hour: win(five),
			weekly: win(week),
			per_model: vec![],
			source: UsageSource::StatusLine,
			updated_at: now_ms(),
		}
	}

	/// Puts a real widget in the real taskbar. `FACTORAI_WIDGET_HOLD_SECS` keeps
	/// it there longer (default 5) for a screenshot.
	#[test]
	#[ignore = "embeds a window in this machine's taskbar"]
	fn embeds_into_taskbar() {
		let hold = std::env::var("FACTORAI_WIDGET_HOLD_SECS")
			.ok()
			.and_then(|s| s.parse().ok())
			.unwrap_or(5);
		let initial = vec![usage("claude", 62.0, 31.0), usage("codex", 78.0, 93.0)];
		let (tx, shared) = spawn(Host::default(), initial.clone()).expect("widget thread");
		// SAFETY: a lookup by class name.
		let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), null()) };
		assert!(!taskbar.is_null(), "no taskbar on this machine");
		let deadline = Instant::now() + Duration::from_secs(2);
		let mut parent = null_mut();
		while Instant::now() < deadline {
			let surface = shared.surface();
			if !surface.is_null() {
				// SAFETY: a window-tree query.
				parent = unsafe { GetParent(surface) };
				if parent == taskbar {
					break;
				}
			}
			std::thread::sleep(Duration::from_millis(50));
		}
		assert_eq!(parent, taskbar);
		std::thread::sleep(Duration::from_secs(hold.min(3)));
		if let Some(path) = std::env::var_os("FACTORAI_WIDGET_DUMP") {
			dump_surface(shared.surface(), &initial, &path);
		}
		std::thread::sleep(Duration::from_secs(hold.saturating_sub(3)));
		// Still there after the 2 s self-check.
		assert!(!shared.surface().is_null(), "widget fell back to the tray");
		// SAFETY: a visibility query.
		assert_ne!(unsafe { IsWindowVisible(shared.surface()) }, 0);
		let _ = tx.send(Msg::Stop);
		shared.wake();
	}

	/// What the widget pushed and where its window really is, for a composite
	/// over a capture of the taskbar: `<path>.png` plus `<path>.txt` holding the
	/// surface's screen rect. (A disconnected session has no screen to grab.)
	fn dump_surface(surface: HWND, initial: &[ProviderUsage], path: &std::ffi::OsStr) {
		let mut r = RECT::default();
		// SAFETY: geometry queries on a live window.
		let dpi = unsafe {
			SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
			GetWindowRect(surface, &mut r);
			GetDpiForWindow(surface)
		};
		let font = crate::platform::taskbar_widget::load_system_font().unwrap();
		let rows = rows_from(initial, now_ms());
		let pm = render(&rows, &font, dpi as f32 / 96.0, light_theme());
		let path = std::path::PathBuf::from(path);
		pm.save_png(path.with_extension("png")).unwrap();
		let rect = format!("{} {} {} {} {}", r.left, r.top, r.right, r.bottom, dpi);
		std::fs::write(path.with_extension("txt"), rect).unwrap();
	}
}
