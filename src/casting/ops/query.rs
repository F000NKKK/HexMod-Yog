//! Looking at the world: the caster, entities, raycasts, media, spell
//! circles, sentinels (`casting/actions/{selectors,queryentity,raycast,
//! circles,environment,types}`, `OpGetSentinel*`).

use std::collections::HashMap;
use std::sync::Mutex;

use yog_api::Server;
use yog_api::core::{EntityInfo, entity_flags as ef};

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Block, Ent, Player, PosDouble, block_center};
use crate::casting::env::{CastEnv, Source, Stack, get_sentinel, look_vec, to_ypos};
use crate::casting::mishap::{MResult, Mishap};
use crate::iota::{EntityRef, Iota};
use crate::item::media::DUST;
use crate::math::Vec3;

/// `Action.RAYCAST_DISTANCE`.
const RAYCAST_DISTANCE: f64 = 32.0;

pub fn actions() -> Vec<Action> {
    actions! {
        "get_caster" NE "qaq" => konst [env] {
            match env.caster_info() {
                None => Iota::Null,
                Some(info) => {
                    let r = EntityRef { uuid: env.caster_uuid().unwrap_or_default(), name: info.name.clone() };
                    env.assert_entity_in_range(&r, &info)?;
                    Iota::Entity(r)
                }
            }
        };
        "entity_pos/eye" E "aa" => konst [env, e: Ent] { e.in_range(env)?; e.eye() };
        "entity_pos/foot" NE "dd" => konst [env, e: Ent] { e.in_range(env)?; e.pos() };
        "get_entity_look" E "wa" => konst [env, e: Ent] { e.in_range(env)?; look_vec(env.srv, e.uuid()) };
        "get_entity_height" NE "awq" => konst [env, e: Ent] { e.in_range(env)?; e.info.height };
        "get_entity_velocity" E "wq" => konst [env, e: Ent] { e.in_range(env)?; velocity(env.srv, &e.info, e.uuid()) };
        "raycast" E "wqaawdd" => konst(DUST / 100) [env, origin: Vec3, look: Vec3] {
            env.assert_vec_in_range(origin)?;
            block_hit(env, origin, look).map(|(pos, _)| block_center(pos))
        };
        "raycast/axis" E "weddwaa" => konst(DUST / 100) [env, origin: Vec3, look: Vec3] {
            env.assert_vec_in_range(origin)?;
            block_hit(env, origin, look).map(|(_, n)| Vec3::new(n.0 as f64, n.1 as f64, n.2 as f64))
        };
        "raycast/entity" E "weaqa" => konst(DUST / 100) [env, origin: Vec3, look: Vec3] {
            env.assert_vec_in_range(origin)?;
            let end = raycast_end(origin, look);
            let exclude = env.caster_uuid().unwrap_or_default();
            env.srv
                .raycast_entity(&env.dim, origin.tuple(), end.tuple(), &exclude)
                .and_then(|u| Some((env.srv.entity_info(&u)?, u)))
                .filter(|(info, _)| env.is_entity_in_range(info))
                .map(|(info, uuid)| Iota::Entity(EntityRef { uuid, name: info.name }))
        };
        "get_media" W "dde" => konst [env] {
            let raw = i64::MAX - env.extract_media(i64::MAX, true);
            raw as f64 / DUST as f64
        };
        "circle/impetus_pos" SW "eaqwqae" => konst [env] { block_center(circle(env)?.impetus) };
        "circle/impetus_dir" SW "eaqwqaewede" => konst [env] {
            let f = circle(env)?.facing;
            Vec3::new(f.0 as f64, f.1 as f64, f.2 as f64)
        };
        "circle/bounds/min" SW "eaqwqaewdd" => konst [env] {
            let m = circle(env)?.min;
            Vec3::new(m.0 as f64 + 0.5, m.1 as f64 + 0.5, m.2 as f64 + 0.5)
        };
        "circle/bounds/max" W "aqwqawaaqa" => konst [env] {
            // The bounds box ends past the max block.
            let m = circle(env)?.max;
            Vec3::new(m.0 as f64 + 0.5, m.1 as f64 + 0.5, m.2 as f64 + 0.5)
        };

        "get_entity" SE "qqqqqdaqa" => konst [env, pos: Vec3] { entity_at(env, pos, |_| true) };
        "get_entity/animal" SE "qqqqqdaqaawa" => konst [env, pos: Vec3] { entity_at(env, pos, is_animal) };
        "get_entity/monster" SE "qqqqqdaqaawq" => konst [env, pos: Vec3] { entity_at(env, pos, is_monster) };
        "get_entity/item" SE "qqqqqdaqaaww" => konst [env, pos: Vec3] { entity_at(env, pos, is_item) };
        "get_entity/player" SE "qqqqqdaqaawe" => konst [env, pos: Vec3] { entity_at(env, pos, is_player) };
        "get_entity/living" SE "qqqqqdaqaawd" => konst [env, pos: Vec3] { entity_at(env, pos, is_living) };
        "zone_entity" SE "qqqqqwded" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, |_| true, false) };
        "zone_entity/animal" SE "qqqqqwdeddwa" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_animal, false) };
        "zone_entity/not_animal" NE "eeeeewaqaawa" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_animal, true) };
        "zone_entity/monster" SE "qqqqqwdeddwq" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_monster, false) };
        "zone_entity/not_monster" NE "eeeeewaqaawq" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_monster, true) };
        "zone_entity/item" SE "qqqqqwdeddww" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_item, false) };
        "zone_entity/not_item" NE "eeeeewaqaaww" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_item, true) };
        "zone_entity/player" SE "qqqqqwdeddwe" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_player, false) };
        "zone_entity/not_player" NE "eeeeewaqaawe" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_player, true) };
        "zone_entity/living" SE "qqqqqwdeddwd" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_living, false) };
        "zone_entity/not_living" NE "eeeeewaqaawd" => konst [env, pos: Vec3, r: PosDouble] { zone(env, pos, r.0, is_living, true) };

        "compare_block/lenient" NW "qqqqqeqeeeee" => konst [env, a: Block, b: Block] {
            env.assert_pos_in_range(a.0)?;
            env.assert_pos_in_range(b.0)?;
            env.block_id(a.0) == env.block_id(b.0)
        };
        "compare_block/strict" NW "qwawqwadadwewdwe" => konst [env, a: Block, b: Block] {
            env.assert_pos_in_range(a.0)?;
            env.assert_pos_in_range(b.0)?;
            let state = |p| env.srv.block_info(&env.dim, to_ypos(p)).map(|b| b.state);
            state(a.0) == state(b.0)
        };
        "compare_entity" NW "aqaeqded" => konst [env, a: Ent, b: Ent] {
            a.in_range(env)?;
            b.in_range(env)?;
            a.info.type_id == b.info.type_id
        };
        "compare_item/lenient" NW "qaeaqeqedqde" => konst [env, a: Ent, b: Ent] {
            let (x, y) = (held_item(env, &a, 1)?, held_item(env, &b, 0)?);
            x.id == y.id
        };
        "compare_item/strict" NW "qaeaqewqedqde" => konst [env, a: Ent, b: Ent] {
            let (x, y) = (held_item(env, &a, 1)?, held_item(env, &b, 0)?);
            x.id == y.id && x.nbt == y.nbt
        };

        "sentinel/get_pos" E "waeawaede" => konst(DUST / 10) [env] {
            sentinel(env)?
        };
        "sentinel/wayfind" E "waeawaedwa" => konst(DUST / 10) [env, from: Vec3] {
            sentinel(env)?.map(|s| (s - from).normalize())
        };
        "flight/can_fly" NE "dwdwdeweaqa" => konst [env, p: Player] {
            p.in_range(env)?;
            super::spells::flight_of(env.srv, p.uuid()).is_some()
        };
    }
}

