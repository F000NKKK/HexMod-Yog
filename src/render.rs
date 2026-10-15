//! What Hex Casting draws in the world: the pattern on a placed slate
//! (`BlockEntitySlateRenderer`, `WorldlyPatternRenderHelpers`) and the
//! player's own sentinel (`HexAdditionalRenderers.renderSentinel`).
//!
//! The server sends each player the slates near them and their sentinel;
//! the client draws them with the world's camera.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use yog_api::gfx_core::{DataType, DrawMode, blend};
use yog_api::gfx_gl::{Buffer, ShaderProgram, VertexArray};
use yog_api::{GfxContext, Registry, Server, Storage, split_state};

use crate::casting::env::{MOD_ID, get_pigment, get_sentinel, to_ypos};
use crate::item::pigment::Pigment;
use crate::math::{HexPattern, Vec3};
use crate::ui::render_lib::{Mesh, Vec2, draw_line_seq, get_centered_pattern, screen_col};
use crate::wire::{Reader, Writer};

/// Server → client: the slates near the player.
const CH_SLATES: &str = "hexcasting:slates";
/// Server → client: the player's sentinel.
const CH_SENTINEL: &str = "hexcasting:sentinel";
/// How far slates are sent, in blocks.
const SLATE_RANGE: f64 = 48.0;
const EVERY: u64 = 10;

/// `PatternColors.DEFAULT_PATTERN_COLOR`: inner and outer strokes.
const INNER: u32 = 0xff_554d54;
const OUTER: u32 = 0xff_d2c8c8;
/// `SLATE_WOBBLY_PURPLE_COLOR` (`glowyStroke`): an energized slate.
const GLOW: u32 = 0xff_cfa0f3;

// ── slates ──────────────────────────────────────────────────────────────────

/// A placed slate with a pattern, as the client draws it.
#[derive(Debug, Clone, PartialEq)]
struct Slate {
    pos: (i32, i32, i32),
    /// `floor`, `wall` or `ceiling`.
    face: String,
    facing: String,
    energized: bool,
    pattern: HexPattern,
}

/// Every slate with a pattern, by dimension and position (the server's).
static SLATES: LazyLock<Mutex<HashMap<(String, (i32, i32, i32)), HexPattern>>> = LazyLock::new(Default::default);
const SLATES_KEY: &str = "render.slates";

fn global(srv: &dyn Server) -> Storage {
    Storage::open(&srv.game_dir(), MOD_ID)
}

fn save_slates(srv: &dyn Server) {
    let slates = SLATES.lock().unwrap();
    let lines: Vec<String> = slates
        .iter()
        .map(|((dim, p), pat)| format!("{dim}|{},{},{}|{}", p.0, p.1, p.2, pat.serialized_form()))
        .collect();
    global(srv).set(SLATES_KEY, lines.join("\n"));
}

fn load_slates(srv: &dyn Server) {
    let text = global(srv).get_str(SLATES_KEY).unwrap_or("").to_owned();
    let mut slates = SLATES.lock().unwrap();
    slates.clear();
    for line in text.lines() {
        let mut it = line.splitn(3, '|');
        let (Some(dim), Some(pos), Some(pat)) = (it.next(), it.next(), it.next()) else { continue };
        let mut n = pos.split(',').filter_map(|v| v.parse().ok());
        let (Some(x), Some(y), Some(z)) = (n.next(), n.next(), n.next()) else { continue };
        if let Some(p) = HexPattern::from_serialized(pat) {
            slates.insert((dim.to_owned(), (x, y, z)), p);
        }
    }
}

/// A slate was placed with `pattern`, or broken (`None`).
pub fn slate_changed(srv: &dyn Server, dim: &str, pos: (i32, i32, i32), pattern: Option<&HexPattern>) {
    {
        let mut slates = SLATES.lock().unwrap();
        match pattern {
            Some(p) => slates.insert((dim.to_owned(), pos), p.clone()),
            None => slates.remove(&(dim.to_owned(), pos)),
        };
    }
    save_slates(srv);
}

