//! A pattern drawn centred in a box — what Hex Casting shows in a scroll's
//! or slate's tooltip (`PatternTooltipComponent`, 128×128 over the scroll
//! texture) and on paper in the world. Built on [`render_lib`]; nothing
//! calls it yet (Yog has no item-tooltip hook to draw it in).
//!
//! [`render_lib`]: super::render_lib

use yog_api::GfxContext;

use super::render_lib::{Mesh, Vec2, draw_line_seq, draw_spot, get_centered_pattern};
use crate::math::HexPattern;

/// `PatternColors.DEFAULT_PATTERN_COLOR`: inner stroke, outer stroke.
pub const INNER_COLOR: u32 = 0xff_554d54;
pub const OUTER_COLOR: u32 = 0xff_d2c8c8;
/// `PatternColors.STARTING_DOT`.
pub const STARTING_DOT: u32 = 0xff_5b7bd7;
/// `PatternColors.GRID_DOTS`.
pub const GRID_DOTS: u32 = 0x80_d2c8c8;
/// The tooltip's size (`RENDER_SIZE`).
pub const TOOLTIP_SIZE: f32 = 128.0;

/// Adds `pattern`, centred in the `w`×`h` box at `x, y`, to `mesh`: a pale
/// outline under a dark stroke (`stroke` thick), a dot on every point it
/// visits and the starting dot.
pub fn mesh_pattern(
    mesh: &mut Mesh,
    pattern: &HexPattern,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    stroke: f32,
) {
    let (scale, pts) = get_centered_pattern(pattern, w, h, 16.0_f32.max(w.min(h) / 4.0));
    let center = Vec2::new(x + w / 2.0, y + h / 2.0);
    let pts: Vec<Vec2> = pts.into_iter().map(|p| p + center).collect();
    let dot = (scale * 0.08).max(1.0);
    for &p in &pts {
        draw_spot(mesh, p, dot * 1.6, GRID_DOTS);
    }
    draw_line_seq(mesh, &pts, stroke * 2.0, OUTER_COLOR, OUTER_COLOR, false);
    draw_line_seq(mesh, &pts, stroke, INNER_COLOR, INNER_COLOR, false);
    if let Some(&first) = pts.first() {
        draw_spot(mesh, first, dot * 2.0, STARTING_DOT);
    }
}

/// Draws `pattern` centred in the `w`×`h` box at `x, y` (GUI pixels).
pub fn draw_pattern(gfx: &GfxContext, pattern: &HexPattern, x: f32, y: f32, w: f32, h: f32) {
    let mut mesh = Mesh::new();
    // `StrokeSettings.fromStroke(0.8/16)` of the box.
    mesh_pattern(&mut mesh, pattern, x, y, w, h, w.min(h) * 0.8 / 16.0);
    mesh.draw(gfx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn a_pattern_fills_its_box() {
        let p = HexPattern::from_angles("waqaw", HexDir::East).unwrap();
        let mut mesh = Mesh::new();
        mesh_pattern(&mut mesh, &p, 0.0, 0.0, TOOLTIP_SIZE, TOOLTIP_SIZE, 6.4);
        assert!(mesh.vertex_count() > 0);
    }
}
