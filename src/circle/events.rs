//! The circles' blocks placed, used and broken.

use super::impetus::{
    IMPETUSES, KNOWN, Known, PLACING, TICKS, forget_impetus, impetus, is_impetus, load_known,
    save_impetus, save_known,
};
use super::run::{end, start};
use super::triggers::{set_slate_pattern, slate_item_nbt, slate_pattern, tick};
use super::{Dir, Kind, MAX_MEDIA, SOUND_REDSTONE_CLEAR, SOUND_REDSTONE_DING, kind};
use crate::casting::env::{Caster, from_ypos};
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::item::holder;
use crate::math::Vec3;
use yog_api::event::block::{Break, Insert, Place, Use};
use yog_api::yog_ids::gamemode;
use yog_api::{EventPhase, Registry};

/// The block state properties the circles set and read.
const FACING: &str = "facing";
const POWERED: &str = "powered";
const TRUE: &str = "true";

/// How fast a broken slate's item is tossed up.
const SLATE_TOSS: (f64, f64, f64) = (0.0, 0.1, 0.0);

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::life::Started, _>(|_, _| {
        load_known();
        *TICKS.lock() = 0;
    });
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());
    registry.on::<Place, _>(|ev, cx| {
        if let Some(k) = kind(&ev.block_id) {
            match cx.phase() {
                EventPhase::Pre => placing(ev, k),
                _ => placed(ev, k),
            }
        }
        true
    });
    // `BlockEntityAbstractImpetus` as a container: hoppers and droppers
    // feed it media (`insertMedia`); a creative unlocker makes it endless.
    for id in IMPETUSES {
        registry.provide_key::<Insert>(id, |ev, _| insert(ev));
    }
    registry.on::<Break, _>(|ev, cx| {
        if cx.phase() == EventPhase::Post {
            broken(ev);
        }
        true
    });
    registry.on::<Use, _>(|ev, cx| cx.phase() != EventPhase::Pre || used(ev));
}

/// A slate about to be placed: the pattern of the item placing it (it's in
/// a hand now; it may be used up after).
fn placing(ev: &Place, k: Kind) {
    if k != Kind::Slate {
        return;
    }
    let selected = yog_api::event::player::info(&ev.player_name).map_or(0, |p| p.selected_slot);
    let pattern = [selected, crate::casting::env::SLOT_OFFHAND]
        .into_iter()
        .map(|slot| crate::casting::env::Stack::read(&ev.player_name, slot))
        .find(|s| s.id == Blocks::SLATE)
        .and_then(|s| holder::written_pattern(&s.id, &s.nbt));
    PLACING.lock().insert(ev.player_name.clone(), pattern);
}

/// A circle's block placed: turned if sneaking, a slate's pattern kept, an
/// impetus known.
fn placed(ev: &Place, k: Kind) {
    let pos = from_ypos(ev.pos);
    // `placeStateDirAndSneak`: sneaking turns it the other way.
    let turns = matches!(
        k,
        Kind::Impetus | Kind::EmptyImpetus | Kind::DirEmpty | Kind::DirRedstone | Kind::DirBoolean
    );
    if turns
        && ev.sneaking
        && let Some(f) = yog_api::event::block::property(&ev.dimension, ev.pos, FACING)
            .as_deref()
            .and_then(Dir::parse)
    {
        let props = format!("{FACING}={}", f.opposite().name());
        yog_api::event::block::set_properties(&ev.dimension, ev.pos, &props);
    }
    if k == Kind::Slate {
        let pattern = PLACING.lock().remove(&ev.player_name).flatten();
        set_slate_pattern(&ev.dimension, pos, pattern.as_ref());
        crate::client::render::slate_changed(&ev.dimension, pos, pattern.as_ref());
    }
    if is_impetus(&ev.block_id) {
        forget_impetus(&ev.dimension, pos);
        let powered = yog_api::event::block::property(&ev.dimension, ev.pos, POWERED).as_deref()
            == Some(TRUE);
        let known = Known {
            id: ev.block_id.clone(),
            due: None,
            powered,
        };
        KNOWN.lock().insert((ev.dimension.clone(), pos), known);
        save_known();
    }
}

