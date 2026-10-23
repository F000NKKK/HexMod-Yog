//! Spells on blocks: breaking, placing, fluids, fire, conjured blocks,
//! edified trees, note blocks.

use crate::casting::args::{pos_int_under, pos_int_under_incl};

use crate::casting::args::Arg;
use crate::casting::args::Ent;
use crate::casting::env::{CastEnv, Spray, Stack, random, to_ypos};
use crate::casting::mishap::{BlockPos, MResult, Mishap};
use crate::casting::vm::Spell;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::item::media::{CRYSTAL, DUST};
use crate::math::Vec3;
use crate::things::akashic;
use std::collections::{HashSet, VecDeque};
use std::f64::consts::PI;
use yog_api::core::Face;
use yog_api::event::particle::ParticleOptions;
use yog_api::vanilla;
use yog_api::vanilla::sounds;

// ── blocks ─────────────────────────────────────────────────────────────────

/// `OpBreakBlock`.
pub(super) fn break_block(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
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

pub(super) fn is_block_item(s: &Stack) -> bool {
    yog_api::event::item::block(&s.id).is_some()
}

pub(super) fn replaceable(env: &CastEnv, pos: BlockPos) -> bool {
    yog_api::event::block::info(&env.dim, to_ypos(pos)).is_some_and(|b| b.replaceable)
}

/// `OpPlaceBlock`.
pub(super) fn place_block(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
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

pub(super) const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];
pub(super) const MAX_DESTROY_COUNT: u32 = 1024;
/// Smoke where fluid is drained: how far it may start from the block's
/// middle, how many, how it rises.
const SMOKE_JITTER: f64 = 0.4;
const SMOKE_COUNT: i32 = 2;
const SMOKE_RISE: (f64, f64, f64) = (0.0, 0.05, 0.0);
/// The hiss of fire put out.
const EXTINGUISH_PITCH: f32 = 0.95;
/// The spray an edified tree grows in.
const EDIFY_FUZZ: f64 = 0.1;
const EDIFY_SPREAD: f64 = 4.0;
const EDIFY_COUNT: u32 = 100;

pub(super) fn offset(p: BlockPos, d: (i32, i32, i32)) -> BlockPos {
    (p.0 + d.0, p.1 + d.1, p.2 + d.2)
}

pub(super) fn smoke(env: &CastEnv, here: BlockPos) {
    let j = || 0.5 + (random() - 0.5) * SMOKE_JITTER;
    yog_api::event::particle::spawn_particles(
        &env.dim,
        (
            here.0 as f64 + j(),
            here.1 as f64 + j(),
            here.2 as f64 + j(),
        ),
        ParticleOptions::plain(vanilla::particles::SMOKE),
        SMOKE_COUNT,
        SMOKE_RISE,
        0.0,
    );
}

/// `OpDestroyFluid`: drains a cauldron, or soaks up fluid like a sponge.
pub(super) fn destroy_fluid(env: &mut CastEnv, base: BlockPos) {
    let id = env.block_id(base);
    let filled = yog_api::yog_ids::game_path(&id).is_some_and(|p| p.ends_with("cauldron"));
    if id != vanilla::blocks::CAULDRON && filled {
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
            EXTINGUISH_PITCH,
        );
    }
}

/// `OpExtinguish`.
pub(super) fn extinguish(env: &mut CastEnv, target: BlockPos) {
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
            EXTINGUISH_PITCH,
        );
    }
}

/// `OpIgnite`.
pub(super) fn ignite(env: &mut CastEnv, target: &Iota) -> MResult<Spell> {
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
/// (`BlockConjured.setColor`, [`crate::things::conjured`]).
pub(super) fn conjure(env: &mut CastEnv, pos: BlockPos, light: bool) -> MResult<Spell> {
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
                crate::things::conjured::conjured(&env.dim, pos, light, env.pigment());
            }
        },
    ))
}

/// `AkashicTreeGrower`'s trees.
pub(super) const EDIFIED_TREES: [&str; 3] = [
    crate::ids::features::AMETHYST_EDIFIED_TREE,
    crate::ids::features::AVENTURINE_EDIFIED_TREE,
    crate::ids::features::CITRINE_EDIFIED_TREE,
];

