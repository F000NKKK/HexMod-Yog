//! A pattern drawn centred in a box — what Hex Casting shows in a scroll's
//! or slate's tooltip (`PatternTooltipComponent`, 128×128 over the scroll
//! texture, [`register`]) and on paper in the world. Built on [`render_lib`].
//!
//! [`render_lib`]: super::render_lib

use yog_api::ui::Rect;
use yog_api::{GfxContext, Registry};

use crate::content::Items;
use crate::item::holder;
use crate::snbt::Nbt;

use super::render_lib::{Line, Mesh, Vec2, draw_line_seq, draw_spot, get_centered_pattern};
use super::rune_text::Ink;
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

/// `PatternTooltipComponent`'s papers: a scroll's (ancient if it came
/// with its action's id), a slate's.
const PRISTINE_BG: &str = "hexcasting:gui/scroll";
const ANCIENT_BG: &str = "hexcasting:gui/scroll_ancient";
const SLATE_BG: &str = "hexcasting:gui/slate";

/// Scrolls and slates that bear a pattern show it under their tooltip, on
/// their paper, as Hex Casting's `getTooltipImage` does.
pub fn register(registry: &mut Registry) {
    for item in [
        Items::SCROLL,
        Items::SCROLL_SMALL,
        Items::SCROLL_MEDIUM,
        holder::SLATE,
    ] {
        registry.on_item_tooltip(
            item,
            |item, snbt| tooltip(item, snbt).map(|_| (TOOLTIP_SIZE, TOOLTIP_SIZE)),
            |gfx, item, snbt, [x, y, w, h]| {
                if let Some((pattern, paper)) = tooltip(item, snbt) {
                    gfx.draw2d().image(paper, Rect::new(x, y, w, h));
                    draw_pattern(gfx, &pattern, Rect::new(x, y, w, h));
                }
            },
        );
    }
    register_hex_tooltips(registry);
}

/// What stores a hex (cyphers, trinkets, artifacts) shows it under its
/// tooltip: its iotas, the patterns as runes, as the casting grid's stack does.
const HEX_TOOLTIP_WIDTH: f32 = 160.0;
/// A line of it: a rune and the gap under it.
const HEX_TOOLTIP_LINE: f32 = super::rune_text::RUNE_HEIGHT + 2.0;

fn register_hex_tooltips(registry: &mut Registry) {
    for item in [
        Items::CYPHER,
        Items::ANCIENT_CYPHER,
        Items::TRINKET,
        Items::ARTIFACT,
    ] {
        registry.on_item_tooltip(
            item,
            |_, snbt| {
                let (lines, _) = hex_lines(snbt, &approx_width)?;
                let w = lines
                    .iter()
                    .map(|l| super::rune_text::width(l, &approx_width))
                    .fold(0.0, f32::max);
                Some((w.ceil(), lines.len() as f32 * HEX_TOOLTIP_LINE))
            },
            |gfx, _, snbt, [x, y, _, _]| {
                let Some((lines, color)) = hex_lines(snbt, &approx_width) else {
                    return;
                };
                for (i, line) in lines.iter().enumerate() {
                    let at = Vec2::new(x, y + i as f32 * HEX_TOOLTIP_LINE);
                    super::rune_text::draw(
                        gfx,
                        line,
                        at,
                        Ink {
                            color,
                            shadow: true,
                        },
                    );
                }
            },
        );
    }
}

/// The game font's average character width, in pixels.
const APPROX_CHAR_WIDTH: f32 = 6.0;

/// The font's width of `text`, near enough to lay a tooltip out before it's drawn.
fn approx_width(text: &str) -> f32 {
    text.chars().count() as f32 * APPROX_CHAR_WIDTH
}

/// A stored hex as lines of words and runes, and its color; `None` when
/// the stack holds none.
fn hex_lines(
    snbt: &str,
    width_of: &dyn Fn(&str) -> f32,
) -> Option<(Vec<Vec<crate::text::Segment>>, u32)> {
    let program = crate::iota::Iota::deserialize(
        Nbt::parse(snbt).get_str(crate::item::packaged::TAG_PROGRAM)?,
    );
    if !matches!(program, crate::iota::Iota::List(_)) {
        return None;
    }
    let segs = program.display().segments();
    Some((
        super::rune_text::wrap(&segs, HEX_TOOLTIP_WIDTH, width_of),
        program.kind().color(),
    ))
}

/// The pattern a stack bears and the paper it's shown on.
fn tooltip(item: &str, snbt: &str) -> Option<(HexPattern, &'static str)> {
    let nbt = Nbt::parse(snbt);
    let paper = if item == holder::SLATE {
        SLATE_BG
    } else if nbt.contains(holder::TAG_OP_ID) {
        ANCIENT_BG
    } else {
        PRISTINE_BG
    };
    Some((holder::written_pattern(item, &nbt)?, paper))
}

/// A pattern in a box: its least scale, and at least this share of the
/// box's smaller side; its grid dots' size of its scale, and how much
/// bigger than its line's dot; its stroke (`StrokeSettings.fromStroke(0.8/16)`).
const MIN_SCALE: f32 = 16.0;
const SCALE_SHARE: f32 = 4.0;
const DOT_SHARE: f32 = 0.08;
const GRID_DOT_SIZE: f32 = 1.6;
const STROKE_SHARE: f32 = 0.8 / 16.0;
/// A rune: how much wider than tall it may be (least, most), how flat a
/// pattern counts as a line, its starting dot.
const RUNE_ASPECT: (f32, f32) = (0.5, 3.0);
const FLAT: f32 = 0.01;
const RUNE_DOT: f32 = 0.9;

