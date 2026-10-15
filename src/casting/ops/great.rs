//! Great spells (`casting/actions/spells/great/*`).

use std::collections::HashMap;
use std::sync::Mutex;

use yog_api::{Server, Storage};

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Ent, ItemEnt, Living, Mob, Player, PosDouble};
use crate::casting::env::{CastEnv, MOD_ID, Spray, Stack, get_pigment, random, to_ypos};
use crate::casting::mishap::{BlockPos, MResult, Mishap};
use crate::casting::ops::spells::{
    assert_teleportable, craft_battery, create_fluid, create_sentinel, potion, teleport_by,
};
use crate::casting::vm::Spell;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::item::media::{CRYSTAL, DUST, SHARD};
use crate::math::Vec3;
use crate::snbt::Nbt;

pub fn actions() -> Vec<Action> {
    actions! {
        "lightning" E "waadwawdaaweewq" => spell [env, img; target: Vec3] {
            env.assert_vec_in_range(target)?;
            if !env.can_edit_block_at(target.block()) {
                return Err(Mishap::BadLocation { pos: target, kind: "forbidden" });
            }
            let spray = Spray {
                pos: target + Vec3::new(0.0, 2.0, 0.0),
                vel: Vec3::new(0.0, -1.0, 0.0),
                fuzziness: 0.5,
                spread: 0.1,
                count: 20,
            };
            Ok(Spell::new(3 * SHARD, vec![spray], move |env, _| {
                env.srv.spawn_entity("minecraft:lightning_bolt", &env.dim, target.x, target.y, target.z);
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
            potion(env, e, dur.0, Some(&pot), "minecraft:regeneration", DUST, true)
        };
        "potion/night_vision" W "qqqaawawaeqdd" => spell [env, img; e: Living, dur: PosDouble] {
            potion(env, e, dur.0, None, "minecraft:night_vision", DUST / 5, false)
        };
        "potion/absorption" SW "qqaawawaeqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), "minecraft:absorption", DUST, true)
        };
        "potion/haste" SE "qaawawaeqqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), "minecraft:haste", DUST / 3, true)
        };
        "potion/strength" E "aawawaeqqqqdd" => spell [env, img; e: Living, dur: PosDouble, pot: Iota] {
            potion(env, e, dur.0, Some(&pot), "minecraft:strength", DUST / 3, true)
        };
    }
}

// ── weather ────────────────────────────────────────────────────────────────

/// `OpWeather` (summoned rain never brings thunder: the loader sets only
/// rain).
fn weather(rain: bool) -> MResult<Spell> {
    let cost = if rain { CRYSTAL } else { SHARD };
    Ok(Spell::new(cost, vec![], move |env, _| {
        if env.srv.world_is_raining(&env.dim) != rain {
            let (min, max) = if rain { (30, 90) } else { (60, 180) };
            let minutes = min + (random() * (max - min) as f64) as i32;
            env.srv.world_set_weather(&env.dim, rain, minutes * 20 * 60);
        }
    }))
}

// ── teleport ───────────────────────────────────────────────────────────────

/// `OpTeleport`.
fn teleport(env: &mut CastEnv<'_>, e: Ent, delta: Vec3) -> MResult<Spell> {
    e.in_range(env)?;
    assert_teleportable(env, &e)?;
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
            teleport_by(env.srv, &env.dim, &uuid, delta);
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
    let Some(selected) = env.srv.player_info(name).map(|p| p.selected_slot) else {
        return;
    };
    let base = distance / 10_000.0;
    let drop = |s: Stack| {
        let Some(info) = env.caster_info() else {
            return;
        };
        // Player.drop(stack, dropAround = true)
        let (f, g) = (random() * 0.5, random() * std::f64::consts::TAU);
        let vel = (-g.sin() * f, 0.2, g.cos() * f);
        let eye = info.eye_pos();
        env.srv.spawn_item(
            &env.dim,
            (eye.0, eye.1 - 0.3, eye.2),
            vel,
            &s.id,
            s.count,
            &s.nbt.to_snbt(),
        );
        env.save(&Stack { count: 0, ..s });
    };
    for slot in 36..40 {
        let s = env.stack_at(slot);
        if s.is_empty() || s.nbt.to_snbt().contains("minecraft:binding_curse") {
            continue;
        }
        if random() < base * 0.25 {
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
        "minecraft:amethyst_block",
        Sweepee::Villager(None, 3),
        "minecraft:budding_amethyst",
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
        "minecraft:amethyst_block",
        Sweepee::Entity("minecraft:allay"),
        Blocks::QUENCHED_ALLAY,
        100_000,
    ),
];

fn sweepee_matches(srv: &dyn Server, mob: &Ent, want: &Sweepee) -> bool {
    match want {
        Sweepee::Entity(t) => mob.info.type_id == *t,
        Sweepee::Villager(profession, level) => {
            if mob.info.type_id != "minecraft:villager" {
                return false;
            }
            let nbt = srv
                .entity_get_nbt(mob.uuid())
                .map(|s| Nbt::parse(&s))
                .unwrap_or_default();
            let Some(data) = nbt.get("VillagerData") else {
                return false;
            };
            let p = data.get_str("profession").unwrap_or_default();
            let p = p.strip_prefix("minecraft:").unwrap_or(p);
            profession.is_none_or(|want| want == p) && data.get_i64("level").unwrap_or(0) >= *level
        }
    }
}

/// `IXplatAbstractions.isBrainswept`.
pub fn is_brainswept(srv: &dyn Server, uuid: &str) -> bool {
    Storage::open_entity(&srv.game_dir(), MOD_ID, uuid)
        .get_bool("brainswept")
        .unwrap_or(false)
}

/// `HexAPI.brainsweep`: the mob stays, but its mind is gone.
fn mark_brainswept(srv: &dyn Server, uuid: &str) {
    let mut s = Storage::open_entity(&srv.game_dir(), MOD_ID, uuid);
    s.set("brainswept", true);
    srv.entity_set_nbt(uuid, "{NoAI:1b}");
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
fn brainsweep(env: &mut CastEnv<'_>, mob: Mob, v: Vec3) -> MResult<Spell> {
    let pos: BlockPos = v.block();
    env.assert_vec_in_range(v)?;
    mob.in_range(env)?;
    if !env.can_edit_block_at(pos) {
        return Err(Mishap::BadLocation {
            pos: v,
            kind: "forbidden",
        });
    }
    if env
        .srv
        .has_entity_tag(&mob.info.type_id, "hexcasting:cannot_brainsweep")
    {
        return Err(Mishap::BadBrainsweep {
            mob: mob.r.clone(),
            pos,
        });
    }
    if is_brainswept(env.srv, mob.uuid()) {
        return Err(Mishap::AlreadyBrainswept {
            mob: mob.r.clone(),
            eye: mob.eye(),
        });
    }
    let state = env
        .srv
        .block_info(&env.dim, to_ypos(pos))
        .map(|b| b.state)
        .unwrap_or_default();
    let block = state.split('[').next().unwrap_or_default();
    let (_, _, result, cost) = RECIPES
        .iter()
        .find(|(b, want, ..)| *b == block && sweepee_matches(env.srv, &mob, want))
        .ok_or_else(|| Mishap::BadBrainsweep {
            mob: mob.r.clone(),
            pos,
        })?;
    let (uuid, type_id, at) = (mob.uuid().to_owned(), mob.info.type_id.clone(), mob.pos());
    let target = with_properties(&state, result);
    let plain = result.to_string();
    let particles = vec![
        Spray::cloud(mob.pos(), 1.0),
        Spray::burst_n(Vec3::center_of(pos), 0.3, 100),
    ];
    Ok(Spell::new(*cost, particles, move |env, _| {
        if !env.srv.set_block(&env.dim, to_ypos(pos), &target) {
            env.srv.set_block(&env.dim, to_ypos(pos), &plain);
        }
        mark_brainswept(env.srv, &uuid);
        let path = type_id.split_once(':').map_or(type_id.as_str(), |(_, p)| p);
        let death = format!("minecraft:entity.{path}.death");
        env.srv
            .play_sound(&env.dim, at.x, at.y, at.z, &death, 0.8, 1.0);
        env.srv.play_sound(
            &env.dim,
            at.x,
            at.y,
            at.z,
            "minecraft:entity.player.levelup",
            0.5,
            0.8,
        );
    })
    .mute())
}

// ── Altiora ────────────────────────────────────────────────────────────────

/// `OpAltiora.GRACE_PERIOD` (ticks before landing ends it).
const GRACE_PERIOD: i32 = 20;

/// Players under Altiora, by uuid, with the grace ticks left.
static ALTIORA: Mutex<Option<HashMap<String, i32>>> = Mutex::new(None);

/// `OpAltiora` (the elytra-like glide itself needs the loader: here the
/// spell lifts the caster and keeps its effects until they land).
fn altiora(env: &mut CastEnv<'_>, p: Player) -> MResult<Spell> {
    p.in_range(env)?;
    let particles = vec![
        Spray::burst(p.pos(), 0.5),
        Spray {
            pos: p.pos(),
            vel: Vec3::new(0.0, 2.0, 0.0),
            fuzziness: 0.0,
            spread: 0.1,
            count: 20,
        },
    ];
    let uuid = p.uuid().to_owned();
    Ok(Spell::new(CRYSTAL, particles, move |env, _| {
        env.srv.entity_add_velocity(&uuid, 0.0, 1.5, 0.0);
        let mut map = ALTIORA.lock().unwrap_or_else(|e| e.into_inner());
        map.get_or_insert_with(HashMap::new)
            .insert(uuid, GRACE_PERIOD);
    }))
}

/// `OpAltiora.checkPlayerCollision`, every tick.
pub fn tick_altiora(srv: &dyn Server) {
    let mut map = ALTIORA.lock().unwrap_or_else(|e| e.into_inner());
    let Some(map) = map.as_mut() else { return };
    map.retain(|uuid, grace| {
        let Some(info) = srv.entity_info(uuid) else {
            return false;
        };
        let pos = Vec3::from_tuple(info.pos);
        let dim = info.dimension.as_str();
        let on_ground = || {
            srv.entity_get_nbt(uuid)
                .is_some_and(|s| Nbt::parse(&s).get_bool("OnGround"))
        };
        if *grace == 0 && on_ground() {
            srv.play_sound(
                dim,
                pos.x,
                pos.y,
                pos.z,
                "hexcasting:flight.finish",
                2.0,
                1.0,
            );
            return false;
        }
        *grace = (*grace - 1).max(0);
        if random() < 0.02 {
            srv.play_sound(
                dim,
                pos.x,
                pos.y,
                pos.z,
                "hexcasting:flight.ambience",
                0.2,
                1.0,
            );
        }
        Spray {
            pos,
            vel: Vec3::new(0.0, -0.2, 0.0),
            fuzziness: 0.4,
            spread: std::f64::consts::PI * 0.5,
            count: 3,
        }
        .spawn(srv, dim, &get_pigment(srv, uuid));
        true
    });
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
