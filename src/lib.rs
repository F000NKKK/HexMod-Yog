//! HexCasting ported to Yog loader.
//! Programmable magic through hex patterns.

use yog_api::{info, Mod, Registry};

pub mod casting;
pub mod iota;
pub mod math;
pub mod item;
pub mod block;
pub mod events;
pub mod rendering;
pub mod player;
pub mod book;
pub mod grid;
pub mod grid_gl;

pub struct HexMod;

impl Mod for HexMod {
    fn register(registry: &mut Registry) {
        info!("[hexcasting-yog] initializing the gate between worlds...");

        // Register items and blocks first.
        item::register(registry);
        block::register(registry);

        // Register the in-game book so the runtime knows to create a YogBookItem
        // for hexcasting:thehexbook and handle right-click → open book UI.
        registry.register_book(&book::register_book());

        // The casting grid UI (drawn patterns → server-side VM).
        registry.register_ui(grid::UI_ID, grid::handle_event);
        registry.on_ui_render(grid::UI_ID, grid::render);

        // Register event handlers and rendering hooks.
        events::register(registry);
        rendering::register(registry);

        info!("[hexcasting-yog] gate opened. The book is ready.");
    }
}

yog_api::export_mod!(HexMod);
