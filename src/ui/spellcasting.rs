//! Hex Casting's staff screen (`GuiSpellcasting`), as in the original: the
//! whole screen is a transparent grid. Dots fade in around the cursor,
//! dragging between them draws a pattern, which goes to the server when the
//! button comes up; the patterns drawn so far stay on the grid in the colour
//! of how they resolved. The stack is listed top-left, the ravenmind
//! top-right. Esc first drops the stroke being drawn, then closes; the
//! screen also closes when the staff leaves the hand it was opened with.
//!
//! The screen (`spellcasting.yui`) is one `<Surface>`; [`GridSurface`] draws
//! it and takes the pointer.

/// The generated screen (`ID`, `register`, `close`, …), apart so its
/// `open()` doesn't clash with [`open`].
mod view {
    use super::SpellcastingVm;
    yog_api::ui_include!("ui/spellcasting");
}

pub use view::{handle, register, ID};
use view::close;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, Once};

use yog_api::client::{self, ui_flags, SoundHandle};
use yog_api::ui::{Pointer, Rect, Surface};
use yog_api::{GfxContext, Server};
use yog_kit::prelude::*;

use super::render_lib::{
    coord_to_px, draw_pattern_from_points, draw_spot, find_dup_indices, positions_from, px_to_coord, range_around,
    ticks, to_lines, Mesh, PatternStyle, Vec2, DEFAULT_READABILITY_OFFSET,
};
use crate::iota::Iota;
use crate::math::{HexAngle, HexCoord, HexDir, HexPattern};
use crate::protocol::{Hand, NewPattern, OpenGui, PatternResult, ResolvedPattern, ResolvedType, CH_NEW_PATTERN};
use crate::wire::Writer;

/// `MsgShiftScrollC2S` (client → server): `num` main-hand delta, `num`
/// off-hand delta, `int` 1 when the sprint key is down. Scrolls the
/// spellbook page or the abacus in that hand.
pub const CH_SCROLL: &str = "hexcasting:scroll";

const SURFACE_ID: &str = "hexcasting:spellcasting_grid";

const LHS_IOTAS_ALLOCATION: f32 = 0.7;
const RHS_IOTAS_ALLOCATION: f32 = 0.15;
const BOX_COLOR: u32 = 0x50_303030;
/// `ChatFormatting.GRAY`, the "..." after a cut-off iota.
const ELLIPSIS_COLOR: u32 = 0xFF_AAAAAA;

const SOUND_AMBIANCE: &str = "hexcasting:casting.ambiance";
const SOUND_START: &str = "hexcasting:casting.pattern.start";
const SOUND_ADD_SEGMENT: &str = "hexcasting:casting.pattern.add_segment";

/// Introspection, Consideration-of-N and Interjection: their EVALUATED
/// patterns can still be undone by Evanition.
const UNDOABLE_EVALUATED: [&str; 3] = ["qqq", "qdaqadq", "aqqqqqwded"];

// ── the grid's state machine ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum DrawState {
    /// Waiting for the button to go down.
    BetweenPatterns,
    /// The button is down on `start`; no stroke yet.
    JustStarted(HexCoord),
    /// Drawing `wip` from `start`; the stroke is at `current`.
    Drawing { start: HexCoord, current: HexCoord, wip: HexPattern },
}

/// What the grid wants done outside it.
#[derive(Debug, Clone, PartialEq)]
enum Effect {
    StartSound,
    SegmentSound,
    Send(NewPattern),
}

/// An iota as shown in a box: its text cut to fit, and its colour.
#[derive(Debug, Clone, PartialEq)]
struct Desc {
    text: String,
    cut: bool,
    color: u32,
}

/// One casting session, from the server's `OpenGui` to closing.
struct Grid {
    hand: Hand,
    patterns: Vec<ResolvedPattern>,
    /// Bottom first.
    stack: Vec<Iota>,
    ravenmind: Option<Iota>,
    #[allow(dead_code)] // the original keeps it but no longer shows it
    paren_count: i64,
    draw: DrawState,
    used_spots: HashSet<HexCoord>,
    /// The cursor, in the screen's GUI pixels.
    mouse: Vec2,
    /// The screen's size (`this.width`, `this.height`).
    size: (f32, f32),
    scroll_acc: f64,
    /// Top first; with the width they were cut for.
    stack_descs: Option<(f32, Vec<Desc>)>,
    ravenmind_desc: Option<(f32, Option<Desc>)>,
}