fn encode_slates(slates: &[Slate]) -> Vec<u8> {
    let mut w = Writer::new();
    w.int(slates.len() as i64);
    for s in slates {
        w.int(s.pos.0 as i64).int(s.pos.1 as i64).int(s.pos.2 as i64);
        w.str(&s.face).str(&s.facing).int(s.energized as i64).str(&s.pattern.serialized_form());
    }
    w.finish().into_bytes()
}

fn decode_slates(b: &[u8]) -> Option<Vec<Slate>> {
    let mut r = Reader::new(std::str::from_utf8(b).ok()?);
    let n = r.int()?;
    (0..n)
        .map(|_| {
            let pos = (r.int()? as i32, r.int()? as i32, r.int()? as i32);
            let face = r.str()?.to_owned();
            let facing = r.str()?.to_owned();
            let energized = r.int()? != 0;
            let pattern = HexPattern::from_serialized(r.str()?)?;
            Some(Slate { pos, face, facing, energized, pattern })
        })
        .collect()
}

// ── the sentinel ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Shown {
    pos: Vec3,
    great: bool,
    pigment: Pigment,
}

fn encode_sentinel(s: Option<&Shown>) -> Vec<u8> {
    let mut w = Writer::new();
    match s {
        None => {
            w.int(0);
        }
        Some(s) => {
            w.int(1).num(s.pos.x).num(s.pos.y).num(s.pos.z).int(s.great as i64);
            w.str(&s.pigment.item).str(&s.pigment.owner);
        }
    }
    w.finish().into_bytes()
}

fn decode_sentinel(b: &[u8]) -> Option<Option<Shown>> {
    let mut r = Reader::new(std::str::from_utf8(b).ok()?);
    if r.int()? == 0 {
        return Some(None);
    }
    let pos = Vec3::new(r.num()?, r.num()?, r.num()?);
    let great = r.int()? != 0;
    let pigment = Pigment { item: r.str()?.to_owned(), owner: r.str()?.to_owned() };
    Some(Some(Shown { pos, great, pigment }))
}

// ── the server ──────────────────────────────────────────────────────────────

/// What each player was last sent: slates, sentinel.
static SENT: LazyLock<Mutex<HashMap<String, (Vec<u8>, Vec<u8>)>>> = LazyLock::new(Default::default);
static TICKS: Mutex<u64> = Mutex::new(0);

fn tick(srv: &dyn Server) {
    let now = {
        let mut t = TICKS.lock().unwrap();
        *t += 1;
        *t
    };
    if !now.is_multiple_of(EVERY) {
        return;
    }
    let slates = SLATES.lock().unwrap().clone();
    let mut sent = SENT.lock().unwrap();
    for name in srv.online_players() {
        let Some(p) = srv.player_info(&name) else { continue };
        let Some(e) = srv.entity_info(&p.uuid) else { continue };
        let here = Vec3::from_tuple(e.pos);
        let near: Vec<Slate> = slates
            .iter()
            .filter(|((dim, pos), _)| *dim == e.dimension && Vec3::center_of(*pos).distance_sqr(here) <= SLATE_RANGE * SLATE_RANGE)
            .filter_map(|((dim, pos), pattern)| {
                let state = srv.get_block_state(dim, to_ypos(*pos))?;
                let (_, props) = split_state(&state);
                let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v).to_owned();
                Some(Slate { pos: *pos, face: get("face"), facing: get("facing"), energized: get("energized") == "true", pattern: pattern.clone() })
            })
            .collect();
        let sentinel = get_sentinel(srv, &p.uuid)
            .filter(|s| s.dim == e.dimension)
            .map(|s| Shown { pos: s.pos, great: s.great, pigment: get_pigment(srv, &p.uuid) });
        let (a, b) = (encode_slates(&near), encode_sentinel(sentinel.as_ref()));
        let last = sent.entry(name.clone()).or_default();
        if last.0 != a {
            srv.send_to_player(&name, CH_SLATES, &a);
            last.0 = a;
        }
        if last.1 != b {
            srv.send_to_player(&name, CH_SENTINEL, &b);
            last.1 = b;
        }
    }
}

