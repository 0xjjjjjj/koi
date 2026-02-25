pub mod atlas;
pub mod glyph_cache;
pub mod rects;
pub mod shader;
pub mod text;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use glyph_cache::GlyphCache;
use rects::{RectInstance, RectRenderer};
use text::{GlyphInstance, TextRenderer};

/// Terminal color theme.
#[derive(Clone)]
pub struct Theme {
    pub colors: [[f32; 3]; 16],
    pub fg: [f32; 3],
    pub bg: [f32; 3],
    pub surface0: [f32; 3],     // inactive tab bg
    pub overlay0: [f32; 3],     // divider/separator
    pub cursor: [f32; 3],       // cursor block
    pub selection: [f32; 4],    // selection highlight
    pub border: [f32; 4],       // active pane border
}

impl Theme {
    pub fn latte() -> Self {
        Self {
            colors: [
                [0.267, 0.278, 0.353], // black   #434556
                [0.820, 0.176, 0.243], // red     #d20f3f
                [0.247, 0.627, 0.169], // green   #40a02b
                [0.875, 0.569, 0.000], // yellow  #df9100
                [0.118, 0.400, 0.949], // blue    #1e66f2
                [0.533, 0.259, 0.949], // magenta #8842f2
                [0.008, 0.596, 0.533], // cyan    #029888
                [0.675, 0.694, 0.745], // white   #acb0be
                [0.427, 0.443, 0.518], // bright black  #6c7086
                [0.820, 0.176, 0.243], // bright red    #d20f3f
                [0.247, 0.627, 0.169], // bright green  #40a02b
                [0.875, 0.569, 0.000], // bright yellow #df9100
                [0.118, 0.400, 0.949], // bright blue   #1e66f2
                [0.533, 0.259, 0.949], // bright magenta#8842f2
                [0.008, 0.596, 0.533], // bright cyan   #029888
                [0.675, 0.694, 0.745], // bright white  #acb0be
            ],
            fg: [0.298, 0.310, 0.412],       // #4c4f69
            bg: [0.937, 0.945, 0.961],       // #eff1f5
            surface0: [0.800, 0.816, 0.855], // #ccd0da
            overlay0: [0.725, 0.745, 0.792], // #b9bece (separators)
            cursor: [0.298, 0.310, 0.412],   // same as fg
            selection: [0.122, 0.471, 0.706, 0.3],
            border: [0.122, 0.471, 0.706, 1.0],
        }
    }

    pub fn mocha() -> Self {
        Self {
            colors: [
                [0.180, 0.192, 0.243], // black   #45475a (surface1)
                [0.953, 0.545, 0.659], // red     #f38ba8
                [0.651, 0.890, 0.631], // green   #a6e3a1
                [0.976, 0.886, 0.686], // yellow  #f9e2af
                [0.537, 0.706, 0.980], // blue    #89b4fa
                [0.796, 0.651, 0.969], // magenta #cba6f7
                [0.580, 0.886, 0.878], // cyan    #94e2d5
                [0.706, 0.733, 0.827], // white   #bac2de (subtext1)
                [0.384, 0.408, 0.506], // bright black  #585b70 (surface2)
                [0.953, 0.545, 0.659], // bright red    #f38ba8
                [0.651, 0.890, 0.631], // bright green  #a6e3a1
                [0.976, 0.886, 0.686], // bright yellow #f9e2af
                [0.537, 0.706, 0.980], // bright blue   #89b4fa
                [0.796, 0.651, 0.969], // bright magenta#cba6f7
                [0.580, 0.886, 0.878], // bright cyan   #94e2d5
                [0.804, 0.827, 0.906], // bright white  #a6adc8 (subtext0)
            ],
            fg: [0.804, 0.839, 0.957],       // #cdd6f4 (text)
            bg: [0.118, 0.118, 0.180],       // #1e1e2e (base)
            surface0: [0.192, 0.200, 0.275], // #313244
            overlay0: [0.427, 0.443, 0.537], // #6c7086
            cursor: [0.804, 0.839, 0.957],   // same as fg
            selection: [0.537, 0.706, 0.980, 0.3],
            border: [0.537, 0.706, 0.980, 1.0],
        }
    }

