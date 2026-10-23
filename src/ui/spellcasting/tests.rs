use super::grid::{DrawState, Effect, Grid};
use crate::iota::Iota;
use crate::math::{HexCoord, HexDir, HexPattern};
use crate::protocol::{Hand, NewPattern, OpenGui, PatternResult, ResolvedPattern, ResolvedType};
use crate::ui::render_lib::Vec2;

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
    let fx = draw(
        &mut g,
        &[
            HexCoord(0, 0),
            HexCoord(1, 0),
            HexCoord(1, 1),
            HexCoord(0, 1),
        ],
    );
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
    let fx = draw(
        &mut g,
        &[
            HexCoord(0, 0),
            HexCoord(1, 0),
            HexCoord(2, 0),
            HexCoord(1, 0),
        ],
    );
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
        &[
            HexCoord(0, 0),
            HexCoord(1, 0),
            HexCoord(0, 1),
            HexCoord(0, 0),
            HexCoord(1, 0),
        ],
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
    PatternResult {
        stack_clear: false,
        kind,
        stack: vec![Iota::Double(1.0)],
        ravenmind: None,
        index,
    }
}

#[test]
fn results_color_their_patterns() {
    let mut g = grid();
    draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0)]);
    assert!(g.recv_server_update(result(ResolvedType::Errored, 0)));
    assert_eq!(g.patterns[0].kind, ResolvedType::Errored);
    assert_eq!(g.stack, [Iota::Double(1.0)]);
    assert!(!g.recv_server_update(PatternResult {
        stack_clear: true,
        ..result(ResolvedType::Evaluated, 0)
    }));
}

#[test]
fn evanition_undoes_the_last_undoable() {
    let mut g = grid();
    let pat = |sig: &str| HexPattern::from_angles(sig, HexDir::East).unwrap();
    g.patterns = vec![
        ResolvedPattern {
            pattern: pat("qqq"),
            origin: HexCoord(0, 0),
            kind: ResolvedType::Evaluated,
        },
        ResolvedPattern {
            pattern: pat("qaq"),
            origin: HexCoord(5, 0),
            kind: ResolvedType::Escaped,
        },
        ResolvedPattern {
            pattern: pat("aaa"),
            origin: HexCoord(9, 0),
            kind: ResolvedType::Evaluated,
        },
        ResolvedPattern {
            pattern: pat("eee"),
            origin: HexCoord(0, 5),
            kind: ResolvedType::Unresolved,
        },
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
fn the_grid_draws() {
    let mut g = grid();
    g.mouse = g.coord_to_px(HexCoord(0, 0));
    assert!(g.mesh(false).vertex_count() > 0);
    draw(&mut g, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(2, 0)]);
    let px = g.coord_to_px(HexCoord(-3, 0));
    g.draw_start(px.x, px.y);
    assert!(g.mesh(true).vertex_count() > 100);
}
