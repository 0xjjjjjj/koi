use crate::gl;
use crate::gl::types::*;

/// A glyph stored in the atlas.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// Row-based glyph packing into an OpenGL texture.
pub struct Atlas {
    tex_id: GLuint,
    width: i32,
    height: i32,
    row_extent: i32,
    row_baseline: i32,
    row_tallest: i32,
}

impl Atlas {
    fn alloc_texture(tex_id: &mut GLuint, size: i32) {
        unsafe {
            gl::GenTextures(1, tex_id);
            gl::BindTexture(gl::TEXTURE_2D, *tex_id);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGB8 as i32,
                size,
                size,
                0,
                gl::RGB,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
        }
    }

    pub fn new(size: i32) -> Self {
        let mut tex_id: GLuint = 0;
        Self::alloc_texture(&mut tex_id, size);

        Atlas {
            tex_id,
            width: size,
            height: size,
            row_extent: 0,
            row_baseline: 0,
            row_tallest: 0,
        }
    }

    pub fn tex_id(&self) -> GLuint {
        self.tex_id
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    /// Destroy the current texture and allocate a new one at `new_size`.
    /// Resets all packing state — callers must clear their glyph caches.
    pub fn regrow(&mut self, new_size: i32) {
        unsafe { gl::DeleteTextures(1, &self.tex_id); }
        Self::alloc_texture(&mut self.tex_id, new_size);
        self.width = new_size;
        self.height = new_size;
        self.row_extent = 0;
        self.row_baseline = 0;
        self.row_tallest = 0;
    }

    /// Insert a glyph into the atlas. Returns None if atlas is full.
    pub fn insert(
        &mut self,
        glyph_width: i32,
        glyph_height: i32,
        buffer: &[u8],
        left: f32,
        top: f32,
    ) -> Option<Glyph> {
        if glyph_width == 0 || glyph_height == 0 {
            return Some(Glyph {

                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                left,
                top,
                width: 0.0,
                height: 0.0,
            });
        }

        // Check if glyph fits in current row
        if self.row_extent + glyph_width > self.width {
            // Move to next row
            self.row_baseline += self.row_tallest;
            self.row_extent = 0;
            self.row_tallest = 0;
        }

        // Check if glyph fits vertically
        if self.row_baseline + glyph_height > self.height {
            return None; // Atlas full
        }

        let x = self.row_extent;
        let y = self.row_baseline;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.tex_id);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                x,
                y,
                glyph_width,
                glyph_height,
                gl::RGB,
                gl::UNSIGNED_BYTE,
                buffer.as_ptr() as *const _,
            );
        }

        self.row_extent += glyph_width;
        if glyph_height > self.row_tallest {
            self.row_tallest = glyph_height;
        }

        let w = self.width as f32;
        let h = self.height as f32;

        Some(Glyph {
            uv_x: x as f32 / w,
            uv_y: y as f32 / h,
            uv_w: glyph_width as f32 / w,
            uv_h: glyph_height as f32 / h,
            left,
            top,
            width: glyph_width as f32,
            height: glyph_height as f32,
        })
    }
}

impl Drop for Atlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.tex_id);
        }
    }
}
