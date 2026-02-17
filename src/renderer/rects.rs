use crate::gl;
use crate::gl::types::*;

use super::shader;

const MAX_RECTS: usize = 10_000;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RectInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

const VERT_SRC: &str = r#"
#version 330 core

layout(location = 0) in vec2 aQuad;

layout(location = 1) in vec2 aPos;
layout(location = 2) in vec2 aSize;
layout(location = 3) in vec4 aColor;

uniform vec4 uProjection;

flat out vec4 vColor;

void main() {
    vec2 pos = aPos + aQuad * aSize;
    vec2 clip = pos * uProjection.xy + uProjection.zw;
    gl_Position = vec4(clip, 0.0, 1.0);
    vColor = aColor;
}
"#;

const FRAG_SRC: &str = r#"
#version 330 core

flat in vec4 vColor;
out vec4 FragColor;

void main() {
    FragColor = vColor;
}
"#;

pub struct RectRenderer {
    program: GLuint,
    vao: GLuint,
    quad_vbo: GLuint,
    instance_vbo: GLuint,
    loc_projection: GLint,
    batch: Vec<RectInstance>,
}

impl RectRenderer {
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

            let stride = std::mem::size_of::<RectInstance>() as i32;
            gl::GenBuffers(1, &mut instance_vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, instance_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (MAX_RECTS * std::mem::size_of::<RectInstance>()) as isize,
                std::ptr::null(),
                gl::DYNAMIC_DRAW,
            );

            let mut offset = 0isize;
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(1, 1);
            offset += 8;

            gl::EnableVertexAttribArray(2);
            gl::VertexAttribPointer(2, 2, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(2, 1);
            offset += 8;

            gl::EnableVertexAttribArray(3);
            gl::VertexAttribPointer(3, 4, gl::FLOAT, gl::FALSE, stride, offset as *const _);
            gl::VertexAttribDivisor(3, 1);

            gl::BindVertexArray(0);
        }

        RectRenderer {
            program,
            vao,
            quad_vbo,
            instance_vbo,
            loc_projection,
            batch: Vec::with_capacity(MAX_RECTS),
        }
    }

    pub fn add(&mut self, rect: RectInstance) {
        if self.batch.len() < MAX_RECTS {
            self.batch.push(rect);
        }
    }

    pub fn flush(&mut self, width: f32, height: f32) {
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

            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.instance_vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (self.batch.len() * std::mem::size_of::<RectInstance>()) as isize,
                self.batch.as_ptr() as *const _,
            );

            gl::DrawArraysInstanced(
                gl::TRIANGLE_STRIP,
                0,
                4,
                self.batch.len() as i32,
            );

            gl::BindVertexArray(0);
        }

        self.batch.clear();
    }
}
