//! Altiora (`OpAltiora`, `AltioraAbility`, `AltioraLayer`): wings of media
//! that let a player glide as with an elytra — deployed by jumping in the
//! air, gone once they touch anything.
//!
//! Built on the loader's bodies (`yog_api::body`): the wings are a rig
//! drawn on the player, the glide is the player's travel step, the gliding
//! pose a turn of the body and the camera lowered as for an elytra.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use yog_api::body::{self, Anchor, BoneOp, CameraFrame, CameraOut, Frame, Mat4, Ops, Rig, TravelFrame};
use yog_api::core::entity_flags;
use yog_api::{EventPhase, Registry, Server};

use crate::content::Models;
use crate::casting::env::{SLOT_OFFHAND, Spray, Stack, get_pigment, random};
use crate::math::Vec3;
use crate::snbt::Nbt;
use crate::sounds;

/// Server → client: `[state][uuid]` — who has wings ([`WINGS`]) and who
/// glides ([`GLIDING`]), or neither.
pub const CH_ALTIORA: &str = "hexcasting:altiora";
/// Client → server: `[1 | 0]` — the sender deployed or folded the wings.
pub const CH_GLIDE: &str = "hexcasting:altiora_glide";
/// Server → client: `[ticks: u16][uuid]` — a rocket pushes the glider.
pub const CH_BOOST: &str = "hexcasting:altiora_boost";

const NONE: u8 = 0;
const WINGS: u8 = 1;
const GLIDING: u8 = 2;

/// `OpAltiora.GRACE_PERIOD`: ticks before touching ground ends the wings.
const GRACE_PERIOD: i32 = 20;
/// How high the spell throws the target (blocks per tick).
pub const LIFT: f64 = 1.5;
/// Chance per tick of the wings' hum.
const AMBIENCE_CHANCE: f64 = 0.02;
const AMBIENCE_VOLUME: f32 = 0.2;
const FINISH_VOLUME: f32 = 2.0;
/// `checkSlowFallDistance`: falling slower than this (blocks per tick)
/// doesn't count as falling.
const SLOW_FALL: f64 = -0.5;

const FIREWORK: &str = "minecraft:firework_rocket";
/// `FireworkRocketEntity`: lifetime = 10 × (1 + flight) + 0–5 + 0–6 ticks.
const ROCKET_TICKS_PER_FLIGHT: i32 = 10;

/// Eye height standing and gliding (`Pose.STANDING`, `Pose.FALL_FLYING`).
const STANDING_EYE: f32 = 1.62;
const GLIDING_EYE: f32 = 0.4;

// ── Server ──────────────────────────────────────────────────────────────────

struct Wings {
    grace: i32,
    gliding: bool,
    last_y: Option<f64>,
}

static SERVER: LazyLock<Mutex<HashMap<String, Wings>>> = LazyLock::new(Default::default);

fn packet(state: u8, uuid: &str) -> Vec<u8> {
    let mut b = vec![state];
    b.extend_from_slice(uuid.as_bytes());
    b
}

fn broadcast(srv: &dyn Server, state: u8, uuid: &str) {
    let p = packet(state, uuid);
    for name in srv.online_players() {
        srv.send_to_player(&name, CH_ALTIORA, &p);
    }
}

/// `OpAltiora`'s spell: up into the air, wings on.
pub fn give(srv: &dyn Server, uuid: &str) {
    srv.entity_add_velocity(uuid, 0.0, LIFT, 0.0);
    SERVER.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid.to_owned(), Wings { grace: GRACE_PERIOD, gliding: false, last_y: None });
    broadcast(srv, WINGS, uuid);
}

fn take(srv: &dyn Server, uuid: &str) {
    if SERVER.lock().unwrap_or_else(|e| e.into_inner()).remove(uuid).is_some() {
        broadcast(srv, NONE, uuid);
    }
}

fn gliding(uuid: &str) -> bool {
    SERVER.lock().unwrap_or_else(|e| e.into_inner()).get(uuid).is_some_and(|w| w.gliding)
}

