//! The mesh of colored triangles patterns are drawn as, and the GPU
//! program that draws it.

use super::{Vec2, alpha, blue, green, red};
use yog_api::GfxContext;
use yog_api::gfx_core::{DataType, DrawMode, blend};
use yog_api::gfx_gl::{Attrib, Buffer, ShaderProgram, VertexArray};
use yog_api::sync::Mutex;

/// Colored triangles in GUI pixels, drawn in the order they were added.
#[derive(Default)]
pub struct Mesh {
    /// `x, y, r, g, b, a` per vertex.
    verts: Vec<f32>,
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex_count(&self) -> usize {
        self.verts.len() / 6
    }

    /// `x, y, r, g, b, a` per vertex, three a triangle.
    pub fn vertices(&self) -> &[f32] {
        &self.verts
    }

    /// Adds `other`'s triangles after these.
    pub fn append(&mut self, other: Mesh) {
        self.verts.extend(other.verts);
    }

    /// Moves every vertex by `dx, dy`.
    pub fn offset(&mut self, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for v in self.verts.as_chunks_mut::<6>().0 {
            v[0] += dx;
            v[1] += dy;
        }
    }

    fn vertex(&mut self, color: u32, p: Vec2) {
        self.verts.extend_from_slice(&[
            p.x,
            p.y,
            red(color) as f32 / 255.0,
            green(color) as f32 / 255.0,
            blue(color) as f32 / 255.0,
            alpha(color) as f32 / 255.0,
        ]);
    }

    pub(super) fn tri(
        &mut self,
        (c1, p1): (u32, Vec2),
        (c2, p2): (u32, Vec2),
        (c3, p3): (u32, Vec2),
    ) {
        self.vertex(c1, p1);
        self.vertex(c2, p2);
        self.vertex(c3, p3);
    }

    /// A `TRIANGLE_FAN` around `center`, as triangles.
    pub(super) fn fan(&mut self, color: u32, center: Vec2, rim: &[Vec2]) {
        for w in rim.windows(2) {
            self.tri((color, center), (color, w[0]), (color, w[1]));
        }
    }

    /// A filled axis-aligned rectangle (`renderQuad`).
    pub fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let (a, b, c, d) = (
            Vec2::new(x, y),
            Vec2::new(x, y + h),
            Vec2::new(x + w, y + h),
            Vec2::new(x + w, y),
        );
        self.tri((color, a), (color, b), (color, c));
        self.tri((color, a), (color, c), (color, d));
    }

    /// Sends the triangles to the GPU (blended, no depth test or culling).
    pub fn draw(&self, gfx: &GfxContext) {
        if self.verts.is_empty() {
            return;
        }
        let mut guard = GL.lock();
        if guard.is_none() {
            *guard = Gl::new(gfx);
        }
        let Some(gl) = guard.as_ref() else { return };
        gfx.set_blend(true, blend::SRC_ALPHA, blend::ONE_MINUS_SRC_ALPHA);
        gfx.set_depth(false, false);
        // SAFETY: `f32`s are plain data.
        unsafe { gl.vbo.upload(gfx, &self.verts, true) };
        let (sw, sh) = gfx.screen_size();
        gl.prog.uniform_2f(gfx, "uScreen", [sw as f32, sh as f32]);
        gfx.draw_arrays(
            &gl.vao,
            &gl.prog,
            DrawMode::Triangles,
            0,
            self.vertex_count() as u32,
        );
    }
}

pub(super) const VERT: &str = "
#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec4 aColor;
uniform vec2 uScreen;
out vec4 vColor;
void main() {
    vColor = aColor;
    gl_Position = vec4(aPos.x / uScreen.x * 2.0 - 1.0, 1.0 - aPos.y / uScreen.y * 2.0, 0.0, 1.0);
}";

pub(super) const FRAG: &str = "
#version 330 core
in vec4 vColor;
out vec4 fragColor;
void main() {
    fragColor = vColor;
}";

pub(super) struct Gl {
    prog: ShaderProgram,
    vao: VertexArray,
    vbo: Buffer,
}

/// Vertex attribute `index`: `components` floats, `at` floats into a
/// vertex of `floats` floats.
pub(crate) fn attrib(index: u32, components: u8, at: u32, floats: u32) -> Attrib {
    let size = std::mem::size_of::<f32>() as u32;
    Attrib {
        index,
        components,
        dtype: DataType::F32,
        normalized: false,
        stride: floats * size,
        offset: at * size,
    }
}

impl Gl {
    fn new(gfx: &GfxContext) -> Option<Self> {
        let prog = gfx.create_shader(VERT, FRAG).ok()?;
        let vbo = gfx.create_buffer();
        let vao = gfx.create_vao();
        vao.attrib(gfx, &vbo, attrib(0, 2, 0, 6));
        vao.attrib(gfx, &vbo, attrib(1, 4, 2, 6));
        Some(Gl { prog, vao, vbo })
    }
}

pub(super) static GL: Mutex<Option<Gl>> = Mutex::new(None);
