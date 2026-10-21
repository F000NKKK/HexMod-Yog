//! Spells (`casting/actions/spells/*`, `spells/sentinel/*`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::f64::consts::PI;
use std::sync::Mutex;

use yog_api::core::Face;
use yog_api::vanilla;

use crate::akashic;
use crate::casting::actions::{Action, actions};
use crate::casting::args::{
    Arg, Block, Ent, ItemEnt, Living, Player, PosDouble, double_between, pos_int_under,
    pos_int_under_incl,
};
use crate::casting::cont::Cont;
use crate::casting::env::{
    CastEnv, DIM_TP_DENYLIST, Sentinel, Spray, Stack, get_pigment, get_sentinel, look_vec,
    player_storage, random, set_sentinel, to_ypos,
};
use crate::casting::image::Image;
use crate::casting::mishap::{BlockPos, MResult, Mishap, dye, item_name};
use crate::casting::ops::rw::others_name;
use crate::casting::vm::{OpResult, SideEffect, Sound, Spell};
use crate::content::{Blocks, Items};
use crate::iota::Iota;
use crate::item::media::{self, CRYSTAL, DUST, SHARD};
use crate::item::pigment::{Pigment, dye_pigment, is_pigment};
use crate::item::{holder, packaged};
use crate::math::Vec3;
use crate::snbt::Nbt;
use crate::text::Text;

pub fn actions() -> Vec<Action> {
    actions! {
        "print" NE "de" => raw(print);
        "explode" E "aawaawaa" => spell [env, img; pos: Vec3, strength: Iota] { explode(env, pos, &strength, false) };
        "explode/fire" E "ddwddwdd" => spell [env, img; pos: Vec3, strength: Iota] { explode(env, pos, &strength, true) };
        "add_motion" SW "awqqqwaqw" => spell [env, img; e: Ent, motion: Vec3] {
            e.in_range(env)?;
            let motion = if motion.length_sqr() > MAX_MOTION * MAX_MOTION { motion.normalize() * MAX_MOTION } else { motion };
            let mut for_cost = motion.length_sqr();
            if img.check_and_mark_moved(e.uuid()) {
                for_cost += 1.0;
            }
            let spray = Spray {
                pos: e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0),
                vel: motion.normalize(),
                fuzziness: 0.0,
                spread: 0.1,
                count: 20,
            };
            let uuid = e.uuid().to_owned();
            Ok(Spell::new(non_zero(for_cost * DUST as f64), vec![spray], move |env, _| {
                yog_api::event::entity::add_velocity(&uuid, (motion.x, motion.y, motion.z));
            }))
        };
        "blink" SW "awqqqwaq" => spell [env, img; e: Ent, delta: f64] { blink(env, e, delta) };
        "break_block" E "qaqqqqq" => spell [env, img; pos: Block] { break_block(env, pos.0) };
        "place_block" SW "eeeeede" => spell [env, img; pos: Block] { place_block(env, pos.0) };
        "colorize" E "awddwqawqwawq" => spell [env, img] { colorize(env) };
        "cycle_variant" W "dwaawedwewdwe" => spell [env, img] { cycle_variant(env) };
        "create_water" SE "aqawqadaq" => spell [env, img; pos: Block] { create_fluid(env, pos.0, false) };
        "destroy_water" SW "dedwedade" => spell [env, img; v: Vec3] {
            let pos = v.block();
            env.assert_pos_in_range_for_editing(pos)?;
            Ok(Spell::new(2 * CRYSTAL, vec![Spray::burst(Vec3::center_of(pos), 3.0)], move |env, _| destroy_fluid(env, pos)))
        };
        "ignite" SE "aaqawawa" => spell [env, img; target: Iota] { ignite(env, &target) };
        "extinguish" SW "ddedwdwd" => spell [env, img; v: Vec3] {
            let pos = v.block();
            env.assert_pos_in_range_for_editing(pos)?;
            Ok(Spell::new(6 * DUST, vec![Spray::burst(Vec3::center_of(pos), 1.0)], move |env, _| extinguish(env, pos)))
        };
        "conjure_block" NE "qqa" => spell [env, img; v: Vec3] { conjure(env, v.block(), false) };
        "conjure_light" NE "qqd" => spell [env, img; v: Vec3] { conjure(env, v.block(), true) };
        "bonemeal" NE "wqaqwawqaqw" => spell [env, img; pos: Block] {
            let pos = pos.0;
            env.assert_pos_in_range_for_editing(pos)?;
            Ok(Spell::new((DUST as f64 * 1.125) as i64, vec![Spray::burst(Vec3::center_of(pos), 1.0)], move |env, _| {
                let player = env.caster_name().unwrap_or_default().to_owned();
                yog_api::event::block::use_item(&env.dim, to_ypos(pos), Face::Up as u8, vanilla::items::BONE_MEAL, &player);
            }))
        };
        "recharge" NW "qqqqqwaeaeaeaeaea" => spell [env, img; e: ItemEnt] { recharge(env, e) };
        "erase" E "qdqawwaww" => spell [env, img] { erase(env) };
        "edify" NE "wqaqwd" => spell [env, img; v: Vec3] { edify(env, v.block()) };
        "beep" W "adaa" => spell [env, img; target: Vec3, instrument: Iota, note: Iota] {
            let instrument = pos_int_under(&instrument, 1, INSTRUMENTS.len() as i64)? as usize;
            let note = pos_int_under_incl(&note, 0, 24)?;
            env.assert_vec_in_range(target)?;
            Ok(Spell::new(DUST / 10, vec![Spray::cloud(target, 1.0)], move |env, _| beep(env, target, instrument, note))
                .mute())
        };
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
            potion(env, e, dur.0, Some(&pot), vanilla::effects::WEAKNESS, DUST / 10, false)
        };
        "potion/levitation" W "qqqqqawwawawd" => spell [env, img; e: Living, dur: PosDouble] {
            potion(env, e, dur.0, None, vanilla::effects::LEVITATION, DUST / 5, false)
        };
        "potion/wither" SW "qqqqqaewawawe" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), vanilla::effects::WITHER, DUST, false)
        };
        "potion/poison" SE "qqqqqadwawaww" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), vanilla::effects::POISON, DUST / 3, false)
        };
        "potion/slowness" SE "qqqqqadwawaw" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), vanilla::effects::SLOWNESS, DUST / 3, false)
        };
        "flight/range" SW "awawaawq" => spell [env, img; p: Player, arg: PosDouble] { flight(env, p, arg.0, true) };
        "flight/time" NE "dwdwdewq" => spell [env, img; p: Player, arg: PosDouble] { flight(env, p, arg.0, false) };
        "sentinel/create" E "waeawae" => spell [env, img; target: Vec3] { create_sentinel(env, target, false) };
        "sentinel/destroy" NE "qdwdqdw" => spell [env, img] {
            let uuid = env.caster_uuid().ok_or(Mishap::BadCaster)?;
            let current = get_sentinel(&uuid);
            let mut particles = Vec::new();
            if let Some(s) = &current {
                if s.dim != env.dim {
                    return Err(Mishap::WrongDimension(s.dim.clone()));
                }
                particles.push(Spray::cloud(s.pos, 2.0));
            }
            Ok(Spell::new(DUST / 10, particles, move |env, _| set_sentinel(&uuid, None)))
        };
    }
}

