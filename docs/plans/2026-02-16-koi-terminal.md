# Koi Terminal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a GPU-accelerated Rust terminal emulator with native tabs and pane splitting, using Alacritty's terminal engine and an OpenGL renderer adapted from Alacritty's source.

**Architecture:** Koi uses `alacritty_terminal` for VT emulation and PTY management. The renderer adapts Alacritty's GLSL3 instanced-rendering pipeline (glyph atlas + textured quads). Tabs and panes are managed in-process with a GL-rendered tab bar. Each tab contains a tree of panes, each pane owns a `Term` instance and PTY on a dedicated IO thread.

**Tech Stack:** Rust, alacritty_terminal 0.25.1, glutin 0.32.2, winit 0.30.9, crossfont 0.8.1, OpenGL 3.3

**Reference:** Alacritty source (MIT) at github.com/alacritty/alacritty - specifically `alacritty/src/renderer/` and `alacritty/src/display/`.

---

## Task 1: Project Scaffolding + Window

**Goal:** Cargo project that opens a blank window with an OpenGL context.

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/gl.rs` (OpenGL bindings)

**Step 1: Create Cargo.toml with pinned dependencies**

```toml
[package]
name = "koi"
version = "0.1.0"
edition = "2021"

[dependencies]
alacritty_terminal = "0.25.1"
crossfont = "0.8.0"
glutin = "0.32.2"
glutin-winit = "0.5.0"
winit = "0.30.9"
raw-window-handle = "0.6"
log = "0.4"
env_logger = "0.11"
bitflags = "2"
unicode-width = "0.2"
parking_lot = "0.12"

[dependencies.gl]
package = "gl_generator"
version = "0.14"
optional = true

