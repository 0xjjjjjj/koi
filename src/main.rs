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

struct Koi {
    window: Option<Window>,
    gl_context: Option<glutin::context::PossiblyCurrentContext>,
    gl_surface: Option<glutin::surface::Surface<WindowSurface>>,
    renderer: Option<Renderer>,
    tab_manager: Option<TabManager>,
    event_proxy: EventProxy,
    modifiers: ModifiersState,
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
        }
    }

    fn grid_size(&self) -> (usize, usize) {
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            let size = window.inner_size();
            let cols = (size.width as f32 / renderer.cell_width()) as usize;
            let rows = (size.height as f32 / renderer.cell_height()) as usize;
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

        // Calculate terminal grid size
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
            WindowEvent::Resized(new_size) => {
                if let (Some(renderer), Some(tab_manager)) =
                    (&self.renderer, &self.tab_manager)
                {
                    let cw = renderer.cell_width();
                    let ch = renderer.cell_height();
                    let cols = (new_size.width as f32 / cw) as usize;
                    let rows = (new_size.height as f32 / ch) as usize;
                    let cols = cols.max(2);
                    let rows = rows.max(1);
                    tab_manager.resize_all(cols, rows, cw, ch);
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

                for layout in &layouts {
                    if let Some(pane) = tab.panes.get(&layout.pane_id) {
                        let term = pane.term.lock();
                        renderer.draw_grid(
                            &*term,
                            layout.x,
                            layout.y + tab_bar_height,
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
                }

                renderer.flush(w, h);
                surface.swap_buffers(context).unwrap();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                let super_pressed = self.modifiers.super_key();
                let shift_pressed = self.modifiers.shift_key();

                // Handle tab keybindings (Cmd+...)
                if super_pressed {
                    match event.logical_key {
                        // Cmd+T: New tab
                        Key::Character(ref s) if s == "t" => {
                            let (cols, rows) = self.grid_size();
                            let cw = self.renderer.as_ref().map(|r| r.cell_width()).unwrap_or(8.0);
                            let ch = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(18.0);
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.add_tab(cols, rows, cw, ch, &self.event_proxy);
                                log::info!("New tab (total: {})", tab_manager.count());
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
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.split_active(
                                    panes::Split::Vertical,
                                    cols, rows, cw, ch, &self.event_proxy,
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
                            if let Some(tab_manager) = &mut self.tab_manager {
                                tab_manager.split_active(
                                    panes::Split::Horizontal,
                                    cols, rows, cw, ch, &self.event_proxy,
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
                        _ => {}
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
                    Key::Character(ref s) => {
                        Some(Cow::Owned(s.as_bytes().to_vec()))
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: KoiEvent) {
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
            KoiEvent::ChildExit(code) => {
                log::info!("Child process exited with code {}", code);
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
