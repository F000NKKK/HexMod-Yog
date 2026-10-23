//! `ShiftScrollListener`: the wheel turns the spellbook's page or the
//! abacus' number in the hand — on the casting grid always, in the world
//! while sneaking (the hotbar then stays where it is).

use yog_api::Registry;
use yog_api::client::{self, local};
use yog_api::event::EventPhase;
use yog_api::event::client::input::Scroll;

use crate::content::Items;
use crate::encoding::wire::Writer;

/// `MsgShiftScrollC2S` (client → server): `num` main-hand delta, `num`
/// off-hand delta, `int` 1 when the sprint key is down. Scrolls the
/// spellbook page or the abacus in that hand.
pub const CH_SCROLL: &str = "hexcasting:scroll";

/// The wheel in the world: the held spellbook or abacus' while sneaking.
pub fn register(registry: &mut Registry) {
    registry.on::<Scroll, _>(|ev, cx| cx.phase() != EventPhase::Pre || !scroll(ev.delta, true));
}

/// `onScroll(delta, needsSneaking)`: scrolls the spellbook or abacus in the
/// main hand, else in the off hand; whether one was.
pub fn scroll(delta: f64, needs_sneaking: bool) -> bool {
    if needs_sneaking && !local::state().is_some_and(|s| s.sneaking) {
        return false;
    }
    let scrollable =
        |id: Option<String>| id.is_some_and(|id| id == Items::SPELLBOOK || id == Items::ABACUS);
    let (main, off) = if scrollable(client::held_item(false).map(|s| s.item.to_string())) {
        (delta, 0.0)
    } else if scrollable(client::held_item(true).map(|s| s.item.to_string())) {
        (0.0, delta)
    } else {
        return false;
    };
    let mut w = Writer::new();
    // The sprint key is Ctrl by default.
    w.num(main).num(off).int(client::control_down() as i64);
    yog_api::event::net::send_to_server(CH_SCROLL, w.finish().as_bytes());
    true
}
