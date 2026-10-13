//! Items: foci, armor, consumables.

pub mod equipment;

use yog_api::Registry;

use crate::content::Items;

pub fn register(registry: &mut Registry) {
    // The staff, the tome, amethyst dust (and the amethyst sconce block) are
    // content/ files, registered by `yog_api::content!()` in lib.rs.
    //
    // thehexbook says `"register": "manual"`: it's registered here, before
    // `registry.register_book()` in lib.rs makes it open the book UI on
    // right-click.
    registry.register_item(Items::thehexbook());
}
