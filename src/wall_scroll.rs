//! Scrolls hung on a wall (`EntityWallScroll`, `WallScrollRenderer`). The
//! loader hangs them — the scrolls' `.yitem`s say `hangs`, `wall_scroll` is
//! a hanging entity — and keeps the scroll each was made from, dropping it
//! when the scroll is broken or loses its wall. Here they're drawn: the
//! paper and its pattern; amethyst dust rubbed on one shows the pattern's
//! stroke order.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use yog_api::Registry;
use yog_api::body::{self, Anchor, Color, Frame, Look, Mode, Ops, Quad};
use yog_api::entities::Hanging;
use yog_api::event::player::action::Interact;

use crate::casting::env::{SLOT_OFFHAND, Stack};
use crate::content::{Entities, Items};
use crate::item::holder::{self, TAG_OP_ID};
use crate::math::HexPattern;
use crate::snbt::Nbt;
use crate::ui::pattern_render::{INNER_COLOR, OUTER_COLOR, STARTING_DOT};
use crate::ui::render_lib::{
    Mesh, Vec2, draw_line_seq, draw_spot, find_dup_indices, make_zappy, to_lines,
};

/// Entity data: the scroll shows its stroke order (`SHOWS_STROKE_ORDER`), saved with it.
const STROKE_ORDER: &str = "hexcasting:shows_stroke_order";
/// The interact event's off hand.
const OFF_HAND: &str = "off_hand";

/// `WallScrollRenderer`'s papers by size (1, 2, 3+ blocks): pristine, ancient.
const PRISTINE: [&str; 3] = [
    "hexcasting:textures/block/scroll_paper.png",
    "hexcasting:textures/entity/scroll_medium.png",
    "hexcasting:textures/entity/scroll_large.png",
];
const ANCIENT: [&str; 3] = [
    "hexcasting:textures/block/ancient_scroll_paper.png",
    "hexcasting:textures/entity/scroll_ancient_medium.png",
    "hexcasting:textures/entity/scroll_ancient_large.png",
];
/// What the pattern's flat colors are drawn with.
const WHITE: &str = "hexcasting:textures/entity/white.png";
/// Untinted.
const PLAIN: u32 = 0xFF_FFFFFF;

pub fn register(registry: &mut Registry) {
    registry.before::<Interact, _>(|ev, _| ev.entity_type != Entities::WALL_SCROLL || !dust(ev));
    body::track_type(Entities::WALL_SCROLL, true);
    registry.on_body(draw);
}

fn shows_stroke_order(data: Option<&[u8]>) -> bool {
    data.is_some_and(|v| v.first() == Some(&1))
}

/// `EntityWallScroll.interactAt`: amethyst dust on a scroll that doesn't show
/// its stroke order yet makes it show it, using up the dust (not in
/// creative). `true` if it did.
fn dust(ev: &Interact) -> bool {
    if shows_stroke_order(yog_api::event::entity::data(&ev.entity_uuid, STROKE_ORDER).as_deref()) {
        return false;
    }
    let Some(player) = yog_api::event::player::info(&ev.player_name) else {
        return false;
    };
    let slot = if ev.hand == OFF_HAND {
        SLOT_OFFHAND
    } else {
        player.selected_slot
    };
    let mut stack = Stack::read(&ev.player_name, slot);
    if !stack.is(Items::AMETHYST_DUST) {
        return false;
    }
    if player.gamemode != "creative" {
        stack.count -= 1;
        stack.save(&ev.player_name);
    }
    yog_api::event::entity::set_data(&ev.entity_uuid, STROKE_ORDER, Some(&[1]), true);
    if let Some(e) = yog_api::event::entity::info(&ev.entity_uuid) {
        yog_api::event::sound::play(
            e.dimension,
            (e.pos.x, e.pos.y, e.pos.z),
            crate::sounds::SCROLL_DUST,
            1.0,
            1.0,
        );
    }
    true
}

// ── drawing ─────────────────────────────────────────────────────────────────

/// The scroll as it was drawn last, kept while it doesn't change.
struct Drawn {
    key: (String, bool, u32, i32),
    paper: &'static str,
    paper_quads: Vec<Quad>,
    /// The pattern's strokes and dot, each one color.
    pattern: Vec<(u32, Vec<Quad>)>,
}

static DRAWN: LazyLock<Mutex<HashMap<String, Drawn>>> = LazyLock::new(Default::default);
/// More scrolls than this remembered: forget them all (the ones in sight come back).
const DRAWN_MAX: usize = 512;

