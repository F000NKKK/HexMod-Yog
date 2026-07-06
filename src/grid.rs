//! The casting grid — the heart of Hexcasting.
//!
//! Opened by right-clicking the staff. The player drags the mouse across a
//! hex lattice of dots; each completed stroke becomes a `HexPattern` that is
//! sent to the server for execution. The server answers with the current
//! stack, rendered in a side panel.
//!
//! Dots and pattern lines are drawn as anti-aliased SDF quads (see
//! `crate::grid_gl`) — real circles and rounded-cap segments, not blocky
//! rects; interaction uses the host's raw mouse events
//! (`click:`/`drag:`/`release:`).

use std::sync::Mutex;

use yog_api::{GfxContext, Server};

use crate::grid_gl;
use crate::math::{HexCoord, HexDir, HexPattern};

pub const UI_ID: &str = "hexcasting:grid";
pub const CH_CAST: &str = "hexcasting:cast";
pub const CH_STACK: &str = "hexcasting:stack";
pub const CH_OPEN: &str = "hexcasting:open_grid";

// ── Grid geometry ────────────────────────────────────────────────────────────

/// Distance between neighbouring dots in GUI pixels.
const SPACING: f32 = 26.0;
/// sqrt(3)/2 — vertical spacing factor for the hex lattice.
const ROW_F: f32 = 0.866;
/// Snap radius for the cursor to latch onto a dot.
const SNAP: f32 = SPACING * 0.45;

/// Fraction of the screen used by the grid; the rest is the stack panel.
const GRID_FRAC: f32 = 0.68;

fn coord_to_screen(c: HexCoord, ox: f32, oy: f32) -> (f32, f32) {
    (
        ox + SPACING * (c.0 as f32 + c.1 as f32 * 0.5),
        oy + SPACING * ROW_F * c.1 as f32,
    )
}

/// Nearest lattice dot to a screen point, if within the snap radius.
fn screen_to_coord(mx: f32, my: f32, ox: f32, oy: f32) -> Option<HexCoord> {
    let r = ((my - oy) / (SPACING * ROW_F)).round() as i32;
    let q = ((mx - ox) / SPACING - r as f32 * 0.5).round() as i32;
    // Check the candidate and its neighbours (rounding on a skewed lattice).
    let mut best: Option<(f32, HexCoord)> = None;
    let cand = HexCoord(q, r);
    for c in std::iter::once(cand).chain(cand.neighbors()) {
        let (sx, sy) = coord_to_screen(c, ox, oy);
        let d2 = (sx - mx).powi(2) + (sy - my).powi(2);
        if d2 <= SNAP * SNAP && best.map_or(true, |(bd, _)| d2 < bd) {
            best = Some((d2, c));
        }
    }
    best.map(|(_, c)| c)
}

fn dir_between(a: HexCoord, b: HexCoord) -> Option<HexDir> {
    let d = b.sub(a);
    (0..6u8).map(HexDir::from_ordinal).find(|dir| dir.as_delta() == d)
}

// ── UI state ─────────────────────────────────────────────────────────────────

/// One completed stroke plus how its cast resolved, for stroke colouring.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StrokeResult { Pending, Ok, Mishap }

/// Dots of the stroke currently being drawn.
static STROKE: Mutex<Vec<HexCoord>> = Mutex::new(Vec::new());
/// Completed strokes of this casting session, with their cast result.
static DRAWN: Mutex<Vec<(Vec<HexCoord>, StrokeResult)>> = Mutex::new(Vec::new());
/// Live cursor position while dragging (rubber-band segment).
static CURSOR: Mutex<Option<(f32, f32)>> = Mutex::new(None);
/// Stack lines as reported by the server (top first).
static STACK_VIEW: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Grid origin of the last frame (screen-size dependent).
static ORIGIN: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

