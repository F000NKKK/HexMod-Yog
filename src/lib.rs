//! Hex Casting, ported to the Yog loader: programmable magic through hex
//! patterns.

// A `Mishap` (168 bytes) carries the offending iotas, like the original's
// exceptions. Boxing it wouldn't shrink the hot `MResult<OpResult>`: the
// `OpResult` itself is bigger (184 bytes).
#![allow(clippy::result_large_err)]

use yog_api::{Mod, Registry, info};

pub mod akashic;
pub mod altiora;
pub mod casting;
pub mod circle;
pub mod conjured;
pub mod events;
pub mod ids;
pub mod iota;
pub mod item;
pub mod lens;
pub mod loot;
pub mod math;
pub mod protocol;
pub mod render;
pub mod snbt;
pub mod sounds;
pub mod text;
pub mod ui;
pub mod wall_scroll;
pub mod wire;

// `crate::res` — typed accessors for resources/*.yres.
yog_api::resources!();

pub struct HexMod;

impl Mod for HexMod {
    fn register(registry: &mut Registry) {
        info!("[hexcasting-yog] initializing the gate between worlds...");



        // The staff's casting grid; the book's pattern pages.
        ui::spellcasting::register(registry);
        ui::book_page::register(registry);
        // Scrolls and slates show their pattern under the tooltip.
        ui::pattern_render::register(registry);

        events::register(registry);
        circle::register(registry);
        lens::register(registry);
        render::register(registry);
        wall_scroll::register(registry);
        conjured::register(registry);
        loot::register(registry);

        info!("[hexcasting-yog] gate opened. The book is ready.");
    }
}

// Blocks and items of content/ (*.yblock, *.yitem): registered on load.
yog_api::content!();

yog_api::export_mod!(HexMod);
