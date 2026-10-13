//! Hex Casting, ported to the Yog loader: programmable magic through hex
//! patterns.

// A `Mishap` carries the offending iotas, like the original's exceptions;
// boxing it on every `Err` path buys nothing at hex-evaluation rates.
#![allow(clippy::result_large_err)]

use yog_api::{info, Mod, Registry};

pub mod akashic;
pub mod book;
pub mod casting;
pub mod events;
pub mod iota;
pub mod item;
pub mod math;
pub mod protocol;
pub mod snbt;
pub mod text;
pub mod ui;
pub mod wire;

// `crate::res` — typed accessors for resources/*.yres.
yog_api::resources!();

pub struct HexMod;

impl Mod for HexMod {
    fn register(registry: &mut Registry) {
        info!("[hexcasting-yog] initializing the gate between worlds...");

        item::register(registry);

        // thehexbook: right-click opens the book UI.
        registry.register_book(&book::register_book());

        // The staff's casting grid.
        ui::spellcasting::register(registry);

        events::register(registry);

        info!("[hexcasting-yog] gate opened. The book is ready.");
    }
}

// Blocks and items of content/ (*.yblock, *.yitem): registered on load.
yog_api::content!();

yog_api::export_mod!(HexMod);