/// `OpAltiora.checkPlayerCollision`, every tick.
fn tick(srv: &dyn Server) {
    let uuids: Vec<String> = SERVER.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
    for uuid in uuids {
        let Some(info) = srv.entity_info(&uuid) else {
            take(srv, &uuid);
            continue;
        };
        let pos = Vec3::from_tuple(info.pos);
        let dim = info.dimension.as_str();
        let landed = info.has(entity_flags::ON_GROUND) || info.has(entity_flags::HORIZONTAL_COLLISION);
        let (over, glides, fell) = {
            let mut map = SERVER.lock().unwrap_or_else(|e| e.into_inner());
            let Some(w) = map.get_mut(&uuid) else { continue };
            let over = w.grace == 0 && landed;
            w.grace = (w.grace - 1).max(0);
            let fell = w.last_y.map_or(0.0, |y| pos.y - y);
            w.last_y = Some(pos.y);
            (over, w.gliding, fell)
        };
        if over {
            take(srv, &uuid);
            srv.play_sound(dim, pos.x, pos.y, pos.z, sounds::FLIGHT_FINISH, FINISH_VOLUME, 1.0);
            continue;
        }
        if glides && fell > SLOW_FALL {
            srv.entity_set_fall_distance(&uuid, 1.0);
        }
        if random() < AMBIENCE_CHANCE {
            srv.play_sound(dim, pos.x, pos.y, pos.z, sounds::FLIGHT_AMBIENCE, AMBIENCE_VOLUME, 1.0);
        }
        Spray { pos, vel: Vec3::new(0.0, -0.2, 0.0), fuzziness: 0.4, spread: std::f64::consts::FRAC_PI_2, count: 3 }
            .spawn(srv, dim, &get_pigment(srv, &uuid));
    }
}

/// A rocket used while gliding pushes the glider (`FireworkRocketItem.use`
/// while fall-flying): the glider's client is told how long.
fn rocket(e: &yog_api::UseItemEvent, srv: &dyn Server) -> bool {
    if e.item_id != FIREWORK {
        return true;
    }
    let Some(info) = srv.player_info(&e.player_name) else { return true };
    if !gliding(&info.uuid) {
        return true;
    }
    let main = Stack::read(srv, &e.player_name, info.selected_slot);
    let mut stack = if main.is(FIREWORK) { main } else { Stack::read(srv, &e.player_name, SLOT_OFFHAND) };
    if !stack.is(FIREWORK) {
        return true;
    }
    let flight = stack.nbt.get("Fireworks").and_then(|f| f.get_i64("Flight")).unwrap_or(0) as i32;
    let ticks = ROCKET_TICKS_PER_FLIGHT * (1 + flight) + (random() * 6.0) as i32 + (random() * 7.0) as i32;
    if info.gamemode != "creative" {
        stack.count -= 1;
        stack.save(srv, &e.player_name);
    }
    let mut boost = (ticks.clamp(0, u16::MAX as i32) as u16).to_le_bytes().to_vec();
    boost.extend_from_slice(info.uuid.as_bytes());
    srv.send_to_player(&e.player_name, CH_BOOST, &boost);
    if let Some(p) = srv.entity_info(&info.uuid) {
        srv.play_sound(&p.dimension, p.pos.0, p.pos.1, p.pos.2, sounds::vanilla::FIREWORK_LAUNCH, 3.0, 1.0);
    }
    false
}

// ── Client ──────────────────────────────────────────────────────────────────

/// A player with wings, as this client sees them.
struct Seen {
    gliding: bool,
    /// The age (ticks) the glide began at; NaN until the next frame says.
    since: f32,
    /// The wings' smoothed turn (`AbstractClientPlayer.elytraRot*`).
    wing: body::Vec3,
    /// Ticks a rocket still pushes (the local player).
    boost: i32,
    was_jumping: bool,
    /// The camera's lowering for the gliding eye height, 0–1, and when it
    /// was last moved.
    eye: f32,
    eye_at: Option<Instant>,
}

impl Seen {
    fn new() -> Seen {
        Seen {
            gliding: false,
            since: f32::NAN,
            wing: body::Vec3::new(0.2617994, 0.0, -0.2617994),
            boost: 0,
            was_jumping: false,
            eye: 0.0,
            eye_at: None,
        }
    }

    fn glide(&mut self, on: bool) {
        if self.gliding != on {
            self.gliding = on;
            self.since = f32::NAN;
        }
    }
}

