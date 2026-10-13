//! Code-behind of `hex_grid.yui`: the [`GridSurface`] its `<Surface>`
//! shows — the hex lattice, drawn with anti-aliased SDF dots and lines
//! (`grid_gl`), and the pointer logic that turns a drag across the dots
//! into a [`HexPattern`].

yog_api::ui_include!("ui/hex_grid");

use std::sync::{Arc, Mutex, OnceLock};

use yog_api::ui::{Pointer, Rect, Surface};
use yog_api::GfxContext;

use super::grid_gl;
use crate::math::{HexCoord, HexDir, HexPattern};

impl HexGrid {}

/// Distance between neighbouring dots in GUI pixels.
const SPACING: f32 = 26.0;
/// sqrt(3)/2 — vertical spacing factor for the hex lattice.
const ROW_F: f32 = 0.866;
/// Snap radius for the cursor to latch onto a dot.
const SNAP: f32 = SPACING * 0.45;
/// Dots closer than this to the edge aren't drawn.
const MARGIN: f32 = 8.0;

const DOT_ACTIVE: (f32, u32) = (2.4, 0xFF_FFD75E);
const DOT_IN_STROKE: (f32, u32) = (2.0, 0xFF_C9A94B);
const DOT_IDLE: (f32, u32) = (1.4, 0x88_8888AA);
const LINE_PENDING: u32 = 0x99_9A7B2F;
const LINE_OK: u32 = 0xCC_4FD67A;
const LINE_MISHAP: u32 = 0xCC_E05A4F;
const LINE_CURRENT: u32 = 0xFF_E8C24A;
const LINE_RUBBER: u32 = 0x99_E8C24A;

/// A dot's position relative to the grid's centre.
fn coord_to_local(c: HexCoord) -> (f32, f32) {
    (
        SPACING * (c.0 as f32 + c.1 as f32 * 0.5),
        SPACING * ROW_F * c.1 as f32,
    )
}

/// The dot within snapping distance of `(x, y)` (relative to the centre).
fn local_to_coord(x: f32, y: f32) -> Option<HexCoord> {
    let r = (y / (SPACING * ROW_F)).round() as i32;
    let q = (x / SPACING - r as f32 * 0.5).round() as i32;
    // The candidate and its neighbours (rounding on a skewed lattice).
    let cand = HexCoord(q, r);
    std::iter::once(cand)
        .chain(cand.neighbors())
        .map(|c| {
            let (sx, sy) = coord_to_local(c);
            ((sx - x).powi(2) + (sy - y).powi(2), c)
        })
        .filter(|(d2, _)| *d2 <= SNAP * SNAP)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

fn dir_between(a: HexCoord, b: HexCoord) -> Option<HexDir> {
    let d = b - a;
    (0..6u8).map(HexDir::from_ordinal).find(|dir| dir.as_delta() == d)
}

/// How a drawn pattern's cast went (colours its lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeResult {
    Pending,
    Ok,
    Mishap,
}

#[derive(Default)]
struct GridState {
    /// Dots of the stroke being drawn.
    stroke: Vec<HexCoord>,
    /// Patterns drawn this session and how they went.
    drawn: Vec<(Vec<HexCoord>, StrokeResult)>,
    /// Cursor while dragging (local, relative to the centre).
    cursor: Option<(f32, f32)>,
}

type CastFn = Box<dyn Fn(&HexPattern) + Send + Sync>;

/// The casting grid — see the [module docs](self).
pub struct GridSurface {
    state: Mutex<GridState>,
    cast: CastFn,
}

impl GridSurface {
    /// What `<HexGrid>` draws by default.
    pub const ID: &'static str = "hexcasting:casting_grid";

