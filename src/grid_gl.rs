//! Smooth, resolution-independent rendering for the casting grid: dots and
//! pattern lines are drawn as signed-distance-field quads (real circles and
//! rounded-cap segments with anti-aliased edges) instead of blocky rects —
//! looks crisp at any display resolution / GUI scale, unlike stepped quads.

use std::sync::Mutex;

use yog_api::gfx_core::DrawMode;
use yog_api::gfx_gl::{Buffer, ShaderProgram, VertexArray};
use yog_api::GfxContext;

const VERT: &str = "
#version 330 core
in vec2 aLocal; // unit quad corner, 0..1
uniform vec2 uScreen;
uniform vec2 uMin;
uniform vec2 uMax;
out vec2 vPos;
void main() {
    vec2 pixelPos = mix(uMin, uMax, aLocal);
    vPos = pixelPos;
    vec2 ndc = vec2(pixelPos.x / uScreen.x * 2.0 - 1.0, 1.0 - pixelPos.y / uScreen.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
}";

const FRAG: &str = "
#version 330 core
in vec2 vPos;
uniform int uMode;      // 0 = circle, 1 = capsule (line segment)
uniform vec2 uA;        // circle center, or line P0
uniform vec2 uB;        // unused for circle; line P1
uniform float uRadius;  // circle radius, or line half-thickness
uniform vec4 uColor;
out vec4 fragColor;

float circleSdf(vec2 p, vec2 c, float r) { return length(p - c) - r; }
float capsuleSdf(vec2 p, vec2 a, vec2 b, float r) {
    vec2 pa = p - a, ba = b - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

void main() {
    float d = (uMode == 0) ? circleSdf(vPos, uA, uRadius) : capsuleSdf(vPos, uA, uB, uRadius);
    float aa = 1.25; // anti-alias band width, GUI pixels
    float alpha = clamp(0.5 - d / aa, 0.0, 1.0);
    if (alpha <= 0.001) discard;
    fragColor = vec4(uColor.rgb, uColor.a * alpha);
}";

struct GridGl {
    prog: ShaderProgram,
    vao: VertexArray,
    _vbo: Buffer,
}

static GL: Mutex<Option<GridGl>> = Mutex::new(None);

fn with_gl<F: FnOnce(&GfxContext, &GridGl)>(ctx: &GfxContext, f: F) {
    let mut guard = GL.lock().unwrap();
    if guard.is_none() {
        let Ok(prog) = ctx.create_shader(VERT, FRAG) else { return };
        let vbo = ctx.create_buffer();
        // Unit quad, triangle-strip order: (0,0) (1,0) (0,1) (1,1).
        let verts: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        unsafe { vbo.upload(ctx, &verts, false) };
        let vao = ctx.create_vao();
        vao.attrib(ctx, &vbo, 0, 2, yog_api::gfx_core::DataType::F32, false, 8, 0);
        *guard = Some(GridGl { prog, vao, _vbo: vbo });
    }
    f(ctx, guard.as_ref().unwrap());
}

fn draw_quad(ctx: &GfxContext, gl: &GridGl, min: (f32, f32), max: (f32, f32),
             mode: i32, a: (f32, f32), b: (f32, f32), radius: f32, color: u32) {
    let (sw, sh) = ctx.screen_size();
    let (r, g, bl, al) = (
        ((color >> 16) & 0xFF) as f32 / 255.0,
        ((color >> 8) & 0xFF) as f32 / 255.0,
        (color & 0xFF) as f32 / 255.0,
        ((color >> 24) & 0xFF) as f32 / 255.0,
    );
    gl.prog.uniform_2f(ctx, "uScreen", sw as f32, sh as f32);
    gl.prog.uniform_2f(ctx, "uMin", min.0, min.1);
    gl.prog.uniform_2f(ctx, "uMax", max.0, max.1);
    gl.prog.uniform_1i(ctx, "uMode", mode);
    gl.prog.uniform_2f(ctx, "uA", a.0, a.1);
    gl.prog.uniform_2f(ctx, "uB", b.0, b.1);
    gl.prog.uniform_1f(ctx, "uRadius", radius);
    gl.prog.uniform_4f(ctx, "uColor", r, g, bl, al);
    ctx.draw_arrays(&gl.vao, &gl.prog, DrawMode::TriangleStrip, 0, 4);
}

/// Draw an anti-aliased filled circle centered at `(x, y)` with the given
/// `radius` (GUI pixels) and `color` (`0xAARRGGBB`).
pub fn dot(ctx: &GfxContext, x: f32, y: f32, radius: f32, color: u32) {
    ctx.set_blend(true, yog_api::gfx_core::blend::SRC_ALPHA, yog_api::gfx_core::blend::ONE_MINUS_SRC_ALPHA);
    ctx.set_depth(false, false);
    let pad = radius + 2.0;
    with_gl(ctx, |ctx, gl| {
        draw_quad(ctx, gl, (x - pad, y - pad), (x + pad, y + pad), 0, (x, y), (0.0, 0.0), radius, color);
    });
}

/// Draw an anti-aliased rounded-cap line segment from `(x0, y0)` to
/// `(x1, y1)` with the given `thickness` (GUI pixels) and `color`.
pub fn line(ctx: &GfxContext, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: u32) {
    ctx.set_blend(true, yog_api::gfx_core::blend::SRC_ALPHA, yog_api::gfx_core::blend::ONE_MINUS_SRC_ALPHA);
    ctx.set_depth(false, false);
    let r = thickness / 2.0;
    let pad = r + 2.0;
    let min = (x0.min(x1) - pad, y0.min(y1) - pad);
    let max = (x0.max(x1) + pad, y0.max(y1) + pad);
    with_gl(ctx, |ctx, gl| {
        draw_quad(ctx, gl, min, max, 1, (x0, y0), (x1, y1), r, color);
    });
}