fn raycast_end(origin: Vec3, look: Vec3) -> Vec3 {
    origin + look.normalize() * RAYCAST_DISTANCE
}

type BlockPos = (i32, i32, i32);

/// The block hit (and its face's normal), if it's in range.
fn block_hit(env: &CastEnv, origin: Vec3, look: Vec3) -> Option<(BlockPos, BlockPos)> {
    let hit = env
        .srv
        .raycast_block(&env.dim, origin.tuple(), raycast_end(origin, look).tuple())?;
    let pos = (hit.pos.x, hit.pos.y, hit.pos.z);
    env.is_vec_in_range(Vec3::center_of(pos))
        .then(|| (pos, hit.face.normal()))
}

fn circle(env: &CastEnv) -> MResult<crate::casting::env::CircleRun> {
    match &env.source {
        Source::Circle(c) => Ok(c.clone()),
        _ => Err(Mishap::NoSpellCircle),
    }
}

/// `OpGetSentinelPos`: `None` without a sentinel.
fn sentinel(env: &CastEnv) -> MResult<Option<Vec3>> {
    let uuid = env.caster_uuid().ok_or(Mishap::BadCaster)?;
    match get_sentinel(env.srv, &uuid) {
        None => Ok(None),
        Some(s) if s.dim != env.dim => Err(Mishap::WrongDimension(s.dim)),
        Some(s) => Ok(Some(s.pos)),
    }
}

fn is_animal(i: &EntityInfo) -> bool {
    i.has(ef::ANIMAL)
}

fn is_monster(i: &EntityInfo) -> bool {
    i.has(ef::MONSTER)
}

fn is_item(i: &EntityInfo) -> bool {
    i.has(ef::ITEM)
}

fn is_player(i: &EntityInfo) -> bool {
    i.has(ef::PLAYER)
}

fn is_living(i: &EntityInfo) -> bool {
    !i.has(ef::ARMOR_STAND) && (i.has(ef::LIVING) || i.has(ef::PART))
}