    /// A grid that hands every finished pattern to `cast`.
    pub fn new(cast: impl Fn(&HexPattern) + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::new(GridState::default()),
            cast: Box::new(cast),
        }
    }

    /// The game's grid (sends patterns to the server), registered as
    /// [`Self::ID`] on first use.
    pub fn shared() -> Arc<GridSurface> {
        static GRID: OnceLock<Arc<GridSurface>> = OnceLock::new();
        GRID.get_or_init(|| {
            let grid = Arc::new(GridSurface::new(|p| {
                if let Some(srv) = yog_api::server() {
                    srv.send_to_server(crate::protocol::CH_CAST, p.serialized_form().as_bytes());
                }
                crate::ui::casting_screen::pattern_sent(p);
            }));
            yog_api::ui::surface::register(Self::ID, grid.clone());
            grid
        })
        .clone()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GridState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forgets this session's patterns (the screen reopened).
    pub fn clear(&self) {
        *self.state() = GridState::default();
    }

    /// Colours the oldest pattern still waiting for the server.
    pub fn resolve(&self, ok: bool) {
        let mut st = self.state();
        if let Some(entry) = st.drawn.iter_mut().find(|(_, r)| *r == StrokeResult::Pending) {
            entry.1 = if ok { StrokeResult::Ok } else { StrokeResult::Mishap };
        }
    }

    /// The patterns drawn this session and their results.
    pub fn drawn(&self) -> Vec<StrokeResult> {
        self.state().drawn.iter().map(|(_, r)| *r).collect()
    }

    fn down(&self, x: f32, y: f32) {
        let mut st = self.state();
        st.cursor = Some((x, y));
        st.stroke.clear();
        if let Some(c) = local_to_coord(x, y) {
            st.stroke.push(c);
        }
    }

    fn drag(&self, x: f32, y: f32) {
        let mut st = self.state();
        st.cursor = Some((x, y));
        let Some(c) = local_to_coord(x, y) else {
            return;
        };
        let Some(&last) = st.stroke.last() else {
            return;
        };
        if c == last {
            return;
        }
        // Dragging back onto the previous dot undoes the last segment.
        if st.stroke.len() >= 2 && st.stroke[st.stroke.len() - 2] == c {
            st.stroke.pop();
            return;
        }
        // Hex Casting's rule: an edge is drawn at most once; dots may repeat
        // (closed shapes pass through their start again).
        if dir_between(last, c).is_some() {
            let edge = |a: HexCoord, b: HexCoord| if (a.0, a.1) <= (b.0, b.1) { (a, b) } else { (b, a) };
            let new_edge = edge(last, c);
            if !st.stroke.windows(2).any(|p| edge(p[0], p[1]) == new_edge) {
                st.stroke.push(c);
            }
        }
    }

    fn up(&self) {
        let pattern = {
            let mut st = self.state();
            st.cursor = None;
            let stroke = std::mem::take(&mut st.stroke);
            if stroke.len() < 2 {
                return;
            }
            let dirs = stroke.windows(2).filter_map(|p| dir_between(p[0], p[1])).collect();
            st.drawn.push((stroke, StrokeResult::Pending));
            HexPattern::from_list(dirs)
        };
        (self.cast)(&pattern);
    }
}

