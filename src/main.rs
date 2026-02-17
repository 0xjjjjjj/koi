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
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

use event::{EventProxy, KoiEvent};
use renderer::Renderer;
use tabs::TabManager;

fn clipboard_paste() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
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
        }
    }

    fn grid_size(&self) -> (usize, usize) {
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            let scale = window.scale_factor() as f32;
            let size = window.inner_size();
            let ch = renderer.cell_height();
            // Subtract tab bar height when multiple tabs exist
            let tab_bar_h = if self.tab_manager.as_ref().map_or(false, |t| t.count() > 1) {
                ch * scale
            } else {
                0.0
            };
            let cols = (size.width as f32 / (renderer.cell_width() * scale)) as usize;
            let rows = ((size.height as f32 - tab_bar_h) / (ch * scale)) as usize;
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
            let version = std::ffi::CStr::from_ptr(gl::GetString(gl::VERSION) as *const _);
            let renderer_str =
                std::ffi::CStr::from_ptr(gl::GetString(gl::RENDERER) as *const _);
            log::info!("OpenGL version: {:?}", version);
            log::info!("GPU renderer: {:?}", renderer_str);
        }

        // Create renderer after GL is initialized
        let renderer = Renderer::new("IBM Plex Mono", 14.0);
        let cw = renderer.cell_width();
        let ch = renderer.cell_height();
        log::info!("Cell size: {}x{}", cw, ch);

        // Calculate terminal grid size (physical pixels / (logical cell * scale))
        let scale = window.scale_factor() as f32;
        let cols = (size.width as f32 / (cw * scale)) as usize;
        let rows = (size.height as f32 / (ch * scale)) as usize;
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
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                // Click to focus pane
                if let (Some(tab_manager), Some(window)) =
                    (&mut self.tab_manager, &self.window)
                {
                    let size = window.inner_size();
                    let tab_bar_h = if tab_manager.count() > 1 {
                        self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(18.0)
                    } else {
                        0.0
                    };
                    let scale = window.scale_factor() as f32;
                    // cursor_pos is in logical pixels, layouts are in physical
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
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            break;
                        }
                    }
                }
            }
            WindowEvent::Resized(new_size) => {
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
                surface.swap_buffers(context).unwrap();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

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
                        // Cmd+V: Paste from clipboard
                        Key::Character(ref s) if s == "v" => {
                            if let Some(text) = clipboard_paste() {
                                if let Some(tab_manager) = &self.tab_manager {
                                    if let Some(pane) = tab_manager.active_pane() {
                                        // Bracket paste: sanitize end-sequence to prevent injection
                                        let sanitized = text.replace("\x1b[201~", "");
                                        let mut bytes = Vec::new();
                                        bytes.extend_from_slice(b"\x1b[200~");
                                        bytes.extend_from_slice(sanitized.as_bytes());
                                        bytes.extend_from_slice(b"\x1b[201~");
                                        pane.notifier.send_input(&bytes);
                                    }
                                }
                            }
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

                let bytes: Option<Cow<'static, [u8]>> = match event.logical_key {
                    Key::Named(NamedKey::Enter) => Some(Cow::Borrowed(b"\r")),
                    Key::Named(NamedKey::Backspace) => Some(Cow::Borrowed(b"\x7f")),
                    Key::Named(NamedKey::Tab) => Some(Cow::Borrowed(b"\t")),
                    Key::Named(NamedKey::Escape) => Some(Cow::Borrowed(b"\x1b")),
                    Key::Named(NamedKey::ArrowUp) => Some(Cow::Borrowed(b"\x1b[A")),
                    Key::Named(NamedKey::ArrowDown) => Some(Cow::Borrowed(b"\x1b[B")),
                    Key::Named(NamedKey::ArrowRight) => Some(Cow::Borrowed(b"\x1b[C")),
                    Key::Named(NamedKey::ArrowLeft) => Some(Cow::Borrowed(b"\x1b[D")),
                    Key::Named(NamedKey::Home) => Some(Cow::Borrowed(b"\x1b[H")),
                    Key::Named(NamedKey::End) => Some(Cow::Borrowed(b"\x1b[F")),
                    Key::Named(NamedKey::Delete) => Some(Cow::Borrowed(b"\x1b[3~")),
                    Key::Named(NamedKey::PageUp) => Some(Cow::Borrowed(b"\x1b[5~")),
                    Key::Named(NamedKey::PageDown) => Some(Cow::Borrowed(b"\x1b[6~")),
                    Key::Named(NamedKey::Space) => {
                        if ctrl_pressed {
                            Some(Cow::Borrowed(b"\x00")) // Ctrl+Space = NUL
                        } else {
                            Some(Cow::Borrowed(b" "))
                        }
                    }
                    Key::Character(ref s) => {
                        if ctrl_pressed && s.len() == 1 {
                            // Ctrl+key sends control characters (Ctrl+C = 0x03, etc.)
                            let c = s.chars().next().unwrap();
                            if c.is_ascii_lowercase() || (c >= '@' && c <= '_') {
                                let ctrl_byte = (c.to_ascii_uppercase() as u8) & 0x1f;
                                Some(Cow::Owned(vec![ctrl_byte]))
                            } else {
                                Some(Cow::Owned(s.as_bytes().to_vec()))
                            }
                        } else if alt_pressed {
                            // Option-as-Alt: send ESC prefix
                            let mut bytes = vec![0x1b];
                            bytes.extend_from_slice(s.as_bytes());
                            Some(Cow::Owned(bytes))
                        } else {
                            Some(Cow::Owned(s.as_bytes().to_vec()))
                        }
                    }
                    _ => None,
                };

                if let Some(bytes) = bytes {
                    notifier.send_input(&bytes);
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: KoiEvent) {
        match event {
            KoiEvent::Wakeup => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            KoiEvent::Title(title) => {
                if let Some(w) = &self.window {
                    w.set_title(&title);
                }
            }
            KoiEvent::ChildExit(pane_id, code) => {
                log::info!("Pane {} exited with code {}", pane_id, code);
                if let Some(tab_manager) = &mut self.tab_manager {
                    if tab_manager.close_pane_by_id(pane_id) {
                        event_loop.exit();
                        return;
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
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