[build-dependencies]
gl_generator = "0.14"
```

**Step 2: Create build.rs for OpenGL bindings**

```rust
// build.rs
use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() {
    let dest = env::var("OUT_DIR").unwrap();
    let mut file = File::create(Path::new(&dest).join("gl_bindings.rs")).unwrap();
    Registry::new(Api::Gl, (3, 3), Profile::Core, Fallbacks::All, [])
        .write_bindings(GlobalGenerator, &mut file)
        .unwrap();
}
```

**Step 3: Create src/gl.rs**

```rust
#![allow(clippy::all)]
include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
```

**Step 4: Create src/main.rs with basic window**

```rust
mod gl;

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
            .build(Some(window.window_handle().unwrap().into()));

        let gl_context = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .unwrap()
        };

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            window.window_handle().unwrap().into(),
            window.inner_size().width.try_into().unwrap(),
            window.inner_size().height.try_into().unwrap(),
        );

        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attrs)
                .unwrap()
        };

        let gl_context = gl_context.make_current(&gl_surface).unwrap();

        // Load GL function pointers
        gl::load_with(|symbol| {
            let symbol = std::ffi::CString::new(symbol).unwrap();
            gl_display.get_proc_address(symbol.as_c_str()).cast()
        });

        // Clear to Catppuccin Latte base color
        unsafe {
            gl::ClearColor(0.937, 0.945, 0.961, 1.0); // #eff1f5
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
```

**Step 5: Build and verify window opens**

Run: `cd ~/git/koi && cargo build 2>&1 | tail -5`
Expected: Compiles successfully (many deps, ~60s first build)

Run: `cd ~/git/koi && cargo run`
Expected: A window opens with Catppuccin Latte background (#eff1f5)

**Step 6: Commit**

```bash
cd ~/git/koi && git init && git add -A && git commit -m "feat: project scaffolding with OpenGL window"
```

---

## Task 2: Glyph Atlas + Text Renderer

**Goal:** Render a static string using OpenGL instanced rendering with a glyph atlas. This is the hardest task - everything after this builds on it.

**Files:**
- Create: `src/renderer/mod.rs`
- Create: `src/renderer/atlas.rs`
- Create: `src/renderer/glyph_cache.rs`
- Create: `src/renderer/shader.rs`
- Create: `src/renderer/text.rs`
- Create: `src/renderer/shaders/text.v.glsl`
- Create: `src/renderer/shaders/text.f.glsl`
- Modify: `src/main.rs`
- Modify: `src/gl.rs`

**Approach:** Adapt Alacritty's renderer/text/glsl3.rs (instanced rendering), renderer/text/atlas.rs (glyph packing), and shaders. Simplify by dropping GLES2 fallback - GLSL3 only.

**Step 1: Create GLSL vertex shader** (`src/renderer/shaders/text.v.glsl`)

Adapted from Alacritty's text shader. Each instance = one terminal cell with position, glyph UV coords, colors.

```glsl
#version 330 core

// Cell dimensions uniform
uniform vec2 cellDim;
uniform vec4 projection;

// Per-vertex (quad corners)
in vec2 aPos;

// Per-instance (one per cell)
in vec2 gridCoords;    // column, row
in vec4 glyph;         // x, y offset + width, height in atlas
in vec4 uv;            // u, v + u_width, v_height
in vec4 fg;            // foreground RGBA
in vec4 bg;            // background RGBA
in uint flags;         // rendering flags

flat out vec4 vFg;
flat out vec4 vBg;
out vec2 vUV;
flat out uint vFlags;

void main() {
    vec2 cellPos = cellDim * gridCoords;

    // Background quad fills entire cell
    // Glyph quad uses glyph metrics
    bool isBackground = (flags & 1u) != 0u;

    vec2 pos;
    if (isBackground) {
        pos = cellPos + aPos * cellDim;
        vUV = vec2(0.0);
    } else {
        vec2 glyphOffset = glyph.xy;
        vec2 glyphSize = glyph.zw;
        pos = cellPos + glyphOffset + aPos * glyphSize;
        vUV = uv.xy + aPos * uv.zw;
    }

    // Apply projection (ortho: 2/width, 2/height, offset)
    vec2 projected = pos * projection.xy + projection.zw;
    gl_Position = vec4(projected, 0.0, 1.0);

    vFg = fg;
    vBg = bg;
    vFlags = flags;
}
```

**Step 2: Create GLSL fragment shader** (`src/renderer/shaders/text.f.glsl`)

```glsl
#version 330 core

uniform sampler2D atlas;

flat in vec4 vFg;
flat in vec4 vBg;
in vec2 vUV;
flat in uint vFlags;

out vec4 FragColor;

void main() {
    bool isBackground = (vFlags & 1u) != 0u;

    if (isBackground) {
        FragColor = vBg;
    } else {
        float a = texture(atlas, vUV).r;
        FragColor = vec4(vFg.rgb, vFg.a * a);
    }
}
```

**Step 3: Create shader loader** (`src/renderer/shader.rs`)

Compile vertex + fragment shaders, link program, query uniform/attribute locations.

**Step 4: Create glyph atlas** (`src/renderer/atlas.rs`)

Row-based glyph packing into GL textures. Adapted from Alacritty's atlas.rs (~320 lines → ~250 lines simplified).

**Step 5: Create glyph cache** (`src/renderer/glyph_cache.rs`)

Wraps crossfont rasterizer. Loads IBM Plex Mono, rasterizes glyphs on demand, inserts into atlas. HashMap<GlyphKey, Glyph> for cache.

**Step 6: Create text renderer** (`src/renderer/text.rs`)

Instanced renderer: one VBO for quad vertices (4 corners), one VBO for per-instance data (grid coords, glyph metrics, colors). Batch cells, upload to GPU, draw with glDrawElementsInstanced.

**Step 7: Create renderer coordinator** (`src/renderer/mod.rs`)

```rust
pub mod atlas;
pub mod glyph_cache;
pub mod shader;
pub mod text;

pub struct Renderer {
    pub text: text::TextRenderer,
    pub glyph_cache: glyph_cache::GlyphCache,
}

impl Renderer {
    pub fn new(font_size: f32) -> Self { /* init GL state, load font */ }
    pub fn draw_string(&mut self, col: usize, row: usize, text: &str, fg: [f32; 4], bg: [f32; 4]) { /* test method */ }
    pub fn flush(&mut self) { /* upload batch + draw */ }
}
```

**Step 8: Wire into main.rs, render "Hello from Koi"**

In the RedrawRequested handler, call renderer.draw_string() and renderer.flush().

**Step 9: Build and verify text renders**

Run: `cargo run`
Expected: Window shows "Hello from Koi" in IBM Plex Mono on Catppuccin Latte background

**Step 10: Commit**

```bash
git add -A && git commit -m "feat: OpenGL text renderer with glyph atlas"
```

---

## Task 3: Terminal Emulation + PTY

**Goal:** Connect alacritty_terminal to the renderer. Type commands, see output.

**Files:**
- Create: `src/terminal.rs`
- Create: `src/pty.rs`
- Create: `src/event.rs`
- Modify: `src/main.rs`
- Modify: `src/renderer/mod.rs`

**Step 1: Create event types** (`src/event.rs`)

```rust
use alacritty_terminal::event::EventListener;

#[derive(Clone)]
pub struct JsonEventProxy;

impl EventListener for JsonEventProxy {
    fn send_event(&self, _event: alacritty_terminal::event::Event) {}
}
```

**Step 2: Create terminal wrapper** (`src/terminal.rs`)

```rust
use alacritty_terminal::term::Term;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::vte::ansi::Handler;

pub struct Terminal {
    pub term: Term<JsonEventProxy>,
}

impl Terminal {
    pub fn new(cols: u16, rows: u16) -> Self {
        let config = TermConfig::default();
        let size = alacritty_terminal::term::cell::Dimensions::new(cols as usize, rows as usize);
        let term = Term::new(config, &size, JsonEventProxy);
        Self { term }
    }
}
```

**Step 3: Create PTY manager** (`src/pty.rs`)

Spawn default shell, create PTY, wire reader thread that feeds bytes into `term.advance()`.

```rust
use std::io::{Read, Write};
use std::sync::Arc;
use parking_lot::Mutex;

pub struct Pty {
    writer: Box<dyn Write + Send>,
    // reader runs on dedicated thread, feeds Term
}

impl Pty {
    pub fn new(cols: u16, rows: u16, term: Arc<Mutex<Term>>) -> Self {
        // Use alacritty_terminal::tty to spawn PTY
        // Spawn reader thread
    }

    pub fn write(&mut self, data: &[u8]) {
        self.writer.write_all(data).ok();
    }
}
```

**Step 4: Add grid rendering to renderer**

Replace draw_string with draw_grid that iterates `term.renderable_content()` cells:

```rust
pub fn draw_grid(&mut self, term: &Term<impl EventListener>) {
    let content = term.renderable_content();
    for cell in content.display_iter {
        let fg = color_to_rgba(cell.fg);
        let bg = color_to_rgba(cell.bg);
        self.text.add_cell(cell.point.column, cell.point.line, cell.character, fg, bg);
    }
    self.text.flush();
}
```

**Step 5: Wire terminal + PTY into main event loop**

- On keyboard input: write to PTY
- On PTY output (via channel): mark dirty, request redraw
- On redraw: render grid from Term

**Step 6: Build and verify working terminal**

Run: `cargo run`
Expected: Shell prompt appears. You can type commands and see output.

**Step 7: Commit**

```bash
git add -A && git commit -m "feat: terminal emulation with PTY"
```

---

## Task 4: Tab Manager

**Goal:** Multiple terminal sessions in tabs. Cmd+T creates, Cmd+W closes, Cmd+Shift+[/] switches.

**Files:**
- Create: `src/tabs.rs`
- Create: `tests/tabs.rs`
- Modify: `src/main.rs`

**Step 1: Write failing test for tab manager**

```rust
// tests/tabs.rs
use koi::tabs::TabManager;

#[test]
fn test_new_tab_manager_has_one_tab() {
    let mgr = TabManager::new();
    assert_eq!(mgr.count(), 1);
    assert_eq!(mgr.active_index(), 0);
}

#[test]
fn test_add_tab() {
    let mut mgr = TabManager::new();
    mgr.add_tab(1);
    assert_eq!(mgr.count(), 2);
    assert_eq!(mgr.active_index(), 1); // new tab becomes active
}

#[test]
fn test_close_tab() {
    let mut mgr = TabManager::new();
    mgr.add_tab(1);
    mgr.close_active();
    assert_eq!(mgr.count(), 1);
    assert_eq!(mgr.active_index(), 0);
}

#[test]
fn test_close_last_tab_returns_should_quit() {
    let mut mgr = TabManager::new();
    assert!(mgr.close_active());
}

#[test]
fn test_switch_tabs() {
    let mut mgr = TabManager::new();
    mgr.add_tab(1);
    mgr.add_tab(2);
    mgr.prev_tab();
    assert_eq!(mgr.active_index(), 1);
    mgr.next_tab();
    assert_eq!(mgr.active_index(), 2);
}

#[test]
fn test_next_tab_wraps() {
    let mut mgr = TabManager::new();
    mgr.add_tab(1);
    mgr.next_tab(); // index 1 → wraps to 0
    assert_eq!(mgr.active_index(), 0);
}

#[test]
fn test_goto_tab() {
    let mut mgr = TabManager::new();
    mgr.add_tab(1);
    mgr.add_tab(2);
    mgr.goto_tab(0);
    assert_eq!(mgr.active_index(), 0);
}
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --test tabs`
Expected: Compilation error - module doesn't exist

**Step 3: Implement TabManager** (`src/tabs.rs`)

```rust
pub struct Tab {
    pub id: usize,
    pub title: String,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active: usize,
    next_id: usize,
}

impl TabManager {
    pub fn new() -> Self { /* create with one tab */ }
    pub fn add_tab(&mut self, id: usize) { /* push + activate */ }
    pub fn close_active(&mut self) -> bool { /* remove, return true if last */ }
    pub fn next_tab(&mut self) { /* wrap around */ }
    pub fn prev_tab(&mut self) { /* wrap around */ }
    pub fn goto_tab(&mut self, index: usize) { /* bounds check */ }
    pub fn count(&self) -> usize { self.tabs.len() }
    pub fn active_index(&self) -> usize { self.active }
    pub fn active_tab(&self) -> &Tab { &self.tabs[self.active] }
}
```

**Step 4: Run tests, verify they pass**

Run: `cargo test --test tabs`
Expected: All 7 tests pass

**Step 5: Wire TabManager into main**

Each tab owns a (Terminal, Pty) pair. Tab switch swaps which terminal the renderer draws.

**Step 6: Add keybindings**

In window_event handler, match keyboard input:
- `Cmd+T` → add_tab, spawn new Terminal+Pty
- `Cmd+W` → close_active, drop Terminal+Pty
- `Cmd+Shift+[` → prev_tab
- `Cmd+Shift+]` → next_tab
- `Cmd+1-9` → goto_tab

**Step 7: Build and verify tabs work**

Run: `cargo run`
Expected: Cmd+T opens new shell tab. Cmd+Shift+[/] switches. Cmd+W closes.

**Step 8: Commit**

```bash
git add -A && git commit -m "feat: tab manager with keybindings"
```

---

## Task 5: GL-Rendered Tab Bar

**Goal:** Visual tab bar at top of window, rendered in the GL pipeline.

**Files:**
- Create: `src/renderer/rects.rs`
- Modify: `src/renderer/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create rectangle renderer** (`src/renderer/rects.rs`)

Simple GL quad renderer for solid-color rectangles (tab backgrounds, borders, dividers). Uses a separate shader program from text.

**Step 2: Add tab bar rendering to Renderer**

```rust
pub fn draw_tab_bar(&mut self, tabs: &TabManager, width: f32) {
    let bar_height = self.cell_height; // one row tall
    let tab_width = width / tabs.count() as f32;

    for (i, tab) in tabs.iter().enumerate() {
        let x = i as f32 * tab_width;
        let is_active = i == tabs.active_index();

        // Background
        let bg = if is_active {
            [0.937, 0.945, 0.961, 1.0] // Latte base #eff1f5
        } else {
            [0.800, 0.816, 0.855, 1.0] // Latte surface0 #ccd0da
        };
        self.rects.draw_rect(x, 0.0, tab_width, bar_height, bg);

        // Tab title (process name)
        let title = &tab.title;
        self.draw_string(x + 8.0, 0.0, title, fg, bg);
    }
}
```

**Step 3: Offset terminal grid below tab bar**

Terminal viewport starts at y = cell_height (one row below tab bar).

**Step 4: Build and verify tab bar renders**

Run: `cargo run`
Expected: Tab bar visible at top. Active tab highlighted. New tabs appear in bar.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: GL-rendered tab bar"
```

---

## Task 6: Pane Manager

**Goal:** Split panes within tabs. Cmd+D vertical, Cmd+Shift+D horizontal, Cmd+Shift+Enter zoom.

**Files:**
- Create: `src/panes.rs`
- Create: `tests/panes.rs`
- Modify: `src/main.rs`
- Modify: `src/tabs.rs`

**Step 1: Write failing tests for pane tree**

```rust
// tests/panes.rs
use koi::panes::{PaneTree, Split};

#[test]
fn test_new_pane_tree_has_one_pane() {
    let tree = PaneTree::new(0);
    assert_eq!(tree.pane_count(), 1);
    assert_eq!(tree.active_pane_id(), 0);
}

#[test]
fn test_vertical_split() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Vertical, 1);
    assert_eq!(tree.pane_count(), 2);
    assert_eq!(tree.active_pane_id(), 1); // focus moves to new pane
}

