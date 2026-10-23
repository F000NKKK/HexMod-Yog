//! Great spells (`casting/actions/spells/great/*`).

use yog_api::vanilla;

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Ent, ItemEnt, Living, Mob, Player, PosDouble};
use crate::casting::env::{CastEnv, Spray, Stack, random, to_ypos};
use crate::casting::mishap::{BlockPos, MResult, Mishap};
use crate::casting::ops::spells::{
    Potion, assert_teleportable, craft_battery, create_fluid, create_sentinel, potion, teleport_by,
};
use crate::casting::vm::Spell;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::item::media::{CRYSTAL, DUST, SHARD};
use crate::math::Vec3;
use crate::snbt::Nbt;

/// The sprays of summoned lightning and of Altiora's lift.
const LIGHTNING: Spray = Spray::look((0.0, -1.0, 0.0), 0.5, 0.1, 20);
const LOFT: Spray = Spray::look((0.0, 2.0, 0.0), 0.0, 0.1, 20);
/// Items flung off a far teleport: how high they're tossed, from how far
/// below the eyes, and the share of the distance's chance each armor
/// piece has.
const DROP_UP: f64 = 0.2;
const DROP_BELOW_EYES: f64 = 0.3;
const ARMOR_DROP_SHARE: f64 = 0.25;
/// A brainsweep: its burst (size, count), the mob's death (volume) and
/// the level-up after (volume, pitch).
const BRAINSWEEP_BURST: (f64, u32) = (0.3, 100);
const DEATH_VOLUME: f32 = 0.8;
const LEVELUP: (f32, f32) = (0.5, 0.8);

pub fn actions() -> Vec<Action> {
    actions! {
        "lightning" E "waadwawdaaweewq" => spell [env, img; target: Vec3] {
            env.assert_vec_in_range(target)?;
            if !env.can_edit_block_at(target.block()) {
                return Err(Mishap::BadLocation { pos: target, kind: "forbidden" });
            }
            let spray = Spray { pos: target + Vec3::new(0.0, 2.0, 0.0), ..LIGHTNING };
            Ok(Spell::new(3 * SHARD, vec![spray], move |env, _| {
                yog_api::event::entity::spawn(vanilla::entities::LIGHTNING_BOLT, &env.dim, (target.x, target.y, target.z));
            }))
        };
        "flight" NW "eawwaeawawaa" => spell [env, img; p: Player] { altiora(env, p) };
        "create_lava" E "eaqawqadaqd" => spell [env, img; v: Vec3] { create_fluid(env, v.block(), true) };
        "teleport/great" E "wwwqqqwwwqqeqqwwwqqwqqdqqqqqdqq" => spell [env, img; e: Ent, delta: Vec3] {
            teleport(env, e, delta)
        };
        "sentinel/create/great" E "waeawaeqqqwqwqqwq" => spell [env, img; target: Vec3] {
            create_sentinel(env, target, true)
        };
        "dispel_rain" E "eeewwweeewwaqqddqdqd" => spell [env, img] { weather(false) };
        "summon_rain" W "wwweeewwweewdawdwad" => spell [env, img] { weather(true) };
        "brainsweep" NE "qeqwqwqwqwqeqaeqeaqeqaeqaqded" => spell [env, img; mob: Mob, v: Vec3] { brainsweep(env, mob, v) };
        "craft/battery" SW "aqqqaqwwaqqqqqeqaqqqawwqwqwqwqwqw" => spell [env, img; e: ItemEnt] { craft_battery(env, e) };
        "potion/regeneration" NW "qqqqaawawaedd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::REGENERATION, base: DUST, cubic: true })
        };
        "potion/night_vision" W "qqqaawawaeqdd" => spell [env, img; e: Living, dur: PosDouble] {
            potion(env, e, dur.0, None, Potion { effect: vanilla::effects::NIGHT_VISION, base: DUST / 5, cubic: false })
        };
        "potion/absorption" SW "qqaawawaeqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::ABSORPTION, base: DUST, cubic: true })
        };
        "potion/haste" SE "qaawawaeqqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::HASTE, base: DUST / 3, cubic: true })
        };
        "potion/strength" E "aawawaeqqqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), Potion { effect: vanilla::effects::STRENGTH, base: DUST / 3, cubic: true })
        };
    }
}

// ── weather ────────────────────────────────────────────────────────────────

/// `OpWeather` (summoned rain never brings thunder: the loader sets only
/// rain).
fn weather(rain: bool) -> MResult<Spell> {
    let cost = if rain { CRYSTAL } else { SHARD };
    Ok(Spell::new(cost, vec![], move |env, _| {
        if yog_api::event::world::is_raining(&env.dim) != rain {
            let (min, max) = if rain { (30, 90) } else { (60, 180) };
            let minutes = min + (random() * (max - min) as f64) as i32;
            yog_api::event::world::set_weather(&env.dim, rain, minutes * 20 * 60);
        }
    }))
}

// ── teleport ───────────────────────────────────────────────────────────────

