//! Spells on items: dye, variants, recharging, erasing, crafting.

use super::motion::{assert_battery_media, item_entity_stack, set_item_entity_stack};
use crate::casting::args::ItemEnt;
use crate::casting::env::{CastEnv, Spray, Stack};
use crate::casting::mishap::{MResult, Mishap, item_name};
use crate::casting::ops::rw::others_name;
use crate::casting::vm::Spell;
use crate::content::Items;
use crate::encoding::snbt::Nbt;
use crate::iota::Iota;
use crate::item::media::{CRYSTAL, DUST, SHARD};
use crate::item::pigment::{Pigment, is_pigment};
use crate::item::{holder, media, packaged};

// ── items ──────────────────────────────────────────────────────────────────

/// `OpColorize`.
pub(super) fn colorize(env: &mut CastEnv) -> MResult<Spell> {
    let (stack, _) = env
        .held_item_to_operate_on(|s| is_pigment(&s.id))
        .ok_or_else(|| Mishap::bad_offhand(None, "colorizer", vec![]))?;
    Ok(Spell::new(DUST, vec![], move |env, _| {
        let same = |s: &Stack| s.id == stack.id && s.nbt == stack.nbt;
        if env.withdraw_item(same, 1, true) {
            let owner = env
                .caster_uuid()
                .unwrap_or_else(|| Pigment::default().owner);
            // `env.setPigment`: the caster's, or a circle's impetus'.
            env.set_pigment(Pigment {
                item: stack.id.clone(),
                owner,
            });
        }
    }))
}

/// Items with variants (`ADVariantItem`), and how many.
pub(super) fn variants(id: &str) -> Option<i64> {
    matches!(
        id,
        holder::FOCUS
            | holder::SPELLBOOK
            | Items::CYPHER
            | Items::ANCIENT_CYPHER
            | Items::TRINKET
            | Items::ARTIFACT
    )
    .then_some(8)
}

/// `OpCycleVariant`.
pub(super) fn cycle_variant(env: &mut CastEnv) -> MResult<Spell> {
    let (stack, _) = env
        .held_item_to_operate_on(|s| variants(&s.id).is_some())
        .ok_or_else(|| Mishap::bad_offhand(None, "variant", vec![]))?;
    Ok(Spell::new(DUST / 10, vec![], move |env, _| {
        let mut s = env.stack_at(stack.slot);
        if s.id != stack.id {
            return;
        }
        let n = variants(&s.id).unwrap_or(1);
        let v = s.nbt.get_i64("variant").unwrap_or(0);
        s.nbt.set("variant", Nbt::int(((v + 1) % n) as i32));
        env.save(&s);
    }))
}

/// `OpRecharge`.
pub(super) fn recharge(env: &mut CastEnv, e: ItemEnt) -> MResult<Spell> {
    let (held, _) = env
        .held_item_to_operate_on(|s| {
            media::can_recharge(&s.id) && media::insert_media(&mut s.clone(), -1, true) != 0
        })
        .ok_or_else(|| Mishap::bad_offhand(None, "rechargable", vec![]))?;
    e.in_range(env)?;
    if !item_entity_stack(e.uuid()).is_some_and(|s| media::is_media_item(&s)) {
        return Err(Mishap::bad_entity(env, &e.r, "media"));
    }
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        SHARD,
        vec![Spray::burst(e.pos(), 0.5)],
        move |env, _| {
            let mut target = env.stack_at(held.slot);
            let Some(mut source) = item_entity_stack(&uuid) else {
                return;
            };
            if target.id != held.id || source.is_empty() {
                return;
            }
            let room = media::insert_media(&mut target, -1, true);
            let amount = media::withdraw(&mut source, room, false);
            media::insert_media(&mut target, amount, false);
            env.save(&target);
            set_item_entity_stack(&uuid, &source);
        },
    ))
}

/// `OpErase`.
pub(super) fn erase(env: &mut CastEnv) -> MResult<Spell> {
    let erasable = |s: &Stack| packaged::has_hex(s) || holder::write(&mut s.clone(), None, true);
    let (stack, _) = env
        .held_item_to_operate_on(erasable)
        .ok_or_else(|| Mishap::bad_offhand(None, "eraseable", vec![]))?;
    Ok(Spell::new(
        DUST * stack.count as i64,
        vec![],
        move |env, _| {
            let mut s = env.stack_at(stack.slot);
            if s.id != stack.id {
                return;
            }
            if packaged::has_hex(&s) {
                packaged::clear_hex(&mut s);
            }
            if holder::write(&mut s.clone(), None, true) {
                holder::write(&mut s, None, false);
            }
            env.save(&s);
        },
    ))
}

/// `OpMakePackagedSpell`.
pub(super) fn craft_packaged(
    env: &mut CastEnv,
    e: ItemEnt,
    hex: Vec<Iota>,
    item: &'static str,
    cost: i64,
) -> MResult<Spell> {
    let wanted = || Mishap::BadOffhandItem {
        item: None,
        wanted: item_name(item),
    };
    let (held, _) = env
        .held_item_to_operate_on(|s| s.is(item) && !packaged::has_hex(s))
        .ok_or_else(wanted)?;
    e.in_range(env)?;
    assert_battery_media(env, &e)?;
    others_name(env, &Iota::List(hex.clone()), env.caster_uuid().as_deref())?;
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        cost,
        vec![Spray::burst(e.pos(), 0.5)],
        move |env, _| {
            let mut target = env.stack_at(held.slot);
            let Some(mut source) = item_entity_stack(&uuid) else {
                return;
            };
            if !target.is(item) || packaged::has_hex(&target) {
                return;
            }
            let amount = media::extract_for_battery(&mut source, false);
            if amount > 0 {
                packaged::write_hex(&mut target, &hex, Some(&env.pigment()), amount);
                env.save(&target);
            }
            set_item_entity_stack(&uuid, &source);
        },
    ))
}

/// `OpMakeBattery`.
pub fn craft_battery(env: &mut CastEnv, e: ItemEnt) -> MResult<Spell> {
    let phial_base = |s: &Stack| {
        !s.is_empty() && yog_api::event::tag::has_item(&s.id, crate::ids::tags::PHIAL_BASE)
    };
    let (held, hand) = env
        .held_item_to_operate_on(|s| phial_base(s))
        .ok_or_else(|| Mishap::bad_offhand(None, "bottle", vec![]))?;
    if held.count != 1 {
        return Err(Mishap::bad_offhand(held.shown(), "only_one", vec![]));
    }
    e.in_range(env)?;
    assert_battery_media(env, &e)?;
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        CRYSTAL,
        vec![Spray::burst(e.pos(), 0.5)],
        move |env, _| {
            let Some(mut source) = item_entity_stack(&uuid) else {
                return;
            };
            let amount = media::extract_for_battery(&mut source, false);
            if amount > 0 {
                let mut nbt = Nbt::default();
                nbt.set(media::TAG_MEDIA, Nbt::long(amount));
                nbt.set(media::TAG_MAX_MEDIA, Nbt::long(amount));
                let phial = Stack {
                    slot: held.slot,
                    id: media::BATTERY.into(),
                    count: 1,
                    nbt,
                };
                if !env.replace_item(|s| *s == held, &phial, Some(hand)) {
                    return;
                }
            }
            set_item_entity_stack(&uuid, &source);
        },
    ))
}
