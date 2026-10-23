//! The grid's state machine: strokes drawn, sent, answered, undone.

use super::UNDOABLE_EVALUATED;
use super::draw::grid_zoom;
use crate::iota::Iota;
use crate::math::{HexAngle, HexCoord, HexDir, HexPattern};
use crate::protocol::{Hand, NewPattern, OpenGui, PatternResult, ResolvedPattern, ResolvedType};
use crate::ui::render_lib::McVec2;
use crate::ui::render_lib::{Vec2, coord_to_px, positions_from, px_to_coord};
use std::collections::HashSet;

// ── the grid's state machine ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(super) enum DrawState {
    /// Waiting for the button to go down.
    BetweenPatterns,
    /// The button is down on `start`; no stroke yet.
    JustStarted(HexCoord),
    /// Drawing `wip` from `start`; the stroke is at `current`.
    Drawing {
        start: HexCoord,
        current: HexCoord,
        wip: HexPattern,
    },
}

/// What the grid wants done outside it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Effect {
    StartSound,
    SegmentSound,
    Send(NewPattern),
}

/// An iota as shown in a box: its text cut to fit, and its color.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Desc {
    /// Its words and runes, cut to fit.
    pub(super) segs: Vec<crate::text::Segment>,
    pub(super) cut: bool,
    pub(super) color: u32,
}

/// One casting session, from the server's `OpenGui` to closing.
pub(super) struct Grid {
    pub(super) hand: Hand,
    pub(super) patterns: Vec<ResolvedPattern>,
    /// Bottom first.
    pub(super) stack: Vec<Iota>,
    pub(super) ravenmind: Option<Iota>,
    #[allow(dead_code)] // the original keeps it but no longer shows it
    pub(super) paren_count: i64,
    pub(super) draw: DrawState,
    pub(super) used_spots: HashSet<HexCoord>,
    /// The cursor, in the screen's GUI pixels.
    pub(super) mouse: Vec2,
    /// The screen's size (`this.width`, `this.height`).
    pub(super) size: (f32, f32),
    pub(super) scroll_acc: f64,
    /// Top first; with the width they were cut for.
    pub(super) stack_descs: Option<(f32, Vec<Desc>)>,
    pub(super) ravenmind_desc: Option<(f32, Option<Desc>)>,
}

impl Grid {
    pub(super) fn new(gui: OpenGui) -> Self {
        let used_spots = gui
            .patterns
            .iter()
            .flat_map(|p| positions_from(&p.pattern, p.origin))
            .collect();
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
    pub(super) fn hex_size(&self) -> f32 {
        let base = (self.size.0 as f64 * self.size.1 as f64 / 512.0).sqrt();
        (base / grid_zoom()) as f32
    }

    pub(super) fn coords_offset(&self) -> Vec2 {
        Vec2::new(self.size.0 * 0.5, self.size.1 * 0.5)
    }

    pub(super) fn coord_to_px(&self, c: HexCoord) -> Vec2 {
        coord_to_px(c, self.hex_size(), self.coords_offset())
    }

    pub(super) fn px_to_coord(&self, px: Vec2) -> HexCoord {
        px_to_coord(px, self.hex_size(), self.coords_offset())
    }

    pub(super) fn clamp(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x.clamp(0.0, self.size.0), y.clamp(0.0, self.size.1))
    }

    pub(super) fn draw_start(&mut self, x: f32, y: f32) -> Option<Effect> {
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

    pub(super) fn draw_move(&mut self, x: f32, y: f32) -> Option<Effect> {
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
        let new_dir = HexDir::from_ordinal(
            (crate::ui::render_lib::round_to_int(snapped * 6.0) + 1).rem_euclid(6) as u8,
        );
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

    pub(super) fn draw_end(&mut self) -> Option<Effect> {
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
    pub(super) fn cancel_stroke(&mut self) -> bool {
        if self.draw == DrawState::BetweenPatterns {
            return false;
        }
        self.draw = DrawState::BetweenPatterns;
        true
    }

    /// `recvServerUpdate`; `false` = the stack was cleared, close.
    pub(super) fn recv_server_update(&mut self, res: PatternResult) -> bool {
        if res.stack_clear {
            return false;
        }
        let index = usize::try_from(res.index).ok();
        if res.kind == ResolvedType::Undone {
            // The last pattern that can be undone (not the Evanition just
            // drawn) shows as undone; the Evanition itself as evaluated.
            let n = self.patterns.len();
            if let Some(p) = self
                .patterns
                .iter_mut()
                .take(n.saturating_sub(1))
                .rev()
                .find(|p| can_be_undone(p))
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
    pub(super) fn scroll(&mut self, delta: f64) -> bool {
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
}

pub(super) fn try_append_dir(p: &mut HexPattern, new_dir: HexDir) -> bool {
    let pts = p.positions();
    let end = *pts.last().unwrap_or(&HexCoord(0, 0));
    let next = end + new_dir.as_delta();
    if pts
        .windows(2)
        .any(|w| (w[0] == end && w[1] == next) || (w[0] == next && w[1] == end))
    {
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
pub(super) fn can_be_undone(p: &ResolvedPattern) -> bool {
    match p.kind {
        ResolvedType::Escaped => true,
        ResolvedType::Evaluated => UNDOABLE_EVALUATED.contains(&p.pattern.signature().as_str()),
        _ => false,
    }
}
