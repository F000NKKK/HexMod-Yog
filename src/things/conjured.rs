//! Conjured blocks and lights (`BlockConjured`, `BlockEntityConjured`): they
//! sparkle in the pigment of whoever conjured them — now and then on their
//! own, and under the feet of whatever walks on a block.
//!
//! The pigment is kept per position in the chunk's mod storage; the
//! sparkles are colored dust, sent from the server.

use std::collections::HashMap;
use yog_api::sync::{LazyLock, Mutex};

use yog_api::{EventPhase, Registry};

use crate::casting::env::{random, spawn_dust, to_ypos};
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::item::pigment::Pigment;
use crate::math::Vec3;

/// Sparkles show this close to a player.
const RANGE: f64 = 32.0;
const KEY: &str = "conjured.index";

/// A conjured block by its dimension and position: whether it's a light,
/// and its pigment.
type Known = HashMap<(String, BlockPos), (bool, Pigment)>;

static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Default::default);
static TICKS: Mutex<u64> = Mutex::new(0);

/// A conjured block, as it's kept with the world.
#[derive(yog_api::Schema)]
struct Kept {
    dim: String,
    pos: BlockPos,
    light: bool,
    pigment: Pigment,
}

fn save() {
    let known: Vec<Kept> = KNOWN
        .lock()
        .iter()
        .map(|((dim, pos), (light, pigment))| Kept {
            dim: dim.clone(),
            pos: *pos,
            light: *light,
            pigment: pigment.clone(),
        })
        .collect();
    if let Some(store) = yog_api::storage::world() {
        store.set(KEY, &known);
    }
}

fn load() {
    let kept: Vec<Kept> = yog_api::storage::world()
        .map(|s| s.get_or_default(KEY))
        .unwrap_or_default();
    *KNOWN.lock() = kept
        .into_iter()
        .map(|k| ((k.dim, k.pos), (k.light, k.pigment)))
        .collect();
}

/// `BlockConjured.setColor`: a block was conjured with `pigment`.
pub fn conjured(dim: &str, pos: BlockPos, light: bool, pigment: Pigment) {
    KNOWN.lock().insert((dim.to_owned(), pos), (light, pigment));
    save();
}

fn color(p: &Pigment, time: f32, near: Vec3) -> u32 {
    let jitter = Vec3::new(random(), random(), random()) * (random() * COLOR_JITTER);
    p.color(time, near + jitter)
}

/// `random()` spread evenly over `-spread..spread`.
fn jitter(spread: f64) -> f64 {
    (random() * 2.0 - 1.0) * spread
}

/// How far a sparkle's place strays for its color, and the time its
/// color's taken at (a random point of the pigment's cycle, ticks).
const COLOR_JITTER: f64 = 3.0;
const COLOR_TIME: f64 = 16384.0;
/// A conjured light's sparkles: how often, how far from its middle, their
/// drift aside and up (least, most).
const LIGHT_CHANCE: f64 = 0.5;
const LIGHT_SPOT: f64 = 0.05;
const LIGHT_DRIFT: f64 = 0.005;
const LIGHT_RISE: (f64, f64) = (-0.002, 0.02);
/// A conjured block's sparkles: how often, their drift.
const BLOCK_CHANCE: f64 = 0.2;
const BLOCK_DRIFT: f64 = 0.02;
/// Stepping on a conjured block: the block under the feet (this far
/// below them), the sparks a step, how far round the feet, how high on
/// the block (from, plus up to), their drift.
const FEET_DEPTH: f64 = 0.01;
const STEP_SPARKS: u32 = 3;
const STEP_SPREAD: f64 = 0.3;
const STEP_HEIGHT: (f64, f64) = (0.95, 0.05);
const STEP_DRIFT: f64 = 0.02;