/// Adds `pattern`, centred in the `w`×`h` box at `x, y`, to `mesh`: a pale
/// outline under a dark stroke (`stroke` thick), a dot on every point it
/// visits and the starting dot.
pub fn mesh_pattern(mesh: &mut Mesh, pattern: &HexPattern, r: Rect, stroke: f32) {
    let min_size = MIN_SCALE.max(r.w.min(r.h) / SCALE_SHARE);
    let (scale, pts) = get_centered_pattern(pattern, r.w, r.h, min_size);
    let center = Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let pts: Vec<Vec2> = pts.into_iter().map(|p| p + center).collect();
    let dot = (scale * DOT_SHARE).max(1.0);
    for &p in &pts {
        draw_spot(mesh, p, dot * GRID_DOT_SIZE, GRID_DOTS);
    }
    draw_line_seq(mesh, &pts, Line::solid(stroke * 2.0, OUTER_COLOR));
    draw_line_seq(mesh, &pts, Line::solid(stroke, INNER_COLOR));
    if let Some(&first) = pts.first() {
        draw_spot(mesh, first, dot * 2.0, STARTING_DOT);
    }
}

/// Draws `pattern` centred in box `r` (GUI pixels).
pub fn draw_pattern(gfx: &GfxContext, pattern: &HexPattern, r: Rect) {
    let mut mesh = Mesh::new();
    mesh_pattern(&mut mesh, pattern, r, r.w.min(r.h) * STROKE_SHARE);
    mesh.draw(gfx);
}

/// How wide `pattern` is as a rune `h` tall in a line of text.
pub fn rune_width(pattern: &HexPattern, h: f32) -> f32 {
    let (w, ht) = extent(&super::render_lib::to_lines(pattern, 1.0, Vec2::ZERO));
    let aspect = if ht < FLAT {
        RUNE_ASPECT.1
    } else {
        (w / ht).clamp(RUNE_ASPECT.0, RUNE_ASPECT.1)
    };
    (h * aspect).ceil()
}

/// Adds `pattern` as a rune `h` tall in a line of text (its top-left at
/// `x, y`, [`rune_width`] wide): its stroke in `color`.
pub fn mesh_rune(mesh: &mut Mesh, pattern: &HexPattern, Vec2 { x, y }: Vec2, h: f32, color: u32) {
    let w = rune_width(pattern, h);
    let pts = super::render_lib::to_lines(pattern, 1.0, Vec2::ZERO);
    let (pw, ph) = extent(&pts);
    let (min_x, min_y) = pts
        .iter()
        .fold((f32::INFINITY, f32::INFINITY), |(a, b), p| {
            (a.min(p.x), b.min(p.y))
        });
    // A pixel's margin all round, the stroke inside it.
    let scale = ((w - 2.0) / pw.max(FLAT)).min((h - 2.0) / ph.max(FLAT));
    let off = Vec2::new(x + (w - pw * scale) / 2.0, y + (h - ph * scale) / 2.0);
    let pts: Vec<Vec2> = pts
        .iter()
        .map(|p| Vec2::new((p.x - min_x) * scale, (p.y - min_y) * scale) + off)
        .collect();
    draw_line_seq(mesh, &pts, Line::solid(1.0, color));
    if let Some(&first) = pts.first() {
        draw_spot(mesh, first, RUNE_DOT, color);
    }
}

/// Width and height of `points`' bounds.
fn extent(points: &[Vec2]) -> (f32, f32) {
    let (mut lo, mut hi) = (
        Vec2::new(f32::INFINITY, f32::INFINITY),
        Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
    );
    for p in points {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    if points.is_empty() {
        (0.0, 0.0)
    } else {
        (hi.x - lo.x, hi.y - lo.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn written_scrolls_and_slates_get_a_picture() {
        let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        let written = format!("{{pattern:\"{}\"}}", p.serialized_form());
        assert_eq!(
            tooltip(Items::SCROLL, &written).map(|(_, bg)| bg),
            Some(PRISTINE_BG)
        );
        let ancient = format!(
            "{{op_id:\"hexcasting:get_caster\",pattern:\"{}\"}}",
            p.serialized_form()
        );
        assert_eq!(
            tooltip(Items::SCROLL_SMALL, &ancient).map(|(_, bg)| bg),
            Some(ANCIENT_BG)
        );
        let slate = format!("{{BlockEntityTag:{{pattern:\"{}\"}}}}", p.serialized_form());
        assert_eq!(
            tooltip(holder::SLATE, &slate).map(|(_, bg)| bg),
            Some(SLATE_BG)
        );
        assert!(
            tooltip(Items::SCROLL, "{}").is_none(),
            "a blank scroll: no picture"
        );
    }

    #[test]
    fn a_stored_hex_shows_in_lines_of_runes() {
        let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        let hex =
            crate::iota::Iota::List(vec![crate::iota::Iota::Pattern(p.clone()); 12]).serialize();
        let snbt = format!(
            "{{patterns:\"{}\"}}",
            hex.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let (lines, _) = hex_lines(&snbt, &approx_width).expect("a hex");
        assert!(lines.len() > 1, "twelve runes wrap");
        assert!(
            lines
                .iter()
                .flatten()
                .any(|s| matches!(s, crate::text::Segment::Rune(r) if *r == p))
        );
        assert!(
            hex_lines("{}", &approx_width).is_none(),
            "an empty trinket: nothing"
        );
    }

    #[test]
    fn a_pattern_fills_its_box() {
        let p = HexPattern::from_angles("waqaw", HexDir::East).unwrap();
        let mut mesh = Mesh::new();
        mesh_pattern(
            &mut mesh,
            &p,
            Rect::new(0.0, 0.0, TOOLTIP_SIZE, TOOLTIP_SIZE),
            6.4,
        );
        assert!(mesh.vertex_count() > 0);
    }
}