/// The server's stack packet is `#ok\t<name>` or `#mishap\t<msg>` on its first
/// line (the cast result), followed by the stack itself, top first. The
/// result line colours the stroke that triggered it; the rest is displayed.
pub fn on_stack_packet(payload: &[u8]) {
    let text = String::from_utf8_lossy(payload);
    let mut lines = text.lines();
    let result = match lines.next() {
        Some(l) if l.starts_with("#ok") => Some(StrokeResult::Ok),
        Some(l) if l.starts_with("#mishap") => Some(StrokeResult::Mishap),
        _ => None,
    };
    if let Some(result) = result {
        if let Some(last) = DRAWN.lock().unwrap().last_mut() {
            last.1 = result;
        }
    }
    *STACK_VIEW.lock().unwrap() = lines.map(|s| s.to_string()).collect();
}

/// Reset per-session drawing visuals when the grid opens. Called on every
/// open (the in-progress stroke is always transient); does NOT touch
/// STACK_VIEW — the stack is server-authoritative and survives reopening.
pub fn on_open() {
    STROKE.lock().unwrap().clear();
    DRAWN.lock().unwrap().clear();
    *CURSOR.lock().unwrap() = None;
}

/// Called when the server actually wiped the caster's stack (shift + staff).
/// Clears the displayed stack so a stale view doesn't linger after reset.
pub fn on_reset() {
    STACK_VIEW.lock().unwrap().clear();
}

// ── Rendering ────────────────────────────────────────────────────────────────

pub fn render(gfx: &GfxContext) {
    let d2d = gfx.draw2d();
    let (sw_i, sh_i) = gfx.screen_size();
    let (sw, sh) = (sw_i as f32, sh_i as f32);

    let grid_w = sw * GRID_FRAC;
    let (ox, oy) = (grid_w / 2.0, sh / 2.0);
    *ORIGIN.lock().unwrap() = (ox, oy);

    // Panels
    d2d.rect(0.0, 0.0, grid_w, sh, 0xAA_0b0b12);
    d2d.rect(grid_w, 0.0, sw, sh, 0xCC_14141c);

    // Lattice dots covering the grid panel — smooth anti-aliased circles.
    let r_range = (sh / (SPACING * ROW_F) / 2.0).ceil() as i32 + 1;
    let q_extra = (grid_w / SPACING / 2.0).ceil() as i32 + 1;
    let stroke = STROKE.lock().unwrap().clone();
    for r in -r_range..=r_range {
        for q in (-q_extra - r / 2 - 1)..=(q_extra - r / 2 + 1) {
            let c = HexCoord(q, r);
            let (x, y) = coord_to_screen(c, ox, oy);
            if x < 8.0 || x > grid_w - 8.0 || y < 8.0 || y > sh - 8.0 {
                continue;
            }
            let active = stroke.last() == Some(&c);
            let in_stroke = stroke.contains(&c);
            let (radius, color) = if active {
                (2.4, 0xFF_FFD75E)
            } else if in_stroke {
                (2.0, 0xFF_C9A94B)
            } else {
                (1.4, 0x88_8888AA)
            };
            grid_gl::dot(gfx, x, y, radius, color);
        }
    }

    // Completed strokes of this session — colour follows the cast result.
    for (pat, result) in DRAWN.lock().unwrap().iter() {
        let color = match result {
            StrokeResult::Pending => 0x99_9a7b2f,
            StrokeResult::Ok      => 0xCC_4fd67a,
            StrokeResult::Mishap  => 0xCC_e05a4f,
        };
        for pair in pat.windows(2) {
            let (x0, y0) = coord_to_screen(pair[0], ox, oy);
            let (x1, y1) = coord_to_screen(pair[1], ox, oy);
            grid_gl::line(gfx, x0, y0, x1, y1, 2.0, color);
        }
    }

    // Current stroke (bright gold) + rubber band to the cursor
    for pair in stroke.windows(2) {
        let (x0, y0) = coord_to_screen(pair[0], ox, oy);
        let (x1, y1) = coord_to_screen(pair[1], ox, oy);
        grid_gl::line(gfx, x0, y0, x1, y1, 2.5, 0xFF_E8C24A);
    }
    if let (Some(&last), Some((cx, cy))) = (stroke.last(), *CURSOR.lock().unwrap()) {
        let (x0, y0) = coord_to_screen(last, ox, oy);
        grid_gl::line(gfx, x0, y0, cx, cy, 2.0, 0x99_E8C24A);
    }

    // Stack panel
    let px = grid_w + 10.0;
    d2d.text("The Stack", px, 10.0, 0xFF_FFD700, true);
    let stack = STACK_VIEW.lock().unwrap();
    if stack.is_empty() {
        d2d.text("(empty)", px, 28.0, 0x88_888888, false);
    } else {
        let mut y = 28.0;
        for (i, line) in stack.iter().enumerate() {
            let color = if i == 0 { 0xFF_FFFFFF } else { 0xAA_BBBBBB };
            d2d.text(line, px, y, color, false);
            y += 12.0;
            if y > sh - 24.0 {
                d2d.text("...", px, y, 0x88_888888, false);
                break;
            }
        }
    }

    // Hint
    d2d.text(
        "Drag across the dots to draw a pattern. ESC to close.",
        8.0, sh - 14.0, 0x77_AAAAAA, false,
    );
}

