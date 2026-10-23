//! Altiora (`OpAltiora`, `AltioraAbility`, `AltioraLayer`): wings of media
//! that let a player glide as with an elytra — deployed by jumping in the
//! air, gone once they touch anything.
//!
//! The server decides: who has wings and who glides is entity data on the
//! player (`hexcasting:altiora`), synced by the loader to everyone who sees
//! them; a client only asks to deploy or fold the wings, and the server
//! checks. Built on the loader's bodies (`yog_api::body`): the wings are a
//! rig drawn on the player, the glide is the player's travel step, the
//! gliding pose a turn of the body, and the glider's hitbox and eyes an
//! elytra flier's.

use crate::casting::ops::spells::{AMBIENCE_CHANCE, AMBIENCE_VOLUME, FINISH_VOLUME};
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_4;
use std::f32::consts::PI;
use std::time::{Duration, Instant};
use yog_api::sync::{LazyLock, Mutex};
use yog_api::yog_ids::gamemode;

use yog_api::Registry;
use yog_api::body::{self, Anchor, BoneOp, Frame, Mat4, Ops, Rig, TravelFrame};
use yog_api::core::entity_flags;

use crate::casting::env::{SLOT_OFFHAND, Spray, Stack, get_pigment, random};
use crate::content::Models;
use crate::math::Vec3;
use crate::res::SoundEvents;

/// Entity data on a player: `[WINGS | GLIDING]`, absent without wings.
pub const DATA: &str = "hexcasting:altiora";
/// Client → server: `[1 | 0]` — the sender asks to deploy or fold the wings.
pub const CH_GLIDE: &str = "hexcasting:altiora_glide";
/// Server → client: `[ticks: u16][uuid]` — a rocket pushes the glider.
pub const CH_BOOST: &str = "hexcasting:altiora_boost";

const WINGS: u8 = 1;
const GLIDING: u8 = 2;

/// `OpAltiora.GRACE_PERIOD`: ticks before touching ground ends the wings.
const GRACE_PERIOD: i32 = 20;
/// How high the spell throws the target (blocks per tick).
pub const LIFT: f64 = 1.5;
/// Chance per tick of the wings' hum.
/// `checkSlowFallDistance`: falling slower than this (blocks per tick)
/// doesn't count as falling.
const SLOW_FALL: f64 = -0.5;

const FIREWORK: &str = yog_api::vanilla::items::FIREWORK_ROCKET;
/// `FireworkRocketEntity`: lifetime = 10 × (1 + flight) + 0–5 + 0–6 ticks.
const ROCKET_TICKS_PER_FLIGHT: i32 = 10;
/// A rocket's random extra ticks (two rolls); its launch's volume; its
/// push each tick (`FireworkRocketEntity`: along the look, toward the look
/// this fast, eased by the last).
const ROCKET_EXTRA: (u8, u8) = (6, 7);
const LAUNCH_VOLUME: f32 = 3.0;
const ROCKET_PUSH: (f32, f32, f32) = (0.1, 1.5, 0.5);
/// The glide's trail.
const GLIDE_TRAIL: Spray = Spray::look((0.0, -0.2, 0.0), 0.4, std::f64::consts::FRAC_PI_2, 3);
/// `ElytraModel`'s left wing: at rest (15°, 0, -15°), gliding (20° up),
/// how a dive folds it, crouched (40°, 5°, -45°, dropped 3), and how fast
/// it eases to where it's going.
const WING_REST: body::Vec3 = body::Vec3::new(0.261_799_4, 0.0, -0.261_799_4);
const WING_GLIDE_X: f32 = 0.349_065_84;
const WING_DIVE: f32 = 1.5;
const WING_CROUCH_X: f32 = 0.698_131_7;
const WING_CROUCH_Y: f32 = 0.087_266_46;
const WING_CROUCH: body::Vec3 = body::Vec3::new(WING_CROUCH_X, WING_CROUCH_Y, -FRAC_PI_4);
const WING_CROUCH_DROP: f32 = 3.0;
const WING_EASE: f32 = 0.1;
/// The body leans into a glide over this many ticks squared, to a right
/// angle; the limbs follow the glide after these ticks.
const LEAN_IN: f32 = 100.0;
const RIGHT_ANGLE: f32 = 90.0;
const LIMBS_AFTER: f32 = 4.0;
/// `HumanoidModel`'s limbs: the speed they slow from, their swing's rate,
/// a leg's swing.
const LIMB_SPEED: f32 = 0.2;
const LIMB_SWING: f32 = 0.6662;
const LEG_SWING: f32 = 1.4;