/// `OpEdifySapling`.
pub(super) fn edify(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let id = env.block_id(pos);
    if !yog_api::event::tag::has_block(&id, vanilla::block_tags::SAPLINGS) {
        return Err(Mishap::bad_block(pos, "sapling"));
    }
    let spray = Spray {
        pos: Vec3::center_of(pos),
        vel: Vec3::new(0.0, 2.0, 0.0),
        fuzziness: EDIFY_FUZZ,
        spread: PI / EDIFY_SPREAD,
        count: EDIFY_COUNT,
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
/// A note block's notes: the one at pitch 1 (F♯ an octave up), the
/// highest; how loud a beep is, and how far above it its note rises.
pub(super) const NOTE_ONE: i64 = 12;
pub(super) const MAX_NOTE: i64 = 24;
const BEEP_VOLUME: f32 = 3.0;
const NOTE_RISE: f64 = 0.2;

pub(super) const INSTRUMENTS: [&str; 23] = [
    sounds::BLOCK_NOTE_BLOCK_HARP,
    sounds::BLOCK_NOTE_BLOCK_BASEDRUM,
    sounds::BLOCK_NOTE_BLOCK_SNARE,
    sounds::BLOCK_NOTE_BLOCK_HAT,
    sounds::BLOCK_NOTE_BLOCK_BASS,
    sounds::BLOCK_NOTE_BLOCK_FLUTE,
    sounds::BLOCK_NOTE_BLOCK_BELL,
    sounds::BLOCK_NOTE_BLOCK_GUITAR,
    sounds::BLOCK_NOTE_BLOCK_CHIME,
    sounds::BLOCK_NOTE_BLOCK_XYLOPHONE,
    sounds::BLOCK_NOTE_BLOCK_IRON_XYLOPHONE,
    sounds::BLOCK_NOTE_BLOCK_COW_BELL,
    sounds::BLOCK_NOTE_BLOCK_DIDGERIDOO,
    sounds::BLOCK_NOTE_BLOCK_BIT,
    sounds::BLOCK_NOTE_BLOCK_BANJO,
    sounds::BLOCK_NOTE_BLOCK_PLING,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_ZOMBIE,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_SKELETON,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_CREEPER,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_ENDER_DRAGON,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_WITHER_SKELETON,
    sounds::BLOCK_NOTE_BLOCK_IMITATE_PIGLIN,
    sounds::UI_BUTTON_CLICK,
];

/// `MsgBeepS2C.handle`, from the server.
pub(super) fn beep(env: &mut CastEnv, at: Vec3, instrument: usize, note: i64) {
    let pitch = 2f64.powf((note - NOTE_ONE) as f64 / NOTE_ONE as f64) as f32;
    let Some(&sound) = INSTRUMENTS.get(instrument) else {
        return;
    };
    yog_api::event::sound::play(&env.dim, (at.x, at.y, at.z), sound, BEEP_VOLUME, pitch);
    yog_api::event::particle::spawn_particles(
        &env.dim,
        (at.x, at.y + NOTE_RISE, at.z),
        ParticleOptions::plain(vanilla::particles::NOTE),
        0,
        // A note particle's color: its note out of the two octaves.
        (note as f64 / MAX_NOTE as f64, 0.0, 0.0),
        1.0,
    );
}

/// The bursts of draining, putting out and bonemeal, and bonemeal's cost
/// in dust.
const DRAIN_BURST: f64 = 3.0;
const BONEMEAL_COST: f64 = 1.125;

/// `OpDestroyFluid`'s spell on `pos`.
pub(super) fn destroy_water(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let burst = Spray::burst(Vec3::center_of(pos), DRAIN_BURST);
    Ok(Spell::new(2 * CRYSTAL, vec![burst], move |env, _| {
        destroy_fluid(env, pos)
    }))
}

/// `OpExtinguish`'s spell on `pos`.
pub(super) fn put_out(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let burst = Spray::burst(Vec3::center_of(pos), 1.0);
    Ok(Spell::new(6 * DUST, vec![burst], move |env, _| {
        extinguish(env, pos)
    }))
}

/// `OpTheOnlyReasonAnyoneDownloadedPsi`: bonemeal on `pos`, as the caster.
pub(super) fn bonemeal(env: &mut CastEnv, pos: BlockPos) -> MResult<Spell> {
    env.assert_pos_in_range_for_editing(pos)?;
    let burst = Spray::burst(Vec3::center_of(pos), 1.0);
    let cost = (DUST as f64 * BONEMEAL_COST) as i64;
    Ok(Spell::new(cost, vec![burst], move |env, _| {
        let player = env.caster_name().unwrap_or_default().to_owned();
        let face = Face::Up as u8;
        yog_api::event::block::use_item(
            &env.dim,
            to_ypos(pos),
            face,
            vanilla::items::BONE_MEAL,
            &player,
        );
    }))
}

/// `OpBeep`: `note` (0–24) on `instrument` (a note block's, 1-based) at `target`.
pub(super) fn beep_spell(
    env: &mut CastEnv,
    target: Vec3,
    instrument: &Iota,
    note: &Iota,
) -> MResult<Spell> {
    let instrument = pos_int_under(instrument, 1, INSTRUMENTS.len() as i64)? as usize;
    let note = pos_int_under_incl(note, 0, MAX_NOTE)?;
    env.assert_vec_in_range(target)?;
    let cloud = Spray::cloud(target, 1.0);
    Ok(Spell::new(DUST / 10, vec![cloud], move |env, _| {
        beep(env, target, instrument, note)
    })
    .mute())
}
