use asset::Character;
use glam::{Mat4, Vec2, Vec3, Vec4};
use wasm_bindgen::JsValue;
use web_sys::{
    HtmlImageElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

const MAX_JOINTS: usize = 128;
const VERTEX_STRIDE: i32 = 52;
const OUTLINE_WIDTH: f32 = 3.0;

const SKINNED_VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec3 position;
layout(location = 1) in vec3 normal;
layout(location = 2) in vec2 uv;
layout(location = 3) in uvec4 joints;
layout(location = 4) in vec4 weights;
uniform mat4 view_projection;
uniform mat4 model;
uniform float outline;
layout(std140) uniform Skin { mat4 skin[128]; };
out vec2 v_uv;
out vec3 v_normal;
void main() {
    mat4 blended = weights.x * skin[joints.x] + weights.y * skin[joints.y]
        + weights.z * skin[joints.z] + weights.w * skin[joints.w];
    v_normal = mat3(model) * mat3(blended) * normal;
    vec3 push = v_normal * (outline / max(length(v_normal), 0.0001));
    gl_Position = view_projection * (model * blended * vec4(position, 1.0) + vec4(push, 0.0));
    v_uv = uv;
}
"#;

const SKINNED_FRAGMENT: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
in vec3 v_normal;
uniform sampler2D albedo;
uniform float self_illumination;
uniform vec4 tint;
uniform float opacity;
uniform vec4 outline_color;
out vec4 color;
void main() {
    vec4 texel = texture(albedo, v_uv);
    if (texel.a < 0.3) discard;
    if (outline_color.a > 0.0) {
        color = outline_color;
        return;
    }
    float light = 0.45 + 0.65 * max(dot(normalize(v_normal), normalize(vec3(-0.3, 1.0, -0.5))), 0.0);
    vec3 lit = texel.rgb * mix(light, 1.0, self_illumination);
    color = vec4(mix(lit, tint.rgb * (0.6 + 0.6 * dot(lit, vec3(0.33))), tint.a), opacity);
}
"#;

const DECAL_VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec2 corner;
uniform mat4 view_projection;
uniform vec4 placement;
out vec2 v_local;
out vec2 v_world;
void main() {
    v_local = corner;
    v_world = placement.xy + corner * placement.z;
    gl_Position = view_projection * vec4(v_world.x, placement.w, v_world.y, 1.0);
}
"#;

const DECAL_FRAGMENT: &str = r#"#version 300 es
precision highp float;
in vec2 v_local;
in vec2 v_world;
uniform int kind;
uniform float progress;
uniform vec4 tint;
uniform vec4 placement;
uniform vec4 shape;
out vec4 color;

float line(float coordinate, float spacing, float width) {
    float distance = abs(fract(coordinate / spacing + 0.5) - 0.5) * spacing;
    return 1.0 - smoothstep(0.0, width * fwidth(coordinate), distance);
}

vec4 indicator(float distance) {
    float pixel = 1.5 * fwidth(distance);
    float inside = 1.0 - smoothstep(-pixel, 0.0, distance);
    float border = 1.0 - smoothstep(0.0, pixel, abs(distance + 3.0) - 3.0);
    return vec4(tint.rgb, tint.a * max(inside * 0.3, border));
}

void main() {
    float radius = length(v_local);
    if (kind == 0) {
        float minor = max(line(v_world.x, 100.0, 1.0), line(v_world.y, 100.0, 1.0));
        float major = max(line(v_world.x, 500.0, 1.5), line(v_world.y, 500.0, 1.5));
        vec3 grass = vec3(0.13, 0.19, 0.12);
        vec3 tint = mix(grass, vec3(0.2, 0.27, 0.18), max(minor * 0.5, major));
        float fade = 1.0 - smoothstep(0.6, 1.0, radius);
        color = vec4(mix(vec3(0.05, 0.07, 0.05), tint, fade), 1.0);
    } else if (kind == 1) {
        color = vec4(0.0, 0.0, 0.0, 0.45 * (1.0 - smoothstep(0.35, 1.0, radius)));
    } else if (kind == 3) {
        color = vec4(tint.rgb, tint.a * (1.0 - smoothstep(0.55, 1.0, radius)));
    } else if (kind == 4) {
        float edge = 1.0 - smoothstep(0.0, 1.5 * fwidth(radius), abs(radius - 1.0 + progress) - progress);
        color = vec4(tint.rgb, tint.a * edge);
    } else if (kind == 5) {
        vec2 point = v_local * placement.z;
        float along = dot(point, shape.xy);
        float across = abs(dot(point, vec2(-shape.y, shape.x)));
        float head = min(shape.z * 2.5, placement.z * 0.4);
        float body = placement.z - head;
        float shaft = max(max(-along, along - body), across - shape.z);
        float spread = shape.z * 1.8 * (placement.z - along) / head;
        float tip = max(max(body - along, along - placement.z), across - spread);
        color = indicator(min(shaft, tip));
    } else {
        vec2 point = v_local * placement.z;
        float length_ = length(point);
        float angle = acos(clamp(dot(point / max(length_, 0.001), shape.xy), -1.0, 1.0));
        color = indicator(max(length_ - placement.z, (angle - shape.z) * length_));
    }
}
"#;

#[derive(Clone)]
pub struct Renderer {
    gl: Gl,
    skinned: Program,
    decal: Program,
    quad: WebGlVertexArrayObject,
    skin: WebGlBuffer,
}

struct Shading {
    tint: Vec4,
    opacity: f32,
    outline: Option<Vec4>,
}

pub struct CharacterMesh {
    vertex_array: WebGlVertexArrayObject,
    textures: Vec<WebGlTexture>,
    draws: Vec<(i32, i32, usize)>,
    self_illumination: f32,
}

#[derive(Clone)]
struct Program {
    program: WebGlProgram,
    view_projection: Option<WebGlUniformLocation>,
    model: Option<WebGlUniformLocation>,
    placement: Option<WebGlUniformLocation>,
    kind: Option<WebGlUniformLocation>,
    progress: Option<WebGlUniformLocation>,
    tint: Option<WebGlUniformLocation>,
    shape: Option<WebGlUniformLocation>,
    self_illumination: Option<WebGlUniformLocation>,
    opacity: Option<WebGlUniformLocation>,
    outline: Option<WebGlUniformLocation>,
    outline_color: Option<WebGlUniformLocation>,
}

pub enum Decal {
    Ground,
    Shadow,
    Glow(Vec4),
    Ring(Vec4, f32),
    Arrow(Vec4, Vec2, f32),
    Cone(Vec4, Vec2, f32),
}

impl Renderer {
    pub fn new(gl: Gl) -> Result<Self, JsValue> {
        let skinned = Program::new(&gl, SKINNED_VERTEX, SKINNED_FRAGMENT)?;
        let decal = Program::new(&gl, DECAL_VERTEX, DECAL_FRAGMENT)?;
        let block = gl.get_uniform_block_index(&skinned.program, "Skin");
        gl.uniform_block_binding(&skinned.program, block, 0);

        let quad = gl.create_vertex_array().ok_or("vertex array")?;
        gl.bind_vertex_array(Some(&quad));
        let corners: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        upload(&gl, Gl::ARRAY_BUFFER, &floats_to_bytes(&corners))?;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.bind_vertex_array(None);

        let skin = gl.create_buffer().ok_or("skin buffer")?;
        gl.bind_buffer(Gl::UNIFORM_BUFFER, Some(&skin));
        gl.buffer_data_with_i32(
            Gl::UNIFORM_BUFFER,
            (MAX_JOINTS * 64) as i32,
            Gl::DYNAMIC_DRAW,
        );
        gl.bind_buffer_base(Gl::UNIFORM_BUFFER, 0, Some(&skin));

        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        Ok(Self {
            gl,
            skinned,
            decal,
            quad,
            skin,
        })
    }

    pub fn character(
        &self,
        character: &Character,
        images: &[HtmlImageElement],
    ) -> Result<CharacterMesh, JsValue> {
        let gl = &self.gl;
        let vertex_array = gl.create_vertex_array().ok_or("vertex array")?;
        gl.bind_vertex_array(Some(&vertex_array));
        upload(gl, Gl::ARRAY_BUFFER, &vertex_bytes(character))?;
        upload(gl, Gl::ELEMENT_ARRAY_BUFFER, &index_bytes(character))?;
        let float = |location: u32, size: i32, offset: i32| {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(
                location,
                size,
                Gl::FLOAT,
                false,
                VERTEX_STRIDE,
                offset,
            );
        };
        float(0, 3, 0);
        float(1, 3, 12);
        float(2, 2, 24);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_i_pointer_with_i32(3, 4, Gl::UNSIGNED_BYTE, VERTEX_STRIDE, 32);
        float(4, 4, 36);
        gl.bind_vertex_array(None);

        let textures = images
            .iter()
            .map(|image| texture(gl, image))
            .collect::<Result<Vec<_>, _>>()?;
        let draws = character
            .submeshes
            .iter()
            .filter(|submesh| submesh.visible)
            .map(|submesh| {
                (
                    submesh.start as i32,
                    submesh.count as i32,
                    usize::from(submesh.texture),
                )
            })
            .collect();
        Ok(CharacterMesh {
            vertex_array,
            textures,
            draws,
            self_illumination: character.self_illumination,
        })
    }

    pub fn begin(&self, width: i32, height: i32) {
        self.gl.viewport(0, 0, width, height);
        self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
        self.gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
    }

    pub fn decal(&self, view_projection: &Mat4, decal: Decal, center: Vec3, radius: f32) {
        let gl = &self.gl;
        let (kind, progress, tint, shape) = match decal {
            Decal::Ground => (0, 0.0, Vec4::ZERO, Vec4::ZERO),
            Decal::Shadow => (1, 0.0, Vec4::ZERO, Vec4::ZERO),
            Decal::Glow(tint) => (3, 0.0, tint, Vec4::ZERO),
            Decal::Ring(tint, width) => (4, width, tint, Vec4::ZERO),
            Decal::Arrow(tint, direction, width) => {
                (5, 0.0, tint, direction.extend(width / 2.0).extend(0.0))
            }
            Decal::Cone(tint, direction, half_angle) => {
                (6, 0.0, tint, direction.extend(half_angle).extend(0.0))
            }
        };
        gl.disable(Gl::DEPTH_TEST);
        gl.enable(Gl::BLEND);
        gl.use_program(Some(&self.decal.program));
        gl.uniform_matrix4fv_with_f32_array(
            self.decal.view_projection.as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        let placement = Vec4::new(center.x, center.z, radius, center.y);
        gl.uniform4fv_with_f32_array(self.decal.placement.as_ref(), &placement.to_array());
        gl.uniform1i(self.decal.kind.as_ref(), kind);
        gl.uniform1f(self.decal.progress.as_ref(), progress);
        gl.uniform4fv_with_f32_array(self.decal.tint.as_ref(), &tint.to_array());
        gl.uniform4fv_with_f32_array(self.decal.shape.as_ref(), &shape.to_array());
        gl.bind_vertex_array(Some(&self.quad));
        gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
    }

    pub fn draw(
        &self,
        mesh: &CharacterMesh,
        view_projection: &Mat4,
        model: &Mat4,
        skin: &[Mat4],
        tint: Vec4,
        opacity: f32,
    ) {
        let shading = Shading {
            tint,
            opacity,
            outline: None,
        };
        self.submit(mesh, view_projection, model, skin, shading);
    }

    pub fn outline(
        &self,
        mesh: &CharacterMesh,
        view_projection: &Mat4,
        model: &Mat4,
        skin: &[Mat4],
        color: Vec4,
    ) {
        self.gl.enable(Gl::CULL_FACE);
        self.gl.cull_face(Gl::BACK);
        let shading = Shading {
            tint: Vec4::ZERO,
            opacity: 1.0,
            outline: Some(color),
        };
        self.submit(mesh, view_projection, model, skin, shading);
        self.gl.disable(Gl::CULL_FACE);
    }

    fn submit(
        &self,
        mesh: &CharacterMesh,
        view_projection: &Mat4,
        model: &Mat4,
        skin: &[Mat4],
        shading: Shading,
    ) {
        let Shading {
            tint,
            opacity,
            outline,
        } = shading;
        let gl = &self.gl;
        gl.enable(Gl::DEPTH_TEST);
        if opacity < 1.0 {
            gl.enable(Gl::BLEND);
            gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        } else {
            gl.disable(Gl::BLEND);
        }
        gl.use_program(Some(&self.skinned.program));
        gl.uniform_matrix4fv_with_f32_array(
            self.skinned.view_projection.as_ref(),
            false,
            &view_projection.to_cols_array(),
        );
        gl.uniform_matrix4fv_with_f32_array(
            self.skinned.model.as_ref(),
            false,
            &model.to_cols_array(),
        );
        gl.uniform1f(
            self.skinned.self_illumination.as_ref(),
            mesh.self_illumination,
        );
        gl.uniform4fv_with_f32_array(self.skinned.tint.as_ref(), &tint.to_array());
        gl.uniform1f(self.skinned.opacity.as_ref(), opacity);
        gl.uniform1f(
            self.skinned.outline.as_ref(),
            if outline.is_some() {
                OUTLINE_WIDTH
            } else {
                0.0
            },
        );
        gl.uniform4fv_with_f32_array(
            self.skinned.outline_color.as_ref(),
            &outline.unwrap_or(Vec4::ZERO).to_array(),
        );
        let matrices: Vec<f32> = skin
            .iter()
            .take(MAX_JOINTS)
            .flat_map(Mat4::to_cols_array)
            .collect();
        gl.bind_buffer(Gl::UNIFORM_BUFFER, Some(&self.skin));
        gl.buffer_sub_data_with_i32_and_u8_array(
            Gl::UNIFORM_BUFFER,
            0,
            &floats_to_bytes(&matrices),
        );
        gl.bind_vertex_array(Some(&mesh.vertex_array));
        for (start, count, texture) in &mesh.draws {
            gl.bind_texture(Gl::TEXTURE_2D, mesh.textures.get(*texture));
            gl.draw_elements_with_i32(Gl::TRIANGLES, *count, Gl::UNSIGNED_SHORT, start * 2);
        }
    }
}

impl Program {
    fn new(gl: &Gl, vertex: &str, fragment: &str) -> Result<Self, JsValue> {
        let program = link(gl, vertex, fragment)?;
        let uniform = |name| gl.get_uniform_location(&program, name);
        Ok(Self {
            view_projection: uniform("view_projection"),
            model: uniform("model"),
            placement: uniform("placement"),
            kind: uniform("kind"),
            progress: uniform("progress"),
            tint: uniform("tint"),
            shape: uniform("shape"),
            self_illumination: uniform("self_illumination"),
            opacity: uniform("opacity"),
            outline: uniform("outline"),
            outline_color: uniform("outline_color"),
            program,
        })
    }
}

pub fn link(gl: &Gl, vertex: &str, fragment: &str) -> Result<WebGlProgram, JsValue> {
    let program = gl.create_program().ok_or("program")?;
    gl.attach_shader(&program, &shader(gl, Gl::VERTEX_SHADER, vertex)?);
    gl.attach_shader(&program, &shader(gl, Gl::FRAGMENT_SHADER, fragment)?);
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

fn floats_to_bytes(floats: &[f32]) -> Vec<u8> {
    floats
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
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
    Ok(texture)
}

fn vertex_bytes(character: &Character) -> Vec<u8> {
    character
        .vertices
        .iter()
        .flat_map(|vertex| {
            let floats = |values: &[f32]| floats_to_bytes(values);
            [
                floats(&vertex.position),
                floats(&vertex.normal),
                floats(&vertex.uv),
                vertex.joints.to_vec(),
                floats(&vertex.weights),
            ]
            .concat()
        })
        .collect()
}

fn index_bytes(character: &Character) -> Vec<u8> {
    character
        .indices
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect()
}