/// A glider's hitbox and eye height (`Pose.FALL_FLYING`): width, height,
/// eyes, in blocks.
const GLIDING_SIZE: (f32, f32, f32) = (0.6, 0.6, 0.4);

// ── Server ──────────────────────────────────────────────────────────────────

struct Wings {
    grace: i32,
    gliding: bool,
    last_y: Option<f64>,
}

static SERVER: LazyLock<Mutex<HashMap<String, Wings>>> = LazyLock::new(Default::default);

fn show(uuid: &str, state: Option<u8>) {
    yog_api::event::entity::set_data(uuid, DATA, state.as_ref().map(std::slice::from_ref), false);
}

/// `OpAltiora`'s spell: up into the air, wings on.
pub fn give(uuid: &str) {
    yog_api::event::entity::add_velocity(uuid, (0.0, LIFT, 0.0));
    SERVER.lock().insert(
        uuid.to_owned(),
        Wings {
            grace: GRACE_PERIOD,
            gliding: false,
            last_y: None,
        },
    );
    show(uuid, Some(WINGS));
}

fn take(uuid: &str) {
    if SERVER.lock().remove(uuid).is_some() {
        yog_api::event::entity::set_dimensions(uuid, None.map(|(w, h, e)| [w, h, e]));
        show(uuid, None);
    }
}

fn gliding(uuid: &str) -> bool {
    SERVER.lock().get(uuid).is_some_and(|w| w.gliding)
}

/// `OpAltiora.checkPlayerCollision`, every tick.
fn tick() {
    let uuids: Vec<String> = SERVER.lock().keys().cloned().collect();
    for uuid in uuids {
        let Some(info) = yog_api::event::entity::info(&uuid) else {
            take(&uuid);
            continue;
        };
        let pos = Vec3::from_tuple(info.pos);
        let dim = info.dimension.as_str();
        let landed =
            info.has(entity_flags::ON_GROUND) || info.has(entity_flags::HORIZONTAL_COLLISION);
        let (over, glides, fell) = {
            let mut map = SERVER.lock();
            let Some(w) = map.get_mut(&uuid) else {
                continue;
            };
            let over = w.grace == 0 && landed;
            w.grace = (w.grace - 1).max(0);
            let fell = w.last_y.map_or(0.0, |y| pos.y - y);
            w.last_y = Some(pos.y);
            (over, w.gliding, fell)
        };
        if over {
            take(&uuid);
            yog_api::event::sound::play(
                dim,
                (pos.x, pos.y, pos.z),
                SoundEvents::FLIGHT_FINISH,
                FINISH_VOLUME,
                1.0,
            );
            continue;
        }
        if glides && fell > SLOW_FALL {
            yog_api::event::entity::set_fall_distance(&uuid, 1.0);
        }
        if random() < AMBIENCE_CHANCE {
            yog_api::event::sound::play(
                dim,
                (pos.x, pos.y, pos.z),
                SoundEvents::FLIGHT_AMBIENCE,
                AMBIENCE_VOLUME,
                1.0,
            );
        }
        Spray { pos, ..GLIDE_TRAIL }.spawn(dim, &get_pigment(&uuid));
    }
}