#[test]
fn test_horizontal_split() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Horizontal, 1);
    assert_eq!(tree.pane_count(), 2);
}

#[test]
fn test_close_pane() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Vertical, 1);
    let is_last = tree.close_active();
    assert!(!is_last);
    assert_eq!(tree.pane_count(), 1);
}

#[test]
fn test_close_last_pane() {
    let tree = PaneTree::new(0);
    assert!(tree.close_active());
}

#[test]
fn test_zoom_toggle() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Vertical, 1);
    assert!(!tree.is_zoomed());
    tree.toggle_zoom();
    assert!(tree.is_zoomed());
    tree.toggle_zoom();
    assert!(!tree.is_zoomed());
}

#[test]
fn test_focus_navigation() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Vertical, 1);
    tree.focus_prev();
    assert_eq!(tree.active_pane_id(), 0);
    tree.focus_next();
    assert_eq!(tree.active_pane_id(), 1);
}

#[test]
fn test_layout_calculation() {
    let mut tree = PaneTree::new(0);
    tree.split_active(Split::Vertical, 1);
    let layouts = tree.calculate_layouts(800.0, 600.0);
    assert_eq!(layouts.len(), 2);
    // Each pane gets half the width
    assert!((layouts[0].width - 400.0).abs() < 1.0);
    assert!((layouts[1].width - 400.0).abs() < 1.0);
}
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --test panes`
Expected: Compilation error

**Step 3: Implement PaneTree** (`src/panes.rs`)

Binary tree structure where leaves are panes and internal nodes are splits:

```rust
pub enum Split { Vertical, Horizontal }

