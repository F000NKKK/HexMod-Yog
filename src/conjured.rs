//! Conjured blocks and lights (`BlockConjured`, `BlockEntityConjured`): they
//! sparkle in the pigment of whoever conjured them — now and then on their
//! own, and under the feet of whatever walks on a block.
//!
//! The pigment is kept per position in the chunk's mod storage; the
//! sparkles are colored dust, sent from the server.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use yog_api::{EventPhase, Registry, Server, Storage};

use crate::casting::env::{MOD_ID, random, spawn_dust, to_ypos};
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::item::pigment::Pigment;
use crate::math::Vec3;

/// Sparkles show this close to a player.
const RANGE: f64 = 32.0;
const KEY: &str = "conjured.index";

static KNOWN: LazyLock<Mutex<HashMap<(String, BlockPos), (bool, Pigment)>>> =
    LazyLock::new(Default::default);
static TICKS: Mutex<u64> = Mutex::new(0);

fn global(srv: &dyn Server) -> Storage {
    Storage::open(&srv.world_dir(), MOD_ID)
}

fn save(srv: &dyn Server) {
    let known = KNOWN.lock().unwrap();
    let lines: Vec<String> = known
        .iter()
        .map(|((dim, p), (light, pig))| {
            format!(
                "{dim}|{},{},{}|{}|{}|{}",
                p.0, p.1, p.2, *light as u8, pig.item, pig.owner
            )
        })
        .collect();
    global(srv).set(KEY, lines.join("\n"));
}

fn load(srv: &dyn Server) {
    let text = global(srv).get_str(KEY).unwrap_or("").to_owned();
    let mut known = KNOWN.lock().unwrap();
    known.clear();
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        let [dim, pos, light, item, owner] = f[..] else {
            continue;
        };
        let mut n = pos.split(',').filter_map(|v| v.parse().ok());
        let (Some(x), Some(y), Some(z)) = (n.next(), n.next(), n.next()) else {
            continue;
        };
        known.insert(
            (dim.to_owned(), (x, y, z)),
            (
                light == "1",
                Pigment {
                    item: item.into(),
                    owner: owner.into(),
                },
            ),
        );
    }
}

/// `BlockConjured.setColor`: a block was conjured with `pigment`.
pub fn conjured(srv: &dyn Server, dim: &str, pos: BlockPos, light: bool, pigment: Pigment) {
    KNOWN
        .lock()
        .unwrap()
        .insert((dim.to_owned(), pos), (light, pigment));
    save(srv);
}

fn color(p: &Pigment, time: f32, near: Vec3) -> u32 {
    let jitter = Vec3::new(random(), random(), random()) * (random() * 3.0);
    p.color(time, near + jitter)
}

/// `particleEffect` (each tick, the client's `animateTick`).
fn sparkle(srv: &dyn Server, dim: &str, pos: BlockPos, light: bool, p: &Pigment) {
    let (x, y, z) = (pos.0 as f64, pos.1 as f64, pos.2 as f64);
    let c = color(p, (random() * 16384.0) as f32, Vec3::default());
    let r = |a: f64, b: f64| a + random() * (b - a);
    if light {
        if random() < 0.5 {
            let at = Vec3::new(
                x + 0.45 + random() * 0.1,
                y + 0.45 + random() * 0.1,
                z + 0.45 + random() * 0.1,
            );
            spawn_dust(
                srv,
                dim,
                at,
                Vec3::new(r(-0.005, 0.005), r(-0.002, 0.02), r(-0.005, 0.005)),
                c,
            );
        }
    } else if random() < 0.2 {
        let at = Vec3::new(x + random(), y + random(), z + random());
        spawn_dust(
            srv,
            dim,
            at,
            Vec3::new(r(-0.02, 0.02), r(-0.02, 0.02), r(-0.02, 0.02)),
            c,
        );
    }
}

fn tick(srv: &dyn Server) {
    let now = {
        let mut t = TICKS.lock().unwrap();
        *t += 1;
        *t
    };
    let players: Vec<(String, Vec3)> = srv
        .online_players()
        .iter()
        .filter_map(|n| {
            let e = srv.entity_info(&srv.player_info(n)?.uuid)?;
            Some((e.dimension, Vec3::from_tuple(e.pos)))
        })
        .collect();
    let known: Vec<((String, BlockPos), (bool, Pigment))> = KNOWN
        .lock()
        .unwrap()
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
            if srv
                .block_info(&dim, to_ypos(pos))
                .is_some_and(|b| b.block_id() != want)
            {
                gone.push((dim, pos));
                continue;
            }
        }
        sparkle(srv, &dim, pos, light, &pigment);
    }
    if !gone.is_empty() {
        let mut known = KNOWN.lock().unwrap();
        for k in gone {
            known.remove(&k);
        }
        drop(known);
        save(srv);
    }
}

pub fn register(registry: &mut Registry) {
    registry.on_server_started(load);
    registry.on_tick(tick);
    // `stepOn` → `walkParticle`: sparks under the feet of whatever walks on it.
    registry.on_player_move(|e, phase, srv| {
        if phase != EventPhase::Post {
            return true;
        }
        let below = (
            e.x.floor() as i32,
            (e.y - 0.01).floor() as i32,
            e.z.floor() as i32,
        );
        let hit = KNOWN
            .lock()
            .unwrap()
            .get(&(e.dimension.clone(), below))
            .cloned();
        if let Some((false, pigment)) = hit {
            for _ in 0..3 {
                let c = color(
                    &pigment,
                    (now_ticks() % 16384) as f32,
                    Vec3::new(e.x, e.y, e.z),
                );
                let at = Vec3::new(
                    e.x + random() * 0.6 - 0.3,
                    below.1 as f64 + random() * 0.05 + 0.95,
                    e.z + random() * 0.6 - 0.3,
                );
                let vel = Vec3::new(
                    random() * 0.04 - 0.02,
                    random() * 0.02,
                    random() * 0.04 - 0.02,
                );
                spawn_dust(srv, &e.dimension, at, vel, c);
            }
        }
        true
    });
}

fn now_ticks() -> u64 {
    *TICKS.lock().unwrap()
}
