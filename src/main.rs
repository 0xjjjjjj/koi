mod gl;
mod renderer;

use std::num::NonZeroU32;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};

use renderer::Renderer;

struct Koi {
    window: Option<Window>,
    gl_context: Option<glutin::context::PossiblyCurrentContext>,
    gl_surface: Option<glutin::surface::Surface<WindowSurface>>,
    renderer: Option<Renderer>,
}

impl Koi {
    fn new() -> Self {
        Self {
            window: None,
            gl_context: None,
            gl_surface: None,
            renderer: None,
        }
    }
}

impl ApplicationHandler for Koi {
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
            let renderer_str = std::ffi::CStr::from_ptr(gl::GetString(gl::RENDERER) as *const _);
            log::info!("OpenGL version: {:?}", version);
            log::info!("GPU renderer: {:?}", renderer_str);
        }

        // Create renderer after GL is initialized
        let renderer = Renderer::new("IBM Plex Mono", 14.0);
        log::info!(
            "Cell size: {}x{}",
            renderer.cell_width(),
            renderer.cell_height()
        );

        self.renderer = Some(renderer);
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
            WindowEvent::CloseRequested => event_loop.exit(),
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

                let size = window.inner_size();
                let w = size.width as f32;
                let h = size.height as f32;

                unsafe {
                    gl::Viewport(0, 0, size.width as i32, size.height as i32);
                    gl::ClearColor(0.937, 0.945, 0.961, 1.0);
                    gl::Clear(gl::COLOR_BUFFER_BIT);
                }

                // Catppuccin Latte colors
                let fg = [0.298, 0.310, 0.412, 1.0]; // #4c4f69
                let bg = [0.800, 0.816, 0.855, 1.0]; // #ccd0da

                // Test: render some text
                renderer.draw_string(10.0, 10.0, "Hello from Koi!", fg, bg);
                renderer.draw_string(
                    10.0,
                    10.0 + renderer.cell_height(),
                    "GPU-accelerated terminal emulator",
                    fg,
                    [0.937, 0.945, 0.961, 1.0], // transparent bg
                );
                renderer.draw_string(
                    10.0,
                    10.0 + renderer.cell_height() * 2.0,
                    "$ cargo build --release",
                    [0.247, 0.627, 0.169, 1.0], // green #40a02b
                    [0.937, 0.945, 0.961, 1.0],
                );

                renderer.flush(w, h);

                surface.swap_buffers(context).unwrap();
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    let mut app = Koi::new();
    event_loop.run_app(&mut app).unwrap();
}