/// `OpAddMotion.MAX_MOTION` (bug #387).
const MAX_MOTION: f64 = 8192.0;

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
fn print(_: &mut CastEnv, image: Image, cont: &Cont) -> MResult<OpResult> {
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

// ── item entities ──────────────────────────────────────────────────────────

/// The stack in a dropped item entity.
fn item_entity_stack(uuid: &str) -> Option<Stack> {
    let (id, count, snbt) =
        yog_api::event::item::entity_stack(uuid).map(|s| (s.item.to_string(), s.count, s.snbt))?;
    Some(Stack {
        slot: 0,
        id,
        count,
        nbt: Nbt::parse(&snbt),
    })
}

/// Puts the stack back (an empty stack removes the entity).
fn set_item_entity_stack(uuid: &str, stack: &Stack) {
    let count = if stack.is_empty() { 0 } else { stack.count };
    yog_api::event::item::set_entity_stack(uuid, &stack.id, count, stack.nbt.to_snbt());
}

/// `isMediaItem(e) && extractMedia(e, drainForBatteries = true) > 0`, or
/// `MishapBadItem("media_for_battery")`.
fn assert_battery_media(env: &CastEnv, e: &Ent) -> MResult<()> {
    let ok = item_entity_stack(e.uuid()).is_some_and(|mut s| {
        media::is_media_item(&s) && media::extract_for_battery(&mut s, true) > 0
    });
    if ok {
        Ok(())
    } else {
        Err(Mishap::bad_entity(env, &e.r, "media_for_battery"))
    }
}

// ── teleports ──────────────────────────────────────────────────────────────

/// `HexTags.Entities.CANNOT_TELEPORT`.
pub fn assert_teleportable(e: &Ent) -> MResult<()> {
    if yog_api::event::tag::has_entity(e.info.type_id, crate::ids::tags::CANNOT_TELEPORT) {
        Err(Mishap::ImmuneEntity {
            entity: e.r.clone(),
            pos: e.pos(),
        })
    } else {
        Ok(())
    }
}

/// `OpBlink`.
fn blink(env: &mut CastEnv, e: Ent, delta: f64) -> MResult<Spell> {
    e.in_range(env)?;
    assert_teleportable(&e)?;
    let dvec = look_vec(e.uuid()) * delta;
    let end = e.pos() + dvec;
    env.assert_vec_in_range(e.pos())?;
    env.assert_vec_in_range(end)?;
    if !env.is_vec_in_world(end - Vec3::new(0.0, 1.0, 0.0)) {
        return Err(Mishap::BadLocation {
            pos: end,
            kind: "too_close_to_out",
        });
    }
    let mid = e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0);
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        non_zero(SHARD as f64 * delta.abs() * 0.5),
        vec![
            Spray::cloud_n(mid, 2.0, 50),
            Spray::burst_n(mid + dvec, 2.0, 100),
        ],
        move |env, _| teleport_by(&env.dim, &uuid, dvec),
    ))
}