/// `OpTeleport`.
fn teleport(env: &mut CastEnv, e: Ent, delta: Vec3) -> MResult<Spell> {
    e.in_range(env)?;
    assert_teleportable(&e)?;
    let target = e.pos() + delta;
    env.assert_vec_in_world(target)?;
    if !env.is_vec_in_world(target - Vec3::new(0.0, 1.0, 0.0)) {
        return Err(Mishap::BadLocation {
            pos: target,
            kind: "too_close_to_out",
        });
    }
    let mid = e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0);
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        10 * CRYSTAL,
        vec![Spray::cloud(mid, 2.0), Spray::burst(mid + delta, 2.0)],
        move |env, _| {
            teleport_by(&env.dim, &uuid, delta);
            if env.caster_uuid().as_deref() == Some(uuid.as_str()) {
                splat_items(env, delta.length());
            }
        },
    ))
}

/// Long teleports shake loose some of the caster's items (never the held
/// one, nor the off hand; armour and the hotbar less often).
fn splat_items(env: &CastEnv, distance: f64) {
    let Some(name) = env.caster_name() else {
        return;
    };
    let Some(selected) = yog_api::event::player::info(name).map(|p| p.selected_slot) else {
        return;
    };
    let base = distance / 10_000.0;
    let drop = |s: Stack| {
        let Some(info) = env.caster_info() else {
            return;
        };
        // Player.drop(stack, dropAround = true)
        let (f, g) = (random() * 0.5, random() * std::f64::consts::TAU);
        let vel = (-g.sin() * f, DROP_UP, g.cos() * f);
        let eye = info.eye_pos();
        yog_api::event::item::spawn_item(
            &env.dim,
            (eye.x, eye.y - DROP_BELOW_EYES, eye.z),
            vel,
            &s.id,
            s.count,
            s.nbt.to_snbt(),
        );
        env.save(&Stack { count: 0, ..s });
    };
    for slot in 36..40 {
        let s = env.stack_at(slot);
        if s.is_empty()
            || s.nbt
                .to_snbt()
                .contains(vanilla::enchantments::BINDING_CURSE)
        {
            continue;
        }
        if random() < base * ARMOR_DROP_SHARE {
            drop(s);
        }
    }
    for slot in (0..36).filter(|&i| i != selected) {
        let s = env.stack_at(slot);
        let chance = if slot < 9 { base * 0.5 } else { base };
        if !s.is_empty() && random() < chance {
            drop(s);
        }
    }
}

// ── brainsweep ─────────────────────────────────────────────────────────────