impl Grid {
    fn new(gui: OpenGui) -> Self {
        let used_spots = gui.patterns.iter().flat_map(|p| positions_from(&p.pattern, p.origin)).collect();
        Grid {
            hand: gui.hand,
            patterns: gui.patterns,
            stack: gui.stack,
            ravenmind: gui.ravenmind,
            paren_count: gui.paren_count,
            draw: DrawState::BetweenPatterns,
            used_spots,
            mouse: Vec2::ZERO,
            size: (0.0, 0.0),
            scroll_acc: 0.0,
            stack_descs: None,
            ravenmind_desc: None,
        }
    }

    /// Distance between neighbouring dots: 512 hexes' worth of area on
    /// any screen.
    fn hex_size(&self) -> f32 {
        (self.size.0 as f64 * self.size.1 as f64 / 512.0).sqrt() as f32
    }

    fn coords_offset(&self) -> Vec2 {
        Vec2::new(self.size.0 * 0.5, self.size.1 * 0.5)
    }

    fn coord_to_px(&self, c: HexCoord) -> Vec2 {
        coord_to_px(c, self.hex_size(), self.coords_offset())
    }

    fn px_to_coord(&self, px: Vec2) -> HexCoord {
        px_to_coord(px, self.hex_size(), self.coords_offset())
    }