/// `teleportRespectSticky`.
pub fn teleport_by(dim: &str, uuid: &str, delta: Vec3) {
    if DIM_TP_DENYLIST.contains(&dim) {
        return;
    }
    let Some(p) = yog_api::event::entity::position(uuid).map(|p| (p.x, p.y, p.z)) else {
        return;
    };
    let to = Vec3::from_tuple(p) + delta;
    // A "sticky" entity teleports itself and its riders. A teleport takes
    // the riders along, so they get off one that isn't.
    yog_api::event::entity::dismount(uuid);
    let sticky = yog_api::event::entity::info(uuid).is_some_and(|i| {
        yog_api::event::tag::has_entity(i.type_id, crate::ids::tags::STICKY_TELEPORTERS)
    });
    if !sticky {
        for rider in yog_api::event::entity::passengers(uuid) {
            yog_api::event::entity::dismount(&rider);
        }
    }
    yog_api::event::entity::teleport(uuid, (to.x, to.y, to.z));
}

// ── explosions ─────────────────────────────────────────────────────────────

/// `OpExplode`.
fn explode(env: &mut CastEnv, mut pos: Vec3, strength: &Iota, fire: bool) -> MResult<Spell> {
    let strength = match strength {
        Iota::Double(d) if (0.0..=10.0).contains(d) => *d,
        other => {
            return Err(Mishap::invalid(
                other,
                0,
                "double.positive.less.equal",
                vec![Text::num(10.0)],
            ));
        }
    };
    env.assert_vec_in_range(pos)?;
    // Explosions exactly at an entity's eyes don't hurt it.
    let eps = Vec3::splat(0.01);
    let too_close =
        yog_api::event::world::entities_in_box(&env.dim, (pos - eps).tuple(), (pos + eps).tuple())
            .iter()
            .filter_map(yog_api::event::entity::info)
            .any(|i| Vec3::from_tuple(i.eye_pos()).distance_sqr(pos) == 0.0);
    if too_close {
        pos = pos + Vec3::new(0.0, 0.000001, 0.0);
    }
    let cost = DUST as f64 * (3.0 * strength + if fire { 1.0 } else { 0.125 });
    Ok(Spell::new(
        non_zero(cost),
        vec![Spray::burst_n(pos, strength, 50)],
        move |env, _| {
            if env.can_edit_block_at(pos.block()) {
                let source = env.caster_uuid().unwrap_or_default();
                yog_api::event::world::explode(
                    &env.dim,
                    (pos.x, pos.y, pos.z),
                    strength as f32,
                    fire,
                    &source,
                );
            }
        },
    ))
}

// ── blocks ─────────────────────────────────────────────────────────────────

/// `OpBreakBlock`.
fn break_block(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let id = env.block_id(pos);
    let cost = if yog_api::event::tag::has_block(&id, crate::ids::tags::CHEAP_TO_BREAK_BLOCK) {
        DUST / 100
    } else {
        DUST / 8
    };
    Ok(Spell::new(
        cost,
        vec![Spray::burst(Vec3::center_of(pos), 1.0)],
        move |env, _| {
            let Some(info) = yog_api::event::block::info(&env.dim, to_ypos(pos)) else {
                return;
            };
            if info.air || info.hardness < 0.0 {
                return;
            }
            let breaker = env.caster_name().unwrap_or_default().to_owned();
            if yog_api::event::block::destroy(&env.dim, to_ypos(pos), true, &breaker)
                && info.block_id() == akashic::BOOKSHELF
            {
                akashic::set_shelf(&env.dim, pos, None);
            }
        },
    ))
}

fn is_block_item(s: &Stack) -> bool {
    yog_api::event::item::block(&s.id).is_some()
}

fn replaceable(env: &CastEnv, pos: BlockPos) -> bool {
    yog_api::event::block::info(&env.dim, to_ypos(pos)).is_some_and(|b| b.replaceable)
}

