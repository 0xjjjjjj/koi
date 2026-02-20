mod event;
mod gl;
mod panes;
mod renderer;
mod tabs;
mod terminal;

use std::borrow::Cow;
use std::num::NonZeroU32;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

use event::{EventProxy, KoiEvent};
use renderer::Renderer;
use tabs::TabManager;

fn clipboard_paste() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn clipboard_copy(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_owned());
    }
}

/// Pre-computed mouse-to-grid mapping for event handlers.
struct MouseHit {
    col: usize,
    line: usize,
}

/// State for an in-progress divider drag.
struct DividerDrag {
    path: Vec<bool>,
    split: panes::Split,
    origin: f32,
    span: f32,
}

/// Initialized application state — only exists after `resumed()`.
struct KoiState {
    window: Window,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl_surface: glutin::surface::Surface<WindowSurface>,
    renderer: Renderer,
    tab_manager: TabManager,
    modifiers: ModifiersState,
    cursor_pos: (f64, f64),
    cursor_blink: std::time::Instant,
    last_blink_on: bool,
    mouse_left_pressed: bool,
    needs_redraw: bool,
    scroll_accumulator: f64,
    auto_scroll_delta: i32,
    divider_drag: Option<DividerDrag>,
}

impl KoiState {
    /// Map current cursor position to terminal grid coordinates.
    fn mouse_hit(&self) -> Option<MouseHit> {
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let scale = self.window.scale_factor() as f32;
        let cx = self.cursor_pos.0 as f32 * scale;
        let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;
        let size = self.window.inner_size();
        let viewport_h = (size.height as f32 - tab_bar_h).max(0.0);
        let layouts = self.tab_manager.active_layouts(size.width as f32, viewport_h);
        let active_tab = self.tab_manager.active_tab()?;
        let active_id = active_tab.pane_tree.active_pane_id();
        let layout = layouts.iter().find(|l| l.pane_id == active_id)?;
        let col = ((cx - layout.x) / cw).max(0.0) as usize + 1;
        let line = ((cy - layout.y) / ch).max(0.0) as usize + 1;
        Some(MouseHit { col, line })
    }

    fn grid_size(&self) -> (usize, usize) {
        let size = self.window.inner_size();
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let cols = (size.width as f32 / cw) as usize;
        let rows = ((size.height as f32 - tab_bar_h).max(0.0) / ch) as usize;
        (cols.max(2), rows.max(1))
    }

    fn rebuild_renderer(&mut self, font_size: f32, scale: f32) {
        self.renderer = Renderer::new("IBM Plex Mono", font_size, scale);
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let size = self.window.inner_size();
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let vp_h = (size.height as f32 - tab_bar_h).max(0.0);
        self.tab_manager.resize_all(size.width as f32, vp_h, cw, ch);
        self.needs_redraw = true;
        self.window.request_redraw();
    }

    fn handle_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        self.cursor_pos = (position.x, position.y);

        // Skip expensive layout/lock work when not dragging.
        if !self.mouse_left_pressed {
            return;
        }
        self.needs_redraw = true;

        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let scale = self.window.scale_factor() as f32;
        let cx = self.cursor_pos.0 as f32 * scale;
        let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;

        // Handle divider drag — update ratio and resize panes.
        if let Some(ref drag) = self.divider_drag {
            if drag.span < 1.0 {
                return;
            }
            let cursor_along = match drag.split {
                panes::Split::Vertical => cx,
                panes::Split::Horizontal => cy,
            };
            let ratio = ((cursor_along - drag.origin) / drag.span).clamp(0.1, 0.9);
            let path = drag.path.clone();
            self.tab_manager.set_split_ratio(&path, ratio);
            let size = self.window.inner_size();
            let viewport_h = (size.height as f32 - tab_bar_h).max(0.0);
            self.tab_manager.resize_active_tab(size.width as f32, viewport_h, cw, ch);
            self.window.request_redraw();
            return;
        }
        let size = self.window.inner_size();
        let viewport_h = (size.height as f32 - tab_bar_h).max(0.0);
        let layouts = self.tab_manager.active_layouts(size.width as f32, viewport_h);
        let active_tab = self.tab_manager.active_tab();
        let active_id = active_tab.map(|t| t.pane_tree.active_pane_id());
        let layout = active_id.and_then(|id| layouts.iter().find(|l| l.pane_id == id));

        let Some(layout) = layout else { return };
        let layout_y = layout.y;
        let layout_h = layout.height;
        let layout_x = layout.x;

        // Detect out-of-bounds for auto-scroll during selection drag.
        let rows = (layout_h / ch) as i32;
        if cy < layout_y {
            // Cursor above pane — scroll up.
            self.auto_scroll_delta = -1;
        } else if cy > layout_y + layout_h {
            // Cursor below pane — scroll down.
            self.auto_scroll_delta = 1;
        } else {
            self.auto_scroll_delta = 0;
        }

        // Clamp grid position for selection update.
        let col = ((cx - layout_x) / cw).max(0.0) as usize + 1;
        let raw_line = ((cy - layout_y) / ch) as i32;
        let line = raw_line.clamp(0, rows.saturating_sub(1));