    fn clamp(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x.clamp(0.0, self.size.0), y.clamp(0.0, self.size.1))
    }

    fn draw_start(&mut self, x: f32, y: f32) -> Option<Effect> {
        let m = self.clamp(x, y);
        if self.draw != DrawState::BetweenPatterns {
            return None;
        }
        let coord = self.px_to_coord(m);
        if self.used_spots.contains(&coord) {
            return None;
        }
        self.draw = DrawState::JustStarted(coord);
        Some(Effect::StartSound)
    }

    fn draw_move(&mut self, x: f32, y: f32) -> Option<Effect> {
        let mouse = self.clamp(x, y);
        let anchor_coord = match &self.draw {
            DrawState::BetweenPatterns => return None,
            DrawState::JustStarted(start) => *start,
            DrawState::Drawing { current, .. } => *current,
        };
        let anchor = self.coord_to_px(anchor_coord);
        // `gridSnapThreshold` (0.5 by default, clamped to 0.5..1).
        let snap_dist = self.hex_size() * self.hex_size() * 2.0 * 0.5;
        if anchor.distance_to_sqr(mouse) < snap_dist {
            return None;
        }
        let delta = mouse - anchor;
        let angle = delta.y.atan2(delta.x);
        // 0 is right, increases clockwise.
        let snapped = (angle / std::f32::consts::TAU).rem_euclid(6.0);
        let new_dir = HexDir::from_ordinal((super::render_lib::round_to_int(snapped * 6.0) + 1).rem_euclid(6) as u8);
        // A lousy aim still lands on the ideal next dot.
        let ideal_next = anchor_coord + new_dir.as_delta();
        if self.used_spots.contains(&ideal_next) {
            return None;
        }
        let play = match &mut self.draw {
            DrawState::BetweenPatterns => false,
            DrawState::JustStarted(start) => {
                let start = *start;
                self.draw = DrawState::Drawing {
                    start,
                    current: ideal_next,
                    wip: HexPattern::new(new_dir, Vec::new()),
                };
                true
            }
            DrawState::Drawing { current, wip, .. } => {
                let last_dir = *wip.steps.last().unwrap_or(&wip.start);
                if new_dir == last_dir.rotated_by(HexAngle::OneEighty) {
                    // Straight back: undo the last stroke.
                    if wip.steps.is_empty() {
                        self.draw = DrawState::JustStarted(*current + new_dir.as_delta());
                    } else {
                        *current = *current + new_dir.as_delta();
                        wip.steps.pop();
                    }
                    true
                } else if try_append_dir(wip, new_dir) {
                    *current = ideal_next;
                    true
                } else {
                    false
                }
            }
        };
        play.then_some(Effect::SegmentSound)
    }

    fn draw_end(&mut self) -> Option<Effect> {
        match std::mem::replace(&mut self.draw, DrawState::BetweenPatterns) {
            DrawState::BetweenPatterns | DrawState::JustStarted(_) => None,
            DrawState::Drawing { start, wip, .. } => {
                self.used_spots.extend(positions_from(&wip, start));
                self.patterns.push(ResolvedPattern {
                    pattern: wip.clone(),
                    origin: start,
                    kind: ResolvedType::Unresolved,
                });
                Some(Effect::Send(NewPattern {
                    hand: self.hand,
                    pattern: wip,
                    resolved: self.patterns.clone(),
                }))
            }
        }
    }

    /// Esc: drops the stroke being drawn; `false` when there's none (then
    /// the screen closes).
    fn cancel_stroke(&mut self) -> bool {
        if self.draw == DrawState::BetweenPatterns {
            return false;
        }
        self.draw = DrawState::BetweenPatterns;
        true
    }

    /// `recvServerUpdate`; `false` = the stack was cleared, close.
    fn recv_server_update(&mut self, res: PatternResult) -> bool {
        if res.stack_clear {
            return false;
        }
        let index = usize::try_from(res.index).ok();
        if res.kind == ResolvedType::Undone {
            // The last pattern that can be undone (not the Evanition just
            // drawn) shows as undone; the Evanition itself as evaluated.
            let n = self.patterns.len();
            if let Some(p) =
                self.patterns.iter_mut().take(n.saturating_sub(1)).rev().find(|p| can_be_undone(p))
            {
                p.kind = ResolvedType::Undone;
            }
            if let Some(p) = index.and_then(|i| self.patterns.get_mut(i)) {
                p.kind = ResolvedType::Evaluated;
            }
        } else if let Some(p) = index.and_then(|i| self.patterns.get_mut(i)) {
            p.kind = res.kind;
        }
        self.stack = res.stack;
        self.ravenmind = res.ravenmind;
        self.stack_descs = None;
        self.ravenmind_desc = None;
        true
    }

    /// The wheel turned `delta`; once a whole notch has built up, the
    /// scroll it makes (`ShiftScrollListener.onScroll(delta, false)`).
    fn scroll(&mut self, delta: f64) -> bool {
        if self.scroll_acc != 0.0 && delta.signum() != self.scroll_acc.signum() {
            self.scroll_acc = 0.0;
        }
        self.scroll_acc += delta;
        let whole = self.scroll_acc as i64;
        if whole == 0 {
            return false;
        }
        self.scroll_acc -= whole as f64;
        true
    }

    /// Builds the dots, the patterns and the stroke being drawn.
    fn mesh(&self, gradient: bool) -> Mesh {
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
            let color = super::render_lib::argb_f(d, lerp(d, 0.4, 0.5), lerp(d, 0.8, 1.0), lerp(d, 0.7, 0.9));
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
                positions_from(wip, *start).into_iter().map(|c| self.coord_to_px(c)).collect(),
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
    fn stack_descs(&mut self, width_of: &dyn Fn(&str) -> f32) -> &[Desc] {
        let max = (self.size.0 * LHS_IOTAS_ALLOCATION).trunc();
        if self.stack_descs.as_ref().is_none_or(|(w, _)| *w != max) {
            let descs = self.stack.iter().rev().map(|i| describe(i, max, width_of)).collect();
            self.stack_descs = Some((max, descs));
        }
        &self.stack_descs.as_ref().unwrap().1
    }

    fn ravenmind_desc(&mut self, width_of: &dyn Fn(&str) -> f32) -> Option<&Desc> {
        let max = (self.size.0 * RHS_IOTAS_ALLOCATION).trunc();
        if self.ravenmind_desc.as_ref().is_none_or(|(w, _)| *w != max) {
            let desc = self.ravenmind.as_ref().map(|i| describe(i, max, width_of));
            self.ravenmind_desc = Some((max, desc));
        }
        self.ravenmind_desc.as_ref().unwrap().1.as_ref()
    }

    /// The stack and ravenmind boxes (`drawBox`), as a mesh.
    fn boxes(&mut self, width_of: &dyn Fn(&str) -> f32) -> Mesh {
        let mut mesh = Mesh::new();
        let w = self.size.0;
        let rows = self.stack_descs(width_of).len();
        if rows > 0 {
            draw_box(&mut mesh, 10.0, 10.0, w * LHS_IOTAS_ALLOCATION + 5.0, (rows as f32 + 1.0) * 10.0);
        }
        if self.ravenmind_desc(width_of).is_some() {
            let (x, y) = ravenmind_origin(w);
            draw_box(&mut mesh, x, y, w * RHS_IOTAS_ALLOCATION * RAVENMIND_SCALE, 15.0 * RAVENMIND_SCALE);
        }
        mesh
    }
}

