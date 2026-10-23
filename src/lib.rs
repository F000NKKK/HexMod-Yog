//! Hex Casting, ported to the Yog loader: programmable magic through hex
//! patterns.

// A `Mishap` (168 bytes) carries the offending iotas, like the original's
// exceptions. Boxing it wouldn't shrink the hot `MResult<OpResult>`: the
// `OpResult` itself is bigger (184 bytes).
#![allow(clippy::result_large_err)]

mod client;
mod encoding;
mod things;

pub use client::{lens, render, text};
pub use encoding::{protocol, snbt, wire};
pub use things::{akashic, altiora, conjured, loot, wall_scroll};

use yog_api::{Mod, Registry, info};

pub mod casting;
pub mod circle;
pub mod events;
pub mod ids;
pub mod iota;
pub mod item;
pub mod math;
pub mod ui;

// `crate::res` — typed accessors for resources/*.yres.
yog_api::resources!();

pub struct HexMod;

impl Mod for HexMod {
    fn register(registry: &mut Registry) {
        info!("[hexcasting-yog] initializing the gate between worlds...");

        // The staff's casting grid; the book's pattern pages.
        ui::spellcasting::register(registry);
        ui::book_page::register(registry);
        // The wheel on a held spellbook or abacus.
        ui::shift_scroll::register(registry);
        // Scrolls and slates show their pattern under the tooltip.
        ui::pattern_render::register(registry);

        events::register(registry);
        circle::register(registry);
        client::lens::register(registry);
        client::render::register(registry);
        things::wall_scroll::register(registry);
        things::conjured::register(registry);
        things::loot::register(registry);

        info!("[hexcasting-yog] gate opened. The book is ready.");
    }
}

// Blocks and items of content/ (*.yblock, *.yitem): registered on load.
yog_api::content!();

yog_api::export_mod!(HexMod);
