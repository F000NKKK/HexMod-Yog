//! Spells that move things: item entities' stacks, teleports, explosions.

use super::MAX_MOTION;
use crate::casting::image::Image;

use super::non_zero;
use crate::casting::args::Ent;
use crate::casting::env::{CastEnv, DIM_TP_DENYLIST, Spray, Stack, look_vec};
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::Spell;
use crate::client::text::Text;
use crate::encoding::snbt::Nbt;
use crate::iota::Iota;
use crate::item::media;
use crate::item::media::{DUST, SHARD};
use crate::math::Vec3;

// ── item entities ──────────────────────────────────────────────────────────

/// The stack in a dropped item entity.
pub(super) fn item_entity_stack(uuid: &str) -> Option<Stack> {
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
pub(super) fn set_item_entity_stack(uuid: &str, stack: &Stack) {
    let count = if stack.is_empty() { 0 } else { stack.count };
    yog_api::event::item::set_entity_stack(uuid, &stack.id, count, stack.nbt.to_snbt());
}

/// `isMediaItem(e) && extractMedia(e, drainForBatteries = true) > 0`, or
/// `MishapBadItem("media_for_battery")`.
pub(super) fn assert_battery_media(env: &CastEnv, e: &Ent) -> MResult<()> {
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
pub(super) fn blink(env: &mut CastEnv, e: Ent, delta: f64) -> MResult<Spell> {
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

/// The strongest explosion cast, its particles, and its cost in dust:
/// a strength's, and the least without fire (one with).
const MAX_STRENGTH: f64 = 10.0;
const EXPLODE_PARTICLES: u32 = 50;
const EXPLODE_COST: (f64, f64) = (3.0, 0.125);
/// How near an entity's eyes an explosion is looked for, and how far up it
/// moves when it's there (one exactly at the eyes doesn't hurt).
const EYE_REACH: f64 = 0.01;
const EYE_NUDGE: f64 = 0.000_001;

/// `OpExplode`.
pub(super) fn explode(
    env: &mut CastEnv,
    mut pos: Vec3,
    strength: &Iota,
    fire: bool,
) -> MResult<Spell> {
    let strength = match strength {
        Iota::Double(d) if (0.0..=MAX_STRENGTH).contains(d) => *d,
        other => {
            return Err(Mishap::invalid(
                other,
                0,
                "double.positive.less.equal",
                vec![Text::num(MAX_STRENGTH)],
            ));
        }
    };
    env.assert_vec_in_range(pos)?;
    // Explosions exactly at an entity's eyes don't hurt it.
    let eps = Vec3::splat(EYE_REACH);
    let too_close =
        yog_api::event::world::entities_in_box(&env.dim, (pos - eps).tuple(), (pos + eps).tuple())
            .iter()
            .filter_map(yog_api::event::entity::info)
            .any(|i| Vec3::from_tuple(i.eye_pos()).distance_sqr(pos) == 0.0);
    if too_close {
        pos = pos + Vec3::new(0.0, EYE_NUDGE, 0.0);
    }
    let (per_strength, base) = EXPLODE_COST;
    let cost = DUST as f64 * (per_strength * strength + if fire { 1.0 } else { base });
    Ok(Spell::new(
        non_zero(cost),
        vec![Spray::burst_n(pos, strength, EXPLODE_PARTICLES)],
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

/// How the spray of a push spreads, and how many.
const PUSH_SPREAD: f64 = 0.1;
const PUSH_PARTICLES: u32 = 20;

/// `OpAddMotion`: pushes `e` by `motion` (at most [`MAX_MOTION`] long);
/// pushing what was pushed already this cast costs more.
pub(super) fn add_motion(
    env: &mut CastEnv,
    img: &mut Image,
    e: Ent,
    motion: Vec3,
) -> MResult<Spell> {
    e.in_range(env)?;
    let motion = if motion.length_sqr() > MAX_MOTION * MAX_MOTION {
        motion.normalize() * MAX_MOTION
    } else {
        motion
    };
    let mut for_cost = motion.length_sqr();
    if img.check_and_mark_moved(e.uuid()) {
        for_cost += 1.0;
    }
    let spray = Spray {
        pos: e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0),
        vel: motion.normalize(),
        fuzziness: 0.0,
        spread: PUSH_SPREAD,
        count: PUSH_PARTICLES,
    };
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(
        non_zero(for_cost * DUST as f64),
        vec![spray],
        move |_, _| {
            yog_api::event::entity::add_velocity(&uuid, (motion.x, motion.y, motion.z));
        },
    ))
}
