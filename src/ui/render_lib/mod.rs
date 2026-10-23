//! Hex Casting's `RenderLib.kt` (and the grid helpers of `HexUtils.kt`):
//! hex-grid ↔ pixel conversion, the zappy pattern lines, the little
//! hexagonal spots — built into a [`Mesh`] of colored triangles that
//! [`Mesh::draw`] sends to the GPU with a position-color shader, as the
//! original draws with Minecraft's `PositionColorShader`.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::Instant;
use yog_api::sync::OnceLock;

use crate::math::{HexCoord, HexPattern};

mod lines;
mod mesh;
#[cfg(test)]
mod tests;
mod zappy;

pub use lines::{Line, draw_line_seq, draw_spot};
pub use mesh::Mesh;
pub(crate) use mesh::attrib;
pub use zappy::{
    INNER_SHARE, PatternStyle, Zappy, draw_pattern_from_points, find_center, get_centered_pattern,
    make_zappy, static_zappy,
};

pub const SQRT_3: f32 = 1.732_050_8;
pub(super) const TAU: f64 = std::f64::consts::TAU;
pub(super) const PI: f32 = std::f32::consts::PI;
/// Degrees between the triangles of a line's round caps and joins.
/// Degrees in a half turn.
pub(super) const HALF_TURN_DEGREES: f32 = 180.0;
pub const CAP_THETA: f32 = HALF_TURN_DEGREES / 10.0;
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

pub(super) fn screen(n: i32) -> i32 {
    (n + 255) / 2
}

/// Each channel halfway to white (the bright core of a line).
pub fn screen_col(n: u32) -> u32 {
    argb(alpha(n), screen(red(n)), screen(green(n)), screen(blue(n)))
}

pub fn dodge(n: i32) -> f32 {
    n as f32 * 0.9
}

pub(super) fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

pub(super) fn rotate(v: Vec2, theta: f32) -> Vec2 {
    let (sin, cos) = theta.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.y * cos + v.x * sin)
}
