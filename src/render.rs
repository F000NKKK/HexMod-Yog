//! What Hex Casting draws in the world: the pattern on a placed slate
//! (`BlockEntitySlateRenderer`, `WorldlyPatternRenderHelpers`) and the
//! player's own sentinel (`HexAdditionalRenderers.renderSentinel`).
//!
//! The server sends each player the slates near them and their sentinel;
//! the client draws them with the world's camera.

use std::collections::HashMap;
use yog_api::sync::{LazyLock, Mutex};
use yog_api::yog_ids::time::TICKS_PER_SECOND;

use yog_api::gfx_core::{DrawMode, blend};
use yog_api::gfx_gl::{Buffer, ShaderProgram, VertexArray};
use yog_api::{GfxContext, Registry, split_state};

use crate::casting::env::{get_pigment, get_sentinel, to_ypos};
use crate::item::pigment::Pigment;
use crate::math::{HexPattern, Vec3};
use crate::ui::render_lib::{Line, Mesh, Vec2, draw_line_seq, get_centered_pattern, screen_col};
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
    /// A bookshelf's books are tinted with its iota's color.
    tint: Option<u32>,
}

/// Every slate with a pattern, by dimension and position (the server's).
/// A block by its dimension and position.
type At = (String, (i32, i32, i32));
/// What a player was last sent: the slates' bytes and the sentinel's.
type Sent = (Vec<u8>, Vec<u8>);

static SLATES: LazyLock<Mutex<HashMap<At, HexPattern>>> = LazyLock::new(Default::default);
/// Where they're kept in the world store.
const SLATES_KEY: &str = "render_slates";

/// A slate with a pattern, as it's kept with the world.
#[derive(yog_api::Schema)]
struct KeptSlate {
    dim: String,
    pos: (i32, i32, i32),
    pattern: String,
}

fn save_slates() {
    let slates: Vec<KeptSlate> = SLATES
        .lock()
        .iter()
        .map(|((dim, pos), p)| KeptSlate {
            dim: dim.clone(),
            pos: *pos,
            pattern: p.serialized_form(),
        })
        .collect();
    if let Some(store) = yog_api::storage::world() {
        store.set(SLATES_KEY, &slates);
    }
}

fn load_slates() {
    let kept: Vec<KeptSlate> = yog_api::storage::world()
        .map(|s| s.get_or_default(SLATES_KEY))
        .unwrap_or_default();
    *SLATES.lock() = kept
        .into_iter()
        .filter_map(|k| Some(((k.dim, k.pos), HexPattern::from_serialized(&k.pattern)?)))
        .collect();
}

/// A bookshelf got a pattern, or lost it (`None`): drawn on its front.
pub fn shelf_changed(dim: &str, pos: (i32, i32, i32), pattern: Option<&HexPattern>) {
    slate_changed(dim, pos, pattern);
}

/// A slate was placed with `pattern`, or broken (`None`).
pub fn slate_changed(dim: &str, pos: (i32, i32, i32), pattern: Option<&HexPattern>) {
    {
        let mut slates = SLATES.lock();
        match pattern {
            Some(p) => slates.insert((dim.to_owned(), pos), p.clone()),
            None => slates.remove(&(dim.to_owned(), pos)),
        };
    }
    save_slates();
}

fn encode_slates(dim: &str, slates: &[Slate]) -> Vec<u8> {
    let mut w = Writer::new();
    w.str(dim).int(slates.len() as i64);
    for s in slates {
        w.int(s.pos.0 as i64)
            .int(s.pos.1 as i64)
            .int(s.pos.2 as i64);
        w.str(&s.face)
            .str(&s.facing)
            .int(s.energized as i64)
            .str(&s.pattern.serialized_form());
        w.int(s.tint.map_or(-1, |c| c as i64));
    }
    w.finish().into_bytes()
}