static CLIENT: LazyLock<Mutex<HashMap<String, Seen>>> = LazyLock::new(Default::default);

fn tell_server(on: bool) {
    if let Some(srv) = yog_api::server() {
        srv.send_to_server(CH_GLIDE, &[on as u8]);
    }
}

/// `ElytraModel.setupAnim`: the left wing's turn and drop, for a body
/// gliding or crouching or neither.
fn wing_target(f: &Frame, gliding: bool) -> (body::Vec3, f32) {
    let rest = body::Vec3::new(0.2617994, 0.0, -0.2617994);
    if gliding {
        let mut k = 1.0;
        if f.velocity.y < 0.0 {
            k = 1.0 - (-f.velocity.normalize().y).powf(1.5);
        }
        return (body::Vec3::new(k * 0.34906584 + (1.0 - k) * rest.x, 0.0, k * (-PI / 2.0) + (1.0 - k) * rest.z), 0.0);
    }
    if f.has(entity_flags::SNEAKING) {
        return (body::Vec3::new(0.6981317, 0.08726646, -PI / 4.0), 3.0);
    }
    (rest, 0.0)
}

/// `PlayerRenderer.setupRotations` while fall-flying: the body leans into
/// the glide and turns toward where it flies.
fn glide_turn(f: &Frame, ticks: f32) -> Mat4 {
    let lean = (ticks * ticks / 100.0).clamp(0.0, 1.0);
    let mut m = Mat4::rotation_x((lean * (-90.0 - f.pitch)).to_radians());
    let (yaw, pitch) = ((f.body_yaw + f.head_yaw).to_radians(), f.pitch.to_radians());
    let view = body::Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let v = f.velocity;
    let (dv, dl) = (v.x * v.x + v.z * v.z, view.x * view.x + view.z * view.z);
    if dv > 0.0 && dl > 0.0 {
        let cos = ((v.x * view.x + v.z * view.z) / (dv * dl).sqrt()).clamp(-1.0, 1.0);
        let side = v.x * view.z - v.z * view.x;
        m = m * Mat4::rotation_y(side.signum() * cos.acos());
    }
    m
}

/// `HumanoidModel.setupAnim` while fall-flying (after 4 ticks): the head
/// looks ahead, and the limbs swing less the faster the flight.
fn glide_limbs(ops: &mut Ops, f: &Frame) {
    let head = BoneOp { rotation: body::Vec3::new(-PI / 4.0, f.head_yaw.to_radians(), 0.0), set_rotation: true, ..Default::default() };
    ops.bone("head", &head);
    ops.bone("hat", &head);
    let speed = f.velocity.dot(f.velocity) / 0.2;
    let slow = (speed * speed * speed).max(1.0);
    let k = 1.0 / slow - 1.0;
    let swing = f.limb_swing * 0.6662;
    let arm = |phase: f32| (swing + phase).cos() * f.limb_amount * k;
    let leg = |phase: f32| (swing + phase).cos() * 1.4 * f.limb_amount * k;
    for (parts, x) in [
        (["right_arm", "right_sleeve"], arm(PI)),
        (["left_arm", "left_sleeve"], arm(0.0)),
        (["right_leg", "right_pants"], leg(0.0)),
        (["left_leg", "left_pants"], leg(PI)),
    ] {
        for p in parts {
            ops.bone(p, &BoneOp { rotation: body::Vec3::new(x, 0.0, 0.0), ..Default::default() });
        }
    }
}

/// The wings and the gliding pose (`AltioraLayer`, the elytra's pose).
fn draw(f: &Frame, ops: &mut Ops) {
    let mut map = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = map.get_mut(&f.uuid) else { return };
    if s.gliding && s.since.is_nan() {
        s.since = f.age;
    }
    let (target, drop) = wing_target(f, s.gliding);
    s.wing = s.wing + (target - s.wing) * 0.1;
    let Some(model) = body::model(Models::BODY_ALTIORA) else { return };
    let mut rig = Rig::new(model, Anchor::Model);
    rig.glint = true;
    let rest = |name: &str| rig.model.bone(name).map_or(body::Vec3::ZERO, |i| rig.model.bones[i].rotation);
    let rest = (rest("left_wing"), rest("right_wing"));
    let left = rig.manual.bone("left_wing");
    left.rotation = s.wing - rest.0;
    left.position = body::Vec3::new(0.0, drop, 0.0);
    let right = rig.manual.bone("right_wing");
    right.rotation = body::Vec3::new(s.wing.x, -s.wing.y, -s.wing.z) - rest.1;
    right.position = body::Vec3::new(0.0, drop, 0.0);
    rig.write(ops, 0.0);
    if s.gliding {
        let ticks = f.age - s.since;
        ops.root(&glide_turn(f, ticks));
        if ticks > 4.0 {
            glide_limbs(ops, f);
        }
    }
}