/// The ravenmind box is drawn this much bigger.
const RAVENMIND_SCALE: f32 = 1.5;

fn ravenmind_origin(w: f32) -> (f32, f32) {
    (w * (1.0 - RHS_IOTAS_ALLOCATION * RAVENMIND_SCALE) - 10.0, 10.0)
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// `GuiSpellcasting.drawBox`: two translucent rectangles, one inset.
fn draw_box(mesh: &mut Mesh, x: f32, y: f32, w: f32, h: f32) {
    let left_margin = 2.5;
    mesh.quad(x, y, w, h, BOX_COLOR);
    mesh.quad(x + left_margin, y + 2.5, w - left_margin - 2.5, h - 5.0, BOX_COLOR);
}

/// `HexPattern.tryAppendDir`: no edge twice, no doubling back.
fn try_append_dir(p: &mut HexPattern, new_dir: HexDir) -> bool {
    let pts = p.positions();
    let end = *pts.last().unwrap_or(&HexCoord(0, 0));
    let next = end + new_dir.as_delta();
    if pts.windows(2).any(|w| (w[0] == end && w[1] == next) || (w[0] == next && w[1] == end)) {
        return false;
    }
    let last = *p.steps.last().unwrap_or(&p.start);
    if new_dir == last.rotated_by(HexAngle::OneEighty) {
        return false;
    }
    p.steps.push(new_dir);
    true
}

/// `canBeUndone`.
fn can_be_undone(p: &ResolvedPattern) -> bool {
    match p.kind {
        ResolvedType::Escaped => true,
        ResolvedType::Evaluated => UNDOABLE_EVALUATED.contains(&p.pattern.signature().as_str()),
        _ => false,
    }
}

/// `IotaType.getDisplayWithMaxWidth`: the iota's text, and when it's wider
/// than `max_width`, its first line (wrapped at `max_width` less the "...")
/// followed by a grey "...". Coloured by the iota's type.
fn describe(iota: &Iota, max_width: f32, width_of: &dyn Fn(&str) -> f32) -> Desc {
    let (text, cut) = fit(&iota.display().render(), max_width, width_of);
    Desc { text, cut, color: iota.kind().color() }
}

fn fit(text: &str, max_width: f32, width_of: &dyn Fn(&str) -> f32) -> (String, bool) {
    let limit = max_width - width_of("...");
    if width_of(text) <= limit {
        return (text.to_owned(), false);
    }
    // The longest prefix that fits, broken after a space when there is one
    // (how the font splits lines).
    let mut end = 0;
    let mut last_space = None;
    for (i, c) in text.char_indices() {
        let next = i + c.len_utf8();
        if width_of(&text[..next]) > limit {
            break;
        }
        end = next;
        if c == ' ' {
            last_space = Some(next);
        }
    }
    let end = last_space.unwrap_or(end);
    (text[..end].trim_end().to_owned(), true)
}

// ── the screen ───────────────────────────────────────────────────────────────

struct Session {
    grid: Option<Grid>,
    ambiance: SoundHandle,
    /// A close was asked for (the staff left the hand); don't ask again.
    closing: bool,
}

static SESSION: Mutex<Session> = Mutex::new(Session { grid: None, ambiance: SoundHandle(0), closing: false });

fn session() -> MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Draws the grid and takes the pointer — see the [module docs](self).
struct GridSurface;

impl GridSurface {
    fn apply(effect: Option<Effect>) {
        match effect {
            None => {}
            Some(Effect::StartSound) => {
                client::play_sound(SOUND_START, 0.25, 1.0, false);
            }
            Some(Effect::SegmentSound) => {
                client::play_sound(SOUND_ADD_SEGMENT, 0.25, 1.0 + (random() - 0.5) * 0.1, false);
            }
            Some(Effect::Send(msg)) => {
                if let Some(srv) = yog_api::server() {
                    srv.send_to_server(CH_NEW_PATTERN, &msg.encode());
                }
            }
        }
    }
}

impl Surface for GridSurface {
    fn paint(&self, gfx: &GfxContext, rect: Rect) {
        let mut s = session();
        let Some(grid) = s.grid.as_mut() else { return };
        grid.size = (rect.w, rect.h);

        // `tick`: the staff must stay in the hand the grid was opened with.
        let held = client::held_item(grid.hand == Hand::Off);
        if !held.is_some_and(|id| id.starts_with("hexcasting:staff/")) && !s.closing {
            s.closing = true;
            close();
        }
        let Some(grid) = s.grid.as_mut() else { return };

        let mut mesh = grid.mesh(client::control_down());
        let d2 = gfx.draw2d();
        let width_of = |t: &str| d2.text_width(t, "");
        mesh.append(grid.boxes(&width_of));
        mesh.offset(rect.x, rect.y);
        mesh.draw(gfx);

        let (ox, oy) = (rect.x, rect.y);
        let w = grid.size.0;
        let mut y = oy + 10.0 + 7.0;
        for desc in grid.stack_descs(&width_of) {
            draw_desc(&d2, &width_of, desc, ox + 10.0 + 5.0, y, desc.color);
            y += 10.0;
        }
        if let Some(desc) = grid.ravenmind_desc(&width_of) {
            let (x, y) = ravenmind_origin(w);
            let opacity = lerp(((ticks() * 0.2).sin() as f32 + 1.0) / 2.0, 150.0, 255.0) as u32;
            let color = (desc.color & 0x00FF_FFFF) | opacity << 24;
            // The original draws this 1.5× larger; Draw2D has no text scale.
            draw_desc(&d2, &width_of, desc, ox + x + 5.0, oy + y + 5.0, color);
        }
    }

    fn pointer(&self, ev: Pointer, size: (f32, f32)) -> bool {
        let effect = {
            let mut s = session();
            let Some(grid) = s.grid.as_mut() else { return false };
            grid.size = size;
            match ev {
                Pointer::Down { x, y } => {
                    grid.mouse = Vec2::new(x, y);
                    grid.draw_start(x, y)
                }
                Pointer::Drag { x, y } => {
                    grid.mouse = Vec2::new(x, y);
                    grid.draw_move(x, y)
                }
                Pointer::Up { .. } => grid.draw_end(),
                Pointer::Move { x, y } => {
                    grid.mouse = Vec2::new(x, y);
                    None
                }
                Pointer::Leave => None,
            }
        };
        Self::apply(effect);
        true
    }

    fn scroll(&self, dy: f32, _size: (f32, f32)) -> bool {
        let notch = session().grid.as_mut().is_some_and(|g| g.scroll(dy as f64));
        if notch {
            shift_scroll(dy as f64);
        }
        true
    }
}

/// `ShiftScrollListener.onScroll(delta, needsSneaking = false)`: scrolls the
/// spellbook or abacus in the main hand, else in the off hand.
fn shift_scroll(delta: f64) {
    let scrollable = |id: Option<String>| {
        id.is_some_and(|id| id == "hexcasting:spellbook" || id == "hexcasting:abacus")
    };
    let (main, off) = if scrollable(client::held_item(false)) {
        (delta, 0.0)
    } else if scrollable(client::held_item(true)) {
        (0.0, delta)
    } else {
        return;
    };
    let mut w = Writer::new();
    // The sprint key is Ctrl by default.
    w.num(main).num(off).int(client::control_down() as i64);
    if let Some(srv) = yog_api::server() {
        srv.send_to_server(CH_SCROLL, w.finish().as_bytes());
    }
}

fn draw_desc(
    d2: &yog_api::gfx_draw2d::Draw2D<'_>,
    width_of: &dyn Fn(&str) -> f32,
    desc: &Desc,
    x: f32,
    y: f32,
    color: u32,
) {
    d2.text(&desc.text, x, y, color, true);
    if desc.cut {
        let alpha = color & 0xFF00_0000;
        d2.text("...", x + width_of(&desc.text), y, (ELLIPSIS_COLOR & 0x00FF_FFFF) | alpha, true);
    }
}

/// 0..1, for the stroke sounds' pitch.
fn random() -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
            | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The screen's view model: only the lifecycle hooks — the grid is
/// [`GridSurface`]'s.
#[derive(Default)]
pub struct SpellcastingVm;

impl ViewModel for SpellcastingVm {
    fn on_open(&mut self, _cx: &mut Cx<'_, Self>) {
        let mut s = session();
        client::stop_sound(s.ambiance);
        s.ambiance = client::play_sound(SOUND_AMBIANCE, 1.0, 1.0, true);
    }

    fn on_close(&mut self, _cx: &mut Cx<'_, Self>) {
        // The grid stays: reopening (a new `open`) replaces it, and may
        // close this screen after the new grid is in.
        client::stop_sound(std::mem::take(&mut session().ambiance));
    }

    fn on_back(&mut self, _cx: &mut Cx<'_, Self>) -> bool {
        session().grid.as_mut().is_some_and(Grid::cancel_stroke)
    }
}

/// `MsgOpenSpellGuiS2C`: opens the grid with the server's patterns, stack
/// and ravenmind.
pub fn open(gui: OpenGui) {
    static SURFACE: Once = Once::new();
    SURFACE.call_once(|| yog_api::ui::surface::register(SURFACE_ID, Arc::new(GridSurface)));
    {
        let mut s = session();
        s.grid = Some(Grid::new(gui));
        s.closing = false;
    }
    client::open_ui_flags(ID, ui_flags::MODAL | ui_flags::NO_BACKGROUND);
}

/// `MsgNewSpellPatternS2C`: how the last pattern resolved, and the stack
/// after it. A cleared stack closes the grid.
pub fn on_result(res: PatternResult) {
    let keep = {
        let mut s = session();
        let Some(grid) = s.grid.as_mut() else { return };
        grid.recv_server_update(res)
    };
    if !keep {
        close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (427.0, 240.0);

    fn grid() -> Grid {
        let mut g = Grid::new(OpenGui {
            hand: Hand::Main,
            patterns: Vec::new(),
            stack: Vec::new(),
            ravenmind: None,
            paren_count: 0,
        });
        g.size = SIZE;
        g
    }

    /// Drags from dot to dot (a little past each, clear of the snap
    /// radius), then lets go.
    fn draw(g: &mut Grid, dots: &[HexCoord]) -> Vec<Effect> {
        let mut out = Vec::new();
        let px = g.coord_to_px(dots[0]);
        out.extend(g.draw_start(px.x, px.y));
        for w in dots.windows(2) {
            let (from, to) = (g.coord_to_px(w[0]), g.coord_to_px(w[1]));
            let px = from + (to - from) * 1.2;
            out.extend(g.draw_move(px.x, px.y));
        }
        out.extend(g.draw_end());
        out
    }

    fn sent(effects: &[Effect]) -> Option<&NewPattern> {
        effects.iter().find_map(|e| match e {
            Effect::Send(p) => Some(p),
            _ => None,
        })
    }

    #[test]
    fn hex_size_fills_512_hexes() {
        let g = grid();
        assert!((g.hex_size() - (427.0f32 * 240.0 / 512.0).sqrt()).abs() < 1e-3);
        assert_eq!(g.px_to_coord(Vec2::new(213.5, 120.0)), HexCoord(0, 0));
    }

    #[test]
    fn drawing_sends_the_pattern_with_the_grid() {
        let mut g = grid();
        let fx = draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(1, 1), HexCoord(0, 1)]);
        assert_eq!(fx[0], Effect::StartSound);
        assert_eq!(fx.iter().filter(|e| **e == Effect::SegmentSound).count(), 3);
        let msg = sent(&fx).unwrap();
        assert_eq!(msg.pattern.start, HexDir::East);
        assert_eq!(msg.pattern.steps, [HexDir::SouthEast, HexDir::West]);
        assert_eq!(msg.resolved.len(), 1);
        assert_eq!(msg.resolved[0].kind, ResolvedType::Unresolved);
        // Its dots are taken: can't start there again.
        assert!(g.used_spots.contains(&HexCoord(1, 1)));
        let px = g.coord_to_px(HexCoord(1, 0));
        assert_eq!(g.draw_start(px.x, px.y), None);
    }

    #[test]
    fn a_click_without_a_stroke_sends_nothing() {
        let mut g = grid();
        let fx = draw(&mut g, &[HexCoord(0, 0)]);
        assert_eq!(fx, [Effect::StartSound]);
        assert_eq!(g.draw, DrawState::BetweenPatterns);
    }

    #[test]
    fn going_straight_back_undoes_a_stroke() {
        let mut g = grid();
        let fx = draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(2, 0), HexCoord(1, 0)]);
        let msg = sent(&fx).unwrap();
        assert_eq!(msg.pattern.directions(), [HexDir::East]);
        // All the way back: nothing drawn.
        let mut g = grid();
        let fx = draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(0, 0)]);
        assert!(sent(&fx).is_none());
    }

    #[test]
    fn an_edge_is_drawn_once() {
        let mut g = grid();
        // Around a triangle and on along its first edge: refused.
        let fx = draw(
            &mut g,
            &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(0, 1), HexCoord(0, 0), HexCoord(1, 0)],
        );
        assert_eq!(sent(&fx).unwrap().pattern.directions().len(), 3);
    }

    #[test]
    fn small_moves_snap_to_the_anchor() {
        let mut g = grid();
        let px = g.coord_to_px(HexCoord(0, 0));
        g.draw_start(px.x, px.y);
        assert_eq!(g.draw_move(px.x + g.hex_size() * 0.9, px.y), None);
        assert_eq!(g.draw, DrawState::JustStarted(HexCoord(0, 0)));
    }

    #[test]
    fn esc_cancels_the_stroke_first() {
        let mut g = grid();
        assert!(!g.cancel_stroke());
        let px = g.coord_to_px(HexCoord(0, 0));
        g.draw_start(px.x, px.y);
        assert!(g.cancel_stroke());
        assert_eq!(g.draw, DrawState::BetweenPatterns);
    }

    fn result(kind: ResolvedType, index: i64) -> PatternResult {
        PatternResult { stack_clear: false, kind, stack: vec![Iota::Double(1.0)], ravenmind: None, index }
    }

    #[test]
    fn results_colour_their_patterns() {
        let mut g = grid();
        draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0)]);
        assert!(g.recv_server_update(result(ResolvedType::Errored, 0)));
        assert_eq!(g.patterns[0].kind, ResolvedType::Errored);
        assert_eq!(g.stack, [Iota::Double(1.0)]);
        assert!(!g.recv_server_update(PatternResult { stack_clear: true, ..result(ResolvedType::Evaluated, 0) }));
    }

    #[test]
    fn evanition_undoes_the_last_undoable() {
        let mut g = grid();
        let pat = |sig: &str| HexPattern::from_angles(sig, HexDir::East).unwrap();
        g.patterns = vec![
            ResolvedPattern { pattern: pat("qqq"), origin: HexCoord(0, 0), kind: ResolvedType::Evaluated },
            ResolvedPattern { pattern: pat("qaq"), origin: HexCoord(5, 0), kind: ResolvedType::Escaped },
            ResolvedPattern { pattern: pat("aaa"), origin: HexCoord(9, 0), kind: ResolvedType::Evaluated },
            ResolvedPattern { pattern: pat("eee"), origin: HexCoord(0, 5), kind: ResolvedType::Unresolved },
        ];
        g.recv_server_update(result(ResolvedType::Undone, 3));
        assert_eq!(g.patterns[1].kind, ResolvedType::Undone);
        assert_eq!(g.patterns[0].kind, ResolvedType::Evaluated);
        assert_eq!(g.patterns[3].kind, ResolvedType::Evaluated);
        g.patterns[3].kind = ResolvedType::Unresolved;
        g.recv_server_update(result(ResolvedType::Undone, 3));
        // Introspection can be undone too.
        assert_eq!(g.patterns[0].kind, ResolvedType::Undone);
    }

    #[test]
    fn scroll_needs_a_whole_notch() {
        let mut g = grid();
        assert!(!g.scroll(0.5));
        assert!(g.scroll(0.5));
        assert!(!g.scroll(-0.5));
        assert!(g.scroll(-1.0));
    }

    #[test]
    fn long_iotas_are_cut() {
        let width = |s: &str| s.chars().count() as f32 * 6.0;
        assert_eq!(fit("short", 100.0, &width), ("short".to_string(), false));
        let (t, cut) = fit("one two three four", 80.0, &width);
        assert!(cut);
        assert_eq!(t, "one two");
        assert!(width(&t) + width("...") <= 80.0);
    }

    #[test]
    fn the_grid_draws() {
        let mut g = grid();
        g.mouse = g.coord_to_px(HexCoord(0, 0));
        assert!(!g.mesh(false).is_empty());
        draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(2, 0)]);
        let px = g.coord_to_px(HexCoord(-3, 0));
        g.draw_start(px.x, px.y);
        assert!(g.mesh(true).vertex_count() > 100);
    }
}
