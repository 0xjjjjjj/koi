use crate::gl;
use crate::gl::types::*;

use super::shader;

const MAX_INSTANCES: usize = 30_000;

// Per-instance data: position + glyph metrics + UV + color
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GlyphInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

const VERT_SRC: &str = r#"
#version 330 core

// Quad vertex (0,0), (1,0), (0,1), (1,1)
layout(location = 0) in vec2 aQuad;

// Per-instance
layout(location = 1) in vec2 aPos;
layout(location = 2) in vec2 aSize;
layout(location = 3) in vec2 aUV;
layout(location = 4) in vec2 aUVSize;
layout(location = 5) in vec4 aColor;

uniform vec4 uProjection; // (2/w, -2/h, -1, 1)

out vec2 vUV;
flat out vec4 vColor;

void main() {
    vec2 pos = aPos + aQuad * aSize;
    vec2 clip = pos * uProjection.xy + uProjection.zw;
    gl_Position = vec4(clip, 0.0, 1.0);
    vUV = aUV + aQuad * aUVSize;
    vColor = aColor;
}
"#;

const FRAG_SRC: &str = r#"
#version 330 core

uniform sampler2D uAtlas;

in vec2 vUV;
flat in vec4 vColor;

layout(location = 0, index = 0) out vec4 FragColor;
layout(location = 0, index = 1) out vec4 BlendFactor;

void main() {
    vec3 coverage = texture(uAtlas, vUV).rgb;
    FragColor = vec4(vColor.rgb * coverage, 1.0);
    BlendFactor = vec4(coverage, 1.0);
}
"#;

pub struct TextRenderer {
    program: GLuint,
    vao: GLuint,
    quad_vbo: GLuint,
    instance_vbo: GLuint,
    loc_projection: GLint,
    batch: Vec<GlyphInstance>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let vs = shader::compile_shader(VERT_SRC, gl::VERTEX_SHADER);
        let fs = shader::compile_shader(FRAG_SRC, gl::FRAGMENT_SHADER);
        let program = shader::link_program(vs, fs);
        let loc_projection = shader::get_uniform_location(program, "uProjection");

        let mut vao = 0;
        let mut quad_vbo = 0;
        let mut instance_vbo = 0;

        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);

            // Quad vertices (triangle strip: 0,0 -> 1,0 -> 0,1 -> 1,1)
            let quad: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            gl::GenBuffers(1, &mut quad_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, quad_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&quad) as isize,
                quad.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());

            // Instance VBO
            let stride = std::mem::size_of::<GlyphInstance>() as i32;
            gl::GenBuffers(1, &mut instance_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, instance_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (MAX_INSTANCES * std::mem::size_of::<GlyphInstance>()) as isize,
                std::ptr::null(),
                gl::DYNAMIC_DRAW,
            );

            let mut offset = 0isize;
            // location 1: aPos (x, y)
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(1, 1);
            offset += 8;

            // location 2: aSize (w, h)
            gl::EnableVertexAttribArray(2);
            gl::VertexAttribPointer(2, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(2, 1);
            offset += 8;

            // location 3: aUV (uv_x, uv_y)
            gl::EnableVertexAttribArray(3);
            gl::VertexAttribPointer(3, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(3, 1);
            offset += 8;

            // location 4: aUVSize (uv_w, uv_h)
            gl::EnableVertexAttribArray(4);
            gl::VertexAttribPointer(4, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(4, 1);
            offset += 8;

            // location 5: aColor (r, g, b, a)
            gl::EnableVertexAttribArray(5);
            gl::VertexAttribPointer(5, 4, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(5, 1);

            gl::BindVertexArray(0);
        }

        TextRenderer {
            program,
            vao,
            quad_vbo,
            instance_vbo,
            loc_projection,
            batch: Vec::with_capacity(MAX_INSTANCES),
        }
    }

    pub fn add(&mut self, instance: GlyphInstance) {
        if self.batch.len() < MAX_INSTANCES {
            self.batch.push(instance);
        }
    }

    pub fn flush(&mut self, tex_id: GLuint, width: f32, height: f32) {
        if self.batch.is_empty() {
            return;
        }

        unsafe {
            gl::UseProgram(self.program);
            gl::Uniform4f(
                self.loc_projection,
                2.0 / width,
                -2.0 / height,
                -1.0,
                1.0,
            );

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex_id);

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);

            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.instance_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (self.batch.len() * std::mem::size_of::<GlyphInstance>()) as isize,
                self.batch.as_ptr() as *const _,
            );

            gl::DrawArraysInstanced(
                gl::TRIANGLE_STRIP,
                0,
                4,
                self.batch.len() as i32,
            );

            gl::Disable(gl::BLEND);
            gl::BindVertexArray(0);
        }

        self.batch.clear();
    }
}