fn draw(f: &Frame, ops: &mut Ops) {
    if f.entity_type != Entities::WALL_SCROLL {
        return;
    }
    let Some(h) = yog_api::entities::hanging_of(f) else {
        return;
    };
    let size = h.width.max(1);
    let readable = shows_stroke_order(f.data(STROKE_ORDER));
    let seed = seed(f.pos, &h);
    let key = (h.nbt.clone(), readable, size, seed);
    let mut drawn = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    if drawn.get(&f.uuid).is_none_or(|d| d.key != key) {
        if drawn.len() >= DRAWN_MAX {
            drawn.clear();
        }
        drawn.insert(f.uuid.clone(), build(&h, readable, seed, key));
    }
    let d = &drawn[&f.uuid];
    let look = |tint: u32| Look {
        mode: Mode::Cutout,
        glint: false,
        tint: Color(tint),
    };
    ops.mesh(&Anchor::Entity, d.paper, look(PLAIN), &d.paper_quads);
    for (color, quads) in &d.pattern {
        ops.mesh(&Anchor::Entity, WHITE, look(*color), quads);
    }
}

fn build(h: &Hanging, readable: bool, seed: i32, key: (String, bool, u32, i32)) -> Drawn {
    let nbt = Nbt::parse(&h.nbt);
    let size = key.2;
    let papers = if nbt.contains(TAG_OP_ID) {
        ANCIENT
    } else {
        PRISTINE
    };
    let pattern = holder::written_pattern(&h.item, &nbt).map_or_else(Vec::new, |p| {
        pattern_quads(&p, size as f32, readable, seed as f64)
    });
    Drawn {
        paper: papers[(size.clamp(1, 3) - 1) as usize],
        paper_quads: paper_quads(size as f32),
        pattern,
        key,
    }
}

/// `WallScrollRenderer`'s space, after the entity's turn: x right, y down,
/// z into the wall, the origin at the scroll's corner (it hangs `size`
/// blocks square, its back 1/32 off the wall) — in entity space, blocks.
fn place(size: f32, x: f32, y: f32, z: f32) -> body::Vec3 {
    body::Vec3::new(size / 2.0 - x, size / 2.0 - y, z + 1.0 / 32.0)
}

/// The paper: a 1/16-thick slab, the texture on its faces, a sliver of it on the edges.
fn paper_quads(size: f32) -> Vec<Quad> {
    let (dx, dy, dz) = (size, size, -1.0 / 16.0);
    let margin = 1.0 / 48.0;
    // Corners (x, y, z, u, v) and the face's normal, as the original's vertices.
    let faces: [([[f32; 5]; 4], [f32; 3]); 6] = [
        (
            [
                [0., 0., dz, 0., 0.],
                [0., dy, dz, 0., 1.],
                [dx, dy, dz, 1., 1.],
                [dx, 0., dz, 1., 0.],
            ],
            [0., 0., -1.],
        ),
        (
            [
                [0., 0., 0., 0., 0.],
                [dx, 0., 0., 1., 0.],
                [dx, dy, 0., 1., 1.],
                [0., dy, 0., 0., 1.],
            ],
            [0., 0., 1.],
        ),
        (
            [
                [0., 0., 0., 0., 0.],
                [0., 0., dz, 0., margin],
                [dx, 0., dz, 1., margin],
                [dx, 0., 0., 1., 0.],
            ],
            [0., -1., 0.],
        ),
        (
            [
                [0., 0., 0., 0., 0.],
                [0., dy, 0., 0., 1.],
                [0., dy, dz, margin, 1.],
                [0., 0., dz, margin, 0.],
            ],
            [-1., 0., 0.],
        ),
        (
            [
                [dx, 0., dz, 1. - margin, 0.],
                [dx, dy, dz, 1. - margin, 1.],
                [dx, dy, 0., 1., 1.],
                [dx, 0., 0., 1., 0.],
            ],
            [1., 0., 0.],
        ),
        (
            [
                [0., dy, dz, 0., 1. - margin],
                [0., dy, 0., 0., 1.],
                [dx, dy, 0., 1., 1.],
                [dx, dy, dz, 1., 1. - margin],
            ],
            [0., 1., 0.],
        ),
    ];
    faces
        .iter()
        .map(|(c, n)| Quad {
            pos: c.map(|v| place(size, v[0], v[1], v[2])),
            uv: c.map(|v| [v[3], v[4]]),
            // Turned with the paper (`Axis.ZP` 180°).
            normal: body::Vec3::new(-n[0], -n[1], n[2]),
        })
        .collect()
}

