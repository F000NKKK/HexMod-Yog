//! HexCasting ported to Yog loader.
//! Programmable magic through hex patterns.

use yog_api::{info, Mod, Registry};

pub mod book;
pub mod casting;
pub mod events;
pub mod iota;
pub mod item;
pub mod math;
pub mod protocol;
pub mod ui;

// `.yui` components without a code-behind (iota_row, mishap_banner,
// pattern_entry).
yog_api::ui_components!();

// `crate::res` — typed accessors for resources/*.yres.
yog_api::resources!();

pub struct HexMod;

impl Mod for HexMod {
    fn register(registry: &mut Registry) {
        info!("[hexcasting-yog] initializing the gate between worlds...");

        item::register(registry);

        // thehexbook: right-click opens the book UI.
        registry.register_book(&book::register_book());

        // The casting screen and the grid it draws on.
        ui::hex_grid::GridSurface::shared();
        ui::casting_screen::register(registry);

        events::register(registry);

        info!("[hexcasting-yog] gate opened. The book is ready.");
    }
}

// Blocks and items of content/ (*.yblock, *.yitem): registered on load.
yog_api::content!();

yog_api::export_mod!(HexMod);
