//! Patterns' zappy lines and where they're drawn.

use super::super::noise::get_noise;
use super::CHANNEL;
use super::McVec2;
use super::lines::{Line, draw_line_seq, draw_spot};
use super::mesh::Mesh;
use super::{
    TAU, Vec2, alpha, argb_f, blue, dodge, green, red, round_to_int, screen_col, ticks, to_lines,
};
use crate::math::HexPattern;
use std::collections::HashSet;

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
    let (hops, variance, speed) = GUI_ZAP;
    let zap = Zappy {
        hops,
        variance,
        speed,
        flow_irregular: style.flow_irregular,
        readability_offset: style.readability_offset,
        last_segment_len_prop: style.last_segment_len_prop,
        seed: style.seed,
        ticks: ticks(),
    };
    let zappy = make_zappy(points, dup_indices, &zap);
    let nodes = if draw_last {
        points
    } else {
        &points[..points.len().saturating_sub(1)]
    };
    let outer = Line {
        width: GUI_OUTER,
        tail: style.tail,
        head: style.head,
        gradient: style.gradient,
    };
    draw_line_seq(mesh, &zappy, outer);
    let inner = Line {
        width: GUI_INNER,
        tail: screen_col(style.tail),
        head: screen_col(style.head),
        gradient: style.gradient,
    };
    draw_line_seq(mesh, &zappy, inner);
    let h = style.head;
    let spot = argb_f(
        alpha(h) as f32 / CHANNEL,
        dodge(red(h)) / CHANNEL,
        dodge(green(h)) / CHANNEL,
        dodge(blue(h)) / CHANNEL,
    );
    for &node in nodes {
        draw_spot(mesh, node, GUI_SPOT, spot);
    }
}

/// How a zappy line shakes: `hops` points a segment, straying up to
/// `variance` of a hop, shaking `speed` fast and `flow_irregular` unevenly;
/// a point visited twice pulled `readability_offset` into its segments;
/// the last segment drawn `last_segment_len_prop` of the way; `seed` and
/// the time (`ticks`) pick the shaking.
#[derive(Debug, Clone, Copy)]
pub struct Zappy {
    pub hops: i32,
    pub variance: f32,
    pub speed: f32,
    pub flow_irregular: f32,
    pub readability_offset: f32,
    pub last_segment_len_prop: f32,
    pub seed: f64,
    pub ticks: f64,
}

/// The GUI's zap (`drawPatternFromPoints`): hops, variance, speed.
pub(super) const GUI_ZAP: (i32, f32, f32) = (10, 2.5, 0.1);
/// The GUI's line widths: the outer, the inner, a node's spot.
pub(super) const GUI_OUTER: f32 = 5.0;
pub(super) const GUI_INNER: f32 = 2.0;
pub(super) const GUI_SPOT: f32 = 2.0;

/// `ZappySettings.STATIC` and `.READABLE`: hops, variance, flow
/// irregularity; readability offset and the last segment's share when
/// readable.
pub(super) const STATIC_ZAP: (i32, f32, f32) = (10, 0.5, 0.2);
/// A pattern's inner line, this much of its outer one's width.
pub const INNER_SHARE: f32 = 2.0 / 5.0;
pub(super) const READABLE: (f32, f32) = (0.2, 0.8);

/// A still zappy line through `dots` (a pattern's, `dups` visited twice),
/// as scrolls and the book draw them: `readable` pulls crossings apart and
/// stops short of the last point.
pub fn static_zappy(dots: &[Vec2], dups: &HashSet<usize>, readable: bool, seed: f64) -> Vec<Vec2> {
    let (hops, variance, flow_irregular) = STATIC_ZAP;
    let (readability_offset, last_segment_len_prop) = if readable { READABLE } else { (0.0, 1.0) };
    let zap = Zappy {
        hops,
        variance,
        speed: 0.0,
        flow_irregular,
        readability_offset,
        last_segment_len_prop,
        seed,
        ticks: 0.0,
    };
    make_zappy(dots, Some(dups), &zap)
}

/// `makeZappy`'s shaking: how fast a hop's stray ramps up from its ends,
/// how many turns its direction spans, and the noise rows its direction
/// and distance are read from.
const VARIANCE_RAMP: f64 = 8.0;
const THETA_TURNS: f64 = 3.0;
const THETA_NOISE: f64 = 1337.0;
const RADIUS_NOISE: f64 = 69420.0;
/// `getCenteredPattern` fits the pattern in a third of the box.
const FIT_SHARE: f32 = 3.0;

/// `makeZappy`: each segment split into `hops` jittered sub-points that
/// shake over time (`ticks`). Points in `dup_indices` (visited twice) are
/// pulled `readability_offset` of the way into their segments so crossing
/// strokes stay readable.
#[allow(clippy::too_many_arguments)] // the original's signature
pub fn make_zappy(
    bare_points: &[Vec2],
    dup_indices: Option<&HashSet<usize>>,
    z: &Zappy,
) -> Vec<Vec2> {
    if bare_points.is_empty() {
        return Vec::new();
    }
    let readability_offset = z.readability_offset;
    let Some(dups) = dup_indices else {
        return zappify(bare_points, true, z);
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
            points.extend(zappify(&daisy_chain, true, z));
        } else if dups.contains(&(i + 1)) {
            daisy_chain.push(tail - tangent);
            points.extend(zappify(&daisy_chain, false, z));
            daisy_chain.clear();
        }
    }
    points
}

/// `points` with each segment split into jittered hops; the last segment
/// cut to its share when `truncate_last`.
fn zappify(points: &[Vec2], truncate_last: bool, z: &Zappy) -> Vec<Vec2> {
    let hops = z.hops;
    let scale_variance = |it: f64| 1f64.min(VARIANCE_RAMP * (0.5 - (0.5 - it).abs()));
    let z_seed = z.ticks * z.speed as f64;
    let mut out = Vec::with_capacity(points.len() * hops as usize);
    out.push(points[0]);
    for (i, pair) in points.windows(2).enumerate() {
        let (src, target) = (pair[0], pair[1]);
        let delta = target - src;
        let hop_dist = src.distance_to_sqr(target).sqrt() / hops as f32;
        let max_variance = hop_dist * z.variance;
        let max_j = if truncate_last && i == points.len() - 2 {
            round_to_int(z.last_segment_len_prop * hops as f32)
        } else {
            hops
        };
        for j in 1..=max_j {
            let progress = j as f64 / (hops + 1) as f64;
            let pos = src + delta * progress as f32;
            let minor_perturb =
                get_noise(i as f64, j as f64, z_seed.sin()) * z.flow_irregular as f64;
            let turn_at = i as f64 + progress + minor_perturb - z_seed;
            let theta = (THETA_TURNS * get_noise(turn_at, THETA_NOISE, z.seed) * TAU) as f32;
            let stray = get_noise(i as f64 + progress - z_seed, RADIUS_NOISE, z.seed);
            let r = (stray * max_variance as f64 * scale_variance(progress)) as f32;
            out.push(pos + Vec2::new(r * theta.cos(), r * theta.sin()));
            if j == hops {
                out.push(target);
            }
        }
    }
    out
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
    let scale = min_size.min((width / FIT_SHARE / max_dx).min(height / FIT_SHARE / max_dy));
    let com2 = find_center(&to_lines(pattern, scale, Vec2::ZERO));
    (scale, to_lines(pattern, scale, -com2))
}
