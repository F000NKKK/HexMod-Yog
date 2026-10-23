//! Hex Casting's `RenderLib.kt` (and the grid helpers of `HexUtils.kt`):
//! hex-grid ↔ pixel conversion, the zappy pattern lines, the little
//! hexagonal spots — built into a [`Mesh`] of colored triangles that
//! [`Mesh::draw`] sends to the GPU with a position-color shader, as the
//! original draws with Minecraft's `PositionColorShader`.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::Instant;
use yog_api::sync::{Mutex, OnceLock};

use yog_api::GfxContext;
use yog_api::gfx_core::{DataType, DrawMode, blend};
use yog_api::gfx_gl::{Attrib, Buffer, ShaderProgram, VertexArray};

use super::noise::get_noise;
use crate::math::{HexCoord, HexPattern};

pub const SQRT_3: f32 = 1.732_050_8;
const TAU: f64 = std::f64::consts::TAU;
const PI: f32 = std::f32::consts::PI;
/// Degrees between the triangles of a line's round caps and joins.
pub const CAP_THETA: f32 = 180.0 / 10.0;
pub const DEFAULT_READABILITY_OFFSET: f32 = 0.2;

pub use yog_api::ui::Vec2;

/// Minecraft's `Vec2` methods the original uses.
pub trait McVec2 {
    fn distance_to_sqr(self, o: Vec2) -> f32;
    /// `ZERO` when (almost) zero-length, like Minecraft's.
    fn normalized(self) -> Vec2;
}

impl McVec2 for Vec2 {
    fn distance_to_sqr(self, o: Vec2) -> f32 {
        let d = o - self;
        d.dot(d)
    }

    fn normalized(self) -> Vec2 {
        let l = self.length();
        if l < 1.0e-4 {
            Vec2::ZERO
        } else {
            Vec2::new(self.x / l, self.y / l)
        }
    }
}

/// Kotlin's `roundToInt` (`Math.round`: halves go up).
pub fn round_to_int(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// The pixel centre of grid point `coord`.
pub fn coord_to_px(coord: HexCoord, size: f32, offset: Vec2) -> Vec2 {
    Vec2::new(
        SQRT_3 * coord.0 as f32 + SQRT_3 / 2.0 * coord.1 as f32,
        1.5 * coord.1 as f32,
    ) * size
        + offset
}

/// The grid point nearest to pixel `px`.
pub fn px_to_coord(px: Vec2, size: f32, offset: Vec2) -> HexCoord {
    let o = px - offset;
    let mut qf = (SQRT_3 / 3.0 * o.x - 0.33333 * o.y) / size;
    let mut rf = (0.66666 * o.y) / size;
    let q = round_to_int(qf);
    let r = round_to_int(rf);
    qf -= q as f32;
    rf -= r as f32;
    if q.abs() >= r.abs() {
        HexCoord(q + round_to_int(qf + 0.5 * rf), r)
    } else {
        HexCoord(q, r + round_to_int(rf + 0.5 * qf))
    }
}

/// `HexCoord.rangeAround`: every point within `radius` steps of `center`.
pub fn range_around(center: HexCoord, radius: i32) -> Vec<HexCoord> {
    let mut out = Vec::new();
    for q in -radius..=radius {
        let lo = (-radius).max(-q - radius);
        let hi = radius.min(-q + radius);
        for r in lo..=hi {
            out.push(HexCoord(center.0 + q, center.1 + r));
        }
    }
    out
}

/// Every point of `pat` drawn from `origin`.
pub fn positions_from(pat: &HexPattern, origin: HexCoord) -> Vec<HexCoord> {
    pat.positions().into_iter().map(|p| p + origin).collect()
}

/// `HexPattern.toLines`: the pattern's points in pixels.
pub fn to_lines(pat: &HexPattern, size: f32, origin: Vec2) -> Vec<Vec2> {
    pat.positions()
        .into_iter()
        .map(|p| coord_to_px(p, size, origin))
        .collect()
}

/// The indices of every item that occurs more than once.
pub fn find_dup_indices<T: Eq + Hash>(pts: impl IntoIterator<Item = T>) -> HashSet<usize> {
    let mut dedup = HashMap::new();
    let mut found = HashSet::new();
    for (i, pt) in pts.into_iter().enumerate() {
        if let Some(&ix) = dedup.get(&pt) {
            found.insert(i);
            found.insert(ix);
        } else {
            dedup.insert(pt, i);
        }
    }
    found
}

/// `ClientTickCounter.getTotal()`: game ticks (20 a second) since the first
/// call, with the fraction.
pub fn ticks() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 20.0
}

