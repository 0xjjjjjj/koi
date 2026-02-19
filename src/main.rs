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

struct Koi {
    window: Option<Window>,
    gl_context: Option<glutin::context::PossiblyCurrentContext>,
    gl_surface: Option<glutin::surface::Surface<WindowSurface>>,
    renderer: Option<Renderer>,
    tab_manager: Option<TabManager>,
    event_proxy: EventProxy,
    modifiers: ModifiersState,
    cursor_pos: (f64, f64),
    cursor_blink: std::time::Instant,
    mouse_left_pressed: bool,
    needs_redraw: bool,
    font_size: f32,
    scale: f32,
    scroll_accumulator: f64,
}

impl Koi {
    fn new(event_proxy: EventProxy) -> Self {
        Self {
            window: None,
            gl_context: None,
            gl_surface: None,
            renderer: None,
            tab_manager: None,
            event_proxy,
            modifiers: ModifiersState::empty(),
            cursor_pos: (0.0, 0.0),
            cursor_blink: std::time::Instant::now(),
            mouse_left_pressed: false,
            needs_redraw: true,
            font_size: 14.0,
            scale: 1.0,
            scroll_accumulator: 0.0,
        }
    }

    fn rebuild_renderer(&mut self) {
        self.renderer = Some(Renderer::new("IBM Plex Mono", self.font_size, self.scale));
        if let (Some(renderer), Some(window), Some(tab_manager)) =
            (&self.renderer, &self.window, &self.tab_manager)
        {
            let cw = renderer.cell_width();
            let ch = renderer.cell_height();
            let size = window.inner_size();
            let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
            let vp_h = size.height as f32 - tab_bar_h;
            tab_manager.resize_all(size.width as f32, vp_h, cw, ch);
            window.request_redraw();
        }
        self.needs_redraw = true;
    }

    fn grid_size(&self) -> (usize, usize) {
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            let size = window.inner_size();
            let cw = renderer.cell_width();
            let ch = renderer.cell_height();
            // Cell dimensions are already in physical pixels (scaled at rasterization).
            let tab_bar_h = if self.tab_manager.as_ref().map_or(false, |t| t.count() > 1) {
                ch
            } else {
                0.0
            };
            let cols = (size.width as f32 / cw) as usize;
            let rows = ((size.height as f32 - tab_bar_h) / ch) as usize;
            (cols.max(2), rows.max(1))
        } else {
            (80, 24)
        }
    }
}

impl ApplicationHandler<KoiEvent> for Koi {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
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
                        if config.num_samples() > accum.num_samples() {
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

        self.renderer = Some(renderer);
        self.tab_manager = Some(tab_manager);
        self.window = Some(window);
        self.gl_context = Some(gl_context);
        self.gl_surface = Some(gl_surface);

        // Trigger initial draw
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);
                // Only mark dirty if mouse button is pressed (dragging)
                if self.mouse_left_pressed {
                    self.needs_redraw = true;
                }