// ── the client ──────────────────────────────────────────────────────────────

struct Seen {
    slates: Vec<Slate>,
    /// The slates' triangles, `x, y, z, r, g, b, a` (world coordinates).
    slate_mesh: Vec<f32>,
    sentinel: Option<Shown>,
}

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(|| Mutex::new(Seen { slates: Vec::new(), slate_mesh: Vec::new(), sentinel: None }));

/// A horizontal direction's unit vector.
fn dir(name: &str) -> Vec3 {
    match name {
        "north" => Vec3::new(0.0, 0.0, -1.0),
        "south" => Vec3::new(0.0, 0.0, 1.0),
        "west" => Vec3::new(-1.0, 0.0, 0.0),
        _ => Vec3::new(1.0, 0.0, 0.0),
    }
}

/// Where a slate's pattern lies: the middle of the plate's face, and the
/// pattern's right and up in the world.
fn slate_plane(s: &Slate) -> (Vec3, Vec3, Vec3) {
    const OUT: f64 = 1.0 / 16.0 + 0.002;
    let c = Vec3::center_of(s.pos);
    let f = dir(&s.facing);
    // Right of someone facing the pattern.
    let right = Vec3::new(f.z, 0.0, -f.x);
    match s.face.as_str() {
        "floor" => (c + Vec3::new(0.0, OUT - 0.5, 0.0), right, f * -1.0),
        "ceiling" => (c + Vec3::new(0.0, 0.5 - OUT, 0.0), right * -1.0, f * -1.0),
        _ => (c - f * (0.5 - OUT), right, Vec3::new(0.0, 1.0, 0.0)),
    }
}

/// `WorldlyPatternRenderHelpers.renderPatternForSlate`: the pattern in a
/// square padded 2/16, strokes 0.8/16 wide — glowing when energized.
fn slate_triangles(s: &Slate, out: &mut Vec<f32>) {
    const PAD: f32 = 2.0 / 16.0;
    const STROKE: f32 = 0.8 / 16.0;
    // Built in 1/256-block units, so the line helpers' minimums don't show.
    const UNIT: f32 = 256.0;
    let mut mesh = Mesh::new();
    let size = (1.0 - 2.0 * PAD) * UNIT;
    let (_, pts) = get_centered_pattern(&s.pattern, size, size, size / 4.0);
    let center = Vec2::new(UNIT / 2.0, UNIT / 2.0);
    let pts: Vec<Vec2> = pts.into_iter().map(|p| p + center).collect();
    let (inner, outer) = if s.energized { (screen_col(GLOW), GLOW) } else { (INNER, OUTER) };
    draw_line_seq(&mut mesh, &pts, STROKE * 2.0 * UNIT, outer, outer, false);
    draw_line_seq(&mut mesh, &pts, STROKE * UNIT, inner, inner, false);
    let (origin, right, up) = slate_plane(s);
    for v in mesh.vertices().chunks_exact(6) {
        let (px, py) = (v[0] / UNIT - 0.5, 0.5 - v[1] / UNIT);
        let p = origin + right * px as f64 + up * py as f64;
        out.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32, v[2], v[3], v[4], v[5]]);
    }
}

/// The icosahedron in the unit sphere (`Icos`): apexes and rings.
fn icosahedron() -> Vec<([f32; 3], [f32; 3])> {
    let ring = |y: f32, start: f32| -> Vec<[f32; 3]> {
        (0..5)
            .map(|i| {
                let a = start + i as f32 * std::f32::consts::TAU / 5.0;
                let rad = (1.0 - y * y).sqrt();
                [rad * a.cos(), y, rad * a.sin()]
            })
            .collect()
    };
    let (top, bottom) = ([0.0, 1.0, 0.0], [0.0, -1.0, 0.0]);
    // The rings sit at ±1/√5.
    let y = 1.0 / 5f32.sqrt();
    let top_ring = ring(y, 0.0);
    let bottom_ring = ring(-y, std::f32::consts::PI / 5.0);
    let mut edges = Vec::new();
    for (apex, ring) in [(bottom, &bottom_ring), (top, &top_ring)] {
        for i in 0..5 {
            edges.push((apex, ring[i]));
            edges.push((ring[i], ring[(i + 1) % 5]));
        }
    }
    for i in 0..5 {
        let b = bottom_ring[i];
        // The bottom ring sits a fifth of a turn round from the top one: each
        // of its points joins the two top points either side of it.
        edges.push((top_ring[i], b));
        edges.push((b, top_ring[(i + 1) % 5]));
    }
    edges
}

