use glam::{Mat4, Vec2};
use wasm_bindgen::JsValue;
use web_sys::{
    WebGl2RenderingContext as Gl, WebGlProgram, WebGlTexture, WebGlUniformLocation,
    WebGlVertexArrayObject,
};

pub const SIZE: usize = 128;
const FADE_SPEED: f32 = 6.0;
const DARKNESS: f32 = 0.6;

const VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec2 corner;
uniform mat4 view_projection;
uniform vec4 bounds;
uniform float height;
out vec2 v_uv;
void main() {
    v_uv = corner;
    vec2 world = mix(bounds.xy, bounds.zw, corner);
    gl_Position = view_projection * vec4(world.x, height, world.y, 1.0);
}
"#;

const FRAGMENT: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D visibility;
uniform float darkness;
out vec4 color;
void main() {
    float seen = texture(visibility, v_uv).r;
    color = vec4(0.01, 0.02, 0.05, (1.0 - seen) * darkness);
}
"#;

pub struct Fog {
    gl: Gl,
    program: WebGlProgram,
    quad: WebGlVertexArrayObject,
    texture: WebGlTexture,
    view_projection: Option<WebGlUniformLocation>,
    bounds_uniform: Option<WebGlUniformLocation>,
    height: Option<WebGlUniformLocation>,
    darkness: Option<WebGlUniformLocation>,
    visibility: Vec<f32>,
    bounds: (Vec2, Vec2),
}

pub fn reveal(visibility: &mut [f32], bounds: (Vec2, Vec2), sources: &[(Vec2, f32)], blend: f32) {
    let cell = (bounds.1 - bounds.0) / SIZE as f32;
    let mut target = vec![0.0_f32; SIZE * SIZE];
    for (center, radius) in sources {
        let low = ((*center - Vec2::splat(*radius) - bounds.0) / cell).floor();
        let high = ((*center + Vec2::splat(*radius) - bounds.0) / cell).ceil();
        let (x0, z0) = (low.x.max(0.0) as usize, low.y.max(0.0) as usize);
        let (x1, z1) = (
            (high.x.max(0.0) as usize).min(SIZE),
            (high.y.max(0.0) as usize).min(SIZE),
        );
        for z in z0..z1 {
            for x in x0..x1 {
                let middle = bounds.0 + (Vec2::new(x as f32, z as f32) + 0.5) * cell;
                if middle.distance(*center) <= *radius {
                    target[z * SIZE + x] = 1.0;
                }
            }
        }
    }
    for (value, goal) in visibility.iter_mut().zip(target) {
        *value += (goal - *value) * blend;
    }
}

impl Fog {
    pub fn new(gl: &Gl) -> Result<Self, JsValue> {
        let program = crate::render::link(gl, VERTEX, FRAGMENT)?;
        let quad = gl.create_vertex_array().ok_or("vertex array")?;
        gl.bind_vertex_array(Some(&quad));
        let buffer = gl.create_buffer().ok_or("buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
        let corners: Vec<u8> = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, &corners, Gl::STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.bind_vertex_array(None);
        let texture = gl.create_texture().ok_or("texture")?;
        gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
        for (parameter, value) in [
            (Gl::TEXTURE_MIN_FILTER, Gl::LINEAR),
            (Gl::TEXTURE_MAG_FILTER, Gl::LINEAR),
            (Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE),
            (Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameteri(Gl::TEXTURE_2D, parameter, value as i32);
        }
        let uniform = |name| gl.get_uniform_location(&program, name);
        Ok(Self {
            gl: gl.clone(),
            view_projection: uniform("view_projection"),
            bounds_uniform: uniform("bounds"),
            height: uniform("height"),
            darkness: uniform("darkness"),
            program,
            quad,
            texture,
            visibility: vec![0.0; SIZE * SIZE],
            bounds: (Vec2::ZERO, Vec2::ONE),
        })
    }

    pub fn update(&mut self, bounds: (Vec2, Vec2), sources: &[(Vec2, f32)], dt: f32) {
        self.bounds = bounds;
        reveal(
            &mut self.visibility,
            bounds,
            sources,
            (FADE_SPEED * dt).min(1.0),
        );
        let pixels: Vec<u8> = self
            .visibility
            .iter()
            .map(|value| (value.clamp(0.0, 1.0) * 255.0) as u8)
            .collect();
        let gl = &self.gl;
        gl.bind_texture(Gl::TEXTURE_2D, Some(&self.texture));
        gl.pixel_storei(Gl::UNPACK_ALIGNMENT, 1);
        let _ = gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            Gl::TEXTURE_2D,
            0,
            Gl::R8 as i32,
            SIZE as i32,
            SIZE as i32,
            0,
            Gl::RED,
            Gl::UNSIGNED_BYTE,
            Some(&pixels),
        );
    }

    pub fn visibility(&self) -> &[f32] {
        &self.visibility
    }

    pub fn draw(&self, view_projection: &Mat4, height: f32) {
        let gl = &self.gl;
        gl.disable(Gl::DEPTH_TEST);
        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        gl.use_program(Some(&self.program));
        gl.uniform_matrix4fv_with_f32_array(
            self.view_projection.as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        let (min, max) = self.bounds;
        gl.uniform4f(self.bounds_uniform.as_ref(), min.x, min.y, max.x, max.y);
        gl.uniform1f(self.height.as_ref(), height);
        gl.uniform1f(self.darkness.as_ref(), DARKNESS);
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&self.texture));
        gl.bind_vertex_array(Some(&self.quad));
        gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
        gl.bind_vertex_array(None);
        gl.disable(Gl::BLEND);
        gl.enable(Gl::DEPTH_TEST);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_light_up_their_surroundings_only() {
        let bounds = (Vec2::ZERO, Vec2::splat(SIZE as f32 * 100.0));
        let mut visibility = vec![0.0; SIZE * SIZE];
        reveal(
            &mut visibility,
            bounds,
            &[(Vec2::splat(1000.0), 500.0)],
            1.0,
        );
        let at = |visibility: &[f32], x: f32, z: f32| {
            visibility[(z / 100.0) as usize * SIZE + (x / 100.0) as usize]
        };
        assert_eq!(at(&visibility, 1050.0, 1050.0), 1.0);
        assert_eq!(at(&visibility, 3000.0, 3000.0), 0.0);
        reveal(&mut visibility, bounds, &[], 0.5);
        assert_eq!(at(&visibility, 1050.0, 1050.0), 0.5);
    }
}
