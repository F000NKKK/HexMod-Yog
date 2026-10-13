//! What the mod listens to: right-clicks on its items, the grid's packets,
//! and the per-tick bookkeeping.

mod items;
pub mod staff;

use yog_api::content::StartupGrant;
use yog_api::{EventPhase, Registry};

use crate::akashic;
use crate::casting::env::from_ypos;
use crate::casting::ops::{great, query, spells};
use crate::item::holder::ABACUS;
use crate::item::media;
use crate::protocol::{OpenGui, PatternResult, CH_NEW_PATTERN, CH_OPEN_GUI, CH_PATTERN_RESULT};
use crate::ui::spellcasting::{self, CH_SCROLL};

pub fn register(registry: &mut Registry) {
    // The Hexbook and a staff on first join.
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_hexbook").item("hexcasting:thehexbook"),
    );
    registry.register_startup_grant(
        StartupGrant::new("hexcasting:give_staff").item("hexcasting:staff/oak"),
    );

    registry.on_use_item(|ev, phase, srv| {
        if phase == EventPhase::Post {
            let id = ev.item_id.as_str();
            if staff::is_staff(id) {
                staff::use_staff(srv, &ev.player_name, ev.sneaking);
            } else if media::is_packaged(id) {
                items::use_packaged(srv, &ev.player_name, id);
            } else if id == ABACUS && ev.sneaking {
                items::reset_abacus(srv, &ev.player_name);
            }
        }
        true
    });

    registry.on_packet(CH_NEW_PATTERN, |ev, srv| staff::new_pattern(srv, &ev.player, &ev.payload));
    registry.on_packet(CH_SCROLL, |ev, srv| items::shift_scroll(srv, &ev.player, &ev.payload));

    registry.on_client_packet(CH_OPEN_GUI, |ev, _| {
        if let Some(gui) = OpenGui::decode(&ev.payload) {
            spellcasting::open(gui);
        }
    });
    registry.on_client_packet(CH_PATTERN_RESULT, |ev, _| {
        if let Some(res) = PatternResult::decode(&ev.payload) {
            spellcasting::on_result(res);
        }
    });

    registry.on_tick(query::record_players);
    registry.on_tick(spells::tick_flight);
    registry.on_tick(great::tick_altiora);

    // A bookshelf forgets what it held when it's broken or replaced.
    registry.on_block_break(|ev, phase, srv| {
        if phase == EventPhase::Post && ev.block_id == akashic::BOOKSHELF {
            akashic::set_shelf(srv, &ev.dimension, from_ypos(ev.pos), None);
        }
        true
    });
    registry.on_player_place_block(|ev, phase, srv| {
        if phase == EventPhase::Post {
            akashic::set_shelf(srv, &ev.dimension, from_ypos(ev.pos), None);
        }
        true
    });
}