/// A rocket used while gliding pushes the glider (`FireworkRocketItem.use`
/// while fall-flying): the glider's client is told how long.
fn rocket(e: &yog_api::event::player::action::UseItem) -> bool {
    if e.item_id != FIREWORK {
        return true;
    }
    let Some(info) = yog_api::event::player::info(&e.player_name) else {
        return true;
    };
    if !gliding(&info.uuid) {
        return true;
    }
    let main = Stack::read(&e.player_name, info.selected_slot);
    let mut stack = if main.is(FIREWORK) {
        main
    } else {
        Stack::read(&e.player_name, SLOT_OFFHAND)
    };
    if !stack.is(FIREWORK) {
        return true;
    }
    let flight = stack
        .nbt
        .get("Fireworks")
        .and_then(|f| f.get_i64("Flight"))
        .unwrap_or(0) as i32;
    let (more, more_still) = ROCKET_EXTRA;
    let ticks = ROCKET_TICKS_PER_FLIGHT * (1 + flight)
        + (random() * f64::from(more)) as i32
        + (random() * f64::from(more_still)) as i32;
    if info.gamemode != gamemode::CREATIVE {
        stack.count -= 1;
        stack.save(&e.player_name);
    }
    let mut boost = (ticks.clamp(0, u16::MAX as i32) as u16)
        .to_le_bytes()
        .to_vec();
    boost.extend_from_slice(info.uuid.as_bytes());
    yog_api::event::net::send_to_player(&e.player_name, CH_BOOST, &boost);
    if let Some(p) = yog_api::event::entity::info(&info.uuid) {
        yog_api::event::sound::play(
            p.dimension,
            (p.pos.x, p.pos.y, p.pos.z),
            yog_api::vanilla::sounds::ENTITY_FIREWORK_ROCKET_LAUNCH,
            LAUNCH_VOLUME,
            1.0,
        );
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
    /// The local player asked the server to deploy (`true`) or fold the
    /// wings, and when: shown as asked till the server agrees, or for
    /// [`ASK_PATIENCE`] if it doesn't (it refused).
    asked: Option<(bool, Instant)>,
}

impl Seen {
    fn new() -> Seen {
        Seen {
            gliding: false,
            since: f32::NAN,
            wing: WING_REST,
            boost: 0,
            was_jumping: false,
            asked: None,
        }
    }

    /// Follows the server's word (`data`), or what was asked until it agrees.
    fn update(&mut self, data: &[u8]) {
        let told = data.first() == Some(&GLIDING);
        let on = match self.asked {
            Some((a, at)) if a != told && at.elapsed() < ASK_PATIENCE => a,
            _ => {
                self.asked = None;
                told
            }
        };
        if self.gliding != on {
            self.gliding = on;
            self.since = f32::NAN;
        }
    }
}

static CLIENT: LazyLock<Mutex<HashMap<String, Seen>>> = LazyLock::new(Default::default);

/// How long the local player's ask is shown before the server's word wins.
const ASK_PATIENCE: Duration = Duration::from_secs(1);

/// Asks the server to deploy or fold the local player's wings.
fn ask(s: &mut Seen, on: bool) {
    s.asked = Some((on, Instant::now()));
    s.gliding = on;
    s.since = f32::NAN;
    yog_api::event::net::send_to_server(CH_GLIDE, &[on as u8]);
}

/// `ElytraModel.setupAnim`: the left wing's turn and drop, for a body
/// gliding or crouching or neither.
fn wing_target(f: &Frame, gliding: bool) -> (body::Vec3, f32) {
    let rest = WING_REST;
    if gliding {
        let mut k = 1.0;
        if f.velocity.y < 0.0 {
            k = 1.0 - (-f.velocity.normalize().y).powf(WING_DIVE);
        }
        return (
            body::Vec3::new(
                k * WING_GLIDE_X + (1.0 - k) * rest.x,
                0.0,
                k * (-PI / 2.0) + (1.0 - k) * rest.z,
            ),
            0.0,
        );
    }
    if f.has(entity_flags::SNEAKING) {
        return (WING_CROUCH, WING_CROUCH_DROP);
    }
    (rest, 0.0)
}

/// `PlayerRenderer.setupRotations` while fall-flying: the body leans into
/// the glide and turns toward where it flies.
fn glide_turn(f: &Frame, ticks: f32) -> Mat4 {
    let lean = (ticks * ticks / LEAN_IN).clamp(0.0, 1.0);
    let mut m = Mat4::rotation_x((lean * (-RIGHT_ANGLE - f.pitch)).to_radians());
    let (yaw, pitch) = ((f.body_yaw + f.head_yaw).to_radians(), f.pitch.to_radians());
    let view = body::Vec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
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
    let head = BoneOp {
        rotation: body::Vec3::new(-FRAC_PI_4, f.head_yaw.to_radians(), 0.0),
        set_rotation: true,
        ..Default::default()
    };
    ops.bone("head", &head);
    ops.bone("hat", &head);
    let speed = f.velocity.dot(f.velocity) / LIMB_SPEED;
    let slow = (speed * speed * speed).max(1.0);
    let k = 1.0 / slow - 1.0;
    let swing = f.limb_swing * LIMB_SWING;
    let arm = |phase: f32| (swing + phase).cos() * f.limb_amount * k;
    let leg = |phase: f32| (swing + phase).cos() * LEG_SWING * f.limb_amount * k;
    for (parts, x) in [
        (["right_arm", "right_sleeve"], arm(PI)),
        (["left_arm", "left_sleeve"], arm(0.0)),
        (["right_leg", "right_pants"], leg(0.0)),
        (["left_leg", "left_pants"], leg(PI)),
    ] {
        for p in parts {
            ops.bone(
                p,
                &BoneOp {
                    rotation: body::Vec3::new(x, 0.0, 0.0),
                    ..Default::default()
                },
            );
        }
    }
}

/// The wings and the gliding pose (`AltioraLayer`, the elytra's pose).
fn draw(f: &Frame, ops: &mut Ops) {
    let mut map = CLIENT.lock();
    let Some(data) = f.data(DATA) else {
        map.remove(&f.uuid);
        return;
    };
    let s = map.entry(f.uuid.clone()).or_insert_with(Seen::new);
    s.update(data);
    if s.gliding && s.since.is_nan() {
        s.since = f.age;
    }
    let (target, drop) = wing_target(f, s.gliding);
    s.wing = s.wing + (target - s.wing) * WING_EASE;
    let Some(model) = body::model(Models::BODY_ALTIORA) else {
        return;
    };
    let mut rig = Rig::new(model, Anchor::Model);
    rig.look.glint = true;
    let rest = |name: &str| {
        rig.model
            .bone(name)
            .map_or(body::Vec3::ZERO, |i| rig.model.bones[i].rotation)
    };
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
        if ticks > LIMBS_AFTER {
            glide_limbs(ops, f);
        }
    }
}

