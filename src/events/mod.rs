//! What the mod listens to: right-clicks on its items, the grid's packets,
//! and the per-tick bookkeeping.

mod items;
pub mod staff;

use yog_api::content::StartupGrant;
use yog_api::{EventPhase, Registry};

use crate::casting::env::from_ypos;
use crate::casting::ops::{query, spells};
use crate::content::Items;
use crate::encoding::protocol::{
    CH_NEW_PATTERN, CH_OPEN_GUI, CH_PATTERN_RESULT, OpenGui, PatternResult,
};
use crate::item::holder::ABACUS;
use crate::item::media;
use crate::things::akashic;
use crate::ui::shift_scroll::CH_SCROLL;
use crate::ui::spellcasting;

pub fn register(registry: &mut Registry) {
    // The Hexbook and a staff on first join.
    registry.register_startup_grant(
        StartupGrant::new(crate::ids::grants::HEXBOOK).item(Items::THEHEXBOOK),
    );
    registry.register_startup_grant(
        StartupGrant::new(crate::ids::grants::STAFF).item(Items::STAFF_OAK),
    );

    registry.on::<yog_api::event::player::action::UseItem, _>(|ev, cx| {
        let phase = cx.phase();
        if phase == EventPhase::Post {
            let id = ev.item_id.as_str();
            if staff::is_staff(id) {
                staff::use_staff(&ev.player_name, ev.sneaking);
            } else if media::is_packaged(id) {
                items::use_packaged(&ev.player_name, id);
            } else if id == ABACUS && ev.sneaking {
                items::reset_abacus(&ev.player_name);
            } else if id == Items::LORE_FRAGMENT {
                items::read_lore(&ev.player_name);
            }
        }
        true
    });

    registry.on_packet(CH_NEW_PATTERN, |ev| {
        staff::new_pattern(&ev.player, &ev.payload)
    });
    registry.on_packet(CH_SCROLL, |ev| items::shift_scroll(&ev.player, &ev.payload));

    registry.on_client_packet(CH_OPEN_GUI, |ev| {
        if let Some(gui) = OpenGui::decode(&ev.payload) {
            spellcasting::open(gui);
        }
    });
    registry.on_client_packet(CH_PATTERN_RESULT, |ev| {
        if let Some(res) = PatternResult::decode(&ev.payload) {
            spellcasting::on_result(res);
        }
    });

    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| query::record_players());
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| spells::tick_flight());
    crate::things::altiora::register(registry);

    // A bookshelf forgets what it held when it's broken or replaced.
    registry.on::<yog_api::event::block::Break, _>(|ev, cx| {
        let phase = cx.phase();
        if phase == EventPhase::Post && ev.block_id == akashic::BOOKSHELF {
            akashic::set_shelf(&ev.dimension, from_ypos(ev.pos), None);
        }
        true
    });
    registry.on::<yog_api::event::block::Place, _>(|ev, cx| {
        let phase = cx.phase();
        if phase == EventPhase::Post {
            akashic::set_shelf(&ev.dimension, from_ypos(ev.pos), None);
        }
        true
    });
}