/// `particleEffect` (each tick, the client's `animateTick`).
fn sparkle(dim: &str, pos: BlockPos, light: bool, p: &Pigment) {
    let (x, y, z) = (pos.0 as f64, pos.1 as f64, pos.2 as f64);
    let c = color(p, (random() * COLOR_TIME) as f32, Vec3::default());
    if light {
        if random() < LIGHT_CHANCE {
            let at = Vec3::new(
                x + 0.5 + jitter(LIGHT_SPOT),
                y + 0.5 + jitter(LIGHT_SPOT),
                z + 0.5 + jitter(LIGHT_SPOT),
            );
            let (rise_low, rise_high) = LIGHT_RISE;
            let vel = Vec3::new(
                jitter(LIGHT_DRIFT),
                rise_low + random() * (rise_high - rise_low),
                jitter(LIGHT_DRIFT),
            );
            spawn_dust(dim, at, vel, c);
        }
    } else if random() < BLOCK_CHANCE {
        let at = Vec3::new(x + random(), y + random(), z + random());
        let vel = Vec3::new(
            jitter(BLOCK_DRIFT),
            jitter(BLOCK_DRIFT),
            jitter(BLOCK_DRIFT),
        );
        spawn_dust(dim, at, vel, c);
    }
}

fn tick() {
    let now = {
        let mut t = TICKS.lock();
        *t += 1;
        *t
    };
    let players: Vec<(String, Vec3)> = yog_api::event::player::online()
        .iter()
        .filter_map(|n| {
            let e = yog_api::event::entity::info(&yog_api::event::player::info(n)?.uuid)?;
            Some((e.dimension.to_string(), Vec3::from_tuple(e.pos)))
        })
        .collect();
    let known: Vec<((String, BlockPos), (bool, Pigment))> = KNOWN
        .lock()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut gone = Vec::new();
    for ((dim, pos), (light, pigment)) in known {
        let near = players
            .iter()
            .any(|(d, p)| *d == dim && p.distance_sqr(Vec3::center_of(pos)) <= RANGE * RANGE);
        if !near {
            continue;
        }
        if now.is_multiple_of(40) {
            let want = if light {
                Blocks::CONJURED_LIGHT
            } else {
                Blocks::CONJURED_BLOCK
            };
            if yog_api::event::block::info(&dim, to_ypos(pos)).is_some_and(|b| b.block_id() != want)
            {
                gone.push((dim, pos));
                continue;
            }
        }
        sparkle(&dim, pos, light, &pigment);
    }
    if !gone.is_empty() {
        let mut known = KNOWN.lock();
        for k in gone {
            known.remove(&k);
        }
        drop(known);
        save();
    }
}

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::life::Started, _>(|_, _| load());
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());
    // `stepOn` → `walkParticle`: sparks under the feet of whatever walks on it.
    registry.on::<yog_api::event::player::action::Move, _>(|e, cx| {
        let phase = cx.phase();
        if phase != EventPhase::Post {
            return true;
        }
        let below = (
            e.x.floor() as i32,
            (e.y - FEET_DEPTH).floor() as i32,
            e.z.floor() as i32,
        );
        let hit = KNOWN.lock().get(&(e.dimension.clone(), below)).cloned();
        if let Some((false, pigment)) = hit {
            for _ in 0..STEP_SPARKS {
                let time = (now_ticks() % COLOR_TIME as u64) as f32;
                let c = color(&pigment, time, Vec3::new(e.x, e.y, e.z));
                let (base, rise) = STEP_HEIGHT;
                let at = Vec3::new(
                    e.x + jitter(STEP_SPREAD),
                    below.1 as f64 + base + random() * rise,
                    e.z + jitter(STEP_SPREAD),
                );
                let vel = Vec3::new(
                    jitter(STEP_DRIFT),
                    random() * STEP_DRIFT,
                    jitter(STEP_DRIFT),
                );
                spawn_dust(&e.dimension, at, vel, c);
            }
        }
        true
    });
}

fn now_ticks() -> u64 {
    *TICKS.lock()
}
