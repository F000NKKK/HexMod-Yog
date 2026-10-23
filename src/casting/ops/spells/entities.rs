//! Spells on entities: potions and sentinels.

use crate::casting::env::get_sentinel;

use super::non_zero;
use crate::casting::args::{Living, double_between};
use crate::casting::env::{CastEnv, Sentinel, Spray, set_sentinel};
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::Spell;
use crate::iota::Iota;
use crate::item::media::DUST;
use crate::math::Vec3;
use yog_api::yog_ids::time::TICKS_PER_SECOND;

// ── entities ───────────────────────────────────────────────────────────────

/// `OpPotionEffect`.
/// A potion spell: the effect it gives, what a second of it costs at
/// potency 1, and whether its cost grows with potency cubed (else squared).
#[derive(Clone, Copy)]
pub struct Potion {
    pub effect: &'static str,
    pub base: i64,
    pub cubic: bool,
}

/// The strongest potency asked for (an effect's highest amplifier + 1).
const MAX_POTENCY: f64 = 127.0;

pub fn potion(
    env: &mut CastEnv,
    e: Living,
    duration: f64,
    potency: Option<&Iota>,
    kind: Potion,
) -> MResult<Spell> {
    let potency = match potency {
        Some(p) => double_between(p, 0, 1.0, MAX_POTENCY)?,
        None => 1.0,
    };
    e.in_range(env)?;
    let scale = if kind.cubic {
        potency * potency * potency
    } else {
        potency * potency
    };
    let cost = kind.base as f64 * duration * scale;
    let effect = kind.effect;
    let spray = Spray::cloud(e.pos() + Vec3::new(0.0, e.info.eye_height / 2.0, 0.0), 1.0);
    let uuid = e.uuid().to_owned();
    Ok(Spell::new(non_zero(cost), vec![spray], move |_env, _| {
        let ticks = duration * f64::from(TICKS_PER_SECOND);
        if ticks >= 1.0 {
            let amplifier = (potency as i64 - 1).clamp(0, i64::from(u8::MAX)) as u8;
            yog_api::event::entity::add_effect(&uuid, effect, ticks as i32, amplifier, true);
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

/// The cloud a destroyed sentinel goes in.
const SENTINEL_CLOUD: f64 = 2.0;

/// `OpDestroySentinel`: the caster's sentinel goes (it must be in this dimension).
pub(super) fn destroy_sentinel(env: &mut CastEnv) -> MResult<Spell> {
    let uuid = env.caster_uuid().ok_or(Mishap::BadCaster)?;
    let mut particles = Vec::new();
    if let Some(s) = get_sentinel(&uuid) {
        if s.dim != env.dim {
            return Err(Mishap::WrongDimension(s.dim.clone()));
        }
        particles.push(Spray::cloud(s.pos, SENTINEL_CLOUD));
    }
    Ok(Spell::new(DUST / 10, particles, move |_, _| {
        set_sentinel(&uuid, None)
    }))
}