/// `OpPlaceBlock`.
fn place_block(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    if env.query_for_matching_stack(is_block_item).is_none() {
        return Err(Mishap::lacking_hotbar("placeable"));
    }
    if !replaceable(env, pos) {
        return Err(Mishap::bad_block(pos, "replaceable"));
    }
    Ok(Spell::new(
        DUST / 8,
        vec![Spray::cloud(Vec3::center_of(pos), 1.0)],
        move |env, _| {
            let Some(name) = env.caster_name().map(str::to_owned) else {
                return;
            };
            let Some(stack) = env.query_for_matching_stack(is_block_item) else {
                return;
            };
            if replaceable(env, pos) {
                yog_api::event::player::place_block_from_slot(
                    &name,
                    stack.slot,
                    &env.dim,
                    to_ypos(pos),
                );
            }
        },
    ))
}

/// `OpCreateFluid` (water, or lava for the great spell).
pub fn create_fluid(env: &mut CastEnv, pos: BlockPos, lava: bool) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let cost = if lava { CRYSTAL } else { DUST };
    Ok(Spell::new(
        cost,
        vec![Spray::burst(Vec3::center_of(pos), 1.0)],
        move |env, _| {
            if env.block_id(pos) == vanilla::blocks::CAULDRON {
                let full = if lava {
                    vanilla::blocks::LAVA_CAULDRON
                } else {
                    "minecraft:water_cauldron[level=3]"
                };
                yog_api::event::world::set_block(&env.dim, to_ypos(pos), full);
            } else {
                let bucket = if lava {
                    vanilla::items::LAVA_BUCKET
                } else {
                    vanilla::items::WATER_BUCKET
                };
                yog_api::event::block::pour_fluid(&env.dim, to_ypos(pos), bucket);
            }
        },
    ))
}

const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];
const MAX_DESTROY_COUNT: u32 = 1024;

fn offset(p: BlockPos, d: (i32, i32, i32)) -> BlockPos {
    (p.0 + d.0, p.1 + d.1, p.2 + d.2)
}

fn smoke(env: &CastEnv, here: BlockPos) {
    let j = || 0.5 + random() * 0.4 - 0.2;
    yog_api::event::particle::spawn_particles(
        &env.dim,
        (
            here.0 as f64 + j(),
            here.1 as f64 + j(),
            here.2 as f64 + j(),
        ),
        vanilla::particles::SMOKE,
        2,
        (0.0, 0.05, 0.0),
        0.0,
    );
}

/// `OpDestroyFluid`: drains a cauldron, or soaks up fluid like a sponge.
fn destroy_fluid(env: &mut CastEnv, base: BlockPos) {
    let id = env.block_id(base);
    if id != vanilla::blocks::CAULDRON && id.starts_with("minecraft:") && id.ends_with("cauldron") {
        yog_api::event::world::set_block(&env.dim, to_ypos(base), vanilla::blocks::CAULDRON);
        return;
    }
    let mut todo = VecDeque::new();
    let mut seen = HashSet::new();
    for x in -2..=2 {
        for y in -2..=2 {
            for z in -2..=2 {
                todo.push_back(offset(base, (x, y, z)));
            }
        }
    }
    let mut successes = 0;
    while successes <= MAX_DESTROY_COUNT {
        let Some(here) = todo.pop_front() else { break };
        if !env.can_edit_block_at(here) || !seen.insert(here) {
            continue;
        }
        let Some(info) = yog_api::event::block::info(&env.dim, to_ypos(here)) else {
            continue;
        };
        if info.fluid.is_none() {
            continue;
        }
        let at = to_ypos(here);
        let success = if info.state.contains("waterlogged=true") {
            // BucketPickup on a waterlogged block.
            yog_api::event::world::set_block(
                &env.dim,
                at,
                info.state.replace("waterlogged=true", "waterlogged=false"),
            )
        } else if matches!(
            info.block_id(),
            vanilla::blocks::WATER | vanilla::blocks::LAVA
        ) || info.block_id() == info.fluid.as_deref().unwrap_or_default()
        {
            yog_api::event::world::set_block(&env.dim, at, vanilla::blocks::AIR)
        } else if yog_api::event::tag::has_block(info.block_id(), crate::ids::tags::WATER_PLANTS) {
            yog_api::event::block::destroy(&env.dim, at, true, "");
            yog_api::event::world::set_block(&env.dim, at, vanilla::blocks::AIR);
            true
        } else {
            false
        };
        if success {
            smoke(env, here);
            successes += 1;
            todo.extend(NEIGHBOURS.iter().map(|d| offset(here, *d)));
        }
    }
    if successes > 0 {
        let c = Vec3::center_of(base);
        yog_api::event::sound::play(
            &env.dim,
            (c.x, c.y, c.z),
            yog_api::vanilla::sounds::BLOCK_FIRE_EXTINGUISH,
            1.0,
            0.95,
        );
    }
}