/// `renderSentinel`: the icosahedron bobbing and spinning (tumbling too
/// for a greater sentinel), coloured by the owner's pigment, seen through
/// walls; its lines as thin quads facing the camera.
fn sentinel_triangles(s: &Shown, ticks: f32, cam: Vec3, out: &mut Vec<f32>) {
    let time = ticks / 2.0;
    let bob = (time / 20.0).sin() * 0.1;
    let spin = time / 30.0;
    let tumble = if s.great { spin / 8.0 } else { 0.0 };
    let place = |p: [f32; 3]| -> Vec3 {
        let (x, y, z) = (p[0] * 0.5, p[1] * 0.5, p[2] * 0.5);
        let (sy, cy) = spin.sin_cos();
        let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
        let (st, ct) = tumble.sin_cos();
        let (y, z) = (y * ct - z * st, y * st + z * ct);
        s.pos + Vec3::new(x as f64, (y + bob) as f64, z as f64)
    };
    const WIDTH: f64 = 0.02;
    for (a, b) in icosahedron() {
        let (pa, pb) = (place(a), place(b));
        let along = pb - pa;
        let to_eye = cam - pa;
        let side = along.cross(to_eye).normalize() * WIDTH;
        let color = |p: [f32; 3]| {
            let c = s.pigment.color(time, Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0]
        };
        let (ca, cb) = (color(a), color(b));
        let quad = [(pa - side, ca), (pa + side, ca), (pb + side, cb), (pa - side, ca), (pb + side, cb), (pb - side, cb)];
        for (p, c) in quad {
            out.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32, c[0], c[1], c[2], c[3]]);
        }
    }
}

const VERT: &str = "#version 330 core
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec4 aColor;
uniform mat4 uViewProj;
uniform vec3 uCamera;
out vec4 vColor;
void main() {
    vColor = aColor;
    gl_Position = uViewProj * vec4(aPos - uCamera, 1.0);
}";

const FRAG: &str = "#version 330 core
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

static GL: Mutex<Option<Gl>> = Mutex::new(None);
/// The client's clock for the sentinel's spin, in ticks.
fn client_ticks() -> f32 {
    static START: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    START.elapsed().as_secs_f32() * 20.0
}

