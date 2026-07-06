//! Items: foci, armor, consumables.

pub mod equipment;

use yog_api::{ItemDef, Registry};

pub fn register(registry: &mut Registry) {
    // thehexbook is also registered via registry.register_book() in lib.rs,
    // which makes it open the book UI on right-click.
    registry.register_item(
        ItemDef::new("hexcasting:thehexbook")
            .name("The Hexbook")
            .tooltip("A shimmering tome of hex knowledge."),
    );

    // The staff — right-click opens the casting grid (see events::register).
    registry.register_item(
        ItemDef::new("hexcasting:staff")
            .name("Oaken Staff")
            .tooltip("Channels media into patterns."),
    );

    // Sealed Tome — stores and executes hex patterns (different from thehexbook).
    registry.register_item(
        ItemDef::new("hexcasting:tome")
            .name("Tome of the Glimmen")
            .tooltip("A book bound in shimmering fabric."),
    );
}
