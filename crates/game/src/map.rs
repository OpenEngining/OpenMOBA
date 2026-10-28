use asset::map::{Blending, MapDraw, MapGeometry, MapMaterial};
use glam::Mat4;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};
use web_sys::{
    HtmlImageElement, WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader, WebGlTexture,
    WebGlUniformLocation, WebGlVertexArrayObject,
};

use crate::download::{self, fetch};

const VERTEX_STRIDE: i32 = 28;

const VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec3 position;
layout(location = 1) in vec2 uv;
layout(location = 2) in vec2 lightmap_uv;
uniform mat4 view_projection;
out vec2 v_uv;
out vec2 v_lightmap_uv;
void main() {
    gl_Position = view_projection * vec4(position, 1.0);
    v_uv = uv;
    v_lightmap_uv = lightmap_uv;
}
"#;

const FRAGMENT: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
in vec2 v_lightmap_uv;
uniform sampler2D diffuse;
uniform sampler2D lightmap;
uniform bool has_diffuse;
uniform bool has_lightmap;
uniform vec4 tint;
uniform float threshold;
uniform float lightmap_scale;
out vec4 color;
void main() {
    vec4 texel = has_diffuse ? texture(diffuse, v_uv) : vec4(1.0);
    if (texel.a < threshold) discard;
    vec3 light = has_lightmap ? texture(lightmap, v_lightmap_uv).rgb * lightmap_scale : vec3(1.0);
    color = vec4(texel.rgb * tint.rgb * 2.0 * light, texel.a);
}
"#;

struct Uniforms {
    view_projection: Option<WebGlUniformLocation>,
    has_diffuse: Option<WebGlUniformLocation>,
    has_lightmap: Option<WebGlUniformLocation>,
    tint: Option<WebGlUniformLocation>,
    threshold: Option<WebGlUniformLocation>,
    lightmap_scale: Option<WebGlUniformLocation>,
}

pub struct MapRenderer {
    gl: Gl,
    program: WebGlProgram,
    uniforms: Uniforms,
    vertex_array: WebGlVertexArrayObject,
    textures: Vec<WebGlTexture>,
    materials: Vec<MapMaterial>,
    draws: Vec<MapDraw>,
    lightmap_scale: f32,
}

impl MapRenderer {
    pub async fn load(gl: &Gl, base_url: &str) -> Result<Self, JsValue> {
        let geometry = MapGeometry::from_bytes(&fetch(&format!("{base_url}/map.bin")).await?)
            .map_err(|error| error.to_string())?;
        let images = geometry
            .textures
            .iter()
            .map(|file| image(&format!("{base_url}/{file}")))
            .collect::<js_sys::Array>();
        let images = js_sys::Promise::all(&images);
        let images: js_sys::Array = JsFuture::from(images).await?.dyn_into()?;

        let program = program(gl)?;
        let uniform = |name| gl.get_uniform_location(&program, name);
        let uniforms = Uniforms {
            view_projection: uniform("view_projection"),
            has_diffuse: uniform("has_diffuse"),
            has_lightmap: uniform("has_lightmap"),
            tint: uniform("tint"),
            threshold: uniform("threshold"),
            lightmap_scale: uniform("lightmap_scale"),
        };
        gl.use_program(Some(&program));
        gl.uniform1i(uniform("diffuse").as_ref(), 0);
        gl.uniform1i(uniform("lightmap").as_ref(), 1);

        let vertex_array = gl.create_vertex_array().ok_or("vertex array")?;
        gl.bind_vertex_array(Some(&vertex_array));
        let vertices: Vec<u8> = geometry
            .vertices
            .iter()
            .flat_map(|vertex| {
                vertex
                    .position
                    .into_iter()
                    .chain(vertex.uv)
                    .chain(vertex.lightmap_uv)
                    .flat_map(f32::to_le_bytes)
            })
            .collect();
        let indices: Vec<u8> = geometry
            .indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        upload(gl, Gl::ARRAY_BUFFER, &vertices)?;
        upload(gl, Gl::ELEMENT_ARRAY_BUFFER, &indices)?;
        for (location, size, offset) in [(0, 3, 0), (1, 2, 12), (2, 2, 20)] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(
                location,
                size,
                Gl::FLOAT,
                false,
                VERTEX_STRIDE,
                offset,
            );
        }
        gl.bind_vertex_array(None);

