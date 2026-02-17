mod event;
mod gl;
mod renderer;
mod terminal;

use std::borrow::Cow;
use std::num::NonZeroU32;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty;
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
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes};

use event::{EventProxy, KoiEvent, Notifier};
use renderer::Renderer;
use terminal::TerminalSize;

struct Koi {
    window: Option<Window>,
    gl_context: Option<glutin::context::PossiblyCurrentContext>,
    gl_surface: Option<glutin::surface::Surface<WindowSurface>>,
    renderer: Option<Renderer>,
    terminal: Option<Arc<FairMutex<Term<EventProxy>>>>,
    notifier: Option<Notifier>,
    event_proxy: EventProxy,
}

impl Koi {
    fn new(event_proxy: EventProxy) -> Self {
        Self {
            window: None,
            gl_context: None,
            gl_surface: None,
            renderer: None,
            terminal: None,
            notifier: None,
            event_proxy,
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

        // Create terminal
        let term_size = TerminalSize::new(cols, rows);
        let term = Term::new(TermConfig::default(), &term_size, self.event_proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        // Create PTY
        let window_size = WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cw as u16,
            cell_height: ch as u16,
        };
        let pty = tty::new(&tty::Options::default(), window_size, 0).expect("create PTY");

        // Create PTY event loop (background thread reads PTY output)
        let pty_event_loop = PtyEventLoop::new(
            term.clone(),
            self.event_proxy.clone(),
            pty,
            false,
            false,
        )
        .expect("create PTY event loop");

        let notifier = Notifier(pty_event_loop.channel());
        let _pty_thread = pty_event_loop.spawn();

        self.renderer = Some(renderer);
        self.terminal = Some(term);
        self.notifier = Some(notifier);
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
                if let Some(notifier) = &self.notifier {
                    let _ = notifier.0.send(Msg::Shutdown);
                }
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let (Some(renderer), Some(terminal), Some(notifier)) =
                    (&self.renderer, &self.terminal, &self.notifier)
                {
                    let cw = renderer.cell_width();
                    let ch = renderer.cell_height();
                    let cols = (new_size.width as f32 / cw) as usize;
                    let rows = (new_size.height as f32 / ch) as usize;
                    let cols = cols.max(2);
                    let rows = rows.max(1);

                    let term_size = TerminalSize::new(cols, rows);
                    terminal.lock().resize(term_size);

                    notifier.send_resize(WindowSize {
                        num_lines: rows as u16,
                        num_cols: cols as u16,
                        cell_width: cw as u16,
                        cell_height: ch as u16,
                    });
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
                let Some(terminal) = &self.terminal else {
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

                // Render terminal grid
                let term = terminal.lock();
                renderer.draw_grid(&*term, 0.0, 0.0);
                drop(term);

                renderer.flush(w, h);
                surface.swap_buffers(context).unwrap();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                let Some(notifier) = &self.notifier else {
                    return;
                };

                // Convert key events to bytes for the PTY
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
                        // Handle Ctrl+key combos
                        if event.state == ElementState::Pressed {
                            Some(Cow::Owned(s.as_bytes().to_vec()))
                        } else {
                            None
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: KoiEvent) {
        match event {
            KoiEvent::Wakeup => {
                // Terminal content changed, request redraw
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