pub struct PaneLayout {
    pub pane_id: usize,
    pub x: f32, pub y: f32,
    pub width: f32, pub height: f32,
}

enum Node {
    Leaf { pane_id: usize },
    Split { split: Split, ratio: f32, left: Box<Node>, right: Box<Node> },
}

pub struct PaneTree {
    root: Node,
    active: usize,
    zoomed: bool,
}

impl PaneTree {
    pub fn new(pane_id: usize) -> Self { /* single leaf */ }
    pub fn split_active(&mut self, split: Split, new_id: usize) { /* replace leaf with split node */ }
    pub fn close_active(&mut self) -> bool { /* remove leaf, collapse parent */ }
    pub fn toggle_zoom(&mut self) { self.zoomed = !self.zoomed; }
    pub fn is_zoomed(&self) -> bool { self.zoomed }
    pub fn focus_next(&mut self) { /* cycle through leaves */ }
    pub fn focus_prev(&mut self) { /* cycle through leaves */ }
    pub fn calculate_layouts(&self, width: f32, height: f32) -> Vec<PaneLayout> {
        if self.zoomed {
            // Only return active pane, full size
        } else {
            // Recursively divide space
        }
    }
}
```

**Step 4: Run tests, verify they pass**

Run: `cargo test --test panes`
Expected: All 8 tests pass

**Step 5: Wire PaneTree into tabs**

Each Tab owns a PaneTree. Each pane ID maps to a (Terminal, Pty) pair.

**Step 6: Add pane rendering**

For each pane layout, set GL viewport to that region, render that pane's terminal grid. Draw 2px divider lines between panes.

**Step 7: Add keybindings**

- `Cmd+D` → split_active(Vertical)
- `Cmd+Shift+D` → split_active(Horizontal)
- `Cmd+Shift+Enter` → toggle_zoom
- `Cmd+Opt+Arrow` → focus directional

**Step 8: Build and verify panes work**

Run: `cargo run`
Expected: Cmd+D splits vertically. Each pane has independent shell. Cmd+Shift+Enter zooms.

**Step 9: Commit**

```bash
git add -A && git commit -m "feat: pane splitting with zoom"
```

---

## Task 7: Theme + Polish

**Goal:** Catppuccin Latte colors, selection rendering, cursor, scrollback, copy-on-select.

**Files:**
- Create: `src/theme.rs`
- Modify: `src/renderer/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create theme module** (`src/theme.rs`)

