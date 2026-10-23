//! Drawing the grid: its dots and patterns, the stack and the ravenmind.

use super::grid::{Desc, DrawState, Grid};
use super::{BOX_COLOR, ELLIPSIS_COLOR, LHS_IOTAS_ALLOCATION, RHS_IOTAS_ALLOCATION};
use crate::iota::Iota;
use crate::ui::render_lib::{
    DEFAULT_READABILITY_OFFSET, Mesh, PatternStyle, Vec2, draw_pattern_from_points, draw_spot,
    find_dup_indices, positions_from, range_around, to_lines,
};
use yog_api::ui::Color;
use yog_api::{GfxContext, client};

impl Grid {
    /// Builds the dots, the patterns and the stroke being drawn.
    pub(super) fn mesh(&self, gradient: bool) -> Mesh {
        let mut mesh = Mesh::new();
        let size = self.hex_size();

        // Guide dots around the cursor.
        let radius = 3;
        for dot in range_around(self.px_to_coord(self.mouse), radius) {
            if self.used_spots.contains(&dot) {
                continue;
            }
            let px = self.coord_to_px(dot);
            let delta = (px - self.mouse).length();
            // 1 on the cursor, 0 at the full radius (no popping in and out),
            // with a little island of full brightness around the cursor.
            let d = (1.0 - (delta - size) / (radius as f32 * size)).clamp(0.0, 1.0);
            let color = crate::ui::render_lib::argb_f(
                d,
                lerp(d, 0.4, 0.5),
                lerp(d, 0.8, 1.0),
                lerp(d, 0.7, 0.9),
            );
            draw_spot(&mut mesh, px, d * 2.0, color);
        }

        for (idx, p) in self.patterns.iter().enumerate() {
            let (color, fade) = p.kind.colors();
            let points = to_lines(&p.pattern, size, self.coord_to_px(p.origin));
            let dups = find_dup_indices(p.pattern.positions());
            draw_pattern_from_points(
                &mut mesh,
                &points,
                Some(&dups),
                true,
                PatternStyle {
                    tail: color | 0xC8 << 24,
                    head: fade | 0xC8 << 24,
                    flow_irregular: if p.kind.success() { 0.2 } else { 0.9 },
                    readability_offset: DEFAULT_READABILITY_OFFSET,
                    last_segment_len_prop: 1.0,
                    seed: idx as f64,
                    gradient,
                },
            );
        }

        // The pattern being drawn, up to the cursor.
        let (mut points, dups) = match &self.draw {
            DrawState::BetweenPatterns => return mesh,
            DrawState::JustStarted(start) => (vec![self.coord_to_px(*start)], None),
            DrawState::Drawing { start, wip, .. } => (
                positions_from(wip, *start)
                    .into_iter()
                    .map(|c| self.coord_to_px(c))
                    .collect(),
                Some(find_dup_indices(wip.positions())),
            ),
        };
        points.push(self.mouse);
        draw_pattern_from_points(
            &mut mesh,
            &points,
            dups.as_ref(),
            false,
            PatternStyle {
                tail: 0xff_64c8ff,
                head: 0xff_fecbe6,
                flow_irregular: 0.1,
                readability_offset: DEFAULT_READABILITY_OFFSET,
                last_segment_len_prop: 1.0,
                // The number of patterns, so the zaps don't jump when this
                // one joins them.
                seed: self.patterns.len() as f64,
                gradient,
            },
        );
        mesh
    }

    /// The stack's lines, top first, cut to 70% of the screen.
    pub(super) fn stack_descs(&mut self, width_of: &dyn Fn(&str) -> f32) -> &[Desc] {
        let max = (self.size.0 * LHS_IOTAS_ALLOCATION).trunc();
        if self.stack_descs.as_ref().is_some_and(|(w, _)| *w != max) {
            self.stack_descs = None;
        }
        let (stack, descs) = (&self.stack, &mut self.stack_descs);
        &descs
            .get_or_insert_with(|| {
                let descs = stack
                    .iter()
                    .rev()
                    .map(|i| describe(i, max, width_of))
                    .collect();
                (max, descs)
            })
            .1
    }