// ── colors (`FastColor.ARGB32`) ─────────────────────────────────────────────

pub fn alpha(c: u32) -> i32 {
    (c >> 24) as i32
}
pub fn red(c: u32) -> i32 {
    ((c >> 16) & 0xFF) as i32
}
pub fn green(c: u32) -> i32 {
    ((c >> 8) & 0xFF) as i32
}
pub fn blue(c: u32) -> i32 {
    (c & 0xFF) as i32
}
pub fn argb(a: i32, r: i32, g: i32, b: i32) -> u32 {
    ((a as u32 & 0xFF) << 24)
        | ((r as u32 & 0xFF) << 16)
        | ((g as u32 & 0xFF) << 8)
        | (b as u32 & 0xFF)
}

/// A color from 0..1 components (`drawSpot`'s float overload).
pub fn argb_f(a: f32, r: f32, g: f32, b: f32) -> u32 {
    argb(
        (a * 255.0) as i32,
        (r * 255.0) as i32,
        (g * 255.0) as i32,
        (b * 255.0) as i32,
    )
}

fn screen(n: i32) -> i32 {
    (n + 255) / 2
}

/// Each channel halfway to white (the bright core of a line).
pub fn screen_col(n: u32) -> u32 {
    argb(alpha(n), screen(red(n)), screen(green(n)), screen(blue(n)))
}

pub fn dodge(n: i32) -> f32 {
    n as f32 * 0.9
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

fn rotate(v: Vec2, theta: f32) -> Vec2 {
    let (sin, cos) = theta.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.y * cos + v.x * sin)
}