/// Media fed into an impetus; whether it took any.
fn insert(ev: &Insert) -> bool {
    let pos = from_ypos(ev.pos);
    let mut imp = impetus(&ev.dimension, pos);
    if imp.room() == 0 {
        return false;
    }
    if ev.item_id == crate::content::Items::CREATIVE_UNLOCKER {
        if !ev.simulate {
            imp.media = -1;
            save_impetus(&ev.dimension, pos, &imp);
        }
        return true;
    }
    let mut stack = crate::casting::env::Stack {
        slot: 0,
        id: ev.item_id.clone(),
        count: ev.count,
        nbt: crate::encoding::snbt::Nbt::parse(&ev.nbt),
    };
    let taken = crate::item::media::withdraw(&mut stack, imp.room(), ev.simulate);
    if taken <= 0 {
        return false;
    }
    if !ev.simulate {
        imp.media = (imp.media + taken).min(MAX_MEDIA);
        save_impetus(&ev.dimension, pos, &imp);
    }
    true
}

/// A circle's block broken: a slate drops with its pattern (the original's
/// loot table copies it), an impetus stops and is forgotten.
fn broken(ev: &Break) {
    let pos = from_ypos(ev.pos);
    if ev.block_id == Blocks::SLATE {
        let pattern = slate_pattern(&ev.dimension, pos);
        set_slate_pattern(&ev.dimension, pos, None);
        crate::client::render::slate_changed(&ev.dimension, pos, None);
        let creative = yog_api::event::player::info(&ev.player_name)
            .is_some_and(|p| p.gamemode == gamemode::CREATIVE);
        if !creative {
            let c = Vec3::center_of(pos);
            let nbt = slate_item_nbt(pattern.as_ref());
            yog_api::event::item::spawn_item(
                &ev.dimension,
                c.tuple(),
                SLATE_TOSS,
                Blocks::SLATE,
                1,
                nbt,
            );
        }
    }
    if is_impetus(&ev.block_id) {
        let mut imp = impetus(&ev.dimension, pos);
        end(&ev.dimension, &mut imp);
        forget_impetus(&ev.dimension, pos);
        KNOWN.lock().remove(&(ev.dimension.clone(), pos));
        save_known();
    }
}

/// A circle's block used; whether the use goes on (false: taken here).
fn used(ev: &Use) -> bool {
    let pos = from_ypos(ev.pos);
    match ev.block_id.as_str() {
        // `BlockRightClickImpetus.use`.
        Blocks::IMPETUS_RIGHTCLICK if !ev.sneaking => {
            start(&ev.dimension, pos, Caster::by_name(&ev.player_name));
            false
        }
        Blocks::IMPETUS_REDSTONE => !bind(ev, pos),
        _ => true,
    }
}

/// `BlockRedstoneImpetus.use`: bind the player a held iota names, or
/// (sneaking, empty-handed) unbind. Whether it did either.
fn bind(ev: &Use, pos: BlockPos) -> bool {
    let selected = yog_api::event::player::info(&ev.player_name).map_or(0, |p| p.selected_slot);
    let held = crate::casting::env::Stack::read(&ev.player_name, selected);
    let player = if held.is_empty() && ev.sneaking {
        None
    } else if let Some(Iota::Entity(e)) = holder::read(&held)
        && let Some(info) = yog_api::event::entity::info(&e.uuid)
        && info.has(yog_api::core::entity_flags::PLAYER)
    {
        Some((e.uuid.clone(), info.name))
    } else {
        return false;
    };
    let sound = if player.is_some() {
        SOUND_REDSTONE_DING
    } else {
        SOUND_REDSTONE_CLEAR
    };
    let mut imp = impetus(&ev.dimension, pos);
    imp.player = player;
    save_impetus(&ev.dimension, pos, &imp);
    let c = Vec3::center_of(pos);
    yog_api::event::sound::play(&ev.dimension, (c.x, c.y, c.z), sound, 1.0, 1.0);
    true
}
