use std::ffi::CString;
use std::ptr;

use crate::gl;
use crate::gl::types::*;

pub fn compile_shader(src: &str, kind: GLenum) -> GLuint {
    let shader;
    unsafe {
        shader = gl::CreateShader(kind);
        let c_str = CString::new(src.as_bytes()).unwrap();
        gl::ShaderSource(shader, 1, &c_str.as_ptr(), ptr::null());
        gl::CompileShader(shader);

        let mut success = gl::FALSE as GLint;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
        if success != gl::TRUE as GLint {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetShaderInfoLog(shader, len, ptr::null_mut(), buf.as_mut_ptr() as *mut _);
            buf.truncate(buf.iter().position(|&c| c == 0).unwrap_or(buf.len()));
            panic!(
                "Shader compilation failed:\n{}",
                String::from_utf8_lossy(&buf)
            );
        }
    }
    shader
}

pub fn link_program(vertex: GLuint, fragment: GLuint) -> GLuint {
    let program;
    unsafe {
        program = gl::CreateProgram();
        gl::AttachShader(program, vertex);
        gl::AttachShader(program, fragment);
        gl::LinkProgram(program);

        let mut success = gl::FALSE as GLint;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
        if success != gl::TRUE as GLint {
            let mut len = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetProgramInfoLog(program, len, ptr::null_mut(), buf.as_mut_ptr() as *mut _);
            buf.truncate(buf.iter().position(|&c| c == 0).unwrap_or(buf.len()));
            panic!("Program link failed:\n{}", String::from_utf8_lossy(&buf));
        }

        gl::DeleteShader(vertex);
        gl::DeleteShader(fragment);
    }
    program
}

pub fn get_uniform_location(program: GLuint, name: &str) -> GLint {
    let c_name = CString::new(name).unwrap();
    unsafe { gl::GetUniformLocation(program, c_name.as_ptr()) }
}
