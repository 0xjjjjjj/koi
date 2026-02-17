use std::collections::HashMap;

use crossfont::{
    BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight,
};

use super::atlas::{Atlas, Glyph};

pub struct GlyphCache {
    rasterizer: Rasterizer,
    font_key: FontKey,
    cache: HashMap<GlyphKey, Glyph>,
    atlas: Atlas,
    pub cell_width: f32,
    pub cell_height: f32,
    pub descent: f32,
}

impl GlyphCache {
    pub fn new(font_family: &str, font_size: f32) -> Self {
        let mut rasterizer = Rasterizer::new().expect("create rasterizer");
        let size = Size::new(font_size);

        let font_desc = FontDesc::new(
            font_family,
            Style::Description {
                slant: Slant::Normal,
                weight: Weight::Normal,
            },
        );

        let font_key = rasterizer
            .load_font(&font_desc, size)
            .expect("load font");

        let metrics = rasterizer.metrics(font_key, size).expect("font metrics");
        let cell_width = metrics.average_advance;
        let cell_height = metrics.line_height;
        let descent = metrics.descent;

        log::info!(
            "Font loaded: {}pt, cell={}x{}, descent={}",
            font_size,
            cell_width,
            cell_height,
            descent
        );

        GlyphCache {
            rasterizer,
            font_key,
            cache: HashMap::new(),
            atlas: Atlas::new(1024),
            cell_width: cell_width as f32,
            cell_height: cell_height as f32,
            descent: descent as f32,
        }
    }

    pub fn atlas_tex_id(&self) -> u32 {
        self.atlas.tex_id()
    }

    pub fn get_glyph(&mut self, c: char) -> Glyph {
        let key = GlyphKey {
            font_key: self.font_key,
            character: c,
            size: crossfont::Size::new(0.), // size is already set on font
        };

        if let Some(&glyph) = self.cache.get(&key) {
            return glyph;
        }

        let rasterized = match self.rasterizer.get_glyph(key) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Failed to rasterize '{}': {}", c, e);
                return Glyph {
                    tex_id: 0,
                    width: 0.0,
                    height: 0.0,
                    left: 0.0,
                    top: 0.0,
                    uv_x: 0.0,
                    uv_y: 0.0,
                    uv_w: 0.0,
                    uv_h: 0.0,
                };
            }
        };

        let buffer: Vec<u8> = match &rasterized.buffer {
            BitmapBuffer::Rgb(data) => {
                // Keep RGB channels for subpixel LCD antialiasing
                data.clone()
            }
            BitmapBuffer::Rgba(data) => {
                // Extract RGB channels (drop alpha) for subpixel rendering
                data.chunks(4)
                    .flat_map(|rgba| &rgba[..3])
                    .copied()
                    .collect()
            }
        };

        let glyph = match self.atlas.insert(
            rasterized.width as i32,
            rasterized.height as i32,
            &buffer,
            rasterized.left as f32,
            rasterized.top as f32,
        ) {
            Some(g) => g,
            None => {
                // Atlas full — return invisible glyph
                log::warn!("Glyph atlas full, cannot render '{}'", c);
                return Glyph {
                    tex_id: 0,
                    width: 0.0,
                    height: 0.0,
                    left: 0.0,
                    top: 0.0,
                    uv_x: 0.0,
                    uv_y: 0.0,
                    uv_w: 0.0,
                    uv_h: 0.0,
                };
            }
        };

        self.cache.insert(key, glyph);
        glyph
    }
}