fn draw(gfx: &GfxContext) {
    let seen = SEEN.lock().unwrap();
    if seen.slate_mesh.is_empty() && seen.sentinel.is_none() {
        return;
    }
    let mut gl = GL.lock().unwrap();
    if gl.is_none() {
        let Ok(prog) = gfx.create_shader(VERT, FRAG) else { return };
        let vbo = gfx.create_buffer();
        let vao = gfx.create_vao();
        vao.attrib(gfx, &vbo, 0, 3, DataType::F32, false, 28, 0);
        vao.attrib(gfx, &vbo, 1, 4, DataType::F32, false, 28, 12);
        *gl = Some(Gl { prog, vao, vbo });
    }
    let Some(gl) = gl.as_ref() else { return };
    let cam = gfx.camera_pos();
    gl.prog.uniform_mat4(gfx, "uViewProj", &gfx.view_proj());
    // Coordinates are sent as offsets from the camera's block, so floats keep their precision far out.
    let base = [cam[0].floor(), cam[1].floor(), cam[2].floor()];
    gl.prog.uniform_3f(gfx, "uCamera", cam[0] - base[0], cam[1] - base[1], cam[2] - base[2]);
    let local = |v: &[f32]| -> Vec<f32> {
        v.chunks_exact(7)
            .flat_map(|c| [c[0] - base[0], c[1] - base[1], c[2] - base[2], c[3], c[4], c[5], c[6]])
            .collect()
    };
    gfx.set_blend(true, blend::SRC_ALPHA, blend::ONE_MINUS_SRC_ALPHA);
    if !seen.slate_mesh.is_empty() {
        gfx.set_depth(true, true);
        let verts = local(&seen.slate_mesh);
        // SAFETY: `f32`s are plain data.
        unsafe { gl.vbo.upload(gfx, &verts, true) };
        gfx.draw_arrays(&gl.vao, &gl.prog, DrawMode::Triangles, 0, (verts.len() / 7) as u32);
    }
    if let Some(s) = &seen.sentinel {
        let ticks = client_ticks();
        let mut tris = Vec::new();
        let camera = Vec3::new(cam[0] as f64, cam[1] as f64, cam[2] as f64);
        sentinel_triangles(s, ticks, camera, &mut tris);
        gfx.set_depth(false, false);
        let verts = local(&tris);
        unsafe { gl.vbo.upload(gfx, &verts, true) };
        gfx.draw_arrays(&gl.vao, &gl.prog, DrawMode::Triangles, 0, (verts.len() / 7) as u32);
    }
}

pub fn register(registry: &mut Registry) {
    registry.on_server_started(load_slates);
    registry.on_tick(tick);
    registry.on_player_leave(|e, _, _| {
        SENT.lock().unwrap().remove(&e.player_name);
        true
    });
    registry.on_client_packet(CH_SLATES, |ev, _| {
        if let Some(slates) = decode_slates(&ev.payload) {
            let mut seen = SEEN.lock().unwrap();
            let mut mesh = Vec::new();
            for s in &slates {
                slate_triangles(s, &mut mesh);
            }
            seen.slates = slates;
            seen.slate_mesh = mesh;
        }
    });
    registry.on_client_packet(CH_SENTINEL, |ev, _| {
        if let Some(s) = decode_sentinel(&ev.payload) {
            SEEN.lock().unwrap().sentinel = s;
        }
    });
    registry.on_world_render(draw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn slates_and_sentinels_survive_the_wire() {
        let pattern = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        let s = vec![Slate { pos: (1, -2, 3), face: "wall".into(), facing: "north".into(), energized: true, pattern }];
        assert_eq!(decode_slates(&encode_slates(&s)), Some(s));
        let sentinel = Shown { pos: Vec3::new(1.5, 2.0, -3.25), great: true, pigment: Pigment::default() };
        assert_eq!(decode_sentinel(&encode_sentinel(Some(&sentinel))), Some(Some(sentinel)));
        assert_eq!(decode_sentinel(&encode_sentinel(None)), Some(None));
    }

    #[test]
    fn a_slate_pattern_lies_on_its_plate() {
        let pattern = HexPattern::from_angles("waqaw", HexDir::East).unwrap();
        let s = Slate { pos: (0, 0, 0), face: "floor".into(), facing: "south".into(), energized: false, pattern };
        let mut out = Vec::new();
        slate_triangles(&s, &mut out);
        assert!(!out.is_empty());
        for v in out.chunks_exact(7) {
            assert!((v[1] - (1.0 / 16.0 + 0.002)).abs() < 1e-4, "on the plate's top: {}", v[1]);
            assert!((0.0..=1.0).contains(&v[0]) && (0.0..=1.0).contains(&v[2]));
        }
    }

    #[test]
    fn the_icosahedron_has_thirty_equal_edges_on_the_unit_sphere() {
        let edges = icosahedron();
        assert_eq!(edges.len(), 30);
        let len = |(a, b): ([f32; 3], [f32; 3])| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let first = len(edges[0]);
        assert!(edges.iter().all(|&e| (len(e) - first).abs() < 1e-4), "every edge as long");
        for (a, b) in edges {
            for p in [a, b] {
                let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                assert!((r - 1.0).abs() < 1e-4);
            }
        }
    }
}
