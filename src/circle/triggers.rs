//! What sets an impetus off, each tick, and the slates' patterns.

use super::impetus::{
    KNOWN, KeptSlate, Known, SLATES_KEY, TICKS, forget_impetus, impetus, save_impetus, save_known,
    store,
};
use super::run::{start, tick_run};
use super::{LOOK_RANGE, MAX_LOOK, SOUND_LOOK_TICK};
use crate::casting::env::{Caster, from_ypos, to_ypos};
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::math::{HexPattern, Vec3};
use yog_api::vanilla;

/// Who's looked for: within this many times the look range squared.
const NEAR_SQUARES: f64 = 3.0;
/// The gaze reaches the look range over this.
const REACH_DIV: f64 = 1.5;
/// Every this many steps of filling, a tick is heard, rising in pitch
/// and volume from the first to the second of each.
const TICK_SOUND_EVERY: i32 = 5;
const TICK_PITCH: (f32, f32) = (0.5, 1.2);
const TICK_VOLUME: (f32, f32) = (0.2, 1.2);

/// `BlockEntityLookingImpetus.serverTick`: someone staring at it fills it.
pub(super) fn tick_look(dim: &str, pos: BlockPos) {
    if yog_api::event::block::property(dim, to_ypos(pos), "energized").as_deref() == Some("true") {
        return;
    }
    let mut looker = None;
    for name in yog_api::event::player::online() {
        let Some(info) = yog_api::event::player::info(&name)
            .and_then(|p| Some((p.uuid.clone(), yog_api::event::entity::info(&p.uuid)?)))
        else {
            continue;
        };
        let (uuid, e) = info;
        let eye = Vec3::new(e.pos.x, e.pos.y + e.eye_height, e.pos.z);
        if e.dimension != dim
            || eye.distance_sqr(Vec3::center_of(pos)) > NEAR_SQUARES * LOOK_RANGE * LOOK_RANGE
        {
            continue;
        }
        // A carved pumpkin on the head hides the gaze.
        if crate::casting::env::Stack::read(&name, crate::casting::env::SLOT_HEAD).id
            == vanilla::items::CARVED_PUMPKIN
        {
            continue;
        }
        let end = eye + crate::casting::env::look_vec(&uuid) * (LOOK_RANGE / REACH_DIV);
        if yog_api::event::world::raycast_block(dim, eye.tuple(), end.tuple())
            .is_some_and(|h| from_ypos(h.pos) == pos)
        {
            looker = Caster::by_name(&name);
            break;
        }
    }
    let mut imp = impetus(dim, pos);
    let prev = imp.look;
    let new = (prev + if looker.is_some() { 1 } else { -1 }).clamp(0, MAX_LOOK);
    if new == prev {
        return;
    }
    if new == MAX_LOOK {
        imp.look = 0;
        save_impetus(dim, pos, &imp);
        start(dim, pos, looker);
    } else {
        if new % TICK_SOUND_EVERY == 1 {
            let t = new as f32 / MAX_LOOK as f32;
            let lerp = |(a, b): (f32, f32)| a + (b - a) * t;
            let (pitch, volume) = (lerp(TICK_PITCH), lerp(TICK_VOLUME));
            let c = Vec3::center_of(pos);
            yog_api::event::sound::play(dim, (c.x, c.y, c.z), SOUND_LOOK_TICK, volume, pitch);
        }
        imp.look = new;
        save_impetus(dim, pos, &imp);
    }
}

pub fn tick() {
    let now = {
        let mut t = TICKS.lock();
        *t += 1;
        *t
    };
    let known: Vec<((String, BlockPos), Known)> = KNOWN
        .lock()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut changed = false;
    for ((dim, pos), k) in known {
        // Gone some way a break didn't see (an explosion, a piston).
        if now.is_multiple_of(20)
            && yog_api::event::block::info(&dim, to_ypos(pos)).is_some_and(|b| b.block_id() != k.id)
        {
            KNOWN.lock().remove(&(dim.clone(), pos));
            forget_impetus(&dim, pos);
            changed = true;
            continue;
        }
        if k.due.is_some_and(|d| now >= d) {
            let due = tick_run(&dim, pos, now);
            if let Some(e) = KNOWN.lock().get_mut(&(dim.clone(), pos)) {
                e.due = due;
            }
        }
        match k.id.as_str() {
            Blocks::IMPETUS_LOOK => tick_look(&dim, pos),
            // `BlockRedstoneImpetus.neighborChanged`: power coming on starts it.
            Blocks::IMPETUS_REDSTONE => {
                let powered = yog_api::event::block::property(&dim, to_ypos(pos), "powered")
                    .as_deref()
                    == Some("true");
                if powered != k.powered {
                    if let Some(e) = KNOWN.lock().get_mut(&(dim.clone(), pos)) {
                        e.powered = powered;
                    }
                    if powered {
                        let player = impetus(&dim, pos)
                            .player
                            .and_then(|(_, name)| Caster::by_name(&name));
                        start(&dim, pos, player);
                    }
                }
            }
            _ => {}
        }
    }
    if changed {
        save_known();
    }
}

// ── slates ──────────────────────────────────────────────────────────────────

/// `BlockEntitySlate.pattern`.
pub fn slate_pattern(dim: &str, pos: BlockPos) -> Option<HexPattern> {
    let slates: Vec<KeptSlate> = store(dim, pos)?.get_or_default(SLATES_KEY);
    let slate = slates.into_iter().find(|s| s.pos == pos)?;
    HexPattern::from_serialized(&slate.pattern)
}

pub(super) fn set_slate_pattern(dim: &str, pos: BlockPos, pattern: Option<&HexPattern>) {
    let Some(s) = store(dim, pos) else {
        return;
    };
    s.update(SLATES_KEY, |slates: &mut Vec<KeptSlate>| {
        slates.retain(|s| s.pos != pos);
        if let Some(p) = pattern {
            slates.push(KeptSlate {
                pos,
                pattern: p.serialized_form(),
            });
        }
    });
}

/// The slate item, written with `pattern` if it has one.
pub(super) fn slate_item_nbt(pattern: Option<&HexPattern>) -> String {
    match pattern {
        Some(p) => format!("{{BlockEntityTag:{{pattern:\"{}\"}}}}", p.serialized_form()),
        None => String::new(),
    }
}
