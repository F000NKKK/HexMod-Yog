//! The book's pattern page (`hexcasting:pattern`, Hex Casting's
//! `AbstractPatternComponent`): the page's patterns in a grid of cells the
//! size of its picture, each fitted into its cell, in the book's colors
//! (`READABLE_GRID_SCROLL_COLORS`) and strokes (`fromStroke(4)`), still —
//! the book's patterns don't move. The book lays the page out by
//! `pattern_page.yui` (title, picture, `input → output`, text) and calls
//! [`draw`] for the picture.

use yog_api::ui::Rect;
use yog_api::yog_data::Value;
use yog_api::{GfxContext, Registry};

use super::render_lib::{
    INNER_SHARE, Line, Mesh, Vec2, coord_to_px, draw_line_seq, draw_spot, find_dup_indices,
    static_zappy,
};
use crate::math::{HexDir, HexPattern};

/// The drawer id the book's pages name.
pub const DRAWER: &str = "hexcasting:pattern";

const OUTER: u32 = 0xff_d2c8c8;
const INNER: u32 = 0xff_554d54;
const START_DOT: u32 = 0xff_5b7bd7;
/// Grid dots: the outline color at half alpha.
const GRID_DOT: u32 = 0x80_d2c8c8;

/// The book's sizes are for a box this high; they scale with it: the
/// stroke, the padding, `HexPatternPoints`' base scale (shrunk by the
/// second), and the dots' sizes of the inner stroke.
const BOOK_BOX: f32 = 64.0;
const STROKE: f32 = 4.0;
const PAD: f32 = 4.0;
const BASE_SCALE: f32 = 16.0;
const BASE_SHRINK: f32 = 1.5;
const START_DOT_SIZE: f32 = 0.8;
const GRID_DOT_SIZE: f32 = 0.4;

pub fn register(registry: &mut Registry) {
    registry.on_book_page(DRAWER, |gfx, data, [x, y, w, h]| {
        draw(gfx, &patterns(data), Rect::new(x, y, w, h))
    });
}

/// The page's patterns — the list itself (`PatternPage`'s `patterns`) or
/// an object's `patterns`: `{ startdir: "EAST", signature: "qaq" }` each.
pub fn patterns(data: &Value) -> Vec<HexPattern> {
    let Some(list) = data
        .as_list()
        .or_else(|| data.get("patterns").and_then(Value::as_list))
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|p| {
            let dir = p.get("startdir").and_then(Value::as_str)?;
            let dir = HexDir::ALL
                .into_iter()
                .find(|d| d.name().eq_ignore_ascii_case(dir))?;
            HexPattern::from_angles(
                p.get("signature").and_then(Value::as_str).unwrap_or(""),
                dir,
            )
        })
        .collect()
}