fn decode_slates(b: &[u8]) -> Option<(String, Vec<Slate>)> {
    let mut r = Reader::new(std::str::from_utf8(b).ok()?);
    let dim = r.str()?.to_owned();
    let n = r.int()?;
    let slates = (0..n)
        .map(|_| {
            let pos = (r.int()? as i32, r.int()? as i32, r.int()? as i32);
            let face = r.str()?.to_owned();
            let facing = r.str()?.to_owned();
            let energized = r.int()? != 0;
            let pattern = HexPattern::from_serialized(r.str()?)?;
            let tint = r.int()?;
            Some(Slate {
                pos,
                face,
                facing,
                energized,
                pattern,
                tint: (tint >= 0).then_some(tint as u32),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((dim, slates))
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
            w.int(1)
                .num(s.pos.x)
                .num(s.pos.y)
                .num(s.pos.z)
                .int(s.great as i64);
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
    let pigment = Pigment {
        item: r.str()?.to_owned(),
        owner: r.str()?.to_owned(),
    };
    Some(Some(Shown {
        pos,
        great,
        pigment,
    }))
}

// ── the server ──────────────────────────────────────────────────────────────

/// What each player was last sent: slates, sentinel.
static SENT: LazyLock<Mutex<HashMap<String, Sent>>> = LazyLock::new(Default::default);
static TICKS: Mutex<u64> = Mutex::new(0);

fn tick() {
    let now = {
        let mut t = TICKS.lock();
        *t += 1;
        *t
    };
    if !now.is_multiple_of(EVERY) {
        return;
    }
    let slates = SLATES.lock().clone();
    let mut sent = SENT.lock();
    for name in yog_api::event::player::online() {
        let Some(p) = yog_api::event::player::info(&name) else {
            continue;
        };
        let Some(e) = yog_api::event::entity::info(&p.uuid) else {
            continue;
        };
        let here = Vec3::from_tuple(e.pos);
        let near: Vec<Slate> = slates
            .iter()
            .filter(|((dim, pos), _)| {
                *dim == e.dimension
                    && Vec3::center_of(*pos).distance_sqr(here) <= SLATE_RANGE * SLATE_RANGE
            })
            .filter_map(|((dim, pos), pattern)| {
                let state = yog_api::event::block::state(dim, to_ypos(*pos))?;
                let (_, props) = split_state(&state);
                let get = |k: &str| {
                    props
                        .iter()
                        .find(|(n, _)| *n == k)
                        .map_or("", |(_, v)| *v)
                        .to_owned()
                };
                let face = get("face");
                // `IotaType.getColor`: what a bookshelf holds tints its books.
                let tint = if face.is_empty() {
                    crate::akashic::shelf(dim, *pos)
                        .map(|(_, iota)| iota.kind().color() & 0xFF_FFFF)
                } else {
                    None
                };
                Some(Slate {
                    pos: *pos,
                    face,
                    facing: get("facing"),
                    energized: get("energized") == "true",
                    pattern: pattern.clone(),
                    tint,
                })
            })
            .collect();
        let sentinel = get_sentinel(&p.uuid)
            .filter(|s| s.dim == e.dimension)
            .map(|s| Shown {
                pos: s.pos,
                great: s.great,
                pigment: get_pigment(&p.uuid),
            });
        let (a, b) = (
            encode_slates(&e.dimension, &near),
            encode_sentinel(sentinel.as_ref()),
        );
        let last = sent.entry(name.clone()).or_default();
        if last.0 != a {
            yog_api::event::net::send_to_player(&name, CH_SLATES, &a);
            last.0 = a;
        }
        if last.1 != b {
            yog_api::event::net::send_to_player(&name, CH_SENTINEL, &b);
            last.1 = b;
        }
    }
}

// ── the client ──────────────────────────────────────────────────────────────

struct Seen {
    slates: Vec<Slate>,
    /// The blocks tinted, and the dimension they're in.
    tinted: Vec<(String, (i32, i32, i32))>,
    /// The slates' triangles, `x, y, z, r, g, b, a` (world coordinates).
    slate_mesh: Vec<f32>,
    sentinel: Option<Shown>,
}

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(|| {
    Mutex::new(Seen {
        slates: Vec::new(),
        tinted: Vec::new(),
        slate_mesh: Vec::new(),
        sentinel: None,
    })
});

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
        // An akashic bookshelf: on its front, a hair out.
        "" => (c + f * SHELF_FRONT, right, Vec3::new(0.0, 1.0, 0.0)),
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
    let (_, pts) = get_centered_pattern(&s.pattern, size, size, size / SLATE_MIN_SCALE);
    let center = Vec2::new(UNIT / 2.0, UNIT / 2.0);
    let pts: Vec<Vec2> = pts.into_iter().map(|p| p + center).collect();
    let (inner, outer) = if s.energized {
        (screen_col(GLOW), GLOW)
    } else {
        (INNER, OUTER)
    };
    draw_line_seq(&mut mesh, &pts, Line::solid(STROKE * 2.0 * UNIT, outer));
    draw_line_seq(&mut mesh, &pts, Line::solid(STROKE * UNIT, inner));
    let (origin, right, up) = slate_plane(s);
    // A 2D vertex: x, y, then its color.
    for &[x, y, r, g, b, a] in mesh.vertices().as_chunks::<6>().0 {
        let (px, py) = (x / UNIT - 0.5, 0.5 - y / UNIT);
        let p = origin + right * px as f64 + up * py as f64;
        out.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32, r, g, b, a]);
    }
}

/// An akashic bookshelf's pattern sits this far out of the block's middle
/// (a hair past its face); a slate's pattern is at least a quarter of it.
const SHELF_FRONT: f64 = 0.502;
const SLATE_MIN_SCALE: f32 = 4.0;
/// An icosahedron's rings: five vertices each.
const RING: usize = 5;
/// A sentinel bobs (ticks a period, blocks high), spins (ticks a turn),
/// and a greater one tumbles this much slower than it spins.
const BOB_PERIOD: f32 = 20.0;
const BOB_HEIGHT: f32 = 0.1;
const SPIN_PERIOD: f32 = 30.0;
const TUMBLE_SLOWER: f32 = 8.0;

/// The icosahedron in the unit sphere (`Icos`): apexes and rings.
fn icosahedron() -> Vec<([f32; 3], [f32; 3])> {
    let ring = |y: f32, start: f32| -> Vec<[f32; 3]> {
        (0..RING)
            .map(|i| {
                let a = start + i as f32 * std::f32::consts::TAU / RING as f32;
                let rad = (1.0 - y * y).sqrt();
                [rad * a.cos(), y, rad * a.sin()]
            })
            .collect()
    };
    let (top, bottom) = ([0.0, 1.0, 0.0], [0.0, -1.0, 0.0]);
    // The rings sit at ±1/√5.
    let y = 1.0 / (RING as f32).sqrt();
    let top_ring = ring(y, 0.0);
    let bottom_ring = ring(-y, std::f32::consts::PI / RING as f32);
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
/// for a greater sentinel), colored by the owner's pigment, seen through
/// walls; its lines as thin quads facing the camera.
fn sentinel_triangles(s: &Shown, ticks: f32, cam: Vec3, out: &mut Vec<f32>) {
    let time = ticks / 2.0;
    let bob = (time / BOB_PERIOD).sin() * BOB_HEIGHT;
    let spin = time / SPIN_PERIOD;
    let tumble = if s.great { spin / TUMBLE_SLOWER } else { 0.0 };
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
            let c = s
                .pigment
                .color(time, Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            let [_, r, g, b] = c.to_be_bytes();
            let full = |v: u8| f32::from(v) / f32::from(u8::MAX);
            [full(r), full(g), full(b), 1.0]
        };
        let (ca, cb) = (color(a), color(b));
        let quad = [
            (pa - side, ca),
            (pa + side, ca),
            (pb + side, cb),
            (pa - side, ca),
            (pb + side, cb),
            (pb - side, cb),
        ];
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
    START.elapsed().as_secs_f32() * TICKS_PER_SECOND as f32
}

fn draw(gfx: &GfxContext) {
    let seen = SEEN.lock();
    if seen.slate_mesh.is_empty() && seen.sentinel.is_none() {
        return;
    }
    let mut gl = GL.lock();
    if gl.is_none() {
        let Ok(prog) = gfx.create_shader(VERT, FRAG) else {
            return;
        };
        let vbo = gfx.create_buffer();
        let vao = gfx.create_vao();
        vao.attrib(gfx, &vbo, crate::ui::render_lib::attrib(0, 3, 0, 7));
        vao.attrib(gfx, &vbo, crate::ui::render_lib::attrib(1, 4, 3, 7));
        *gl = Some(Gl { prog, vao, vbo });
    }
    let Some(gl) = gl.as_ref() else { return };
    let cam = gfx.camera_pos();
    gl.prog.uniform_mat4(gfx, "uViewProj", &gfx.view_proj());
    // Coordinates are sent as offsets from the camera's block, so floats keep their precision far out.
    let base = [cam[0].floor(), cam[1].floor(), cam[2].floor()];
    gl.prog.uniform_3f(
        gfx,
        "uCamera",
        [cam[0] - base[0], cam[1] - base[1], cam[2] - base[2]],
    );
    let local = |v: &[f32]| -> Vec<f32> {
        // A world vertex: x, y, z, then its color.
        v.as_chunks::<7>()
            .0
            .iter()
            .flat_map(|&[x, y, z, r, g, b, a]| [x - base[0], y - base[1], z - base[2], r, g, b, a])
            .collect()
    };
    gfx.set_blend(true, blend::SRC_ALPHA, blend::ONE_MINUS_SRC_ALPHA);
    if !seen.slate_mesh.is_empty() {
        gfx.set_depth(true, true);
        let verts = local(&seen.slate_mesh);
        // SAFETY: `f32`s are plain data.
        unsafe { gl.vbo.upload(gfx, &verts, true) };
        gfx.draw_arrays(
            &gl.vao,
            &gl.prog,
            DrawMode::Triangles,
            0,
            (verts.len() / 7) as u32,
        );
    }
    if let Some(s) = &seen.sentinel {
        let ticks = client_ticks();
        let mut tris = Vec::new();
        let camera = Vec3::new(cam[0] as f64, cam[1] as f64, cam[2] as f64);
        sentinel_triangles(s, ticks, camera, &mut tris);
        gfx.set_depth(false, false);
        let verts = local(&tris);
        unsafe { gl.vbo.upload(gfx, &verts, true) };
        gfx.draw_arrays(
            &gl.vao,
            &gl.prog,
            DrawMode::Triangles,
            0,
            (verts.len() / 7) as u32,
        );
    }
}

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::life::Started, _>(|_, _| load_slates());
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());
    registry.on::<yog_api::event::player::session::Leave, _>(|e, _| {
        SENT.lock().remove(&e.player_name);
        true
    });
    registry.on_client_packet(CH_SLATES, |ev| {
        if let Some((dim, slates)) = decode_slates(&ev.payload) {
            let mut seen = SEEN.lock();
            let mut mesh = Vec::new();
            for s in &slates {
                slate_triangles(s, &mut mesh);
            }
            // Books take their shelf's color; shelves no longer sent lose it.
            let at = |p: (i32, i32, i32)| yog_api::BlockPos {
                x: p.0,
                y: p.1,
                z: p.2,
            };
            for (d, p) in seen.tinted.drain(..) {
                yog_api::client::set_block_tint(&d, at(p), None);
            }
            for s in slates.iter().filter(|s| s.tint.is_some()) {
                yog_api::client::set_block_tint(&dim, at(s.pos), s.tint);
                seen.tinted.push((dim.clone(), s.pos));
            }
            seen.slates = slates;
            seen.slate_mesh = mesh;
        }
    });
    registry.on_client_packet(CH_SENTINEL, |ev| {
        if let Some(s) = decode_sentinel(&ev.payload) {
            SEEN.lock().sentinel = s;
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
        let s = vec![Slate {
            pos: (1, -2, 3),
            face: "wall".into(),
            facing: "north".into(),
            energized: true,
            pattern,
            tint: Some(0x55FF55),
        }];
        assert_eq!(
            decode_slates(&encode_slates("minecraft:overworld", &s)),
            Some(("minecraft:overworld".into(), s))
        );
        let sentinel = Shown {
            pos: Vec3::new(1.5, 2.0, -3.25),
            great: true,
            pigment: Pigment::default(),
        };
        assert_eq!(
            decode_sentinel(&encode_sentinel(Some(&sentinel))),
            Some(Some(sentinel))
        );
        assert_eq!(decode_sentinel(&encode_sentinel(None)), Some(None));
    }

    #[test]
    fn a_slate_pattern_lies_on_its_plate() {
        let pattern = HexPattern::from_angles("waqaw", HexDir::East).unwrap();
        let s = Slate {
            pos: (0, 0, 0),
            face: "floor".into(),
            facing: "south".into(),
            energized: false,
            pattern,
            tint: None,
        };
        let mut out = Vec::new();
        slate_triangles(&s, &mut out);
        assert!(!out.is_empty());
        for v in out.as_chunks::<7>().0 {
            assert!(
                (v[1] - (1.0 / 16.0 + 0.002)).abs() < 1e-4,
                "on the plate's top: {}",
                v[1]
            );
            assert!((0.0..=1.0).contains(&v[0]) && (0.0..=1.0).contains(&v[2]));
        }
    }

    #[test]
    fn the_icosahedron_has_thirty_equal_edges_on_the_unit_sphere() {
        let edges = icosahedron();
        assert_eq!(edges.len(), 30);
        let len = |(a, b): ([f32; 3], [f32; 3])| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        let first = len(edges[0]);
        assert!(
            edges.iter().all(|&e| (len(e) - first).abs() < 1e-4),
            "every edge as long"
        );
        for (a, b) in edges {
            for p in [a, b] {
                let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                assert!((r - 1.0).abs() < 1e-4);
            }
        }
    }
}