Hardcoded Catppuccin Latte palette. Maps alacritty_terminal color indices to RGB.

```rust
pub struct Theme {
    pub background: [f32; 4],
    pub foreground: [f32; 4],
    pub cursor: [f32; 4],
    pub selection_bg: [f32; 4],
    pub normal: [[f32; 4]; 8],
    pub bright: [[f32; 4]; 8],
    pub tab_active: [f32; 4],
    pub tab_inactive: [f32; 4],
    pub pane_border_active: [f32; 4],
    pub pane_border_inactive: [f32; 4],
}

pub fn catppuccin_latte() -> Theme {
    Theme {
        background: hex(0xeff1f5),
        foreground: hex(0x4c4f69),
        cursor: hex(0xdc8a78),
        selection_bg: hex(0xdc8a78),
        normal: [
            hex(0xbcc0cc), hex(0xd20f39), hex(0x40a02b), hex(0xdf8e1d),
            hex(0x1e66f5), hex(0xea76cb), hex(0x179299), hex(0x5c5f77),
        ],
        bright: [
            hex(0xacb0be), hex(0xd20f39), hex(0x40a02b), hex(0xdf8e1d),
            hex(0x1e66f5), hex(0xea76cb), hex(0x179299), hex(0x6c6f85),
        ],
        tab_active: hex(0xeff1f5),
        tab_inactive: hex(0xccd0da),
        pane_border_active: hex(0x1e66f5),
        pane_border_inactive: hex(0xbcc0cc),
    }
}
```

