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

    // Amethyst Dust — media currency, dropped by amethyst-related blocks.
    // Registered here (rather than just existing as a bundled texture) so it
    // renders as a real item — e.g. as the "Media" book entry's icon.
    registry.register_item(
        ItemDef::new("hexcasting:amethyst_dust")
            .name("Amethyst Dust")
            .tooltip("A pile of dust, containing a small amount of media."),
    );
}