// ── Events ───────────────────────────────────────────────────────────────────

pub fn handle_event(_ui_id: &str, event: &str) {
    let parse_xy = |rest: &str| -> Option<(f32, f32)> {
        let mut it = rest.split(':');
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    };
    let (ox, oy) = *ORIGIN.lock().unwrap();

    if let Some((mx, my)) = event.strip_prefix("click:").and_then(|r| parse_xy(r)) {
        *CURSOR.lock().unwrap() = Some((mx, my));
        if let Some(c) = screen_to_coord(mx, my, ox, oy) {
            let mut stroke = STROKE.lock().unwrap();
            stroke.clear();
            stroke.push(c);
        }
        return;
    }

    if let Some((mx, my)) = event.strip_prefix("drag:").and_then(|r| parse_xy(r)) {
        *CURSOR.lock().unwrap() = Some((mx, my));
        let Some(c) = screen_to_coord(mx, my, ox, oy) else { return };
        let mut stroke = STROKE.lock().unwrap();
        let Some(&last) = stroke.last() else { return };
        if c == last {
            return;
        }
        // Dragging back onto the previous dot undoes the last segment.
        if stroke.len() >= 2 && stroke[stroke.len() - 2] == c {
            stroke.pop();
            return;
        }
        // Hexcasting rule: an EDGE may be traversed only once, but vertices
        // may repeat — closed shapes like the Mind's Reflection diamond pass
        // through the starting dot again.
        if dir_between(last, c).is_some() {
            let edge = |a: HexCoord, b: HexCoord| if (a.0, a.1) <= (b.0, b.1) { (a, b) } else { (b, a) };
            let new_edge = edge(last, c);
            let used = stroke.windows(2).any(|p| edge(p[0], p[1]) == new_edge);
            if !used {
                stroke.push(c);
            }
        }
        return;
    }

    if event.starts_with("release:") {
        *CURSOR.lock().unwrap() = None;
        let mut stroke = STROKE.lock().unwrap();
        if stroke.len() >= 2 {
            let dirs: Vec<HexDir> = stroke
                .windows(2)
                .filter_map(|p| dir_between(p[0], p[1]))
                .collect();
            let pattern = HexPattern::from_list(dirs);
            if let Some(srv) = yog_api::server() {
                srv.send_to_server(CH_CAST, pattern.serialized_form().as_bytes());
            }
            DRAWN.lock().unwrap().push((std::mem::take(&mut *stroke), StrokeResult::Pending));
        } else {
            stroke.clear();
        }
        return;
    }
}
