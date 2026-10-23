//! Spells (`casting/actions/spells/*`, `spells/sentinel/*`).

use blocks::{break_block, conjure, edify, ignite, place_block};
use flight::flight;
use items::{colorize, craft_packaged, cycle_variant, erase, recharge};
use motion::{blink, explode};

use yog_api::vanilla;

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Block, Ent, ItemEnt, Living, Player, PosDouble};
use crate::casting::cont::Cont;
use crate::casting::env::CastEnv;
use crate::casting::image::Image;
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::{OpResult, SideEffect, Sound};
use crate::content::Items;
use crate::iota::Iota;
use crate::item::media::{CRYSTAL, DUST};
use crate::math::Vec3;

mod blocks;
mod entities;
mod flight;
mod items;
mod motion;
#[cfg(test)]
mod tests;

pub use blocks::create_fluid;
pub use entities::{Potion, create_sentinel, potion};
pub use flight::{FlightAbility, flight_of, tick_flight};
pub use items::craft_battery;
pub use motion::{assert_teleportable, teleport_by};

pub fn actions() -> Vec<Action> {
    actions! {
        "print" NE "de" => raw(print);
        "explode" E "aawaawaa" => spell [env, img; pos: Vec3, strength: Iota] { explode(env, pos, &strength, false) };
        "explode/fire" E "ddwddwdd" => spell [env, img; pos: Vec3, strength: Iota] { explode(env, pos, &strength, true) };
        "add_motion" SW "awqqqwaqw" => spell [env, img; e: Ent, motion: Vec3] { motion::add_motion(env, img, e, motion) };
        "blink" SW "awqqqwaq" => spell [env, img; e: Ent, delta: f64] { blink(env, e, delta) };
        "break_block" E "qaqqqqq" => spell [env, img; pos: Block] { break_block(env, pos.0) };
        "place_block" SW "eeeeede" => spell [env, img; pos: Block] { place_block(env, pos.0) };
        "colorize" E "awddwqawqwawq" => spell [env, img] { colorize(env) };
        "cycle_variant" W "dwaawedwewdwe" => spell [env, img] { cycle_variant(env) };
        "create_water" SE "aqawqadaq" => spell [env, img; pos: Block] { create_fluid(env, pos.0, false) };
        "destroy_water" SW "dedwedade" => spell [env, img; v: Vec3] { blocks::destroy_water(env, v.block()) };
        "ignite" SE "aaqawawa" => spell [env, img; target: Iota] { ignite(env, &target) };
        "extinguish" SW "ddedwdwd" => spell [env, img; v: Vec3] { blocks::put_out(env, v.block()) };
        "conjure_block" NE "qqa" => spell [env, img; v: Vec3] { conjure(env, v.block(), false) };
        "conjure_light" NE "qqd" => spell [env, img; v: Vec3] { conjure(env, v.block(), true) };
        "bonemeal" NE "wqaqwawqaqw" => spell [env, img; pos: Block] { blocks::bonemeal(env, pos.0) };
        "recharge" NW "qqqqqwaeaeaeaeaea" => spell [env, img; e: ItemEnt] { recharge(env, e) };
        "erase" E "qdqawwaww" => spell [env, img] { erase(env) };
        "edify" NE "wqaqwd" => spell [env, img; v: Vec3] { edify(env, v.block()) };
        "beep" W "adaa" => spell [env, img; target: Vec3, instrument: Iota, note: Iota] { blocks::beep_spell(env, target, &instrument, &note) };
        "craft/cypher" E "waqqqqq" => spell [env, img; e: ItemEnt, hex: Vec<Iota>] {
            craft_packaged(env, e, hex, Items::CYPHER, CRYSTAL)
        };
        "craft/trinket" E "wwaqqqqqeaqeaeqqqeaeq" => spell [env, img; e: ItemEnt, hex: Vec<Iota>] {
            craft_packaged(env, e, hex, Items::TRINKET, 5 * CRYSTAL)
        };
        "craft/artifact" E "wwaqqqqqeawqwqwqwqwqwwqqeadaeqqeqqeadaeqq" => spell [env, img; e: ItemEnt, hex: Vec<Iota>] {
            craft_packaged(env, e, hex, Items::ARTIFACT, 10 * CRYSTAL)
        };
        "potion/weakness" NW "qqqqqaqwawaw" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::WEAKNESS, base: DUST / 10, cubic: false })
        };
        "potion/levitation" W "qqqqqawwawawd" => spell [env, img; e: Living, dur: PosDouble] {
            potion(env, e, dur.0, None, Potion { effect: vanilla::effects::LEVITATION, base: DUST / 5, cubic: false })
        };
        "potion/wither" SW "qqqqqaewawawe" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::WITHER, base: DUST, cubic: false })
        };
        "potion/poison" SE "qqqqqadwawaww" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::POISON, base: DUST / 3, cubic: false })
        };
        "potion/slowness" SE "qqqqqadwawaw" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::SLOWNESS, base: DUST / 3, cubic: false })
        };
        "flight/range" SW "awawaawq" => spell [env, img; p: Player, arg: PosDouble] { flight(env, p, arg.0, true) };
        "flight/time" NE "dwdwdewq" => spell [env, img; p: Player, arg: PosDouble] { flight(env, p, arg.0, false) };
        "sentinel/create" E "waeawae" => spell [env, img; target: Vec3] { create_sentinel(env, target, false) };
        "sentinel/destroy" NE "qdwdqdw" => spell [env, img] { entities::destroy_sentinel(env) };
    }
}

/// `OpAddMotion.MAX_MOTION` (bug #387).
pub(super) const MAX_MOTION: f64 = 8192.0;

/// `toNonZeroLong`: rounded, but never free.
pub fn non_zero(x: f64) -> i64 {
    let r = (x + 0.5).floor() as i64;
    match r {
        0 if x >= 0.0 => 1,
        0 => -1,
        r => r,
    }
}

/// `OpPrint`: shows the top iota to the caster, leaving it there.
pub(super) fn print(_: &mut CastEnv, image: Image, cont: &Cont) -> MResult<OpResult> {
    let Some(datum) = image.stack.last().cloned() else {
        return Err(Mishap::NotEnoughArgs {
            expected: 1,
            got: 0,
        });
    };
    let effect =
        Box::new(move |env: &mut CastEnv, _: &mut Image| env.print_message(&datum.display()));
    Ok(OpResult {
        image: image.with_used_op(),
        effects: vec![SideEffect::AttemptSpell(effect)],
        cont: cont.clone(),
        sound: Sound::Spell,
    })
}