        let textures = images
            .iter()
            .map(|image| texture(gl, &image.dyn_into()?))
            .collect::<Result<Vec<_>, JsValue>>()?;
        Ok(Self {
            gl: gl.clone(),
            program,
            uniforms,
            vertex_array,
            textures,
            materials: geometry.materials,
            draws: geometry.draws,
            lightmap_scale: geometry.lightmap_scale,
        })
    }

    pub fn draw(&self, view_projection: &Mat4) {
        let gl = &self.gl;
        gl.use_program(Some(&self.program));
        gl.bind_vertex_array(Some(&self.vertex_array));
        gl.uniform_matrix4fv_with_f32_array(
            self.uniforms.view_projection.as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        gl.uniform1f(self.uniforms.lightmap_scale.as_ref(), self.lightmap_scale);
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_func(Gl::LEQUAL);
        gl.front_face(Gl::CW);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        for draw in &self.draws {
            let material = &self.materials[usize::from(draw.material)];
            let (threshold, blended) = match material.blending {
                Blending::Opaque => (-1.0, false),
                Blending::AlphaTest(threshold) => (threshold, false),
                Blending::Blend(threshold) => (threshold.max(0.004), true),
            };
            if blended {
                gl.enable(Gl::BLEND);
                gl.depth_mask(false);
            } else {
                gl.disable(Gl::BLEND);
                gl.depth_mask(true);
            }
            if material.double_sided {
                gl.disable(Gl::CULL_FACE);
            } else {
                gl.enable(Gl::CULL_FACE);
            }
            gl.uniform1f(self.uniforms.threshold.as_ref(), threshold);
            gl.uniform4fv_with_f32_array(self.uniforms.tint.as_ref(), &material.tint);
            let bind = |unit: u32, index: Option<u16>, flag: &Option<WebGlUniformLocation>| {
                let texture = index.and_then(|index| self.textures.get(usize::from(index)));
                gl.active_texture(Gl::TEXTURE0 + unit);
                gl.bind_texture(Gl::TEXTURE_2D, texture);
                gl.uniform1i(flag.as_ref(), i32::from(texture.is_some()));
            };
            bind(0, material.texture, &self.uniforms.has_diffuse);
            bind(1, draw.lightmap, &self.uniforms.has_lightmap);
            gl.draw_elements_with_i32(
                Gl::TRIANGLES,
                draw.count as i32,
                Gl::UNSIGNED_INT,
                draw.start as i32 * 4,
            );
        }
        gl.active_texture(Gl::TEXTURE0);
        gl.depth_mask(true);
        gl.disable(Gl::BLEND);
        gl.disable(Gl::CULL_FACE);
        gl.front_face(Gl::CCW);
        gl.bind_vertex_array(None);
    }
}

fn image(source: &str) -> js_sys::Promise {
    let source = source.to_owned();
    future_to_promise(async move { download::image(&source).await.map(JsValue::from) })
}

fn program(gl: &Gl) -> Result<WebGlProgram, JsValue> {
    let program = gl.create_program().ok_or("program")?;
    gl.attach_shader(&program, &shader(gl, Gl::VERTEX_SHADER, VERTEX)?);
    gl.attach_shader(&program, &shader(gl, Gl::FRAGMENT_SHADER, FRAGMENT)?);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .is_truthy()
    {
        Ok(program)
    } else {
        Err(gl.get_program_info_log(&program).unwrap_or_default().into())
    }
}

fn shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .is_truthy()
    {
        Ok(shader)
    } else {
        Err(gl.get_shader_info_log(&shader).unwrap_or_default().into())
    }
}

fn upload(gl: &Gl, target: u32, bytes: &[u8]) -> Result<(), JsValue> {
    let buffer = gl.create_buffer().ok_or("buffer")?;
    gl.bind_buffer(target, Some(&buffer));
    gl.buffer_data_with_u8_array(target, bytes, Gl::STATIC_DRAW);
    Ok(())
}

fn texture(gl: &Gl, image: &HtmlImageElement) -> Result<WebGlTexture, JsValue> {
    let texture = gl.create_texture().ok_or("texture")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.tex_image_2d_with_u32_and_u32_and_html_image_element(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        image,
    )?;
    gl.generate_mipmap(Gl::TEXTURE_2D);
    gl.tex_parameteri(
        Gl::TEXTURE_2D,
        Gl::TEXTURE_MIN_FILTER,
        Gl::LINEAR_MIPMAP_LINEAR as i32,
    );
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::REPEAT as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::REPEAT as i32);
    Ok(texture)
}