/// `OpGetEntitiesBy.isReasonablySelectable` (no true-name ambit here).
fn selectable(env: &CastEnv, i: &EntityInfo) -> bool {
    let p = Vec3::from_tuple(i.pos);
    i.dimension == env.dim
        && env.is_vec_in_world(p)
        && env.is_vec_in_range(p)
        && i.has(ef::ALIVE)
        && !i.has(ef::SPECTATOR)
}

/// The selectable entities touching the box around `pos`, nearest first.
fn around(env: &CastEnv, pos: Vec3, r: f64, ok: impl Fn(&EntityInfo) -> bool) -> Vec<(f64, Iota)> {
    let d = Vec3::splat(r);
    let mut got: Vec<(f64, Iota)> = env
        .srv
        .entities_in_box(&env.dim, (pos - d).tuple(), (pos + d).tuple())
        .into_iter()
        .filter_map(|uuid| {
            let info = env.srv.entity_info(&uuid)?;
            (selectable(env, &info) && ok(&info)).then(|| {
                let dist = Vec3::from_tuple(info.pos).distance_sqr(pos);
                (
                    dist,
                    Iota::Entity(EntityRef {
                        uuid,
                        name: info.name,
                    }),
                )
            })
        })
        .collect();
    got.sort_by(|a, b| a.0.total_cmp(&b.0));
    got
}

/// `OpGetEntityAt`.
fn entity_at(env: &CastEnv, pos: Vec3, ok: fn(&EntityInfo) -> bool) -> MResult<Option<Iota>> {
    env.assert_vec_in_range(pos)?;
    Ok(around(env, pos, 0.5, ok).into_iter().next().map(|(_, i)| i))
}

/// `OpGetEntitiesBy`.
fn zone(
    env: &CastEnv,
    pos: Vec3,
    r: f64,
    ok: fn(&EntityInfo) -> bool,
    negate: bool,
) -> MResult<Iota> {
    env.assert_vec_in_range(pos)?;
    let got = around(env, pos, r, |i| ok(i) != negate);
    Ok(Iota::List(
        got.into_iter()
            .filter(|(d, _)| *d <= r * r)
            .map(|(_, i)| i)
            .collect(),
    ))
}

/// `HexItemHolderHandlers.applyHandlerFor`: the item an entity shows.
fn held_item(env: &CastEnv, e: &Ent, rev: usize) -> MResult<Stack> {
    e.in_range(env)?;
    let stack = if e.is(ef::PLAYER) {
        let p = env.srv.player_info(&e.info.name);
        p.map(|p| {
            let main = Stack::read(env.srv, &e.info.name, p.selected_slot);
            if main.is_empty() {
                Stack::read(env.srv, &e.info.name, crate::casting::env::SLOT_OFFHAND)
            } else {
                main
            }
        })
    } else {
        crate::item::holder::entity_stack(env.srv, e.uuid(), &e.info.type_id).map(|(s, _)| s)
    };
    stack
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Mishap::of_type(&e.iota(), rev, "entity.item_holder"))
}

// ── velocity (`HexAPI.getEntityVelocitySpecial`) ───────────────────────────

/// `PlayerPositionRecorder`: a player's last two positions.
/// Last and previous-tick position per player.
type Positions = HashMap<String, (Vec3, Option<Vec3>)>;
static PLAYER_POSITIONS: Mutex<Option<Positions>> = Mutex::new(None);

/// `PlayerPositionRecorder.updateAllPlayers`, every server tick.
pub fn record_players(srv: &dyn Server) {
    let mut map = PLAYER_POSITIONS.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    let mut seen = HashMap::new();
    for name in srv.online_players() {
        let Some(p) = srv.player_info(&name) else {
            continue;
        };
        let Some(pos) = srv.entity_position(&p.uuid) else {
            continue;
        };
        let prev = map.get(&p.uuid).map(|(last, _)| *last);
        seen.insert(p.uuid, (Vec3::from_tuple(pos), prev));
    }
    *map = seen;
}

fn velocity(srv: &dyn Server, info: &EntityInfo, uuid: &str) -> Vec3 {
    if info.has(ef::PLAYER) {
        let map = PLAYER_POSITIONS.lock().unwrap_or_else(|e| e.into_inner());
        return match map.as_ref().and_then(|m| m.get(uuid)) {
            None => Vec3::ZERO,
            Some((last, None)) => Vec3::from_tuple(info.pos) - *last,
            Some((last, Some(prev))) => *last - *prev,
        };
    }
    let v = srv
        .entity_velocity(uuid)
        .map(Vec3::from_tuple)
        .unwrap_or_default();
    let nbt = || {
        srv.entity_get_nbt(uuid)
            .map(|s| crate::snbt::Nbt::parse(&s))
            .unwrap_or_default()
    };
    match info.type_id.as_str() {
        // Resting items still get gravity added every tick.
        "minecraft:item" if v.x * v.x + v.z * v.z <= 1.0e-5 && nbt().get_bool("OnGround") => {
            Vec3::ZERO
        }
        "minecraft:arrow" | "minecraft:spectral_arrow" | "minecraft:trident"
            if nbt().get_bool("inGround") =>
        {
            Vec3::ZERO
        }
        _ => v,
    }
}