/// `OpExtinguish`.
fn extinguish(env: &mut CastEnv, target: BlockPos) {
    let mut todo = VecDeque::from([target]);
    let mut seen = HashSet::new();
    let mut successes = 0;
    while successes <= MAX_DESTROY_COUNT {
        let Some(here) = todo.pop_front() else { break };
        let d = (here.0 - target.0, here.1 - target.1, here.2 - target.2);
        let dist = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
        if !env.can_edit_block_at(here) || dist >= 100 || !seen.insert(here) {
            continue;
        }
        let at = to_ypos(here);
        let state = yog_api::event::block::info(&env.dim, at)
            .map(|b| b.state)
            .unwrap_or_default();
        let id = state.split('[').next().unwrap_or_default();
        let lit = state.contains("lit=true");
        let success = match id {
            vanilla::blocks::FIRE | vanilla::blocks::SOUL_FIRE | vanilla::blocks::NETHER_PORTAL => {
                yog_api::event::world::set_block(&env.dim, at, vanilla::blocks::AIR)
            }
            vanilla::blocks::CAMPFIRE | vanilla::blocks::SOUL_CAMPFIRE if lit => {
                yog_api::event::block::use_item(
                    &env.dim,
                    at,
                    Face::Up as u8,
                    vanilla::items::WOODEN_SHOVEL,
                    "",
                )
            }
            _ if lit && (id.ends_with("candle") || id.ends_with("candle_cake")) => {
                let ok = yog_api::event::world::set_block(
                    &env.dim,
                    at,
                    state.replace("lit=true", "lit=false"),
                );
                if ok {
                    let c = Vec3::center_of(here);
                    yog_api::event::sound::play(
                        &env.dim,
                        (c.x, c.y, c.z),
                        yog_api::vanilla::sounds::BLOCK_CANDLE_EXTINGUISH,
                        1.0,
                        1.0,
                    );
                }
                ok
            }
            _ => false,
        };
        if success {
            smoke(env, here);
            successes += 1;
        }
        todo.extend(NEIGHBOURS.iter().map(|d| offset(here, *d)));
    }
    if successes > 0 {
        yog_api::event::sound::play(
            &env.dim,
            (target.0 as f64, target.1 as f64, target.2 as f64),
            yog_api::vanilla::sounds::BLOCK_FIRE_EXTINGUISH,
            1.0,
            0.95,
        );
    }
}

/// `OpIgnite`.
fn ignite(env: &mut CastEnv, target: &Iota) -> MResult<Spell> {
    match target {
        Iota::Entity(_) => {
            let e = Ent::get(target, 0, env)?;
            e.in_range(env)?;
            let uuid = e.uuid().to_owned();
            Ok(Spell::new(
                DUST,
                vec![Spray::burst(e.pos(), 1.0)],
                move |_env, _| {
                    yog_api::event::entity::set_fire_ticks(&uuid, 8 * 20);
                },
            ))
        }
        Iota::Vec3(v) => {
            let pos = v.block();
            env.assert_pos_in_range_for_editing(pos)?;
            Ok(Spell::new(
                DUST,
                vec![Spray::burst(Vec3::center_of(pos), 1.0)],
                move |env, _| {
                    let at = to_ypos(pos);
                    if !yog_api::event::block::use_item(
                        &env.dim,
                        at,
                        Face::Up as u8,
                        vanilla::items::FIRE_CHARGE,
                        "",
                    ) {
                        yog_api::event::block::use_item(
                            &env.dim,
                            at,
                            Face::Up as u8,
                            vanilla::items::FLINT_AND_STEEL,
                            "",
                        );
                    }
                },
            ))
        }
        other => Err(Mishap::of_type(other, 0, "entity_or_vector")),
    }
}

/// `OpConjureBlock`: the block sparkles in the caster's pigment
/// (`BlockConjured.setColor`, [`crate::conjured`]).
fn conjure(env: &mut CastEnv, pos: BlockPos, light: bool) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    if !replaceable(env, pos) {
        return Err(Mishap::bad_block(pos, "replaceable"));
    }
    Ok(Spell::new(
        DUST,
        vec![Spray::cloud(Vec3::center_of(pos), 1.0)],
        move |env, _| {
            if env.can_edit_block_at(pos) && replaceable(env, pos) {
                let block = if light {
                    Blocks::CONJURED_LIGHT
                } else {
                    Blocks::CONJURED_BLOCK
                };
                yog_api::event::world::set_block(&env.dim, to_ypos(pos), block);
                crate::conjured::conjured(&env.dim, pos, light, env.pigment());
            }
        },
    ))
}

/// `AkashicTreeGrower`'s trees.
const EDIFIED_TREES: [&str; 3] = [
    crate::ids::features::AMETHYST_EDIFIED_TREE,
    crate::ids::features::AVENTURINE_EDIFIED_TREE,
    crate::ids::features::CITRINE_EDIFIED_TREE,
];