**Step 2: Add cursor rendering**

Draw cursor as a colored rect at cursor position. Block cursor (filled), beam cursor (thin line).

**Step 3: Add selection rendering**

Highlight selected cells with selection_bg color.

**Step 4: Add scrollback**

Wire mouse scroll / Shift+PageUp/Down to terminal scrollback.

**Step 5: Add copy-on-select**

When mouse selection ends, copy to system clipboard via `arboard` crate.

**Step 6: Add Option-as-Alt**

On macOS, translate Option+key to Alt escape sequence for shell compatibility.

**Step 7: Build and verify**

Run: `cargo run`
Expected: Full themed terminal with cursor, selection, scrollback, clipboard.

**Step 8: Commit**

```bash
git add -A && git commit -m "feat: Catppuccin Latte theme, cursor, selection, clipboard"
```

---

## Task 8: App Bundle + Release

**Goal:** Create macOS .app bundle, codesign, custom icon. Merge into alacritty-macos repo or new koi repo.

**Files:**
- Create: `scripts/bundle.sh`
- Create: `Info.plist`
- Modify: `Cargo.toml` (metadata)

**Step 1: Create bundle script** (`scripts/bundle.sh`)

Adapts our existing alacritty-macos install.sh:
- `cargo build --release`
- Create /Applications/Koi.app with Info.plist
- Copy binary, convert icon, codesign
- `defaults write com.koi.terminal AppleWindowTabbingMode -string never`

**Step 2: Create Info.plist**

Same structure as our Alacritty bundle but with Koi branding.

**Step 3: Generate icon**

Use our capper skill to generate a Koi-themed icon (koi fish + terminal prompt).

**Step 4: Build release binary**

Run: `cargo build --release`
Expected: ~6MB ARM binary at target/release/koi

**Step 5: Create .app and test**

Run: `./scripts/bundle.sh`
Expected: /Applications/Koi.app launches, fully functional

**Step 6: Commit and push**

```bash
git add -A && git commit -m "feat: macOS app bundle with icon and codesign"
```

---

## Execution Order + Dependencies

```
Task 1 (Window)
    ↓
Task 2 (Renderer) ← HARDEST, most time
    ↓
Task 3 (Terminal + PTY)
    ↓
Task 4 (Tabs) ←→ Task 6 (Panes)  [tests can run in parallel]
    ↓                ↓
Task 5 (Tab Bar)     ↓
    ↓                ↓
    └────────────────┘
             ↓
        Task 7 (Theme + Polish)
             ↓
        Task 8 (App Bundle)
```

**Estimated total:** ~3500 lines of Rust across 8 tasks.
