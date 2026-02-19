use std::collections::HashMap;

use crossfont::{
    BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight,
};

use super::atlas::{Atlas, Glyph};

const INITIAL_ATLAS_SIZE: i32 = 2048;
const MAX_ATLAS_SIZE: i32 = 8192;

pub struct GlyphCache {
    rasterizer: Rasterizer,
    font_key: FontKey,
    bold_key: FontKey,
    italic_key: FontKey,
    bold_italic_key: FontKey,
    cache: HashMap<GlyphKey, Glyph>,
    atlas: Atlas,
    needs_regrow: bool,
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
            .unwrap_or_else(|_| {
                log::warn!("Font '{}' not found, falling back to Menlo", font_family);
                let fallback = FontDesc::new(
                    "Menlo",
                    Style::Description {
                        slant: Slant::Normal,
                        weight: Weight::Normal,
                    },
                );
                rasterizer.load_font(&fallback, size).expect("load fallback font")
            });

        let bold_key = rasterizer
            .load_font(
                &FontDesc::new(font_family, Style::Description { slant: Slant::Normal, weight: Weight::Bold }),
                size,
            )
            .unwrap_or(font_key);

        let italic_key = rasterizer
            .load_font(
                &FontDesc::new(font_family, Style::Description { slant: Slant::Italic, weight: Weight::Normal }),
                size,
            )
            .unwrap_or(font_key);

        let bold_italic_key = rasterizer
            .load_font(
                &FontDesc::new(font_family, Style::Description { slant: Slant::Italic, weight: Weight::Bold }),
                size,
            )
            .unwrap_or(font_key);

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
            bold_key,
            italic_key,
            bold_italic_key,
            cache: HashMap::new(),
            atlas: Atlas::new(INITIAL_ATLAS_SIZE),
            needs_regrow: false,
            cell_width: (cell_width as f32).ceil(),
            cell_height: (cell_height as f32).ceil(),
            descent,
        }
    }

    pub fn atlas_tex_id(&self) -> u32 {
        self.atlas.tex_id()
    }

    /// Regrow the atlas if it filled up during the previous frame.
    /// Must be called before any draw calls to avoid mid-batch texture swaps.
    pub fn try_regrow(&mut self) {
        if !self.needs_regrow {
            return;
        }
        self.needs_regrow = false;

        let cur = self.atlas.width();
        if cur >= MAX_ATLAS_SIZE {
            log::error!(
                "Glyph atlas at max {}x{}, cannot grow further",
                cur, cur
            );
            return;
        }

        let next = (cur * 2).min(MAX_ATLAS_SIZE);
        log::warn!(
            "Glyph atlas full at {}x{}, regrowing to {}x{}",
            cur, cur, next, next
        );
        self.atlas.regrow(next);
        self.cache.clear();
    }

    pub fn get_glyph(&mut self, c: char, bold: bool, italic: bool) -> Glyph {
        let font_key = match (bold, italic) {
            (true, true) => self.bold_italic_key,
            (true, false) => self.bold_key,
            (false, true) => self.italic_key,
            (false, false) => self.font_key,
        };
        let key = GlyphKey {
            font_key,
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
                // Don't regrow mid-frame — batched glyphs already reference the
                // current atlas texture.  Flag for regrow before the next frame.
                self.needs_regrow = true;
                return Glyph {
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