    pub fn fg4(&self) -> [f32; 4] {
        [self.fg[0], self.fg[1], self.fg[2], 1.0]
    }

    pub fn bg4(&self) -> [f32; 4] {
        [self.bg[0], self.bg[1], self.bg[2], 1.0]
    }
}

pub struct Renderer {
    pub glyph_cache: GlyphCache,
    text_renderer: TextRenderer,
    rect_renderer: RectRenderer,
    pub theme: Theme,
}

impl Renderer {
    pub fn new(font_family: &str, font_size: f32, scale: f32) -> Self {
        Self::with_theme(font_family, font_size, scale, Theme::latte())
    }

    pub fn with_theme(font_family: &str, font_size: f32, scale: f32, theme: Theme) -> Self {
        // Rasterize at physical pixel size so glyphs are sharp on HiDPI/Retina.
        let glyph_cache = GlyphCache::new(font_family, font_size * scale);
        let text_renderer = TextRenderer::new();
        let rect_renderer = RectRenderer::new();

        Renderer {
            glyph_cache,
            text_renderer,
            rect_renderer,
            theme,
        }
    }

    pub fn cell_width(&self) -> f32 {
        self.glyph_cache.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.glyph_cache.cell_height
    }

    /// Draw a solid colored rectangle.
    pub fn draw_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.rect_renderer.add(RectInstance {
            x,
            y,
            w,
            h,
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        });
    }

    /// Draw a string at pixel position (x, y) with given colors.
    pub fn draw_string(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        fg: [f32; 4],
        bg: [f32; 4],
    ) {
        let cw = self.glyph_cache.cell_width;
        let ch = self.glyph_cache.cell_height;
        let descent = self.glyph_cache.descent;

        for (i, c) in text.chars().enumerate() {
            let cell_x = x + i as f32 * cw;
            let cell_y = y;

            // Background
            self.draw_rect(cell_x, cell_y, cw, ch, bg);

            if c == ' ' {
                continue;
            }

            // Glyph (tab bar: always regular weight)
            let glyph = self.glyph_cache.get_glyph(c, false, false);
            if glyph.width > 0.0 {
                let gx = cell_x + glyph.left;
                let gy = cell_y + ch + descent - glyph.top;

                self.text_renderer.add(GlyphInstance {
                    x: gx,
                    y: gy,
                    w: glyph.width,
                    h: glyph.height,
                    uv_x: glyph.uv_x,
                    uv_y: glyph.uv_y,
                    uv_w: glyph.uv_w,
                    uv_h: glyph.uv_h,
                    r: fg[0],
                    g: fg[1],
                    b: fg[2],
                    a: fg[3],
                });
            }
        }
    }

    /// Draw the tab bar at the top of the window.
    pub fn draw_tab_bar(&mut self, tab_manager: &crate::tabs::TabManager, width: f32) {
        let ch = self.glyph_cache.cell_height;
        let count = tab_manager.count();
        let tab_width = width / count as f32;

        let active_bg = self.theme.bg4();
        let inactive_bg = [self.theme.surface0[0], self.theme.surface0[1], self.theme.surface0[2], 1.0];
        let fg = self.theme.fg4();

        for (i, tab) in tab_manager.iter().enumerate() {
            let x = i as f32 * tab_width;
            let is_active = i == tab_manager.active_index();
            let bg = if is_active { active_bg } else { inactive_bg };

            // Tab background
            self.draw_rect(x, 0.0, tab_width, ch, bg);

            // Tab title
            let title = &tab.title;
            let padding = 8.0;
            self.draw_string(x + padding, 0.0, title, fg, bg);

            // Separator between tabs
            if i < count - 1 {
                let sep = [self.theme.overlay0[0], self.theme.overlay0[1], self.theme.overlay0[2], 1.0];
                self.draw_rect(x + tab_width - 1.0, 0.0, 1.0, ch, sep);
            }
        }
    }

    /// Draw the terminal grid from alacritty_terminal state.
    pub fn draw_grid<T: EventListener>(
        &mut self,
        term: &Term<T>,
        offset_x: f32,
        offset_y: f32,
        show_cursor: bool,
    ) {
        let cw = self.glyph_cache.cell_width;
        let ch = self.glyph_cache.cell_height;
        let descent = self.glyph_cache.descent;

        let content = term.renderable_content();
        // display_offset > 0 means we're scrolled into scrollback history.
        // display_iter yields Line(-display_offset) as the topmost visible row.
        // Normalize to 0-based viewport rows by adding display_offset.
        let display_offset = content.display_offset as i32;
        let in_scrollback = display_offset > 0;

        for indexed in content.display_iter {
            let col = indexed.point.column.0;
            let line = indexed.point.line.0;
            let row = (line + display_offset) as f32;

            let cell_x = offset_x + col as f32 * cw;
            let cell_y = offset_y + row * ch;

            let cell = &indexed.cell;

            // Skip spacer cells for wide characters (already drawn by the wide cell).
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            // Wide chars (CJK, emoji) occupy two columns.
            let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
            let draw_cw = if is_wide { cw * 2.0 } else { cw };

            // Resolve colors, respecting INVERSE flag.
            let (mut fg_color, mut bg_color) = (
                self.resolve_color(&cell.fg),
                self.resolve_color(&cell.bg),
            );

            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg_color, &mut bg_color);
            }

            // Background — skip if it matches the theme's BG.
            let is_default_bg = (bg_color[0] - self.theme.bg[0]).abs() < 1e-4
                && (bg_color[1] - self.theme.bg[1]).abs() < 1e-4
                && (bg_color[2] - self.theme.bg[2]).abs() < 1e-4;
            if !is_default_bg {
                self.draw_rect(cell_x, cell_y, draw_cw, ch, bg_color);
            }

            // Selection highlight
            if let Some(ref sel) = content.selection {
                let point = alacritty_terminal::index::Point::new(
                    indexed.point.line,
                    indexed.point.column,
                );
                if sel.contains(point) {
                    self.draw_rect(cell_x, cell_y, draw_cw, ch, self.theme.selection);
                }
            }

            let c = cell.c;
            if c == ' ' || c == '\t' {
                continue;
            }

            // Bold brightness boost
            let fg = if cell.flags.contains(Flags::BOLD) {
                [
                    (fg_color[0] * 1.15).min(1.0),
                    (fg_color[1] * 1.15).min(1.0),
                    (fg_color[2] * 1.15).min(1.0),
                    fg_color[3],
                ]
            } else if cell.flags.contains(Flags::DIM) {
                [
                    fg_color[0] * 0.66,
                    fg_color[1] * 0.66,
                    fg_color[2] * 0.66,
                    fg_color[3],
                ]
            } else {
                fg_color
            };

            let glyph = self.glyph_cache.get_glyph(
                c,
                cell.flags.contains(Flags::BOLD),
                cell.flags.contains(Flags::ITALIC),
            );
            if glyph.width > 0.0 {
                let gx = cell_x + glyph.left;
                let gy = cell_y + ch + descent - glyph.top;

                self.text_renderer.add(GlyphInstance {
                    x: gx,
                    y: gy,
                    w: glyph.width,
                    h: glyph.height,
                    uv_x: glyph.uv_x,
                    uv_y: glyph.uv_y,
                    uv_w: glyph.uv_w,
                    uv_h: glyph.uv_h,
                    r: fg[0],
                    g: fg[1],
                    b: fg[2],
                    a: fg[3],
                });
            }
        }

        // Draw cursor — hide when scrolled into history (cursor is below viewport).
        if show_cursor && !in_scrollback {
            let cursor = content.cursor;
            let cursor_x = offset_x + cursor.point.column.0 as f32 * cw;
            let cursor_y =
                offset_y + (cursor.point.line.0 + display_offset) as f32 * ch;
            self.draw_rect(cursor_x, cursor_y, cw, ch,
                [self.theme.cursor[0], self.theme.cursor[1], self.theme.cursor[2], 0.7]);
        }
    }

    /// Dim a base ANSI color (indices 0-7) by 0.66.
    fn dim_color(&self, base_idx: usize) -> [f32; 4] {
        let c = self.theme.colors[base_idx];
        [c[0] * 0.66, c[1] * 0.66, c[2] * 0.66, 1.0]
    }

    /// Convert vte::ansi::Color to [f32; 4] RGBA.
    fn resolve_color(&self, color: &Color) -> [f32; 4] {
        match color {
            Color::Named(named) => {
                let idx = *named as usize;
                if idx < 16 {
                    let c = self.theme.colors[idx];
                    [c[0], c[1], c[2], 1.0]
                } else {
                    match named {
                        NamedColor::Foreground | NamedColor::BrightForeground => {
                            self.theme.fg4()
                        }
                        NamedColor::Background => {
                            self.theme.bg4()
                        }
                        NamedColor::Cursor => {
                            self.theme.fg4()
                        }
                        NamedColor::DimForeground => {
                            let d = 0.66;
                            [self.theme.fg[0] * d, self.theme.fg[1] * d, self.theme.fg[2] * d, 1.0]
                        }
                        // Dim variants: darken the base color by 0.66
                        NamedColor::DimBlack => self.dim_color(0),
                        NamedColor::DimRed => self.dim_color(1),
                        NamedColor::DimGreen => self.dim_color(2),
                        NamedColor::DimYellow => self.dim_color(3),
                        NamedColor::DimBlue => self.dim_color(4),
                        NamedColor::DimMagenta => self.dim_color(5),
                        NamedColor::DimCyan => self.dim_color(6),
                        NamedColor::DimWhite => self.dim_color(7),
                        _ => self.theme.fg4(),
                    }
                }
            }
            Color::Spec(rgb) => {
                [
                    rgb.r as f32 / 255.0,
                    rgb.g as f32 / 255.0,
                    rgb.b as f32 / 255.0,
                    1.0,
                ]
            }
            Color::Indexed(idx) => {
                if (*idx as usize) < 16 {
                    let c = self.theme.colors[*idx as usize];
                    [c[0], c[1], c[2], 1.0]
                } else {
                    // 256-color: convert index to RGB
                    let rgb = index_to_rgb(*idx);
                    [rgb[0], rgb[1], rgb[2], 1.0]
                }
            }
        }
    }

    /// Draw a rectangular border (4 thin rects forming the edges).
    pub fn draw_pane_border(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thickness: f32,
        color: [f32; 4],
    ) {
        // Top edge
        self.draw_rect(x, y, w, thickness, color);
        // Bottom edge
        self.draw_rect(x, y + h - thickness, w, thickness, color);
        // Left edge
        self.draw_rect(x, y + thickness, thickness, h - 2.0 * thickness, color);
        // Right edge
        self.draw_rect(x + w - thickness, y + thickness, thickness, h - 2.0 * thickness, color);
    }

    /// Flush all batched draw calls.
    pub fn flush(&mut self, width: f32, height: f32) {
        // Backgrounds first (no blending)
        self.rect_renderer.flush(width, height);
        // Glyphs on top (with alpha blending)
        let tex_id = self.glyph_cache.atlas_tex_id();
        self.text_renderer.flush(tex_id, width, height);
    }
}

/// Map a single 6-level color-cube axis value (0-5) to its xterm byte value.
/// xterm uses: [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff]
/// which is: if v == 0 { 0 } else { 55 + v * 40 }.
fn cube_component(v: u8) -> u8 {
    if v == 0 { 0 } else { 55 + v * 40 }
}

/// Convert 256-color index (16-255) to RGB floats using the standard xterm palette.
fn index_to_rgb(idx: u8) -> [f32; 3] {
    if idx < 16 {
        // Should not reach here, handled by LATTE_COLORS
        return [0.5, 0.5, 0.5];
    }
    if idx < 232 {
        // Color cube: 6x6x6 — each axis maps through cube_component()
        let i = idx - 16;
        let r = cube_component(i / 36);
        let g = cube_component((i % 36) / 6);
        let b = cube_component(i % 6);
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
    } else {
        // Grayscale ramp: 24 shades, 8 + index * 10
        let level = 8 + (idx - 232) as u16 * 10;
        let v = level as f32 / 255.0;
        [v, v, v]
    }
}
