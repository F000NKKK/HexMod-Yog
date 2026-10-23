//! Hex Casting's staff screen (`GuiSpellcasting`), as in the original: the
//! whole screen is a transparent grid. Dots fade in around the cursor,
//! dragging between them draws a pattern, which goes to the server when the
//! button comes up; the patterns drawn so far stay on the grid in the color
//! of how they resolved. The stack is listed top-left, the ravenmind
//! top-right. Esc first drops the stroke being drawn, then closes; the
//! screen also closes when the staff leaves the hand it was opened with.
//!
//! The screen (`spellcasting.yui`) is one `<Surface>`; `screen::GridSurface` draws
//! it and takes the pointer.

mod draw;
mod grid;
mod screen;
#[cfg(test)]
mod tests;
mod view;

pub use screen::{SpellcastingVm, on_result, open};
use view::close;
pub use view::{ID, register};

pub(super) const SURFACE_ID: &str = "hexcasting:spellcasting_grid";

pub(super) const LHS_IOTAS_ALLOCATION: f32 = 0.7;
pub(super) const RHS_IOTAS_ALLOCATION: f32 = 0.15;
pub(super) const BOX_COLOR: u32 = 0x50_303030;
/// `ChatFormatting.GRAY`, the "..." after a cut-off iota.
pub(super) const ELLIPSIS_COLOR: u32 = 0xFF_AAAAAA;

pub(super) const SOUND_AMBIANCE: &str = crate::res::SoundEvents::CASTING_AMBIANCE;
pub(super) const SOUND_START: &str = crate::res::SoundEvents::CASTING_PATTERN_START;
pub(super) const SOUND_ADD_SEGMENT: &str = crate::res::SoundEvents::CASTING_PATTERN_ADD_SEGMENT;

/// Introspection, Consideration-of-N and Interjection: their EVALUATED
/// patterns can still be undone by Evanition.
pub(super) const UNDOABLE_EVALUATED: [&str; 3] = ["qqq", "qdaqadq", "aqqqqqwded"];