/// The local player's glide: deployed by a jump in the air, an elytra's
/// flight (pushed by rockets), folded on the ground.
fn travel(f: &TravelFrame) -> Option<body::Vec3> {
    let data = body::entity_data(&f.uuid, DATA)?;
    let mut map = CLIENT.lock();
    let s = map.entry(f.uuid.clone()).or_insert_with(Seen::new);
    s.update(&data);
    let jumped = f.jumping && !s.was_jumping;
    s.was_jumping = f.jumping;
    let on_ground = f.flags & entity_flags::ON_GROUND != 0;
    if !s.gliding {
        if !(jumped && !on_ground && f.flags & entity_flags::GLIDING == 0) {
            return None;
        }
        ask(s, true);
    } else if on_ground {
        ask(s, false);
        s.boost = 0;
        return None;
    }
    let mut v = f.velocity;
    if s.boost > 0 {
        s.boost -= 1;
        let (push, pull, ease) = ROCKET_PUSH;
        v = v + f.look * push + (f.look * pull - v) * ease;
    }
    Some(body::movement::elytra(&TravelFrame {
        velocity: v,
        ..f.clone()
    }))
}

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());
    registry.on::<yog_api::event::player::session::Leave, _>(|e, _| {
        take(&e.uuid);
        true
    });
    // A client asks to deploy or fold its wings: deploying needs wings and
    // the air under the player, as the server sees it.
    registry.on_packet(CH_GLIDE, |ev| {
        let Some(info) = yog_api::event::player::info(&ev.player) else {
            return;
        };
        let on = ev.payload.first() == Some(&1);
        if on
            && yog_api::event::entity::info(&info.uuid)
                .is_none_or(|e| e.has(entity_flags::ON_GROUND))
        {
            return;
        }
        let changed = {
            let mut map = SERVER.lock();
            let Some(w) = map.get_mut(&info.uuid) else {
                return;
            };
            let changed = w.gliding != on;
            w.gliding = on;
            changed
        };
        if changed {
            yog_api::event::entity::set_dimensions(
                &info.uuid,
                on.then_some(GLIDING_SIZE).map(|(w, h, e)| [w, h, e]),
            );
            show(&info.uuid, Some(if on { GLIDING } else { WINGS }));
        }
    });
    registry.before::<yog_api::event::player::action::UseItem, _>(|e, _| rocket(e));
    registry.on_client_packet(CH_BOOST, |ev| {
        let [a, b, ref uuid @ ..] = ev.payload[..] else {
            return;
        };
        let Ok(uuid) = std::str::from_utf8(uuid) else {
            return;
        };
        if let Some(s) = CLIENT.lock().get_mut(uuid) {
            s.boost = u16::from_le_bytes([a, b]) as i32;
        }
    });
    // Every player is asked about: the wings show for whoever has them.
    body::track_type(yog_api::vanilla::entities::PLAYER, true);
    registry.on_body(draw);
    registry.on_travel(travel);
}
