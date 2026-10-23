//! Flight: who flies, how far or how long, and the danger of running out.

use super::non_zero;
use crate::casting::args::Player;
use crate::casting::env::{CastEnv, Spray, get_pigment, random};
use crate::casting::mishap::{MResult, dye};
use crate::casting::vm::Spell;
use crate::item::media::DUST;
use crate::item::pigment::dye_pigment;
use crate::math::Vec3;
use std::collections::HashMap;
use std::f64::consts::PI;
use yog_api::sync::Mutex;
use yog_api::yog_ids::gamemode;
use yog_api::yog_ids::time::TICKS_PER_SECOND;

// ── flight ─────────────────────────────────────────────────────────────────

/// `FlightAbility`: what Anchorite's / Wayfarer's Flight granted a player.
#[derive(Debug, Clone, PartialEq, yog_api::Schema)]
pub struct FlightAbility {
    /// Ticks left, or -1 for no limit.
    pub time_left: i64,
    pub dim: String,
    pub origin: Vec3,
    /// Blocks from `origin` (in X/Z), or -1 for no limit.
    pub radius: f64,
}

/// Loaded flights, by player uuid (`None`: known not to fly).
pub(super) static FLIGHTS: Mutex<Option<HashMap<String, Option<FlightAbility>>>> = Mutex::new(None);

/// Each player's flight, kept with the world.
pub(super) static FLIGHT: yog_api::storage::PlayerData<Option<FlightAbility>> =
    yog_api::storage::PlayerData::new("flight");

pub(super) fn load_flight(uuid: &str) -> Option<FlightAbility> {
    FLIGHT.with(uuid, Clone::clone)
}

pub(super) fn store_flight(uuid: &str, f: Option<&FlightAbility>) {
    FLIGHT.with_mut(uuid, |kept| *kept = f.cloned());
}

/// The flight a player was granted, if any.
pub fn flight_of(uuid: &str) -> Option<FlightAbility> {
    let mut map = FLIGHTS.lock();
    let map = map.get_or_insert_with(HashMap::new);
    map.entry(uuid.to_owned())
        .or_insert_with(|| load_flight(uuid))
        .clone()
}

/// `setFlight`; `persist` also writes it to disk.
pub(super) fn set_flight(uuid: &str, f: Option<FlightAbility>, persist: bool) {
    if persist {
        store_flight(uuid, f.as_ref());
    }
    let mut map = FLIGHTS.lock();
    map.get_or_insert_with(HashMap::new)
        .insert(uuid.to_owned(), f);
}

/// `OpFlight`.
pub(super) fn flight(env: &mut CastEnv, p: Player, arg: f64, range: bool) -> MResult<Spell> {
    p.in_range(env)?;
    let unit = 2 * DUST;
    let mut cost = non_zero(arg * unit as f64);
    if range {
        cost = cost.max(unit);
    }
    let spray = Spray {
        pos: p.pos(),
        ..FLY
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
                time_left: (arg * f64::from(TICKS_PER_SECOND)).round() as i64,
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
pub(super) const DIST_DANGER_THRESHOLD: f64 = 4.0;
pub(super) const TIME_DANGER_THRESHOLD: f64 = 7.0 * 20.0;

/// `getDanger`: 0 (fine) to 1 (the flight ends).
pub(super) fn danger(dim: &str, pos: Vec3, f: &FlightAbility) -> f64 {
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
            land(&name, &p, dim, pos);
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
        let persist = time_left >= 0 && time_left % i64::from(TICKS_PER_SECOND) == 0;
        let next = FlightAbility {
            time_left,
            ..f.clone()
        };
        set_flight(&p.uuid, Some(next), persist);
        trail(&p.uuid, dim, pos, d);
        if f.radius >= 0.0 {
            let color = get_pigment(&p.uuid);
            let origin = f.origin + Vec3::new(0.0, 1.0, 0.0);
            Spray {
                pos: origin,
                ..ORIGIN_UP
            }
            .spawn(dim, &color);
            Spray {
                pos: origin,
                ..ORIGIN_DOWN
            }
            .spawn(dim, &color);
        }
    }
}

/// The flight ran out: `name` (`p`, at `pos`) comes down.
fn land(name: &str, p: &yog_api::core::PlayerInfo, dim: &str, pos: Vec3) {
    set_flight(&p.uuid, None, true);
    if p.gamemode != gamemode::CREATIVE && p.gamemode != gamemode::SPECTATOR {
        yog_api::event::player::set_may_fly(name, false);
    }
    let finish = crate::res::SoundEvents::FLIGHT_FINISH;
    yog_api::event::sound::play(dim, (pos.x, pos.y, pos.z), finish, FINISH_VOLUME, 1.0);
    let spray = Spray { pos, ..LANDED };
    spray.spawn(dim, &dye_pigment(dye::RED));
    spray.spawn(dim, &dye_pigment(dye::BLACK));
}

/// A flyer's trail: their pigment's, red and black the more as the
/// danger `d` of running out grows; now and then the wind heard.
fn trail(uuid: &str, dim: &str, pos: Vec3, d: f64) {
    let danger_count = (f64::from(TRAIL.count) * d).round() as u32;
    let color = get_pigment(uuid);
    let count = TRAIL.count - danger_count;
    Spray {
        pos,
        count,
        ..TRAIL
    }
    .spawn(dim, &color);
    let danger = Spray {
        pos,
        count: danger_count.div_ceil(2),
        ..DANGER
    };
    danger.spawn(dim, &dye_pigment(dye::BLACK));
    danger.spawn(dim, &dye_pigment(dye::RED));
    if random() < AMBIENCE_CHANCE {
        let ambience = crate::res::SoundEvents::FLIGHT_AMBIENCE;
        yog_api::event::sound::play(dim, (pos.x, pos.y, pos.z), ambience, AMBIENCE_VOLUME, 1.0);
    }
}

/// How the sprays of flight look (each placed where it goes): the start,
/// running out, the trail and its danger, the range's middle up and down.
const FLY: Spray = Spray::look((0.0, 2.0, 0.0), 0.0, 0.1, 20);
const LANDED: Spray = Spray::look((0.0, 1.0, 0.0), PI, 0.4, 20);
const TRAIL: Spray = Spray::look((0.0, -0.6, 0.0), 0.6, PI * 0.3, 5);
const DANGER: Spray = Spray::look((0.0, 1.0, 0.0), 0.3, PI * 0.75, 0);
const ORIGIN_UP: Spray = Spray::look((0.0, 1.0, 0.0), 0.5, PI * 0.1, 5);
const ORIGIN_DOWN: Spray = Spray::look((0.0, -1.0, 0.0), 1.5, PI * 0.25, 5);
/// How loud running out is, how often (a chance a tick) and how loud the
/// wind is.
pub const FINISH_VOLUME: f32 = 2.0;
pub const AMBIENCE_CHANCE: f64 = 0.02;
pub const AMBIENCE_VOLUME: f32 = 0.2;