impl Surface for GridSurface {
    fn paint(&self, gfx: &GfxContext, rect: Rect) {
        let (ox, oy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let at = |c: HexCoord| {
            let (x, y) = coord_to_local(c);
            (ox + x, oy + y)
        };
        let st = self.state();

        // The lattice, covering the box.
        let rows = (rect.h / (SPACING * ROW_F) / 2.0).ceil() as i32 + 1;
        let cols = (rect.w / SPACING / 2.0).ceil() as i32 + 1;
        for r in -rows..=rows {
            for q in (-cols - r / 2 - 1)..=(cols - r / 2 + 1) {
                let c = HexCoord(q, r);
                let (x, y) = at(c);
                if x < rect.x + MARGIN
                    || x > rect.x + rect.w - MARGIN
                    || y < rect.y + MARGIN
                    || y > rect.y + rect.h - MARGIN
                {
                    continue;
                }
                let (radius, color) = if st.stroke.last() == Some(&c) {
                    DOT_ACTIVE
                } else if st.stroke.contains(&c) {
                    DOT_IN_STROKE
                } else {
                    DOT_IDLE
                };
                grid_gl::dot(gfx, x, y, radius, color);
            }
        }

        // This session's patterns, coloured by their result.
        for (dots, result) in &st.drawn {
            let color = match result {
                StrokeResult::Pending => LINE_PENDING,
                StrokeResult::Ok => LINE_OK,
                StrokeResult::Mishap => LINE_MISHAP,
            };
            for pair in dots.windows(2) {
                let ((x0, y0), (x1, y1)) = (at(pair[0]), at(pair[1]));
                grid_gl::line(gfx, x0, y0, x1, y1, 2.0, color);
            }
        }

        // The stroke being drawn, and a rubber band to the cursor.
        for pair in st.stroke.windows(2) {
            let ((x0, y0), (x1, y1)) = (at(pair[0]), at(pair[1]));
            grid_gl::line(gfx, x0, y0, x1, y1, 2.5, LINE_CURRENT);
        }
        if let (Some(&last), Some((cx, cy))) = (st.stroke.last(), st.cursor) {
            let (x0, y0) = at(last);
            grid_gl::line(gfx, x0, y0, ox + cx, oy + cy, 2.0, LINE_RUBBER);
        }
    }

    fn pointer(&self, ev: Pointer, (w, h): (f32, f32)) -> bool {
        // Local → relative to the centre.
        let c = |x: f32, y: f32| (x - w / 2.0, y - h / 2.0);
        match ev {
            Pointer::Down { x, y } => {
                let (x, y) = c(x, y);
                self.down(x, y);
            }
            Pointer::Drag { x, y } => {
                let (x, y) = c(x, y);
                self.drag(x, y);
            }
            Pointer::Up { .. } => self.up(),
            Pointer::Move { .. } | Pointer::Leave => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (200.0, 200.0);

    /// The local (box) position of dot `c`.
    fn px(c: HexCoord) -> (f32, f32) {
        let (x, y) = coord_to_local(c);
        (x + SIZE.0 / 2.0, y + SIZE.1 / 2.0)
    }

    fn draw(grid: &GridSurface, dots: &[HexCoord]) {
        let (x, y) = px(dots[0]);
        grid.pointer(Pointer::Down { x, y }, SIZE);
        for &d in &dots[1..] {
            let (x, y) = px(d);
            grid.pointer(Pointer::Drag { x, y }, SIZE);
        }
        let (x, y) = px(*dots.last().unwrap());
        grid.pointer(Pointer::Up { x, y }, SIZE);
    }

    fn recording() -> (Arc<Mutex<Vec<String>>>, GridSurface) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let log = sent.clone();
        let grid = GridSurface::new(move |p| log.lock().unwrap().push(crate::casting::vm::angle_signature(p)));
        (sent, grid)
    }

    #[test]
    fn snapping_finds_the_nearest_dot() {
        for c in [HexCoord(0, 0), HexCoord(2, -1), HexCoord(-3, 2)] {
            let (x, y) = coord_to_local(c);
            assert_eq!(local_to_coord(x + 3.0, y - 2.0), Some(c));
        }
        let (x, y) = coord_to_local(HexCoord(0, 0));
        assert_eq!(local_to_coord(x + SPACING / 2.0, y), None);
    }

    #[test]
    fn a_drag_casts_its_pattern() {
        let (sent, grid) = recording();
        // Straight east, then a turn: "w" then whatever the angle is.
        draw(&grid, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(2, 0)]);
        assert_eq!(sent.lock().unwrap().as_slice(), ["w"]);
        assert_eq!(grid.drawn(), [StrokeResult::Pending]);
        grid.resolve(false);
        assert_eq!(grid.drawn(), [StrokeResult::Mishap]);
    }

    #[test]
    fn edges_once_and_backtracking_undoes() {
        let (sent, grid) = recording();
        // Out and back over the same edge: the return is an undo.
        draw(&grid, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(0, 0)]);
        assert!(sent.lock().unwrap().is_empty(), "one dot left: nothing cast");
        // A triangle closes on its start: three edges, two angles.
        draw(&grid, &[HexCoord(0, 0), HexCoord(1, 0), HexCoord(0, 1), HexCoord(0, 0)]);
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(sent.lock().unwrap()[0].len(), 2);
        grid.clear();
        assert!(grid.drawn().is_empty());
    }
}