/// The patterns' meshes for the `w`×`h` box at `x, y`. The book's sizes are
/// for a 64-pixel-high box; they scale with `h`.
pub fn mesh(patterns: &[HexPattern], Rect { x, y, w, h }: Rect) -> Mesh {
    let px = h / BOOK_BOX;
    let stroke = STROKE * px;
    let mut mesh = Mesh::new();
    let n = patterns.len().max(1);
    let cols = (n as f32).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);
    let (cell_w, cell_h) = (w / cols as f32, h / rows as f32);
    for (k, pat) in patterns.iter().enumerate() {
        let pos = pat.positions();
        if pos.len() < 2 {
            continue;
        }
        let dots: Vec<Vec2> = pos
            .iter()
            .map(|&p| coord_to_px(p, 1.0, Vec2::new(0.0, 0.0)))
            .collect();
        let dups = find_dup_indices(pos.iter().copied());
        let zappy = static_zappy(&dots, &dups, true, 0.0);
        // HexPatternPoints: fit into the cell (padding 2, base scale 16).
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &zappy {
            (x0, x1, y0, y1) = (x0.min(p.x), x1.max(p.x), y0.min(p.y), y1.max(p.y));
        }
        let base = BASE_SCALE / BASE_SHRINK * px;
        let (bw, bh) = ((x1 - x0) * base, (y1 - y0) * base);
        let pad = PAD * px + stroke;
        let scale = 1f32.min((cell_h - pad) / bh).min((cell_w - pad) / bw);
        let fin = base * scale;
        let (iw, ih) = (bw * scale + pad, bh * scale + pad);
        let off_x = (iw - bw * scale) / 2.0 + (cell_w - iw).max(0.0) / 2.0;
        let off_y = (ih - bh * scale) / 2.0 + (cell_h - ih).max(0.0) / 2.0;
        let (cx, cy) = (
            x + cell_w * (k % cols) as f32,
            y + cell_h * (k / cols) as f32,
        );
        let at = |p: Vec2| Vec2::new(cx + (p.x - x0) * fin + off_x, cy + (p.y - y0) * fin + off_y);
        let line: Vec<Vec2> = zappy.iter().map(|&p| at(p)).collect();
        draw_line_seq(&mut mesh, &line, Line::solid(stroke, OUTER));
        draw_line_seq(&mut mesh, &line, Line::solid(stroke * INNER_SHARE, INNER));
        let dot = stroke * INNER_SHARE;
        draw_spot(&mut mesh, at(dots[0]), START_DOT_SIZE * dot, START_DOT);
        for &d in &dots[1..] {
            draw_spot(&mut mesh, at(d), GRID_DOT_SIZE * dot, GRID_DOT);
        }
    }
    mesh
}

pub fn draw(gfx: &GfxContext, patterns: &[HexPattern], r: Rect) {
    mesh(patterns, r).draw(gfx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(json: &str) -> Value {
        yog_api::yog_data::text::parse_value(json).unwrap()
    }

    #[test]
    fn a_page_reads_its_patterns() {
        let d = data(
            r#"{ patterns: [ { startdir: "NORTH_EAST", signature: "qaq" }, { startdir: "EAST", signature: "aa" }, { startdir: "UP" } ] }"#,
        );
        let p = patterns(&d);
        assert_eq!(p.len(), 2, "an unknown direction is skipped");
        assert_eq!(p[0].positions().len(), 5);
        assert!(patterns(&data("{ op_id: \"hexcasting:manual\" }")).is_empty());
        assert_eq!(
            patterns(&data(r#"[ { startdir: "EAST", signature: "aa" } ]"#)).len(),
            1,
            "the list itself"
        );
    }

    #[test]
    fn patterns_fill_the_box_in_cells() {
        let p = patterns(&data(
            r#"{ patterns: [ { startdir: "EAST", signature: "qaq" }, { startdir: "EAST", signature: "w" } ] }"#,
        ));
        let one = mesh(&p[..1], Rect::new(0.0, 0.0, 116.0, 64.0));
        let two = mesh(&p, Rect::new(0.0, 0.0, 116.0, 64.0));
        assert!(one.vertex_count() > 0 && two.vertex_count() > one.vertex_count());
    }

    /// For looking at the page: `HEX_PAGE_OUT=file cargo test write_a_pattern_page`
    /// writes the mesh of Mind's Reflection and two others (116×64, scale 4).
    #[test]
    fn write_a_pattern_page() {
        let Some(out) = std::env::var_os("HEX_PAGE_OUT") else {
            return;
        };
        let p = patterns(&data(
            r#"{ patterns: [ { startdir: "NORTH_EAST", signature: "qaq" }, { startdir: "EAST", signature: "aa" }, { startdir: "EAST", signature: "qaqqqqq" } ] }"#,
        ));
        let m = mesh(&p, Rect::new(0.0, 0.0, 116.0 * 4.0, 64.0 * 4.0));
        let text: Vec<String> = m.vertices().iter().map(|v| v.to_string()).collect();
        std::fs::write(out, text.join(" ")).unwrap();
    }
}