/// The eyes drop to a glider's height, smoothly (`Camera.tick`).
fn camera(f: &CameraFrame, out: &mut CameraOut) {
    let mut map = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = map.get_mut(&f.uuid) else { return };
    let now = Instant::now();
    let dt = s.eye_at.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
    s.eye_at = Some(now);
    let target = if s.gliding { 1.0 } else { 0.0 };
    s.eye += (target - s.eye) * (1.0 - 0.5f32.powf(dt * 20.0));
    out.world_offset.y -= (STANDING_EYE - GLIDING_EYE) * s.eye;
}

/// The local player's glide: deployed by a jump in the air, an elytra's
/// flight (pushed by rockets), folded on the ground.
fn travel(f: &TravelFrame) -> Option<body::Vec3> {
    let mut map = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    let s = map.get_mut(&f.uuid)?;
    let jumped = f.jumping && !s.was_jumping;
    s.was_jumping = f.jumping;
    let on_ground = f.flags & entity_flags::ON_GROUND != 0;
    if !s.gliding {
        if !(jumped && !on_ground && f.flags & entity_flags::GLIDING == 0) {
            return None;
        }
        s.glide(true);
        tell_server(true);
    } else if on_ground {
        s.glide(false);
        s.boost = 0;
        tell_server(false);
        return None;
    }
    let mut v = f.velocity;
    if s.boost > 0 {
        s.boost -= 1;
        v = v + f.look * 0.1 + (f.look * 1.5 - v) * 0.5;
    }
    Some(body::movement::elytra(&TravelFrame { velocity: v, ..f.clone() }))
}

pub fn register(registry: &mut Registry) {
    registry.on_tick(tick);
    registry.on_player_join(|e, _, srv| {
        let map = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        for (uuid, w) in map.iter() {
            srv.send_to_player(&e.player_name, CH_ALTIORA, &packet(if w.gliding { GLIDING } else { WINGS }, uuid));
        }
        true
    });
    registry.on_player_leave(|e, _, srv| {
        take(srv, &e.uuid);
        true
    });
    registry.on_packet(CH_GLIDE, |ev, srv| {
        let Some(info) = srv.player_info(&ev.player) else { return };
        let on = ev.payload.first() == Some(&1);
        let changed = {
            let mut map = SERVER.lock().unwrap_or_else(|e| e.into_inner());
            let Some(w) = map.get_mut(&info.uuid) else { return };
            let changed = w.gliding != on;
            w.gliding = on;
            changed
        };
        if changed {
            broadcast(srv, if on { GLIDING } else { WINGS }, &info.uuid);
        }
    });
    registry.on_use_item(|e, phase, srv| phase != EventPhase::Pre || rocket(e, srv));
    registry.on_client_packet(CH_ALTIORA, |ev, _| {
        let Some((&state, uuid)) = ev.payload.split_first() else { return };
        let Ok(uuid) = std::str::from_utf8(uuid) else { return };
        let mut map = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
        if state == NONE {
            map.remove(uuid);
            body::track(uuid, false);
            return;
        }
        map.entry(uuid.to_owned()).or_insert_with(Seen::new).glide(state == GLIDING);
        body::track(uuid, true);
    });
    registry.on_client_packet(CH_BOOST, |ev, _| {
        let [a, b, ref uuid @ ..] = ev.payload[..] else { return };
        let Ok(uuid) = std::str::from_utf8(uuid) else { return };
        if let Some(s) = CLIENT.lock().unwrap_or_else(|e| e.into_inner()).get_mut(uuid) {
            s.boost = u16::from_le_bytes([a, b]) as i32;
        }
    });
    registry.on_body(draw);
    registry.on_camera(camera);
    registry.on_travel(travel);
}