                if let (Some(tab_manager), Some(renderer), Some(window)) =
                    (&self.tab_manager, &self.renderer, &self.window)
                {
                    let scale = window.scale_factor() as f32;
                    let cw = renderer.cell_width();
                    let ch = renderer.cell_height();
                    let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                    let cx = position.x as f32 * scale;
                    let cy = position.y as f32 * scale - tab_bar_h;
                    let size = window.inner_size();
                    let viewport_h = size.height as f32 - tab_bar_h;
                    let layouts = tab_manager.active_layouts(size.width as f32, viewport_h);
                    let active_id = tab_manager.active_tab().pane_tree.active_pane_id();

                    if let Some(layout) = layouts.iter().find(|l| l.pane_id == active_id) {
                        let col = ((cx - layout.x) / cw).max(0.0) as usize + 1;
                        let line = ((cy - layout.y) / ch).max(0.0) as usize + 1;

                        // Check if app wants mouse events
                        if let Some(pane) = tab_manager.active_pane() {
                            use alacritty_terminal::term::TermMode;
                            let term = pane.term.lock();
                            let mode = term.mode();
                            let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                            let sgr = mode.contains(TermMode::SGR_MOUSE);
                            let motion = mode.contains(TermMode::MOUSE_MOTION)
                                || mode.contains(TermMode::MOUSE_DRAG);
                            drop(term);

                            if mouse_mode && self.mouse_left_pressed && (motion) {
                                // SGR mouse drag: button 32 = motion + left
                                if sgr {
                                    let seq = format!("\x1b[<32;{};{}M", col, line);
                                    pane.notifier.send_input(seq.as_bytes());
                                }
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                        }

                        // Normal selection drag
                        if self.mouse_left_pressed {
                            if let Some(pane) = tab_manager.active_pane() {
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
                                let mut term = pane.term.lock();
                                if let Some(ref mut sel) = term.selection {
                                    sel.update(point, side);
                                }
                                drop(term);
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.mouse_left_pressed = true;
                self.needs_redraw = true;
                // Click to focus pane + start selection
                if let (Some(tab_manager), Some(renderer), Some(window)) =
                    (&mut self.tab_manager, &self.renderer, &self.window)
                {
                    let size = window.inner_size();
                    let cw = renderer.cell_width();
                    let ch = renderer.cell_height();
                    let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                    let scale = window.scale_factor() as f32;
                    let cx = self.cursor_pos.0 as f32 * scale;
                    let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;
                    let viewport_h = size.height as f32 - tab_bar_h;
                    let layouts = tab_manager.active_layouts(size.width as f32, viewport_h);
                    for layout in &layouts {
                        if cx >= layout.x
                            && cx < layout.x + layout.width
                            && cy >= layout.y
                            && cy < layout.y + layout.height
                        {
                            tab_manager.focus_pane(layout.pane_id);
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
                            if let Some(pane) = tab_manager.active_pane() {
                                use alacritty_terminal::term::TermMode;
                                let mut term = pane.term.lock();
                                let mode = term.mode();
                                let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                                let sgr = mode.contains(TermMode::SGR_MOUSE);
                                if mouse_mode && sgr {
                                    drop(term);
                                    let seq = format!("\x1b[<0;{};{}M", col, line);
                                    pane.notifier.send_input(seq.as_bytes());
                                } else {
                                    term.selection = Some(alacritty_terminal::selection::Selection::new(
                                        alacritty_terminal::selection::SelectionType::Simple,
                                        point,
                                        side,
                                    ));
                                }
                            }
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            break;
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.mouse_left_pressed = false;
                // Mouse reporting: send SGR release if app wants mouse events
                if let (Some(tab_manager), Some(renderer), Some(window)) =
                    (&self.tab_manager, &self.renderer, &self.window)
                {
                    if let Some(pane) = tab_manager.active_pane() {
                        use alacritty_terminal::term::TermMode;
                        let term = pane.term.lock();
                        let mode = term.mode();
                        let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                        let sgr = mode.contains(TermMode::SGR_MOUSE);
                        drop(term);
                        if mouse_mode && sgr {
                            let scale = window.scale_factor() as f32;
                            let cw = renderer.cell_width();
                            let ch = renderer.cell_height();
                            let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                            let cx = self.cursor_pos.0 as f32 * scale;
                            let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;
                            let size = window.inner_size();
                            let viewport_h = size.height as f32 - tab_bar_h;
                            let layouts = tab_manager.active_layouts(size.width as f32, viewport_h);
                            let active_id = tab_manager.active_tab().pane_tree.active_pane_id();
                            if let Some(layout) = layouts.iter().find(|l| l.pane_id == active_id) {
                                let col = ((cx - layout.x) / cw).max(0.0) as usize + 1;
                                let line = ((cy - layout.y) / ch).max(0.0) as usize + 1;
                                let seq = format!("\x1b[<0;{};{}m", col, line);
                                pane.notifier.send_input(seq.as_bytes());
                            }
                        }
                    }
                }
            }
            WindowEvent::Resized(new_size) => {
                self.needs_redraw = true;
                if let (Some(renderer), Some(tab_manager)) =
                    (&self.renderer, &self.tab_manager)
                {
                    let cw = renderer.cell_width();
                    let ch = renderer.cell_height();
                    let w = new_size.width as f32;
                    let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                    let h = new_size.height as f32 - tab_bar_h;
                    tab_manager.resize_all(w, h, cw, ch);
                }

                // Resize GL surface
                if let (Some(surface), Some(context)) =
                    (&self.gl_surface, &self.gl_context)
                {
                    let w = NonZeroU32::new(new_size.width.max(1)).unwrap();
                    let h = NonZeroU32::new(new_size.height.max(1)).unwrap();
                    surface.resize(context, w, h);
                }

                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.needs_redraw {
                    return;
                }
                self.needs_redraw = false;

                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                let Some(window) = &self.window else { return };
                let Some(surface) = &self.gl_surface else {
                    return;
                };
                let Some(context) = &self.gl_context else {
                    return;
                };
                let Some(tab_manager) = &self.tab_manager else {
                    return;
                };

                let size = window.inner_size();
                let w = size.width as f32;
                let h = size.height as f32;

                unsafe {
                    gl::Viewport(0, 0, size.width as i32, size.height as i32);
                    gl::ClearColor(0.937, 0.945, 0.961, 1.0);
                    gl::Clear(gl::COLOR_BUFFER_BIT);
                }

                // Calculate viewport offset for tab bar
                let tab_bar_height = if tab_manager.count() > 1 {
                    renderer.draw_tab_bar(tab_manager, w);
                    renderer.cell_height()
                } else {
                    0.0
                };

                // Render all panes in the active tab
                let viewport_h = h - tab_bar_height;
                let layouts = tab_manager.active_layouts(w, viewport_h);
                let tab = tab_manager.active_tab();
                let active_pane_id = tab.pane_tree.active_pane_id();

                // Cursor blink: 500ms on, 500ms off — only in active pane
                let blink_on = (self.cursor_blink.elapsed().as_millis() % 1000) < 500;

                for layout in &layouts {
                    if let Some(pane) = tab.panes.get(&layout.pane_id) {
                        let is_active = layout.pane_id == active_pane_id;
                        let show_cursor = is_active && blink_on;
                        let term = pane.term.lock();
                        renderer.draw_grid(
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
                            renderer.draw_rect(
                                layout.x + layout.width - 1.0,
                                layout.y + tab_bar_height,
                                2.0,
                                layout.height,
                                divider_color,
                            );
                        }
                        // Bottom edge divider
                        if layout.y + layout.height < viewport_h - 1.0 {
                            renderer.draw_rect(
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
                        renderer.draw_pane_border(
                            active_layout.x,
                            active_layout.y + tab_bar_height,
                            active_layout.width,
                            active_layout.height,
                            2.0,
                            border_color,
                        );
                    }
                }

                renderer.flush(w, h);
                if let Err(e) = surface.swap_buffers(context) {
                    log::error!("swap_buffers failed: {}", e);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                self.needs_redraw = true;

                // Reset cursor blink so it's visible while typing
                self.cursor_blink = std::time::Instant::now();

                let super_pressed = self.modifiers.super_key();
                let shift_pressed = self.modifiers.shift_key();
                let ctrl_pressed = self.modifiers.control_key();
                let alt_pressed = self.modifiers.alt_key();

                // Ctrl+Tab / Ctrl+Shift+Tab: Cycle tabs
                if ctrl_pressed && matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
                    if let Some(tab_manager) = &mut self.tab_manager {
                        if shift_pressed {
                            tab_manager.prev_tab();
                        } else {
                            tab_manager.next_tab();
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }
                    return;
                }

                // Cmd+Option+Arrow: Directional pane navigation
                if super_pressed && alt_pressed {
                    match event.logical_key {
                        Key::Named(NamedKey::ArrowLeft)
                        | Key::Named(NamedKey::ArrowRight)
                        | Key::Named(NamedKey::ArrowUp)
                        | Key::Named(NamedKey::ArrowDown) => {
                            if let (Some(tab_manager), Some(window)) =
                                (&mut self.tab_manager, &self.window)
                            {
                                let size = window.inner_size();
                                let tab_bar_h = if tab_manager.count() > 1 {
                                    self.renderer
                                        .as_ref()
                                        .map(|r| r.cell_height())
                                        .unwrap_or(18.0)
                                } else {
                                    0.0
                                };
                                let vp_h = size.height as f32 - tab_bar_h;
                                let layouts =
                                    tab_manager.active_layouts(size.width as f32, vp_h);
                                let active_id =
                                    tab_manager.active_tab().pane_tree.active_pane_id();

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
                                        tab_manager.focus_pane(target.pane_id);
                                    }
                                }
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
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
                            let cw = self.renderer.as_ref().map(|r| r.cell_width()).unwrap_or(8.0);
                            let ch = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(18.0);
                            if let Some(tab_manager) = &mut self.tab_manager {
                                let was_single = tab_manager.count() == 1;
                                tab_manager.add_tab(cols, rows, cw, ch, &self.event_proxy);
                                // Tab bar just appeared — resize all panes for reduced viewport
                                if was_single {
                                    if let Some(window) = &self.window {
                                        let size = window.inner_size();
                                        let vp_h = size.height as f32 - ch;
                                        tab_manager.resize_all(size.width as f32, vp_h, cw, ch);
                                    }
                                }
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+W: Close active pane (or tab if last pane)
                        Key::Character(ref s) if s == "w" => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                if tab_manager.close_active_pane() {
                                    event_loop.exit();
                                    return;
                                }
                                // Resize surviving panes to fill the freed space.
                                if let (Some(renderer), Some(window)) =
                                    (&self.renderer, &self.window)
                                {
                                    let size = window.inner_size();
                                    let cw = renderer.cell_width();
                                    let ch = renderer.cell_height();
                                    let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                                    let h = size.height as f32 - tab_bar_h;
                                    tab_manager.resize_all(size.width as f32, h, cw, ch);
                                }
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+Shift+[ : Previous tab
                        Key::Character(ref s) if s == "{" && shift_pressed => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.prev_tab();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+Shift+] : Next tab
                        Key::Character(ref s) if s == "}" && shift_pressed => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.next_tab();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+1-9: Go to tab
                        Key::Character(ref s)
                            if s.len() == 1
                                && s.chars().next().unwrap().is_ascii_digit() =>
                        {
                            let digit = s.chars().next().unwrap() as usize - '0' as usize;
                            if digit >= 1 {
                                if let Some(tab_manager) = &mut self.tab_manager {
                                    tab_manager.goto_tab(digit - 1);
                                    if let Some(w) = &self.window {
                                        w.request_redraw();
                                    }
                                }
                            }
                            return;
                        }
                        // Cmd+D: Split pane vertically
                        Key::Character(ref s) if s == "d" && !shift_pressed => {
                            let (cols, rows) = self.grid_size();
                            let cw = self.renderer.as_ref().map(|r| r.cell_width()).unwrap_or(8.0);
                            let ch = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(18.0);
                            let vp = self.window.as_ref().map(|w| w.inner_size()).unwrap_or_default();
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.split_active(
                                    panes::Split::Vertical,
                                    cols, rows, cw, ch,
                                    vp.width as f32, vp.height as f32,
                                    &self.event_proxy,
                                );
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+Shift+D: Split pane horizontally
                        Key::Character(ref s) if s == "D" && shift_pressed => {
                            let (cols, rows) = self.grid_size();
                            let cw = self.renderer.as_ref().map(|r| r.cell_width()).unwrap_or(8.0);
                            let ch = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(18.0);
                            let vp = self.window.as_ref().map(|w| w.inner_size()).unwrap_or_default();
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.split_active(
                                    panes::Split::Horizontal,
                                    cols, rows, cw, ch,
                                    vp.width as f32, vp.height as f32,
                                    &self.event_proxy,
                                );
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+Shift+Enter: Toggle zoom on active pane
                        Key::Named(NamedKey::Enter) if shift_pressed => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.toggle_zoom();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+]: Focus next pane
                        Key::Character(ref s) if s == "]" && !shift_pressed => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.focus_next_pane();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+[: Focus previous pane
                        Key::Character(ref s) if s == "[" && !shift_pressed => {
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.focus_prev_pane();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            return;
                        }
                        // Cmd+C: Copy selection to clipboard
                        Key::Character(ref s) if s == "c" => {
                            if let Some(tab_manager) = &self.tab_manager {
                                if let Some(pane) = tab_manager.active_pane() {
                                    let mut term = pane.term.lock();
                                    if let Some(text) = term.selection_to_string() {
                                        clipboard_copy(&text);
                                    }
                                    term.selection = None;
                                }
                            }
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            return;
                        }
                        // Cmd+V: Paste from clipboard
                        Key::Character(ref s) if s == "v" => {
                            if let Some(tab_manager) = &self.tab_manager {
                                if let Some(pane) = tab_manager.active_pane() {
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
                            }
                            return;
                        }
                        // Cmd+Q: Quit
                        Key::Character(ref s) if s == "q" => {
                            event_loop.exit();
                            return;
                        }
                        // Cmd+=: Zoom in
                        Key::Character(ref s) if s == "=" || s == "+" => {
                            self.font_size = (self.font_size + 1.0).min(32.0);
                            self.rebuild_renderer();
                            return;
                        }
                        // Cmd+-: Zoom out
                        Key::Character(ref s) if s == "-" => {
                            self.font_size = (self.font_size - 1.0).max(8.0);
                            self.rebuild_renderer();
                            return;
                        }
                        // Cmd+0: Reset zoom
                        Key::Character(ref s) if s == "0" => {
                            self.font_size = 14.0;
                            self.rebuild_renderer();
                            return;
                        }
                        // Cmd+K: Clear screen
                        Key::Character(ref s) if s == "k" => {
                            if let Some(tab_manager) = &self.tab_manager {
                                if let Some(pane) = tab_manager.active_pane() {
                                    // Send clear screen + move cursor home
                                    pane.notifier.send_input(b"\x1b[2J\x1b[H");
                                }
                            }
                            return;
                        }
                        _ => {
                            // Don't forward other Cmd+key combos to PTY
                            return;
                        }
                    }
                }

                // Forward to active pane's PTY
                let Some(tab_manager) = &self.tab_manager else {
                    return;
                };
                let Some(pane) = tab_manager.active_pane() else {
                    return;
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
                                    if c.is_ascii_lowercase() || (c >= '@' && c <= '_') {
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
                    notifier.send_input(&bytes);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.needs_redraw = true;
                let scroll_lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => {
                        self.scroll_accumulator = 0.0;
                        y as i32
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        let ch = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(20.0);
                        self.scroll_accumulator += pos.y / ch as f64;
                        let lines = self.scroll_accumulator as i32;
                        self.scroll_accumulator -= lines as f64;
                        lines
                    }
                };
                if scroll_lines != 0 {
                    if let Some(tab_manager) = &self.tab_manager {
                        if let Some(pane) = tab_manager.active_pane() {
                            // Mouse reporting: send scroll as button 64/65 if app wants it
                            use alacritty_terminal::term::TermMode;
                            let term = pane.term.lock();
                            let mode = term.mode();
                            let mouse_mode = mode.intersects(TermMode::MOUSE_MODE);
                            let sgr = mode.contains(TermMode::SGR_MOUSE);
                            let alt_screen = mode.contains(TermMode::ALT_SCREEN);
                            drop(term);

                            if mouse_mode && sgr {
                                if let (Some(renderer), Some(window)) =
                                    (&self.renderer, &self.window)
                                {
                                    let scale = window.scale_factor() as f32;
                                    let cw = renderer.cell_width();
                                    let ch = renderer.cell_height();
                                    let tab_bar_h =
                                        if tab_manager.count() > 1 { ch } else { 0.0 };
                                    let cx = self.cursor_pos.0 as f32 * scale;
                                    let cy = self.cursor_pos.1 as f32 * scale - tab_bar_h;
                                    let size = window.inner_size();
                                    let viewport_h = size.height as f32 - tab_bar_h;
                                    let layouts =
                                        tab_manager.active_layouts(size.width as f32, viewport_h);
                                    let active_id =
                                        tab_manager.active_tab().pane_tree.active_pane_id();
                                    if let Some(layout) =
                                        layouts.iter().find(|l| l.pane_id == active_id)
                                    {
                                        let col =
                                            ((cx - layout.x) / cw).max(0.0) as usize + 1;
                                        let line =
                                            ((cy - layout.y) / ch).max(0.0) as usize + 1;
                                        // button 64 = scroll up, 65 = scroll down
                                        let button = if scroll_lines > 0 { 64 } else { 65 };
                                        let count = scroll_lines.unsigned_abs();
                                        for _ in 0..count {
                                            let seq =
                                                format!("\x1b[<{};{};{}M", button, col, line);
                                            pane.notifier.send_input(seq.as_bytes());
                                        }
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
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    let new_scale = window.scale_factor() as f32;
                    if (new_scale - self.scale).abs() > 0.01 {
                        self.scale = new_scale;
                        self.rebuild_renderer();
                    }
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: KoiEvent) {
        match event {
            KoiEvent::Wakeup => {
                self.needs_redraw = true;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            KoiEvent::Title(title) => {
                self.needs_redraw = true;
                // Sanitize: strip control chars, limit length.
                let title: String = title.chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect();
                if let Some(w) = &self.window {
                    w.set_title(&title);
                }
                if let Some(tab_manager) = &mut self.tab_manager {
                    tab_manager.set_active_tab_title(title);
                }
            }
            KoiEvent::ChildExit(pane_id, code) => {
                self.needs_redraw = true;
                log::info!("Pane {} exited with code {}", pane_id, code);
                if let Some(tab_manager) = &mut self.tab_manager {
                    if tab_manager.close_pane_by_id(pane_id) {
                        event_loop.exit();
                        return;
                    }
                    // Resize surviving panes to fill freed space.
                    if let (Some(renderer), Some(window)) =
                        (&self.renderer, &self.window)
                    {
                        let size = window.inner_size();
                        let cw = renderer.cell_width();
                        let ch = renderer.cell_height();
                        let tab_bar_h = if tab_manager.count() > 1 { ch } else { 0.0 };
                        let h = size.height as f32 - tab_bar_h;
                        tab_manager.resize_all(size.width as f32, h, cw, ch);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
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
        // Schedule periodic redraws so the cursor blink animates when idle.
        if self.window.is_some() {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(500),
            ));
            if let Some(w) = &self.window {
                w.request_redraw();
            }
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
