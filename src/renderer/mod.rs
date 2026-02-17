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

/// Catppuccin Latte ANSI color palette.
const LATTE_COLORS: [[f32; 3]; 16] = [
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
];

const LATTE_FG: [f32; 3] = [0.298, 0.310, 0.412]; // #4c4f69
const LATTE_BG: [f32; 3] = [0.937, 0.945, 0.961]; // #eff1f5

pub struct Renderer {
    pub glyph_cache: GlyphCache,
    text_renderer: TextRenderer,
    rect_renderer: RectRenderer,
}

impl Renderer {
    pub fn new(font_family: &str, font_size: f32) -> Self {
        let glyph_cache = GlyphCache::new(font_family, font_size);
        let text_renderer = TextRenderer::new();
        let rect_renderer = RectRenderer::new();

        Renderer {
            glyph_cache,
            text_renderer,
            rect_renderer,
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

            // Glyph
            let glyph = self.glyph_cache.get_glyph(c);
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

    /// Draw the terminal grid from alacritty_terminal state.
    pub fn draw_grid<T: EventListener>(
        &mut self,
        term: &Term<T>,
        offset_x: f32,
        offset_y: f32,
    ) {
        let cw = self.glyph_cache.cell_width;
        let ch = self.glyph_cache.cell_height;
        let descent = self.glyph_cache.descent;

        let content = term.renderable_content();

        for indexed in content.display_iter {
            let col = indexed.point.column.0;
            let line = indexed.point.line.0;
            // Lines are negative-indexed from viewport top in alacritty_terminal.
            // Line(0) is the first visible line at the top.
            let row = line as f32;

            let cell_x = offset_x + col as f32 * cw;
            let cell_y = offset_y + row * ch;

            let cell = &indexed.cell;

            // Resolve colors, respecting INVERSE flag.
            let (mut fg_color, mut bg_color) = (
                Self::resolve_color(&cell.fg),
                Self::resolve_color(&cell.bg),
            );

            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg_color, &mut bg_color);
            }

            // Background
            self.draw_rect(cell_x, cell_y, cw, ch, bg_color);

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

            let glyph = self.glyph_cache.get_glyph(c);
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

        // Draw cursor
        let cursor = content.cursor;
        let cursor_x = offset_x + cursor.point.column.0 as f32 * cw;
        let cursor_y = offset_y + cursor.point.line.0 as f32 * ch;
        // Block cursor: semi-transparent overlay
        self.draw_rect(cursor_x, cursor_y, cw, ch, [0.298, 0.310, 0.412, 0.5]);
    }

    /// Convert vte::ansi::Color to [f32; 4] RGBA.
    fn resolve_color(color: &Color) -> [f32; 4] {
        match color {
            Color::Named(named) => {
                let idx = *named as usize;
                if idx < 16 {
                    let c = LATTE_COLORS[idx];
                    [c[0], c[1], c[2], 1.0]
                } else if *named == NamedColor::Foreground {
                    [LATTE_FG[0], LATTE_FG[1], LATTE_FG[2], 1.0]
                } else if *named == NamedColor::Background {
                    [LATTE_BG[0], LATTE_BG[1], LATTE_BG[2], 1.0]
                } else {
                    // Dim/bright variants - use base fg
                    [LATTE_FG[0], LATTE_FG[1], LATTE_FG[2], 1.0]
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
                    let c = LATTE_COLORS[*idx as usize];
                    [c[0], c[1], c[2], 1.0]
                } else {
                    // 256-color: convert index to RGB
                    let rgb = index_to_rgb(*idx);
                    [rgb[0], rgb[1], rgb[2], 1.0]
                }
            }
        }
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

/// Convert 256-color index (16-255) to RGB floats.
fn index_to_rgb(idx: u8) -> [f32; 3] {
    if idx < 16 {
        // Should not reach here, handled by LATTE_COLORS
        return [0.5, 0.5, 0.5];
    }
    if idx < 232 {
        // Color cube: 6x6x6
        let idx = idx - 16;
        let b = (idx % 6) as f32;
        let g = ((idx / 6) % 6) as f32;
        let r = (idx / 36) as f32;
        [r / 5.0, g / 5.0, b / 5.0]
    } else {
        // Grayscale ramp: 24 shades
        let shade = (idx - 232) as f32;
        let v = (8.0 + shade * 10.0) / 255.0;
        [v, v, v]
    }
}