        let grid_col = col.saturating_sub(1);
        let point = alacritty_terminal::index::Point::new(
            alacritty_terminal::index::Line(line),
            alacritty_terminal::index::Column(grid_col),
        );
        let side = if ((cx - layout_x) % cw) > cw / 2.0 {
            alacritty_terminal::index::Side::Right
        } else {
            alacritty_terminal::index::Side::Left
        };

        // Single lock: check mode + update selection in one scope.
        if let Some(pane) = self.tab_manager.active_pane() {
            use alacritty_terminal::term::TermMode;
            let mut term = pane.term.lock();
            let mode = term.mode();
            let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
            let sgr = mode.contains(TermMode::SGR_MOUSE);
            let motion = mode.contains(TermMode::MOUSE_MOTION)
                || mode.contains(TermMode::MOUSE_DRAG);

            if mouse_mode && motion && sgr {
                drop(term);
                pane.notifier.send_bytes(
                    format!("\x1b[<32;{};{}M", col, line as usize + 1).into_bytes(),
                );
            } else {
                // Scroll immediately if OOB, then update selection.
                if self.auto_scroll_delta != 0 {
                    use alacritty_terminal::grid::Scroll;
                    term.scroll_display(Scroll::Delta(self.auto_scroll_delta));
                }
                if let Some(ref mut sel) = term.selection {
                    sel.update(point, side);
                }
            }
        }
        self.window.request_redraw();
    }

    fn handle_mouse_press(&mut self) {
        self.mouse_left_pressed = true;
        self.needs_redraw = true;

        let size = self.window.inner_size();
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let scale = self.window.scale_factor() as f32;
        let cx = self.cursor_pos.0 as f32 * scale;
        let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;
        let viewport_h = (size.height as f32 - tab_bar_h).max(0.0);

        // Check if cursor is on a divider (4px threshold).
        let dividers = self.tab_manager.active_dividers(size.width as f32, viewport_h);
        const THRESHOLD: f32 = 4.0;
        for div in &dividers {
            let (along, perp) = match div.split {
                panes::Split::Vertical => (cx, cy),
                panes::Split::Horizontal => (cy, cx),
            };
            if (along - div.position).abs() <= THRESHOLD
                && perp >= div.perp_start
                && perp <= div.perp_end
            {
                self.divider_drag = Some(DividerDrag {
                    path: div.path.clone(),
                    split: div.split,
                    origin: div.origin,
                    span: div.span,
                });
                return;
            }
        }

        let layouts = self.tab_manager.active_layouts(size.width as f32, viewport_h);

        for layout in &layouts {
            if cx >= layout.x
                && cx < layout.x + layout.width
                && cy >= layout.y
                && cy < layout.y + layout.height
            {
                self.tab_manager.focus_pane(layout.pane_id);
                let col = ((cx - layout.x) / cw).max(0.0) as usize + 1;
                let line = ((cy - layout.y) / ch).max(0.0) as usize + 1;

                // Check mouse mode and either send SGR or start selection (single lock)
                let grid_col = col.saturating_sub(1);
                let grid_line = (line as i32).saturating_sub(1);
                let point = alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(grid_line),
                    alacritty_terminal::index::Column(grid_col),
                );
                let side = if ((cx - layout.x) % cw) > cw / 2.0 {
                    alacritty_terminal::index::Side::Right
                } else {
                    alacritty_terminal::index::Side::Left
                };
                if let Some(pane) = self.tab_manager.active_pane() {
                    use alacritty_terminal::term::TermMode;
                    let mut term = pane.term.lock();
                    let mode = term.mode();
                    let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                    let sgr = mode.contains(TermMode::SGR_MOUSE);
                    if mouse_mode && sgr {
                        drop(term);
                        pane.notifier.send_bytes(
                            format!("\x1b[<0;{};{}M", col, line).into_bytes(),
                        );
                    } else {
                        term.selection = Some(alacritty_terminal::selection::Selection::new(
                            alacritty_terminal::selection::SelectionType::Simple,
                            point,
                            side,
                        ));
                    }
                }
                self.window.request_redraw();
                break;
            }
        }
    }

    fn handle_mouse_release(&mut self) {
        self.mouse_left_pressed = false;
        self.auto_scroll_delta = 0;
        self.divider_drag = None;
        if let Some(pane) = self.tab_manager.active_pane() {
            use alacritty_terminal::term::TermMode;
            let term = pane.term.lock();
            let mode = term.mode();
            let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
            let sgr = mode.contains(TermMode::SGR_MOUSE);
            drop(term);
            if mouse_mode && sgr {
                if let Some(hit) = self.mouse_hit() {
                    pane.notifier.send_bytes(
                        format!("\x1b[<0;{};{}m", hit.col, hit.line).into_bytes(),
                    );
                }
            }
        }
    }

    /// Handle keyboard input. Returns `true` if the application should exit.
    fn handle_keyboard(
        &mut self,
        event: winit::event::KeyEvent,
        event_proxy: &EventProxy,
        font_size: &mut f32,
        scale: f32,
    ) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }

        // Any keypress cancels an in-progress divider drag.
        self.divider_drag = None;
        self.needs_redraw = true;

        // Reset cursor blink so it's visible while typing
        self.cursor_blink = std::time::Instant::now();

        let super_pressed = self.modifiers.super_key();
        let shift_pressed = self.modifiers.shift_key();
        let ctrl_pressed = self.modifiers.control_key();
        let alt_pressed = self.modifiers.alt_key();

        // Ctrl+Tab / Ctrl+Shift+Tab: Cycle tabs
        if ctrl_pressed && matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
            if shift_pressed {
                self.tab_manager.prev_tab();
            } else {
                self.tab_manager.next_tab();
            }
            self.window.request_redraw();
            return false;
        }

        // Cmd+Option+Arrow: Directional pane navigation
        if super_pressed && alt_pressed {
            match event.logical_key {
                Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::ArrowRight)
                | Key::Named(NamedKey::ArrowUp)
                | Key::Named(NamedKey::ArrowDown) => {
                    let size = self.window.inner_size();
                    let tab_bar_h = if self.tab_manager.count() > 1 {
                        self.renderer.cell_height()
                    } else {
                        0.0
                    };
                    let vp_h = (size.height as f32 - tab_bar_h).max(0.0);
                    let layouts =
                        self.tab_manager.active_layouts(size.width as f32, vp_h);
                    if let Some(active_tab) = self.tab_manager.active_tab() {
                    let active_id = active_tab.pane_tree.active_pane_id();

                    // Find active pane's center
                    if let Some(active_layout) =
                        layouts.iter().find(|l| l.pane_id == active_id)
                    {
                        let ax = active_layout.x + active_layout.width / 2.0;
                        let ay = active_layout.y + active_layout.height / 2.0;

                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != active_id)
                            .filter(|l| {
                                let lx = l.x + l.width / 2.0;
                                let ly = l.y + l.height / 2.0;
                                match event.logical_key {
                                    Key::Named(NamedKey::ArrowLeft) => lx < ax,
                                    Key::Named(NamedKey::ArrowRight) => lx > ax,
                                    Key::Named(NamedKey::ArrowUp) => ly < ay,
                                    Key::Named(NamedKey::ArrowDown) => ly > ay,
                                    _ => false,
                                }
                            })
                            .min_by(|a, b| {
                                let da = (a.x + a.width / 2.0 - ax).powi(2)
                                    + (a.y + a.height / 2.0 - ay).powi(2);
                                let db = (b.x + b.width / 2.0 - ax).powi(2)
                                    + (b.y + b.height / 2.0 - ay).powi(2);
                                da.partial_cmp(&db).unwrap()
                            });

                        if let Some(target) = target {
                            self.tab_manager.focus_pane(target.pane_id);
                        }
                    }
                    } // if let Some(active_tab)
                    self.window.request_redraw();
                    return false;
                }
                _ => {}
            }
        }

        // Handle tab keybindings (Cmd+...)
        if super_pressed {
            match event.logical_key {
                // Cmd+T: New tab
                Key::Character(ref s) if s == "t" => {
                    let (cols, rows) = self.grid_size();
                    let cw = self.renderer.cell_width();
                    let ch = self.renderer.cell_height();
                    let was_single = self.tab_manager.count() == 1;
                    self.tab_manager.add_tab(cols, rows, cw, ch, event_proxy);
                    // Tab bar just appeared — resize all panes for reduced viewport
                    if was_single {
                        let size = self.window.inner_size();
                        let vp_h = size.height as f32 - ch;
                        self.tab_manager.resize_all(size.width as f32, vp_h, cw, ch);
                    }
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+W: Close active pane (or tab if last pane)
                Key::Character(ref s) if s == "w" => {
                    if self.tab_manager.close_active_pane() {
                        return true; // signal exit
                    }
                    // Resize surviving panes to fill the freed space.
                    let cw = self.renderer.cell_width();
                    let ch = self.renderer.cell_height();
                    let size = self.window.inner_size();
                    let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
                    let h = size.height as f32 - tab_bar_h;
                    self.tab_manager.resize_all(size.width as f32, h, cw, ch);
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+Shift+[ : Previous tab
                Key::Character(ref s) if s == "{" && shift_pressed => {
                    self.tab_manager.prev_tab();
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+Shift+] : Next tab
                Key::Character(ref s) if s == "}" && shift_pressed => {
                    self.tab_manager.next_tab();
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+1-9: Go to tab
                Key::Character(ref s)
                    if s.len() == 1
                        && s.chars().next().unwrap().is_ascii_digit() =>
                {
                    let digit = s.chars().next().unwrap() as usize - '0' as usize;
                    if digit >= 1 {
                        self.tab_manager.goto_tab(digit - 1);
                        self.window.request_redraw();
                    }
                    return false;
                }
                // Cmd+D: Split pane vertically
                Key::Character(ref s) if s == "d" && !shift_pressed => {
                    let (cols, rows) = self.grid_size();
                    let cw = self.renderer.cell_width();
                    let ch = self.renderer.cell_height();
                    let vp = self.window.inner_size();
                    let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
                    self.tab_manager.split_active(
                        panes::Split::Vertical,
                        cols, rows, cw, ch,
                        vp.width as f32, (vp.height as f32 - tab_bar_h).max(0.0),
                        event_proxy,
                    );
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+Shift+D: Split pane horizontally
                Key::Character(ref s) if s == "D" && shift_pressed => {
                    let (cols, rows) = self.grid_size();
                    let cw = self.renderer.cell_width();
                    let ch = self.renderer.cell_height();
                    let vp = self.window.inner_size();
                    let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
                    self.tab_manager.split_active(
                        panes::Split::Horizontal,
                        cols, rows, cw, ch,
                        vp.width as f32, (vp.height as f32 - tab_bar_h).max(0.0),
                        event_proxy,
                    );
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+Shift+Enter: Toggle zoom on active pane
                Key::Named(NamedKey::Enter) if shift_pressed => {
                    self.tab_manager.toggle_zoom();
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+]: Focus next pane
                Key::Character(ref s) if s == "]" && !shift_pressed => {
                    self.tab_manager.focus_next_pane();
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+[: Focus previous pane
                Key::Character(ref s) if s == "[" && !shift_pressed => {
                    self.tab_manager.focus_prev_pane();
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+C: Copy selection to clipboard
                Key::Character(ref s) if s == "c" => {
                    if let Some(pane) = self.tab_manager.active_pane() {
                        let mut term = pane.term.lock();
                        if let Some(text) = term.selection_to_string() {
                            clipboard_copy(&text);
                        }
                        term.selection = None;
                    }
                    self.window.request_redraw();
                    return false;
                }
                // Cmd+V: Paste from clipboard
                Key::Character(ref s) if s == "v" => {
                    if let Some(pane) = self.tab_manager.active_pane() {
                        if let Some(text) = clipboard_paste() {
                            use alacritty_terminal::term::TermMode;
                            let bracketed = pane.term.lock().mode()
                                .contains(TermMode::BRACKETED_PASTE);
                            if bracketed {
                                // Sanitize: strip both bracket markers from content.
                                let sanitized = text
                                    .replace("\x1b[200~", "")
                                    .replace("\x1b[201~", "");
                                let mut bytes = Vec::new();
                                bytes.extend_from_slice(b"\x1b[200~");
                                bytes.extend_from_slice(sanitized.as_bytes());
                                bytes.extend_from_slice(b"\x1b[201~");
                                pane.notifier.send_input(&bytes);
                            } else {
                                pane.notifier.send_input(text.as_bytes());
                            }
                        }
                    }
                    return false;
                }
                // Cmd+Q: Quit
                Key::Character(ref s) if s == "q" => {
                    return true; // signal exit
                }
                // Cmd+=: Zoom in
                Key::Character(ref s) if s == "=" || s == "+" => {
                    *font_size = (*font_size + 1.0).min(32.0);
                    self.rebuild_renderer(*font_size, scale);
                    return false;
                }
                // Cmd+-: Zoom out
                Key::Character(ref s) if s == "-" => {
                    *font_size = (*font_size - 1.0).max(8.0);
                    self.rebuild_renderer(*font_size, scale);
                    return false;
                }
                // Cmd+0: Reset zoom
                Key::Character(ref s) if s == "0" => {
                    *font_size = 14.0;
                    self.rebuild_renderer(*font_size, scale);
                    return false;
                }
                // Cmd+K: Clear screen
                Key::Character(ref s) if s == "k" => {
                    if let Some(pane) = self.tab_manager.active_pane() {
                        // Send clear screen + move cursor home
                        pane.notifier.send_input(b"\x1b[2J\x1b[H");
                    }
                    return false;
                }
                _ => {
                    // Don't forward other Cmd+key combos to PTY
                    return false;
                }
            }
        }

        // Forward to active pane's PTY
        let Some(pane) = self.tab_manager.active_pane() else {
            return false;
        };
        let notifier = &pane.notifier;

        // Check DECCKM (application cursor keys) mode
        let app_cursor = {
            use alacritty_terminal::term::TermMode;
            pane.term.lock().mode().contains(TermMode::APP_CURSOR)
        };

        // CSI modifier parameter: 1 + (shift?1:0) + (alt?2:0) + (ctrl?4:0)
        // When modifier > 1, named keys use forms like \x1b[1;3A (Alt+Up)
        let modifier = 1
            + if shift_pressed { 1 } else { 0 }
            + if alt_pressed { 2 } else { 0 }
            + if ctrl_pressed { 4 } else { 0 };
        let has_modifier = modifier > 1;

        let bytes: Option<Cow<'static, [u8]>> = match event.logical_key {
            Key::Named(NamedKey::Enter) => Some(Cow::Borrowed(b"\r")),
            Key::Named(NamedKey::Backspace) => Some(Cow::Borrowed(b"\x7f")),
            Key::Named(NamedKey::Tab) => Some(Cow::Borrowed(b"\t")),
            Key::Named(NamedKey::Escape) => Some(Cow::Borrowed(b"\x1b")),
            Key::Named(NamedKey::ArrowUp) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}A", modifier).into_bytes())),
            Key::Named(NamedKey::ArrowDown) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}B", modifier).into_bytes())),
            Key::Named(NamedKey::ArrowRight) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}C", modifier).into_bytes())),
            Key::Named(NamedKey::ArrowLeft) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}D", modifier).into_bytes())),
            Key::Named(NamedKey::Home) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}H", modifier).into_bytes())),
            Key::Named(NamedKey::End) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}F", modifier).into_bytes())),
            Key::Named(NamedKey::ArrowUp) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOA" } else { b"\x1b[A" }
            )),
            Key::Named(NamedKey::ArrowDown) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOB" } else { b"\x1b[B" }
            )),
            Key::Named(NamedKey::ArrowRight) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOC" } else { b"\x1b[C" }
            )),
            Key::Named(NamedKey::ArrowLeft) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOD" } else { b"\x1b[D" }
            )),
            Key::Named(NamedKey::Home) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOH" } else { b"\x1b[H" }
            )),
            Key::Named(NamedKey::End) => Some(Cow::Borrowed(
                if app_cursor { b"\x1bOF" } else { b"\x1b[F" }
            )),
            Key::Named(NamedKey::Delete) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[3;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::Delete) => Some(Cow::Borrowed(b"\x1b[3~")),
            Key::Named(NamedKey::PageUp) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[5;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::PageUp) => Some(Cow::Borrowed(b"\x1b[5~")),
            Key::Named(NamedKey::PageDown) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[6;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::PageDown) => Some(Cow::Borrowed(b"\x1b[6~")),
            Key::Named(NamedKey::F1) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}P", modifier).into_bytes())),
            Key::Named(NamedKey::F2) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}Q", modifier).into_bytes())),
            Key::Named(NamedKey::F3) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}R", modifier).into_bytes())),
            Key::Named(NamedKey::F4) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[1;{}S", modifier).into_bytes())),
            Key::Named(NamedKey::F1) => Some(Cow::Borrowed(b"\x1bOP")),
            Key::Named(NamedKey::F2) => Some(Cow::Borrowed(b"\x1bOQ")),
            Key::Named(NamedKey::F3) => Some(Cow::Borrowed(b"\x1bOR")),
            Key::Named(NamedKey::F4) => Some(Cow::Borrowed(b"\x1bOS")),
            Key::Named(NamedKey::F5) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[15;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F5) => Some(Cow::Borrowed(b"\x1b[15~")),
            Key::Named(NamedKey::F6) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[17;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F6) => Some(Cow::Borrowed(b"\x1b[17~")),
            Key::Named(NamedKey::F7) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[18;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F7) => Some(Cow::Borrowed(b"\x1b[18~")),
            Key::Named(NamedKey::F8) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[19;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F8) => Some(Cow::Borrowed(b"\x1b[19~")),
            Key::Named(NamedKey::F9) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[20;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F9) => Some(Cow::Borrowed(b"\x1b[20~")),
            Key::Named(NamedKey::F10) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[21;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F10) => Some(Cow::Borrowed(b"\x1b[21~")),
            Key::Named(NamedKey::F11) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[23;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F11) => Some(Cow::Borrowed(b"\x1b[23~")),
            Key::Named(NamedKey::F12) if has_modifier =>
                Some(Cow::Owned(format!("\x1b[24;{}~", modifier).into_bytes())),
            Key::Named(NamedKey::F12) => Some(Cow::Borrowed(b"\x1b[24~")),
            Key::Named(NamedKey::Space) => {
                if ctrl_pressed {
                    Some(Cow::Borrowed(b"\x00")) // Ctrl+Space = NUL
                } else {
                    Some(Cow::Borrowed(b" "))
                }
            }
            _ => {
                // For text input, use event.text (canonical winit 0.30 path).
                // Ctrl+key: compute control byte from logical_key.
                if ctrl_pressed {
                    if let Key::Character(ref s) = event.logical_key {
                        if s.len() == 1 {
                            let c = s.chars().next().unwrap();
                            if c.is_ascii_lowercase() || ('@'..='_').contains(&c) {
                                let ctrl_byte = (c.to_ascii_uppercase() as u8) & 0x1f;
                                Some(Cow::Owned(vec![ctrl_byte]))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else if alt_pressed {
                    // Option-as-Alt: send ESC prefix + text
                    event.text.as_ref().map(|t| {
                        let mut bytes = vec![0x1b];
                        bytes.extend_from_slice(t.as_bytes());
                        Cow::Owned(bytes)
                    })
                } else {
                    // Normal text: use event.text directly
                    event.text.as_ref().map(|t| {
                        Cow::Owned(t.as_bytes().to_vec())
                    })
                }
            }
        };

        if let Some(bytes) = bytes {
            // Snap to bottom when typing while scrolled up (like iTerm2/Alacritty).
            {
                use alacritty_terminal::grid::Scroll;
                let mut term = pane.term.lock();
                if term.grid().display_offset() != 0 {
                    term.scroll_display(Scroll::Bottom);
                    self.needs_redraw = true;
                }
            }
            notifier.send_input(&bytes);
        }
        false
    }

    fn handle_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        self.needs_redraw = true;
        let scroll_lines = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => {
                self.scroll_accumulator = 0.0;
                y as i32
            }
            winit::event::MouseScrollDelta::PixelDelta(pos) => {
                let ch = self.renderer.cell_height();
                self.scroll_accumulator += pos.y / ch as f64;
                let lines = self.scroll_accumulator as i32;
                self.scroll_accumulator -= lines as f64;
                lines
            }
        };
        if scroll_lines != 0 {
            if let Some(pane) = self.tab_manager.active_pane() {
                // Mouse reporting: send scroll as button 64/65 if app wants it
                use alacritty_terminal::term::TermMode;
                let term = pane.term.lock();
                let mode = term.mode();
                let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                let sgr = mode.contains(TermMode::SGR_MOUSE);
                let alt_screen = mode.contains(TermMode::ALT_SCREEN);
                drop(term);

                if mouse_mode && sgr {
                    if let Some(hit) = self.mouse_hit() {
                        let button = if scroll_lines > 0 { 64 } else { 65 };
                        let count = scroll_lines.unsigned_abs();
                        for _ in 0..count {
                            pane.notifier.send_bytes(
                                format!("\x1b[<{};{};{}M", button, hit.col, hit.line)
                                    .into_bytes(),
                            );
                        }
                    }
                } else if alt_screen {
                    // On alternate screen (vim, less, Claude Code), send arrow
                    // keys instead of scroll_display — there's no scrollback.
                    let key = if scroll_lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    let count = scroll_lines.unsigned_abs();
                    for _ in 0..count {
                        pane.notifier.send_input(key);
                    }
                } else {
                    use alacritty_terminal::grid::Scroll;
                    pane.term.lock().scroll_display(Scroll::Delta(scroll_lines));
                }
            }
            self.window.request_redraw();
        }
    }

    fn handle_resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.needs_redraw = true;
        let cw = self.renderer.cell_width();
        let ch = self.renderer.cell_height();
        let w = new_size.width as f32;
        let tab_bar_h = if self.tab_manager.count() > 1 { ch } else { 0.0 };
        let h = (new_size.height as f32 - tab_bar_h).max(0.0);
        self.tab_manager.resize_all(w, h, cw, ch);

        // Resize GL surface
        let nw = NonZeroU32::new(new_size.width.max(1)).unwrap();
        let nh = NonZeroU32::new(new_size.height.max(1)).unwrap();
        self.gl_surface.resize(&self.gl_context, nw, nh);

        self.window.request_redraw();
    }

    fn render(&mut self) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        let size = self.window.inner_size();
        let w = size.width as f32;
        let h = size.height as f32;

        unsafe {
            gl::Viewport(0, 0, size.width as i32, size.height as i32);
            gl::ClearColor(0.937, 0.945, 0.961, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }

        // Regrow atlas between frames if it filled up during the last render.
        self.renderer.glyph_cache.try_regrow();

        // Calculate viewport offset for tab bar
        let tab_bar_height = if self.tab_manager.count() > 1 {
            self.renderer.draw_tab_bar(&self.tab_manager, w);
            self.renderer.cell_height()
        } else {
            0.0
        };

        // Render all panes in the active tab
        let viewport_h = (h - tab_bar_height).max(0.0);
        let layouts = self.tab_manager.active_layouts(w, viewport_h);
        if let Some(tab) = self.tab_manager.active_tab() {
            let active_pane_id = tab.pane_tree.active_pane_id();

            // Cursor blink: 500ms on, 500ms off — only in active pane
            let blink_on = (self.cursor_blink.elapsed().as_millis() % 1000) < 500;

            for layout in &layouts {
                if let Some(pane) = tab.panes.get(&layout.pane_id) {
                    let is_active = layout.pane_id == active_pane_id;
                    let show_cursor = is_active && blink_on;
                    let term = pane.term.lock();
                    self.renderer.draw_grid(
                        &*term,
                        layout.x,
                        layout.y + tab_bar_height,
                        show_cursor,
                    );
                    drop(term);
                }
            }

            // Draw pane dividers (2px lines between panes)
            if layouts.len() > 1 {
                let divider_color = [0.725, 0.745, 0.792, 1.0]; // Latte overlay0
                for layout in &layouts {
                    // Right edge divider
                    if layout.x + layout.width < w - 1.0 {
                        self.renderer.draw_rect(
                            layout.x + layout.width - 1.0,
                            layout.y + tab_bar_height,
                            2.0,
                            layout.height,
                            divider_color,
                        );
                    }
                    // Bottom edge divider
                    if layout.y + layout.height < viewport_h - 1.0 {
                        self.renderer.draw_rect(
                            layout.x,
                            layout.y + layout.height + tab_bar_height - 1.0,
                            layout.width,
                            2.0,
                            divider_color,
                        );
                    }
                }

                // Highlight the active pane with a border
                if let Some(active_layout) =
                    layouts.iter().find(|l| l.pane_id == active_pane_id)
                {
                    let border_color = [0.122, 0.471, 0.706, 1.0];
                    self.renderer.draw_pane_border(
                        active_layout.x,
                        active_layout.y + tab_bar_height,
                        active_layout.width,
                        active_layout.height,
                        2.0,
                        border_color,
                    );
                }
            }
        }

        self.renderer.flush(w, h);
        if let Err(e) = self.gl_surface.swap_buffers(&self.gl_context) {
            log::error!("swap_buffers failed: {}", e);
        }
    }
}