/// `OpEdifySapling`.
fn edify(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let id = env.block_id(pos);
    if !yog_api::event::tag::has_block(&id, vanilla::block_tags::SAPLINGS) {
        return Err(Mishap::bad_block(pos, "sapling"));
    }
    let spray = Spray {
        pos: Vec3::center_of(pos),
        vel: Vec3::new(0.0, 2.0, 0.0),
        fuzziness: 0.1,
        spread: PI / 4.0,
        count: 100,
    };
    Ok(Spell::new(CRYSTAL, vec![spray], move |env, _| {
        if !env.can_edit_block_at(pos) {
            return;
        }
        let at = to_ypos(pos);
        let Some(sapling) = yog_api::event::block::info(&env.dim, at).map(|b| b.state) else {
            return;
        };
        // AbstractTreeGrower.growTree: clear the sapling, put it back if
        // no tree fits.
        for _ in 0..8 {
            yog_api::event::world::set_block(&env.dim, at, vanilla::blocks::AIR);
            let tree = EDIFIED_TREES
                [(random() * EDIFIED_TREES.len() as f64) as usize % EDIFIED_TREES.len()];
            if yog_api::event::world::place_feature(&env.dim, at, tree) {
                return;
            }
            yog_api::event::world::set_block(&env.dim, at, sapling);
        }
    }))
}

/// `NoteBlockInstrument`, in order: (sound, the note block's own name).
const INSTRUMENTS: [&str; 23] = [
    "block.note_block.harp",
    "block.note_block.basedrum",
    "block.note_block.snare",
    "block.note_block.hat",
    "block.note_block.bass",
    "block.note_block.flute",
    "block.note_block.bell",
    "block.note_block.guitar",
    "block.note_block.chime",
    "block.note_block.xylophone",
    "block.note_block.iron_xylophone",
    "block.note_block.cow_bell",
    "block.note_block.didgeridoo",
    "block.note_block.bit",
    "block.note_block.banjo",
    "block.note_block.pling",
    "block.note_block.imitate.zombie",
    "block.note_block.imitate.skeleton",
    "block.note_block.imitate.creeper",
    "block.note_block.imitate.ender_dragon",
    "block.note_block.imitate.wither_skeleton",
    "block.note_block.imitate.piglin",
    "ui.button.click",
];

/// `MsgBeepS2C.handle`, from the server.
fn beep(env: &mut CastEnv, at: Vec3, instrument: usize, note: i64) {
    let pitch = 2f64.powf((note - 12) as f64 / 12.0) as f32;
    let sound = format!("minecraft:{}", INSTRUMENTS[instrument]);
    yog_api::event::sound::play(&env.dim, (at.x, at.y, at.z), &sound, 3.0, pitch);
    yog_api::event::particle::spawn_particles(
        &env.dim,
        (at.x, at.y + 0.2, at.z),
        vanilla::particles::NOTE,
        0,
        (note as f64 / 24.0, 0.0, 0.0),
        1.0,
    );
}

// ── items ──────────────────────────────────────────────────────────────────

