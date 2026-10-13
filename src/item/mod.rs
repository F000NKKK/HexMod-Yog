//! Items: the media they hold, the iotas they store, the hexes they cast,
//! and the colours they give.

use yog_api::Registry;

use crate::content::Items;

pub mod holder;
pub mod media;
pub mod packaged;
pub mod pigment;

pub fn register(registry: &mut Registry) {
    // Everything else is a content/ file, registered by
    // `yog_api::content!()` in lib.rs.
    //
    // thehexbook says `"register": "manual"`: it's registered here, before
    // `registry.register_book()` in lib.rs makes it open the book UI on
    // right-click.
    registry.register_item(Items::thehexbook());
}