    pub(super) fn ravenmind_desc(&mut self, width_of: &dyn Fn(&str) -> f32) -> Option<&Desc> {
        let max = (self.size.0 * RHS_IOTAS_ALLOCATION).trunc();
        if self.ravenmind_desc.as_ref().is_some_and(|(w, _)| *w != max) {
            self.ravenmind_desc = None;
        }
        let (ravenmind, desc) = (&self.ravenmind, &mut self.ravenmind_desc);
        desc.get_or_insert_with(|| (max, ravenmind.as_ref().map(|i| describe(i, max, width_of))))
            .1
            .as_ref()
    }

    /// The stack and ravenmind boxes (`drawBox`), as a mesh.
    pub(super) fn boxes(&mut self, width_of: &dyn Fn(&str) -> f32) -> Mesh {
        let mut mesh = Mesh::new();
        let w = self.size.0;
        let rows = self.stack_descs(width_of).len();
        if rows > 0 {
            draw_box(
                &mut mesh,
                10.0,
                10.0,
                w * LHS_IOTAS_ALLOCATION + 5.0,
                (rows as f32 + 1.0) * 10.0,
            );
        }
        if self.ravenmind_desc(width_of).is_some() {
            let (x, y) = ravenmind_origin(w);
            draw_box(
                &mut mesh,
                x,
                y,
                w * RHS_IOTAS_ALLOCATION * RAVENMIND_SCALE,
                15.0 * RAVENMIND_SCALE,
            );
        }
        mesh
    }
}

/// The ravenmind box is drawn this much bigger.
pub(super) const RAVENMIND_SCALE: f32 = 1.5;

pub(super) fn ravenmind_origin(w: f32) -> (f32, f32) {
    (
        w * (1.0 - RHS_IOTAS_ALLOCATION * RAVENMIND_SCALE) - 10.0,
        10.0,
    )
}

pub(super) fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// `GuiSpellcasting.drawBox`: two translucent rectangles, one inset.
pub(super) fn draw_box(mesh: &mut Mesh, x: f32, y: f32, w: f32, h: f32) {
    let left_margin = 2.5;
    mesh.quad(x, y, w, h, BOX_COLOR);
    mesh.quad(
        x + left_margin,
        y + 2.5,
        w - left_margin - 2.5,
        h - 5.0,
        BOX_COLOR,
    );
}

/// `HexPattern.tryAppendDir`: no edge twice, no doubling back.
/// `IotaType.getDisplayWithMaxWidth`: the iota's text — its patterns as
/// runes — and when it's wider than `max_width`, the start that fits less
/// the "..." (a grey "..." follows). Colored by the iota's type.
pub(super) fn describe(iota: &Iota, max_width: f32, width_of: &dyn Fn(&str) -> f32) -> Desc {
    let (segs, cut) = crate::ui::rune_text::fit(&iota.display().segments(), max_width, width_of);
    Desc {
        segs,
        cut,
        color: iota.kind().color(),
    }
}

/// How big the grid is drawn — the player's `hexcasting:grid_zoom`
/// (Clarity enlarges it, Clouding shrinks it); 1 without a player.
pub(super) fn grid_zoom() -> f64 {
    client::attribute_value(GRID_ZOOM)
        .filter(|z| *z > 0.0)
        .unwrap_or(1.0)
}

/// The attribute the grid's size follows (content/effects/grid_zoom.ydef).
pub(super) const GRID_ZOOM: &str = "hexcasting:grid_zoom";

pub(super) fn draw_desc(
    gfx: &GfxContext,
    width_of: &dyn Fn(&str) -> f32,
    desc: &Desc,
    x: f32,
    y: f32,
    color: u32,
) {
    let ink = crate::ui::rune_text::Ink {
        color,
        shadow: true,
    };
    crate::ui::rune_text::draw(gfx, &desc.segs, Vec2::new(x, y), ink);
    if desc.cut {
        let alpha = color & 0xFF00_0000;
        let end = x + crate::ui::rune_text::width(&desc.segs, width_of);
        gfx.draw2d().text(
            "...",
            Vec2::new(end, y),
            Color((ELLIPSIS_COLOR & 0x00FF_FFFF) | alpha),
            true,
        );
    }
}