/// `OpColorize`.
fn colorize(env: &mut CastEnv) -> MResult<Spell> {
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
fn variants(id: &str) -> Option<i64> {
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
fn cycle_variant(env: &mut CastEnv) -> MResult<Spell> {
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
fn recharge(env: &mut CastEnv, e: ItemEnt) -> MResult<Spell> {
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
fn erase(env: &mut CastEnv) -> MResult<Spell> {
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
fn craft_packaged(
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

// ── entities ───────────────────────────────────────────────────────────────

/// `OpPotionEffect`.
pub fn potion(
    env: &mut CastEnv,
    e: Living,
    duration: f64,
    potency: Option<&Iota>,
    effect: &'static str,
    base: i64,
    cubic: bool,
) -> MResult<Spell> {
    let potency = match potency {
        Some(p) => double_between(p, 0, 1.0, 127.0)?,
        None => 1.0,
    };
    e.in_range(env)?;
    let scale = if cubic {
        potency * potency * potency
    } else {
        potency * potency
    };
    let cost = base as f64 * duration * scale;
    let spray = Spray::cloud(e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0), 1.0);
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(non_zero(cost), vec![spray], move |_env, _| {
        if duration >= 1.0 / 20.0 {
            let amplifier = (potency as i64 - 1).clamp(0, 255) as u8;
            yog_api::event::entity::add_effect(
                &uuid,
                effect,
                (duration * 20.0) as i32,
                amplifier,
                true,
            );
        }
    }))
}

/// `OpCreateSentinel`.
pub fn create_sentinel(env: &mut CastEnv, target: Vec3, great: bool) -> MResult<Spell> {
    let uuid = env.caster_uuid().ok_or(Mishap::BadCaster)?;
    env.assert_vec_in_range(target)?;
    let cost = if great { 2 * DUST } else { DUST };
    Ok(Spell::new(
        cost,
        vec![Spray::burst(target, 2.0)],
        move |env, _| {
            let s = Sentinel {
                great,
                pos: target,
                dim: env.dim.clone(),
            };
            set_sentinel(&uuid, Some(&s));
        },
    ))
}

// ── flight ─────────────────────────────────────────────────────────────────

/// `FlightAbility`: what Anchorite's / Wayfarer's Flight granted a player.
#[derive(Debug, Clone, PartialEq)]
pub struct FlightAbility {
    /// Ticks left, or -1 for no limit.
    pub time_left: i64,
    pub dim: String,
    pub origin: Vec3,
    /// Blocks from `origin` (in X/Z), or -1 for no limit.
    pub radius: f64,
}

/// Loaded flights, by player uuid (`None`: known not to fly).
static FLIGHTS: Mutex<Option<HashMap<String, Option<FlightAbility>>>> = Mutex::new(None);

fn load_flight(uuid: &str) -> Option<FlightAbility> {
    let s = player_storage(uuid);
    Some(FlightAbility {
        time_left: s.get_int("flight.time")?,
        dim: s.get_str("flight.dim")?.to_owned(),
        origin: Vec3::new(
            s.get_float("flight.x")?,
            s.get_float("flight.y")?,
            s.get_float("flight.z")?,
        ),
        radius: s.get_float("flight.radius")?,
    })
}

fn store_flight(uuid: &str, f: Option<&FlightAbility>) {
    let mut s = player_storage(uuid);
    match f {
        Some(f) => {
            s.set("flight.time", f.time_left);
            s.set("flight.dim", f.dim.as_str());
            s.set("flight.x", f.origin.x);
            s.set("flight.y", f.origin.y);
            s.set("flight.z", f.origin.z);
            s.set("flight.radius", f.radius);
        }
        None => {
            for k in [
                "flight.time",
                "flight.dim",
                "flight.x",
                "flight.y",
                "flight.z",
                "flight.radius",
            ] {
                s.remove(k);
            }
        }
    }
}

/// The flight a player was granted, if any.
pub fn flight_of(uuid: &str) -> Option<FlightAbility> {
    let mut map = FLIGHTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    map.entry(uuid.to_owned())
        .or_insert_with(|| load_flight(uuid))
        .clone()
}

/// `setFlight`; `persist` also writes it to disk.
fn set_flight(uuid: &str, f: Option<FlightAbility>, persist: bool) {
    if persist {
        store_flight(uuid, f.as_ref());
    }
    let mut map = FLIGHTS.lock().unwrap_or_else(|e| e.into_inner());
    map.get_or_insert_with(HashMap::new)
        .insert(uuid.to_owned(), f);
}

/// `OpFlight`.
fn flight(env: &mut CastEnv, p: Player, arg: f64, range: bool) -> MResult<Spell> {
    p.in_range(env)?;
    let unit = 2 * DUST;
    let mut cost = non_zero(arg * unit as f64);
    if range {
        cost = cost.max(unit);
    }
    let spray = Spray {
        pos: p.pos(),
        vel: Vec3::new(0.0, 2.0, 0.0),
        fuzziness: 0.0,
        spread: 0.1,
        count: 20,
    };
    let (uuid, name) = (p.uuid().to_owned(), p.info.name.clone());
    Ok(Spell::new(cost, vec![spray], move |_env, _| {
        if yog_api::event::player::info(&name).is_none_or(|i| i.may_fly) {
            return; // don't clobber someone else's flight
        }
        let Some(info) = yog_api::event::entity::info(&uuid) else {
            return;
        };
        let origin = Vec3::from_tuple(info.pos);
        let f = if range {
            FlightAbility {
                time_left: -1,
                dim: info.dimension.to_string(),
                origin,
                radius: arg,
            }
        } else {
            FlightAbility {
                time_left: (arg * 20.0).round() as i64,
                dim: info.dimension.to_string(),
                origin,
                radius: -1.0,
            }
        };
        set_flight(&uuid, Some(f), true);
        yog_api::event::player::set_may_fly(&name, true);
    }))
}

/// Blocks from the edge / ticks left where flight starts to look dangerous.
const DIST_DANGER_THRESHOLD: f64 = 4.0;
const TIME_DANGER_THRESHOLD: f64 = 7.0 * 20.0;

/// `getDanger`: 0 (fine) to 1 (the flight ends).
fn danger(dim: &str, pos: Vec3, f: &FlightAbility) -> f64 {
    let radius = if f.radius >= 0.0 {
        if dim != f.dim {
            1.0
        } else {
            let dist = Vec3::new(pos.x, 0.0, pos.z)
                .distance_sqr(Vec3::new(f.origin.x, 0.0, f.origin.z))
                .sqrt();
            let from_edge = f.radius - dist;
            if from_edge >= DIST_DANGER_THRESHOLD {
                0.0
            } else if dist > f.radius {
                1.0
            } else {
                1.0 - from_edge / DIST_DANGER_THRESHOLD
            }
        }
    } else {
        0.0
    };
    let time = if f.time_left >= 0 {
        if f.time_left as f64 >= TIME_DANGER_THRESHOLD {
            0.0
        } else {
            (TIME_DANGER_THRESHOLD - f.time_left as f64) / TIME_DANGER_THRESHOLD
        }
    } else {
        0.0
    };
    radius.max(time)
}

/// `OpFlight.tickDownFlight`, for every player, every tick.
pub fn tick_flight() {
    for name in yog_api::event::player::online() {
        let Some(p) = yog_api::event::player::info(&name) else {
            continue;
        };
        let Some(f) = flight_of(&p.uuid) else {
            continue;
        };
        let Some(info) = yog_api::event::entity::info(&p.uuid) else {
            continue;
        };
        let pos = Vec3::from_tuple(info.pos);
        let dim = info.dimension.as_str();
        let d = danger(dim, pos, &f);
        if d >= 1.0 {
            set_flight(&p.uuid, None, true);
            if p.gamemode != "creative" && p.gamemode != "spectator" {
                yog_api::event::player::set_may_fly(&name, false);
            }
            yog_api::event::sound::play(
                dim,
                (pos.x, pos.y, pos.z),
                crate::sounds::FLIGHT_FINISH,
                2.0,
                1.0,
            );
            let spray = Spray {
                pos,
                vel: Vec3::new(0.0, 1.0, 0.0),
                fuzziness: PI,
                spread: 0.4,
                count: 20,
            };
            spray.spawn(dim, &dye_pigment(dye::RED));
            spray.spawn(dim, &dye_pigment(dye::BLACK));
            continue;
        }
        if !p.may_fly {
            yog_api::event::player::set_may_fly(&name, true);
        }
        let time_left = if f.time_left >= 0 {
            f.time_left - 1
        } else {
            f.time_left
        };
        // Kept on disk once a second, not every tick.
        let persist = time_left >= 0 && time_left % 20 == 0;
        set_flight(
            &p.uuid,
            Some(FlightAbility {
                time_left,
                ..f.clone()
            }),
            persist,
        );

        let count = 5;
        let danger_count = (count as f64 * d).round() as u32;
        let one_danger = danger_count.div_ceil(2);
        let color = get_pigment(&p.uuid);
        Spray {
            pos,
            vel: Vec3::new(0.0, -0.6, 0.0),
            fuzziness: 0.6,
            spread: PI * 0.3,
            count: count - danger_count,
        }
        .spawn(dim, &color);
        let danger_spray = Spray {
            pos,
            vel: Vec3::new(0.0, 1.0, 0.0),
            fuzziness: 0.3,
            spread: PI * 0.75,
            count: one_danger,
        };
        danger_spray.spawn(dim, &dye_pigment(dye::BLACK));
        danger_spray.spawn(dim, &dye_pigment(dye::RED));
        if random() < 0.02 {
            yog_api::event::sound::play(
                dim,
                (pos.x, pos.y, pos.z),
                crate::sounds::FLIGHT_AMBIENCE,
                0.2,
                1.0,
            );
        }
        if f.radius >= 0.0 {
            let origin = f.origin + Vec3::new(0.0, 1.0, 0.0);
            Spray {
                pos: origin,
                vel: Vec3::new(0.0, 1.0, 0.0),
                fuzziness: 0.5,
                spread: PI * 0.1,
                count: 5,
            }
            .spawn(dim, &color);
            Spray {
                pos: origin,
                vel: Vec3::new(0.0, -1.0, 0.0),
                fuzziness: 1.5,
                spread: PI * 0.25,
                count: 5,
            }
            .spawn(dim, &color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_are_never_free() {
        assert_eq!(non_zero(0.2), 1);
        assert_eq!(non_zero(0.0), 1);
        assert_eq!(non_zero(-0.2), -1);
        assert_eq!(non_zero(2.5), 3);
        assert_eq!(non_zero(12_500.4), 12_500);
    }

    #[test]
    fn flight_danger() {
        let f = FlightAbility {
            time_left: -1,
            dim: "d".into(),
            origin: Vec3::ZERO,
            radius: 10.0,
        };
        assert_eq!(danger("d", Vec3::new(1.0, 50.0, 0.0), &f), 0.0);
        assert_eq!(danger("d", Vec3::new(8.0, 0.0, 0.0), &f), 0.5);
        assert_eq!(danger("d", Vec3::new(11.0, 0.0, 0.0), &f), 1.0);
        assert_eq!(danger("other", Vec3::ZERO, &f), 1.0);
        let t = FlightAbility {
            time_left: 70,
            radius: -1.0,
            ..f
        };
        assert_eq!(danger("d", Vec3::ZERO, &t), 0.5);
    }
}