struct Koi {
    event_proxy: EventProxy,
    font_size: f32,
    scale: f32,
    state: Option<KoiState>,
}

impl Koi {
    fn new(event_proxy: EventProxy) -> Self {
        Self {
            event_proxy,
            font_size: 14.0,
            scale: 1.0,
            state: None,
        }
    }
}

impl ApplicationHandler<KoiEvent> for Koi {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = WindowAttributes::default()
            .with_title("Koi")
            .with_inner_size(winit::dpi::LogicalSize::new(960, 600));

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attrs));

        let (window, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|accum, config| {
                        // Prefer fewest MSAA samples — MSAA conflicts with
                        // dual-source subpixel blending and wastes VRAM.
                        if config.num_samples() < accum.num_samples() {
                            config
                        } else {
                            accum
                        }
                    })
                    .unwrap()
            })
            .unwrap();

        let window = window.unwrap();
        let gl_display = gl_config.display();

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(
                window
                    .window_handle()
                    .expect("window handle")
                    .as_raw(),
            ));

        let gl_context = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .expect("create GL context")
        };

        let size = window.inner_size();
        let width = NonZeroU32::new(size.width.max(1)).unwrap();
        let height = NonZeroU32::new(size.height.max(1)).unwrap();

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            window
                .window_handle()
                .expect("window handle")
                .as_raw(),
            width,
            height,
        );

        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attrs)
                .expect("create GL surface")
        };

        let gl_context = gl_context
            .make_current(&gl_surface)
            .expect("make current");

        // Load GL function pointers
        gl::load_with(|symbol| {
            let symbol = std::ffi::CString::new(symbol).unwrap();
            gl_display.get_proc_address(symbol.as_c_str()).cast()
        });

        // Log GL info
        unsafe {
            let version = {
                let ptr = gl::GetString(gl::VERSION);
                if ptr.is_null() { "unknown" }
                else { std::ffi::CStr::from_ptr(ptr as *const _).to_str().unwrap_or("unknown") }
            };
            let renderer_str = {
                let ptr = gl::GetString(gl::RENDERER);
                if ptr.is_null() { "unknown" }
                else { std::ffi::CStr::from_ptr(ptr as *const _).to_str().unwrap_or("unknown") }
            };
            log::info!("OpenGL version: {}", version);
            log::info!("GPU renderer: {}", renderer_str);
        }

        // Disable IME — we handle all key input directly.
        window.set_ime_allowed(false);

        // Setup terminal environment (TERM, COLORTERM).
        alacritty_terminal::tty::setup_env();

        // Store scale factor for DPI-aware font rendering.
        let scale = window.scale_factor() as f32;
        self.scale = scale;

        // Create renderer — font is rasterized at font_size * scale for HiDPI.
        let renderer = Renderer::new("IBM Plex Mono", self.font_size, scale);
        let cw = renderer.cell_width();
        let ch = renderer.cell_height();
        log::info!("Cell size: {}x{} (scale={})", cw, ch, scale);

        // Cell dimensions are in physical pixels, so divide viewport directly.
        let cols = (size.width as f32 / cw) as usize;
        let rows = (size.height as f32 / ch) as usize;
        let cols = cols.max(2);
        let rows = rows.max(1);
        log::info!("Terminal grid: {}x{}", cols, rows);

        // Create tab manager with one initial tab
        let tab_manager = TabManager::new(cols, rows, cw, ch, &self.event_proxy);

        // Enforce minimum window size: 2 cells wide × 1 cell tall + room for tab bar.
        let min_w = (cw * 2.0) as u32;
        let min_h = (ch * 2.0) as u32; // 1 row + 1 row for tab bar
        window.set_min_inner_size(Some(winit::dpi::PhysicalSize::new(min_w, min_h)));

        self.state = Some(KoiState {
            window,
            gl_context,
            gl_surface,
            renderer,
            tab_manager,
            modifiers: ModifiersState::empty(),
            cursor_pos: (0.0, 0.0),
            cursor_blink: std::time::Instant::now(),
            last_blink_on: true,
            mouse_left_pressed: false,
            needs_redraw: true,
            scroll_accumulator: 0.0,
            auto_scroll_delta: 0,
            divider_drag: None,
        });

        // Trigger initial draw
        if let Some(s) = &self.state {
            s.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(s) = &mut self.state else { return };
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                s.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                s.handle_cursor_moved(position);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                s.handle_mouse_press();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                s.handle_mouse_release();
            }
            WindowEvent::Resized(new_size) => {
                s.handle_resize(new_size);
            }
            WindowEvent::RedrawRequested => {
                s.render();
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                if s.handle_keyboard(key_event, &self.event_proxy, &mut self.font_size, self.scale) {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                s.handle_scroll(delta);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let new_scale = s.window.scale_factor() as f32;
                if (new_scale - self.scale).abs() > 0.01 {
                    self.scale = new_scale;
                    s.rebuild_renderer(self.font_size, self.scale);
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: KoiEvent) {
        let Some(s) = &mut self.state else { return };
        match event {
            KoiEvent::Wakeup => {
                s.needs_redraw = true;
                s.window.request_redraw();
            }
            KoiEvent::Title(title, pane_id) => {
                s.needs_redraw = true;
                // Sanitize: strip control chars, limit length.
                let title: String = title.chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                s.tab_manager.set_tab_title_by_pane(pane_id, title.clone());
                // Only update window title if the event came from the active tab.
                if s.tab_manager.active_tab().is_some_and(|t| t.panes.contains_key(&pane_id)) {
                    s.window.set_title(&title);
                }
            }
            KoiEvent::ChildExit(pane_id, code) => {
                s.needs_redraw = true;
                s.auto_scroll_delta = 0;
                s.mouse_left_pressed = false;
                s.divider_drag = None;
                log::info!("Pane {} exited with code {}", pane_id, code);
                if s.tab_manager.close_pane_by_id(pane_id) {
                    event_loop.exit();
                    return;
                }
                // Resize surviving panes to fill freed space.
                let cw = s.renderer.cell_width();
                let ch = s.renderer.cell_height();
                let size = s.window.inner_size();
                let tab_bar_h = if s.tab_manager.count() > 1 { ch } else { 0.0 };
                let h = size.height as f32 - tab_bar_h;
                s.tab_manager.resize_all(size.width as f32, h, cw, ch);
                s.window.request_redraw();
            }
            KoiEvent::Bell => {
                #[cfg(target_os = "macos")]
                {
                    extern "C" { fn NSBeep(); }
                    unsafe { NSBeep(); }
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(s) = &mut self.state {
            // Auto-scroll during selection drag past viewport edge.
            if s.mouse_left_pressed && s.auto_scroll_delta != 0 {
                if let Some(pane) = s.tab_manager.active_pane() {
                    use alacritty_terminal::grid::{Dimensions, Scroll};
                    use alacritty_terminal::term::TermMode;
                    let mut term = pane.term.lock();

                    // Skip auto-scroll when the app owns mouse input (vim, tmux).
                    let mode = term.mode();
                    if mode.intersects(TermMode::MOUSE_MODE) && mode.contains(TermMode::SGR_MOUSE) {
                        drop(term);
                        s.auto_scroll_delta = 0;
                    } else {
                        term.scroll_display(Scroll::Delta(s.auto_scroll_delta));

                        // Extend selection to the edge row.
                        let ch = s.renderer.cell_height();
                        let rows = {
                            let tab_bar_h = if s.tab_manager.count() > 1 { ch } else { 0.0 };
                            let size = s.window.inner_size();
                            let viewport_h = (size.height as f32 - tab_bar_h).max(0.0);
                            let layouts = s.tab_manager.active_layouts(size.width as f32, viewport_h);
                            let active_id = s.tab_manager.active_tab()
                                .map(|t| t.pane_tree.active_pane_id());
                            active_id
                                .and_then(|id| layouts.iter().find(|l| l.pane_id == id))
                                .map(|l| (l.height / ch) as i32)
                                .unwrap_or(1)
                        };

                        let edge_line = if s.auto_scroll_delta < 0 { 0 } else { (rows - 1).max(0) };
                        let cols = term.grid().columns();
                        let edge_col = if s.auto_scroll_delta < 0 { 0 } else { cols.saturating_sub(1) };
                        let edge_side = if s.auto_scroll_delta < 0 {
                            alacritty_terminal::index::Side::Left
                        } else {
                            alacritty_terminal::index::Side::Right
                        };
                        let point = alacritty_terminal::index::Point::new(
                            alacritty_terminal::index::Line(edge_line),
                            alacritty_terminal::index::Column(edge_col),
                        );
                        if let Some(ref mut sel) = term.selection {
                            sel.update(point, edge_side);
                        }
                        drop(term);
                    }
                }
                s.needs_redraw = true;
                s.window.request_redraw();
                // Tick faster while auto-scrolling for smooth UX.
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(50),
                ));
                return;
            }

            // Only redraw when cursor blink phase actually changes.
            let blink_on = (s.cursor_blink.elapsed().as_millis() % 1000) < 500;
            if blink_on != s.last_blink_on {
                s.last_blink_on = blink_on;
                s.needs_redraw = true;
                s.window.request_redraw();
            }
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(500),
            ));
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::<KoiEvent>::with_user_event().build().unwrap();
    let event_proxy = EventProxy::new(event_loop.create_proxy());
    let mut app = Koi::new(event_proxy);
    event_loop.run_app(&mut app).unwrap();
}