/// What a brainsweep recipe takes.
enum Sweepee {
    /// A villager with this profession (any, if `None`) and at least this
    /// level.
    Villager(Option<&'static str>, i64),
    Entity(&'static str),
}

/// `data/hexcasting/recipes/brainsweep/*.json`: (block, mob, result, cost).
const RECIPES: [(&str, Sweepee, &str, i64); 8] = [
    (
        Blocks::AKASHIC_CONNECTOR,
        Sweepee::Villager(Some("librarian"), 5),
        Blocks::AKASHIC_RECORD,
        1_000_000,
    ),
    (
        vanilla::blocks::AMETHYST_BLOCK,
        Sweepee::Villager(None, 3),
        vanilla::blocks::BUDDING_AMETHYST,
        1_000_000,
    ),
    (
        Blocks::DIRECTRIX_EMPTY,
        Sweepee::Villager(Some("shepherd"), 1),
        Blocks::DIRECTRIX_BOOLEAN,
        1_000_000,
    ),
    (
        Blocks::DIRECTRIX_EMPTY,
        Sweepee::Villager(Some("mason"), 1),
        Blocks::DIRECTRIX_REDSTONE,
        1_000_000,
    ),
    (
        Blocks::IMPETUS_EMPTY,
        Sweepee::Villager(Some("fletcher"), 2),
        Blocks::IMPETUS_LOOK,
        1_000_000,
    ),
    (
        Blocks::IMPETUS_EMPTY,
        Sweepee::Villager(Some("toolsmith"), 2),
        Blocks::IMPETUS_RIGHTCLICK,
        1_000_000,
    ),
    (
        Blocks::IMPETUS_EMPTY,
        Sweepee::Villager(Some("cleric"), 2),
        Blocks::IMPETUS_REDSTONE,
        1_000_000,
    ),
    (
        vanilla::blocks::AMETHYST_BLOCK,
        Sweepee::Entity(vanilla::entities::ALLAY),
        Blocks::QUENCHED_ALLAY,
        100_000,
    ),
];

fn sweepee_matches(mob: &Ent, want: &Sweepee) -> bool {
    match want {
        Sweepee::Entity(t) => mob.info.type_id == *t,
        Sweepee::Villager(profession, level) => {
            if mob.info.type_id != vanilla::entities::VILLAGER {
                return false;
            }
            let nbt = yog_api::event::entity::nbt(mob.uuid())
                .map(|s| Nbt::parse(&s))
                .unwrap_or_default();
            let Some(data) = nbt.get("VillagerData") else {
                return false;
            };
            let p = data.get_str("profession").unwrap_or_default();
            let p = yog_api::yog_ids::game_path(p).unwrap_or(p);
            profession.is_none_or(|want| want == p) && data.get_i64("level").unwrap_or(0) >= *level
        }
    }
}

/// What a brainswept mob keeps on itself (saved with it): it's there or not.
const BRAINSWEPT: &str = "hexcasting:brainswept";

/// `IXplatAbstractions.isBrainswept`.
pub fn is_brainswept(uuid: &str) -> bool {
    yog_api::event::entity::data(uuid, BRAINSWEPT).is_some()
}

/// `HexAPI.brainsweep`: the mob stays, but its mind is gone.
fn mark_brainswept(uuid: &str) {
    yog_api::event::entity::set_data(uuid, BRAINSWEPT, Some(&[]), true);
    yog_api::event::entity::set_nbt(uuid, "{NoAI:1b}");
}

/// `BrainsweepRecipe.copyProperties`: the result keeps the block's state
/// (where it has the same properties).
fn with_properties(state: &str, result: &str) -> String {
    match state.find('[') {
        Some(i) => format!("{result}{}", &state[i..]),
        None => result.to_owned(),
    }
}

/// `OpBrainsweep`.
fn brainsweep(env: &mut CastEnv, mob: Mob, v: Vec3) -> MResult<Spell> {
    let pos: BlockPos = v.block();
    env.assert_vec_in_range(v)?;
    mob.in_range(env)?;
    if !env.can_edit_block_at(pos) {
        return Err(Mishap::BadLocation {
            pos: v,
            kind: "forbidden",
        });
    }
    if yog_api::event::tag::has_entity(mob.info.type_id, crate::ids::tags::CANNOT_BRAINSWEEP) {
        return Err(Mishap::BadBrainsweep {
            mob: mob.r.clone(),
            pos,
        });
    }
    if is_brainswept(mob.uuid()) {
        return Err(Mishap::AlreadyBrainswept {
            mob: mob.r.clone(),
            eye: mob.eye(),
        });
    }
    let state = yog_api::event::block::info(&env.dim, to_ypos(pos))
        .map(|b| b.state)
        .unwrap_or_default();
    let block = state.split('[').next().unwrap_or_default();
    let (_, _, result, cost) = RECIPES
        .iter()
        .find(|(b, want, ..)| *b == block && sweepee_matches(&mob, want))
        .ok_or_else(|| Mishap::BadBrainsweep {
            mob: mob.r.clone(),
            pos,
        })?;
    let (uuid, type_id, at) = (mob.uuid().to_owned(), mob.info.type_id, mob.pos());
    let target = with_properties(&state, result);
    let plain = result.to_string();
    let particles = vec![
        Spray::cloud(mob.pos(), 1.0),
        Spray::burst_n(Vec3::center_of(pos), BRAINSWEEP_BURST.0, BRAINSWEEP_BURST.1),
    ];
    Ok(Spell::new(*cost, particles, move |env, _| {
        if !yog_api::event::world::set_block(&env.dim, to_ypos(pos), &target) {
            yog_api::event::world::set_block(&env.dim, to_ypos(pos), &plain);
        }
        mark_brainswept(&uuid);
        // The mob's death sound, as the game names them.
        let (ns, path) = yog_api::yog_ids::split(&type_id);
        let death = yog_api::yog_ids::join(ns, &format!("entity.{path}.death"));
        yog_api::event::sound::play(&env.dim, (at.x, at.y, at.z), &death, DEATH_VOLUME, 1.0);
        let levelup = yog_api::vanilla::sounds::ENTITY_PLAYER_LEVELUP;
        yog_api::event::sound::play(&env.dim, (at.x, at.y, at.z), levelup, LEVELUP.0, LEVELUP.1);
    })
    .mute())
}

// ── Altiora ────────────────────────────────────────────────────────────────

/// `OpAltiora`: the target is lofted into the air with wings (see
/// [`crate::altiora`]).
fn altiora(env: &mut CastEnv, p: Player) -> MResult<Spell> {
    p.in_range(env)?;
    let particles = vec![
        Spray::burst(p.pos(), 0.5),
        Spray {
            pos: p.pos(),
            ..LOFT
        },
    ];
    let uuid = p.uuid().to_owned();
    Ok(Spell::new(CRYSTAL, particles, move |_env, _| {
        crate::altiora::give(&uuid)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brainswept_blocks_keep_their_state() {
        assert_eq!(
            with_properties(
                "hexcasting:impetus/empty[facing=north]",
                Blocks::IMPETUS_LOOK
            ),
            "hexcasting:impetus/look[facing=north]"
        );
        assert_eq!(
            with_properties("minecraft:amethyst_block", "minecraft:budding_amethyst"),
            "minecraft:budding_amethyst"
        );
    }
}