/// `WorldlyPatternRenderHelpers.SCROLL_SETTINGS` (`READABLE_SCROLL_SETTINGS`
/// once dusted): the pattern in the unit square padded 2/16, a pale stroke
/// 0.8/16 wide under a dark one 2/5 of that, the starting dot when readable.
const PAD: f64 = 2.0 / 16.0;
const STROKE: f64 = 0.8 / 16.0;
const OUTER_WIDTH: f64 = STROKE;
const INNER_WIDTH: f64 = STROKE * 2.0 / 5.0;
const START_DOT: f64 = 0.8 * INNER_WIDTH;
/// `PositionSettings.paddedSquare`'s base scale: points this far apart.
const BASE_SCALE: f64 = 0.25;
/// `ZappySettings.STATIC` and `.READABLE`: hops, variance, flow irregularity;
/// readability offset and last segment's share when readable.
const HOPS: i32 = 10;
const VARIANCE: f32 = 0.5;
const FLOW_IRREGULAR: f32 = 0.2;
const READABLE_OFFSET: f32 = 0.2;
const READABLE_SEGMENT: f32 = 0.8;
/// `PatternRenderer`'s layers out of the paper, and the pattern's own gap
/// (`renderPattern`'s z).
const OUTER_Z: f32 = 0.0005;
const INNER_Z: f32 = 0.001;
const DOT_Z: f32 = 0.0011;
const PATTERN_Z: f32 = -1.0 / 16.0 - 0.01;

/// `HexPatternPoints`: the zappy line through the pattern and its dots, fit
/// into the unit square.
fn fitted_points(pattern: &HexPattern, readable: bool, seed: f64) -> (Vec<Vec2>, Vec<Vec2>) {
    let dots = to_lines(pattern, 1.0, Vec2::ZERO);
    let dups: HashSet<usize> = find_dup_indices(pattern.positions());
    let (offset, last) = if readable {
        (READABLE_OFFSET, READABLE_SEGMENT)
    } else {
        (0.0, 1.0)
    };
    let zappy = make_zappy(
        &dots,
        Some(&dups),
        HOPS,
        VARIANCE,
        0.0,
        FLOW_IRREGULAR,
        offset,
        last,
        seed,
        0.0,
    );
    let (mut min_x, mut min_y, mut max_x, mut max_y) =
        (f64::MAX, f64::MAX, f64::MIN_POSITIVE, f64::MIN_POSITIVE);
    for p in &zappy {
        min_x = min_x.min(p.x as f64);
        max_x = max_x.max(p.x as f64);
        min_y = min_y.min(p.y as f64);
        max_y = max_y.max(p.y as f64);
    }
    let base = BASE_SCALE / 1.5;
    let (base_w, base_h) = ((max_x - min_x) * base, (max_y - min_y) * base);
    // No minimum size: only fit it in, never past 1.
    let fit = |extent: f64| (1.0 - 2.0 * PAD - OUTER_WIDTH) / extent;
    let scale = 1f64.min(fit(base_h)).min(fit(base_w));
    let final_scale = base * scale;
    // Centred in what's left over.
    let offset_of = |extent: f64| {
        let inherent = extent * scale + 2.0 * PAD + OUTER_WIDTH;
        (inherent - extent * scale) / 2.0 + (1.0 - inherent).max(0.0) / 2.0
    };
    let (ox, oy) = (offset_of(base_w), offset_of(base_h));
    let fit_point = |p: &Vec2| {
        Vec2::new(
            ((p.x as f64 - min_x) * final_scale + ox) as f32,
            ((p.y as f64 - min_y) * final_scale + oy) as f32,
        )
    };
    (
        zappy.iter().map(fit_point).collect(),
        dots.iter().map(fit_point).collect(),
    )
}

/// `renderPatternForScroll`: the strokes (and the starting dot) on the
/// paper's face, each color its quads. Lit as if from above, as the
/// original turns their normals.
fn pattern_quads(
    pattern: &HexPattern,
    size: f32,
    readable: bool,
    seed: f64,
) -> Vec<(u32, Vec<Quad>)> {
    let (zappy, dots) = fitted_points(pattern, readable, seed);
    let mut layers = Vec::new();
    let mut layer = |color: u32, z: f32, fill: &dyn Fn(&mut Mesh)| {
        let mut mesh = Mesh::new();
        fill(&mut mesh);
        let z = PATTERN_Z - z;
        let quads = mesh
            .vertices()
            .as_chunks::<18>()
            .0
            .iter()
            .map(|t| {
                let p = |i: usize| place(size, t[i * 6] * size, t[i * 6 + 1] * size, z);
                // A triangle as a quad, its last corner twice.
                Quad {
                    pos: [p(0), p(1), p(2), p(2)],
                    uv: [[0.5, 0.5]; 4],
                    normal: body::Vec3::new(0.0, 1.0, 0.0),
                }
            })
            .collect();
        layers.push((color, quads));
    };
    layer(OUTER_COLOR, OUTER_Z, &|m| {
        draw_line_seq(
            m,
            &zappy,
            OUTER_WIDTH as f32,
            OUTER_COLOR,
            OUTER_COLOR,
            false,
        )
    });
    layer(INNER_COLOR, INNER_Z, &|m| {
        draw_line_seq(
            m,
            &zappy,
            INNER_WIDTH as f32,
            INNER_COLOR,
            INNER_COLOR,
            false,
        )
    });
    if readable && let Some(&first) = dots.first() {
        layer(STARTING_DOT, DOT_Z, &|m| {
            draw_spot(m, first, START_DOT as f32, STARTING_DOT)
        });
    }
    layers
}

