//! Text with runes in it ([`Segment`]s): an iota's words with its patterns
//! drawn as small runes a line tall — on the casting grid's stack and in
//! the tooltips of what stores a hex.

use yog_api::GfxContext;
use yog_api::ui::{Color, Vec2};

use super::pattern_render::{mesh_rune, rune_width};
use super::render_lib::Mesh;
use crate::text::Segment;

/// A rune's height: the font's line.
pub const RUNE_HEIGHT: f32 = 8.0;

/// How wide `seg` is drawn.
fn seg_width(seg: &Segment, width_of: &dyn Fn(&str) -> f32) -> f32 {
    match seg {
        Segment::Words(w) => width_of(w),
        Segment::Rune(p) => rune_width(p, RUNE_HEIGHT),
    }
}

/// How wide `segs` are drawn.
pub fn width(segs: &[Segment], width_of: &dyn Fn(&str) -> f32) -> f32 {
    segs.iter().map(|s| seg_width(s, width_of)).sum()
}

/// `segs` cut to `max_width`, room left for a "...": the longest start that
/// fits, words broken after a space when there's one; and whether it was cut.
pub fn fit(
    segs: &[Segment],
    max_width: f32,
    width_of: &dyn Fn(&str) -> f32,
) -> (Vec<Segment>, bool) {
    if width(segs, width_of) <= max_width - width_of("...") {
        return (segs.to_vec(), false);
    }
    let (line, _) = take_line(segs, max_width - width_of("..."), width_of);
    (line, true)
}

/// `segs` in lines no wider than `max_width` (a rune wider than a line
/// gets one of its own).
pub fn wrap(segs: &[Segment], max_width: f32, width_of: &dyn Fn(&str) -> f32) -> Vec<Vec<Segment>> {
    let mut lines = Vec::new();
    let mut rest = segs.to_vec();
    while !rest.is_empty() {
        let (mut line, left) = take_line(&rest, max_width, width_of);
        if line.is_empty() {
            // Nothing fits: the first piece alone.
            line.push(rest[0].clone());
            rest.remove(0);
        } else {
            rest = left;
        }
        if let Some(Segment::Words(w)) = rest.first_mut() {
            *w = w.trim_start().to_owned();
            if w.is_empty() {
                rest.remove(0);
            }
        }
        lines.push(line);
    }
    lines
}

/// The longest start of `segs` no wider than `limit`, and the rest.
fn take_line(
    segs: &[Segment],
    limit: f32,
    width_of: &dyn Fn(&str) -> f32,
) -> (Vec<Segment>, Vec<Segment>) {
    let mut line: Vec<Segment> = Vec::new();
    let mut used = 0.0;
    for (i, seg) in segs.iter().enumerate() {
        let w = seg_width(seg, width_of);
        if used + w <= limit {
            line.push(seg.clone());
            used += w;
            continue;
        }
        let mut rest: Vec<Segment> = segs[i..].to_vec();
        if let Segment::Words(text) = seg {
            // The words that fit, broken after a space when there's one.
            let mut end = 0;
            let mut last_space = None;
            for (j, c) in text.char_indices() {
                let next = j + c.len_utf8();
                if used + width_of(&text[..next]) > limit {
                    break;
                }
                end = next;
                if c == ' ' {
                    last_space = Some(next);
                }
            }
            let end = last_space.unwrap_or(end);
            let head = text[..end].trim_end();
            if !head.is_empty() {
                line.push(Segment::Words(head.to_owned()));
            }
            rest[0] = Segment::Words(text[end..].to_owned());
        }
        return (line, rest);
    }
    (line, Vec::new())
}

/// Draws `segs` with the line's top-left at `x, y`: words in `color` (a
/// shadow under them when `shadow`), runes in it too.
pub fn draw(gfx: &GfxContext, segs: &[Segment], x: f32, y: f32, color: u32, shadow: bool) {
    let d2 = gfx.draw2d();
    let width_of = |t: &str| d2.text_width(t, "");
    let mut mesh = Mesh::new();
    let mut at = x;
    for seg in segs {
        match seg {
            Segment::Words(w) => d2.text(w, Vec2::new(at, y), Color(color), shadow),
            Segment::Rune(p) => mesh_rune(&mut mesh, p, at, y, RUNE_HEIGHT, color),
        }
        at += seg_width(seg, &width_of);
    }
    mesh.draw(gfx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{HexDir, HexPattern};

    fn words(s: &str) -> Segment {
        Segment::Words(s.into())
    }

    #[test]
    fn runes_take_room_in_the_line() {
        let width = |s: &str| s.chars().count() as f32 * 6.0;
        let rune = Segment::Rune(HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap());
        let segs = vec![
            words("["),
            rune.clone(),
            words(", "),
            rune.clone(),
            words("]"),
        ];
        let full = super::width(&segs, &width);
        assert!(full > 4.0 * 6.0, "runes have width");
        assert_eq!(fit(&segs, full + 18.0, &width), (segs.clone(), false));
        let (cut, was_cut) = fit(
            &segs,
            6.0 + rune_width(
                match &rune {
                    Segment::Rune(p) => p,
                    _ => unreachable!(),
                },
                RUNE_HEIGHT,
            ) + 18.0
                + 1.0,
            &width,
        );
        assert!(was_cut);
        assert_eq!(cut, vec![words("["), rune.clone()]);
        assert_eq!(
            fit(&[words("one two three four")], 80.0, &width).0,
            vec![words("one two")]
        );
    }

    #[test]
    fn long_text_wraps_into_lines() {
        let width = |s: &str| s.chars().count() as f32 * 6.0;
        let lines = wrap(&[words("one two three four")], 50.0, &width);
        assert_eq!(
            lines,
            vec![
                vec![words("one two")],
                vec![words("three")],
                vec![words("four")]
            ]
        );
    }
}
