pub mod atlas;
pub mod glyph_cache;
pub mod rects;
pub mod shader;
pub mod text;

use glyph_cache::GlyphCache;
use rects::{RectInstance, RectRenderer};
use text::{GlyphInstance, TextRenderer};

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

    /// Flush all batched draw calls.
    pub fn flush(&mut self, width: f32, height: f32) {
        // Backgrounds first (no blending)
        self.rect_renderer.flush(width, height);
        // Glyphs on top (with alpha blending)
        let tex_id = self.glyph_cache.atlas_tex_id();
        self.text_renderer.flush(tex_id, width, height);
    }
}