/// `scroll.getPos().hashCode()`: the zappy lines' seed, from the block the
/// scroll was hung in front of (`Vec3i.hashCode`).
fn seed(center: [f64; 3], h: &Hanging) -> i32 {
    let (fx, fz) = match h.facing.as_str() {
        "north" => (0.0, -1.0),
        "south" => (0.0, 1.0),
        "west" => (-1.0, 0.0),
        _ => (1.0, 0.0),
    };
    // `getCounterClockWise` of the facing.
    let (cx, cz) = (fz, -fx);
    let even = |n: u32| if n.is_multiple_of(2) { 0.5 } else { 0.0 };
    let (w, hh) = (even(h.width), even(h.height));
    let x = (center[0] + 0.46875 * fx - w * cx - 0.5).round() as i32;
    let y = (center[1] - hh - 0.5).round() as i32;
    let z = (center[2] + 0.46875 * fz - w * cz - 0.5).round() as i32;
    (y.wrapping_add(z.wrapping_mul(31)))
        .wrapping_mul(31)
        .wrapping_add(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    fn hanging(facing: &str, size: u32) -> Hanging {
        Hanging {
            facing: facing.into(),
            width: size,
            height: size,
            item: Items::SCROLL.into(),
            nbt: "{}".into(),
        }
    }

    #[test]
    fn the_seed_is_the_block_it_hangs_in_front_of() {
        // A 1×1 scroll on the north face of a wall, hung at (3, 64, -5): its
        // centre is 1/32 out from the wall.
        let c = [3.5, 64.5, -5.0 + 0.5 + 0.46875];
        assert_eq!(seed(c, &hanging("north", 1)), (64 + -5 * 31) * 31 + 3);
        // A 2×2 one facing east, hung at (0, 10, 0): half a block up and north.
        let c = [0.5 - 0.46875, 11.0, 0.0];
        assert_eq!(seed(c, &hanging("east", 2)), 10 * 31);
    }

    #[test]
    fn the_paper_covers_its_blocks() {
        let quads = paper_quads(3.0);
        assert_eq!(quads.len(), 6);
        for q in &quads {
            for p in q.pos {
                assert!((-1.5..=1.5).contains(&p.x) && (-1.5..=1.5).contains(&p.y));
                assert!((-1.0 / 32.0 - 1e-6..=1.0 / 32.0 + 1e-6).contains(&p.z));
            }
        }
        // The front faces out of the wall.
        assert_eq!(quads[0].normal, body::Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn the_pattern_lies_on_the_paper_and_dust_adds_its_start() {
        let p = HexPattern::from_angles("waqaw", HexDir::East).unwrap();
        let (zappy, _) = fitted_points(&p, false, 7.0);
        for pt in &zappy {
            assert!(
                (PAD as f32 - 0.05..=1.0 - PAD as f32 + 0.05).contains(&pt.x),
                "inside the padding: {pt:?}"
            );
        }
        let plain = pattern_quads(&p, 2.0, false, 7.0);
        assert_eq!(
            plain.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            [OUTER_COLOR, INNER_COLOR]
        );
        let all_quads = plain.iter().flat_map(|(_, q)| q).flat_map(|q| q.pos);
        for v in all_quads {
            assert!(v.z < -1.0 / 32.0, "in front of the paper: {}", v.z);
            assert!((-1.0..=1.0).contains(&v.x) && (-1.0..=1.0).contains(&v.y));
        }
        let readable = pattern_quads(&p, 2.0, true, 7.0);
        assert_eq!(readable.last().map(|(c, _)| *c), Some(STARTING_DOT));
    }

    #[test]
    fn each_size_has_its_paper() {
        let written = |op: bool| {
            let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
            let op = if op {
                "op_id:\"hexcasting:get_caster\","
            } else {
                ""
            };
            Hanging {
                nbt: format!("{{{op}pattern:\"{}\"}}", p.serialized_form()),
                ..hanging("south", 1)
            }
        };
        let d = build(&written(false), false, 0, (String::new(), false, 1, 0));
        assert_eq!(d.paper, PRISTINE[0]);
        assert!(!d.pattern.is_empty());
        let d = build(&written(true), false, 0, (String::new(), false, 3, 0));
        assert_eq!(d.paper, ANCIENT[2]);
        let blank = build(&hanging("south", 2), false, 0, (String::new(), false, 2, 0));
        assert!(blank.pattern.is_empty(), "a blank scroll: just paper");
    }
}
