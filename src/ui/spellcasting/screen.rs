//! The screen: the open session, the surface that draws the grid and
//! takes the pointer, and the view model.

use super::draw::{ALPHA_MASK, LINE, MARGIN, PAD};

use yog_api::ui::Surface;
use yog_kit::prelude::*;

use super::draw::{draw_desc, lerp, ravenmind_origin};
use super::grid::{Effect, Grid};
use super::view::ID;
use super::{SOUND_ADD_SEGMENT, SOUND_AMBIANCE, SOUND_START, SURFACE_ID, close};
use crate::encoding::protocol::{CH_NEW_PATTERN, Hand, OpenGui, PatternResult};
use crate::ui::render_lib::{Vec2, ticks};
use yog_api::client::{SoundHandle, ui_flags};
use yog_api::sync::{Arc, Mutex, MutexGuard, Once};
use yog_api::ui::{Pointer, Rect};
use yog_api::{GfxContext, client};

// ── the screen ───────────────────────────────────────────────────────────────

pub(super) struct Session {
    grid: Option<Grid>,
    ambiance: SoundHandle,
    /// A close was asked for (the staff left the hand); don't ask again.
    closing: bool,
}

pub(super) static SESSION: Mutex<Session> = Mutex::new(Session {
    grid: None,
    ambiance: SoundHandle(0),
    closing: false,
});

pub(super) fn session() -> MutexGuard<'static, Session> {
    SESSION.lock()
}

/// Draws the grid and takes the pointer — see the [module docs](self).
pub(super) struct GridSurface;

/// The stroke sounds' volume, and how far a segment's pitch strays.
const STROKE_VOLUME: f32 = 0.25;
const SEGMENT_PITCH_SPREAD: f32 = 0.1;
/// The stack's first line, below the box's top.
const STACK_TOP: f32 = 7.0;
/// The ravenmind's text pulses: how fast, between what opacities.
const PULSE_SPEED: f64 = 0.2;
const PULSE_OPACITY: (f32, f32) = (150.0, 255.0);

impl GridSurface {
    pub(super) fn apply(effect: Option<Effect>) {
        match effect {
            None => {}
            Some(Effect::StartSound) => {
                client::play_sound(SOUND_START, STROKE_VOLUME, 1.0, false);
            }
            Some(Effect::SegmentSound) => {
                let pitch = 1.0 + (random() - 0.5) * SEGMENT_PITCH_SPREAD;
                client::play_sound(SOUND_ADD_SEGMENT, STROKE_VOLUME, pitch, false);
            }
            Some(Effect::Send(msg)) => {
                yog_api::event::net::send_to_server(CH_NEW_PATTERN, msg.encode());
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
        let held = client::held_item(grid.hand == Hand::Off).map(|s| s.item.to_string());
        if !held.is_some_and(|id| crate::events::staff::is_staff(&id)) && !s.closing {
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
        let mut y = oy + MARGIN + STACK_TOP;
        for desc in grid.stack_descs(&width_of) {
            draw_desc(gfx, &width_of, desc, ox + MARGIN + PAD, y, desc.color);
            y += LINE;
        }
        if let Some(desc) = grid.ravenmind_desc(&width_of) {
            let (x, y) = ravenmind_origin(w);
            let pulse = ((ticks() * PULSE_SPEED).sin() as f32 + 1.0) / 2.0;
            let opacity = lerp(pulse, PULSE_OPACITY.0, PULSE_OPACITY.1) as u32;
            let color = (desc.color & !ALPHA_MASK) | opacity << 24;
            // The original draws this 1.5× larger; Draw2D has no text scale.
            draw_desc(gfx, &width_of, desc, ox + x + PAD, oy + y + PAD, color);
        }
    }

    fn pointer(&self, ev: Pointer, size: (f32, f32)) -> bool {
        let effect = {
            let mut s = session();
            let Some(grid) = s.grid.as_mut() else {
                return false;
            };
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
            crate::ui::shift_scroll::scroll(dy as f64, false);
        }
        true
    }
}

/// 0..1, for the stroke sounds' pitch.
pub(super) fn random() -> f32 {
    use yog_api::sync::atomic::{AtomicU64, Ordering};
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