// ── the mesh ─────────────────────────────────────────────────────────────────

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

    fn tri(&mut self, (c1, p1): (u32, Vec2), (c2, p2): (u32, Vec2), (c3, p3): (u32, Vec2)) {
        self.vertex(c1, p1);
        self.vertex(c2, p2);
        self.vertex(c3, p3);
    }

    /// A `TRIANGLE_FAN` around `center`, as triangles.
    fn fan(&mut self, color: u32, center: Vec2, rim: &[Vec2]) {
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

const VERT: &str = "
#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec4 aColor;
uniform vec2 uScreen;
out vec4 vColor;
void main() {
    vColor = aColor;
    gl_Position = vec4(aPos.x / uScreen.x * 2.0 - 1.0, 1.0 - aPos.y / uScreen.y * 2.0, 0.0, 1.0);
}";

const FRAG: &str = "
#version 330 core
in vec4 vColor;
out vec4 fragColor;
void main() {
    fragColor = vColor;
}";

struct Gl {
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

static GL: Mutex<Option<Gl>> = Mutex::new(None);

// ── RenderLib ────────────────────────────────────────────────────────────────

/// `drawLineSeq`: a `width`-thick polyline through `points`, colored from
/// `tail` to `head` along its length when `gradient` (holding Ctrl in the
/// original), else `tail` throughout; round joins and caps.
pub fn draw_line_seq(
    mesh: &mut Mesh,
    points: &[Vec2],
    width: f32,
    tail: u32,
    head: u32,
    gradient: bool,
) {
    if points.len() <= 1 {
        return;
    }
    let (r1, g1, b1, a1) = (
        red(tail) as f32,
        green(tail) as f32,
        blue(tail) as f32,
        alpha(tail) as f32,
    );
    let head_source = if gradient { head } else { tail };
    let (r2, g2, b2, a2) = (
        red(head_source) as f32,
        green(head_source) as f32,
        blue(head_source) as f32,
        alpha(head_source) as f32,
    );

    let n = points.len();
    let mut join_angles = vec![0f32; n];
    let mut join_offsets = vec![0f32; n];
    for i in 2..n {
        let (p0, p1, p2) = (points[i - 2], points[i - 1], points[i]);
        let prev = p1 - p0;
        let next = p2 - p1;
        let angle = (prev.x * next.y - prev.y * next.x).atan2(prev.x * next.x + prev.y * next.y);
        join_angles[i - 1] = angle;
        let clamp = prev.length().min(next.length()) / (width * 0.5);
        join_offsets[i - 1] = (angle.sin() / (1.0 + angle.cos())).clamp(-clamp, clamp);
    }

    let color = |t: f32| {
        argb(
            lerp(t, a1, a2) as i32,
            lerp(t, r1, r2) as i32,
            lerp(t, g1, g2) as i32,
            lerp(t, b1, b2) as i32,
        )
    };

    for i in 0..n - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        let tangent = (p2 - p1).normalized() * (width * 0.5);
        let normal = Vec2::new(-tangent.y, tangent.x);
        let color1 = color(i as f32 / n as f32);
        let color2 = color((i as f32 + 1.0) / n as f32);
        let (jlow, jhigh) = (join_offsets[i], join_offsets[i + 1]);
        let p1_down = p1 + tangent * 0f32.max(jlow) + normal;
        let p1_up = p1 + tangent * 0f32.max(-jlow) - normal;
        let p2_down = p2 - tangent * 0f32.max(jhigh) + normal;
        let p2_up = p2 - tangent * 0f32.max(-jhigh) - normal;

        mesh.tri((color1, p1_down), (color1, p1), (color1, p1_up));
        mesh.tri((color1, p1_down), (color1, p1_up), (color2, p2_up));
        mesh.tri((color1, p1_down), (color2, p2_up), (color2, p2));
        mesh.tri((color1, p1_down), (color2, p2), (color2, p2_down));

        if i > 0 {
            // The connector to the previous segment.
            let sangle = join_angles[i];
            let angle = sangle.abs();
            let rnormal = -normal;
            let join_steps = (angle * 180.0 / (CAP_THETA * PI)).ceil() as i32;
            if join_steps < 1 {
                continue;
            }
            if sangle < 0.0 {
                let mut prev_vert = p1 - rnormal;
                for j in 1..=join_steps {
                    let fan = rotate(rnormal, -sangle * (j as f32 / join_steps as f32));
                    let fan_shift = p1 - fan;
                    mesh.tri((color1, p1), (color1, prev_vert), (color1, fan_shift));
                    prev_vert = fan_shift;
                }
            } else {
                let start_fan = rotate(normal, -sangle);
                let mut prev_vert = p1 - start_fan;
                for j in (0..join_steps).rev() {
                    let fan = rotate(normal, -sangle * (j as f32 / join_steps as f32));
                    let fan_shift = p1 - fan;
                    mesh.tri((color1, p1), (color1, prev_vert), (color1, fan_shift));
                    prev_vert = fan_shift;
                }
            }
        }
    }

    let mut cap = |color: u32, point: Vec2, prev: Vec2| {
        let tangent = (point - prev).normalized() * (0.5 * width);
        let normal = Vec2::new(-tangent.y, tangent.x);
        let steps = (180.0 / CAP_THETA).ceil() as i32;
        let rim: Vec<Vec2> = (0..=steps)
            .rev()
            .map(|j| point + rotate(normal, -PI * (j as f32 / steps as f32)))
            .collect();
        mesh.fan(color, point, &rim);
    };
    cap(
        argb(a1 as i32, r1 as i32, g1 as i32, b1 as i32),
        points[0],
        points[1],
    );
    cap(
        argb(a2 as i32, r2 as i32, g2 as i32, b2 as i32),
        points[n - 1],
        points[n - 2],
    );
}

/// `drawSpot`: a little hexagon (Hex Casting's "circle").
pub fn draw_spot(mesh: &mut Mesh, point: Vec2, radius: f32, color: u32) {
    const FRAC: i32 = 6;
    let rim: Vec<Vec2> = (0..=FRAC)
        .map(|i| {
            let theta = i as f32 / FRAC as f32 * TAU as f32;
            Vec2::new(
                theta.cos() * radius + point.x,
                theta.sin() * radius + point.y,
            )
        })
        .collect();
    mesh.fan(color, point, &rim);
}

/// How a pattern is drawn (`drawPatternFromPoints`' style arguments).
#[derive(Debug, Clone, Copy)]
pub struct PatternStyle {
    pub tail: u32,
    pub head: u32,
    pub flow_irregular: f32,
    pub readability_offset: f32,
    pub last_segment_len_prop: f32,
    pub seed: f64,
    /// Color the stroke tail → head (Ctrl held).
    pub gradient: bool,
}

/// `drawPatternFromPoints`: the zappy double line through `points` (grid
/// positions in pixels) and a spot on each node — the last one only when
/// `draw_last`.
pub fn draw_pattern_from_points(
    mesh: &mut Mesh,
    points: &[Vec2],
    dup_indices: Option<&HashSet<usize>>,
    draw_last: bool,
    style: PatternStyle,
) {
    let zappy = make_zappy(
        points,
        dup_indices,
        10,
        2.5,
        0.1,
        style.flow_irregular,
        style.readability_offset,
        style.last_segment_len_prop,
        style.seed,
        ticks(),
    );
    let nodes = if draw_last {
        points
    } else {
        &points[..points.len().saturating_sub(1)]
    };
    draw_line_seq(mesh, &zappy, 5.0, style.tail, style.head, style.gradient);
    draw_line_seq(
        mesh,
        &zappy,
        2.0,
        screen_col(style.tail),
        screen_col(style.head),
        style.gradient,
    );
    let h = style.head;
    let spot = argb_f(
        alpha(h) as f32 / 255.0,
        dodge(red(h)) / 255.0,
        dodge(green(h)) / 255.0,
        dodge(blue(h)) / 255.0,
    );
    for &node in nodes {
        draw_spot(mesh, node, 2.0, spot);
    }
}

/// `makeZappy`: each segment split into `hops` jittered sub-points that
/// shake over time (`ticks`). Points in `dup_indices` (visited twice) are
/// pulled `readability_offset` of the way into their segments so crossing
/// strokes stay readable.
#[allow(clippy::too_many_arguments)] // the original's signature
pub fn make_zappy(
    bare_points: &[Vec2],
    dup_indices: Option<&HashSet<usize>>,
    hops: i32,
    variance: f32,
    speed: f32,
    flow_irregular: f32,
    readability_offset: f32,
    last_segment_len_prop: f32,
    seed: f64,
    ticks: f64,
) -> Vec<Vec2> {
    if bare_points.is_empty() {
        return Vec::new();
    }
    let zappify = |points: &[Vec2], truncate_last: bool| -> Vec<Vec2> {
        let scale_variance = |it: f64| 1f64.min(8.0 * (0.5 - (0.5 - it).abs()));
        let z_seed = ticks * speed as f64;
        let mut out = Vec::with_capacity(points.len() * hops as usize);
        out.push(points[0]);
        for (i, pair) in points.windows(2).enumerate() {
            let (src, target) = (pair[0], pair[1]);
            let delta = target - src;
            let hop_dist = src.distance_to_sqr(target).sqrt() / hops as f32;
            let max_variance = hop_dist * variance;
            let max_j = if truncate_last && i == points.len() - 2 {
                round_to_int(last_segment_len_prop * hops as f32)
            } else {
                hops
            };
            for j in 1..=max_j {
                let progress = j as f64 / (hops + 1) as f64;
                let pos = src + delta * progress as f32;
                let minor_perturb =
                    get_noise(i as f64, j as f64, z_seed.sin()) * flow_irregular as f64;
                let theta = (3.0
                    * get_noise(i as f64 + progress + minor_perturb - z_seed, 1337.0, seed)
                    * TAU) as f32;
                let r = (get_noise(i as f64 + progress - z_seed, 69420.0, seed)
                    * max_variance as f64
                    * scale_variance(progress)) as f32;
                out.push(pos + Vec2::new(r * theta.cos(), r * theta.sin()));
                if j == hops {
                    out.push(target);
                }
            }
        }
        out
    };

    let Some(dups) = dup_indices else {
        return zappify(bare_points, true);
    };
    let mut points = Vec::new();
    let mut daisy_chain = Vec::new();
    for (i, pair) in bare_points.windows(2).enumerate() {
        let (head, tail) = (pair[0], pair[1]);
        let tangent = (tail - head) * readability_offset;
        if i != 0 && dups.contains(&i) {
            daisy_chain.push(head + tangent);
        } else {
            daisy_chain.push(head);
        }
        if i == bare_points.len() - 2 {
            daisy_chain.push(tail);
            points.extend(zappify(&daisy_chain, true));
        } else if dups.contains(&(i + 1)) {
            daisy_chain.push(tail - tangent);
            points.extend(zappify(&daisy_chain, false));
            daisy_chain.clear();
        }
    }
    points
}

/// `findCenter`: the middle of the points' bounding box.
pub fn find_center(points: &[Vec2]) -> Vec2 {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
}

/// `getCenteredPattern`: the scale that fits `pattern` into a third of
/// `width`×`height` (at most `min_size`), and its points around `0,0`.
pub fn get_centered_pattern(
    pattern: &HexPattern,
    width: f32,
    height: f32,
    min_size: f32,
) -> (f32, Vec<Vec2>) {
    let lines1 = to_lines(pattern, 1.0, Vec2::ZERO);
    let com1 = find_center(&lines1);
    let (mut max_dx, mut max_dy) = (-1f32, -1f32);
    for dot in &lines1 {
        max_dx = max_dx.max((dot.x - com1.x).abs());
        max_dy = max_dy.max((dot.y - com1.y).abs());
    }
    let scale = min_size.min((width / 3.0 / max_dx).min(height / 3.0 / max_dy));
    let com2 = find_center(&to_lines(pattern, scale, Vec2::ZERO));
    (scale, to_lines(pattern, scale, -com2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn px_round_trips() {
        let off = Vec2::new(213.0, 120.0);
        for q in -4..=4 {
            for r in -4..=4 {
                let c = HexCoord(q, r);
                let px = coord_to_px(c, 18.0, off);
                assert_eq!(px_to_coord(px, 18.0, off), c);
                assert_eq!(px_to_coord(px + Vec2::new(3.0, -2.5), 18.0, off), c);
            }
        }
    }

    #[test]
    fn range_around_is_a_hexagon() {
        let pts = range_around(HexCoord(2, -1), 3);
        assert_eq!(pts.len(), 37);
        assert!(pts.contains(&HexCoord(5, -1)));
        assert!(!pts.contains(&HexCoord(5, 0)));
    }

    #[test]
    fn dup_indices() {
        let d = find_dup_indices([1, 2, 3, 1, 4]);
        assert_eq!(d, HashSet::from([0, 3]));
    }

    #[test]
    fn zappy_keeps_the_ends() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(45.0, 26.0),
        ];
        let z = make_zappy(&pts, None, 10, 2.5, 0.1, 0.2, 0.2, 1.0, 0.0, 12.5);
        assert_eq!(z[0], pts[0]);
        assert_eq!(*z.last().unwrap(), pts[2]);
        assert_eq!(z.len(), 1 + 2 * 11);
        // Stopping short of the last point (the line being drawn).
        let z = make_zappy(&pts, None, 10, 2.5, 0.1, 0.2, 0.2, 0.8, 0.0, 12.5);
        assert_eq!(z.len(), 1 + 11 + 8);
    }

    #[test]
    fn line_and_spot_meshes() {
        let mut m = Mesh::new();
        draw_spot(&mut m, Vec2::new(5.0, 5.0), 2.0, 0xFFFFFFFF);
        assert_eq!(m.vertex_count(), 6 * 3);
        let mut m = Mesh::new();
        draw_line_seq(
            &mut m,
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            5.0,
            0xFF000000,
            0xFFFFFFFF,
            true,
        );
        // One segment (4 triangles) and two caps of 10 triangles.
        assert_eq!(m.vertex_count(), (4 + 20) * 3);
    }

    #[test]
    fn centered_pattern_is_centered() {
        let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        let (scale, pts) = get_centered_pattern(&p, 128.0, 128.0, 16.0);
        assert!(scale > 0.0 && scale <= 16.0);
        let c = find_center(&pts);
        assert!(c.x.abs() < 1e-3 && c.y.abs() < 1e-3);
    }
}
