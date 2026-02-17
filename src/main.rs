mod gl;

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

struct Koi {
    window: Option<Window>,
    gl_context: Option<glutin::context::PossiblyCurrentContext>,
    gl_surface: Option<glutin::surface::Surface<WindowSurface>>,
}

impl Koi {
    fn new() -> Self {
        Self {
            window: None,
            gl_context: None,
            gl_surface: None,
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
            let renderer = std::ffi::CStr::from_ptr(gl::GetString(gl::RENDERER) as *const _);
            log::info!("OpenGL version: {:?}", version);
            log::info!("GPU renderer: {:?}", renderer);
        }

        // Clear to Catppuccin Latte base color (#eff1f5)
        unsafe {
            gl::ClearColor(0.937, 0.945, 0.961, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        gl_surface.swap_buffers(&gl_context).unwrap();

        self.window = Some(window);
        self.gl_context = Some(gl_context);
        self.gl_surface = Some(gl_surface);
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
                if let (Some(surface), Some(context)) =
                    (&self.gl_surface, &self.gl_context)
                {
                    unsafe {
                        gl::Clear(gl::COLOR_BUFFER_BIT);
                    }
                    surface.swap_buffers(context).unwrap();
                }
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
