//! Hands, positions, looks, and what's kept for each player: their
//! pigment and sentinel.

use crate::casting::mishap::BlockPos;
use crate::item::pigment::Pigment;
use crate::math::Vec3;
use crate::protocol::Hand;

pub fn other_hand(h: Hand) -> Hand {
    match h {
        Hand::Main => Hand::Off,
        Hand::Off => Hand::Main,
    }
}

pub fn to_ypos(p: BlockPos) -> yog_api::BlockPos {
    yog_api::BlockPos {
        x: p.0,
        y: p.1,
        z: p.2,
    }
}

pub fn from_ypos(p: yog_api::BlockPos) -> BlockPos {
    (p.x, p.y, p.z)
}

/// `Entity.getLookAngle`.
pub fn look_vec(uuid: &str) -> Vec3 {
    let Some((yaw, pitch)) = yog_api::event::entity::rotation(uuid).map(|r| (r.yaw, r.pitch))
    else {
        return Vec3::ZERO;
    };
    let (yaw, pitch) = ((yaw as f64).to_radians(), (pitch as f64).to_radians());
    Vec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

// ── per-player state ──────────────────────────────────────────────────────

/// Each player's pigment, kept with the world.
pub(super) static PIGMENT: yog_api::storage::PlayerData<Option<Pigment>> =
    yog_api::storage::PlayerData::new("pigment");

pub fn get_pigment(uuid: &str) -> Pigment {
    PIGMENT.with(uuid, Clone::clone).unwrap_or_default()
}

pub fn set_pigment(uuid: &str, p: &Pigment) {
    PIGMENT.with_mut(uuid, |kept| *kept = Some(p.clone()));
}

/// A player's sentinel (`Sentinel`).
#[derive(Debug, Clone, PartialEq, yog_api::Schema)]
pub struct Sentinel {
    /// Greater Sentinels extend the caster's ambit.
    pub great: bool,
    pub pos: Vec3,
    pub dim: String,
}

/// Each player's sentinel, kept with the world.
pub(super) static SENTINEL: yog_api::storage::PlayerData<Option<Sentinel>> =
    yog_api::storage::PlayerData::new("sentinel");

pub fn get_sentinel(uuid: &str) -> Option<Sentinel> {
    SENTINEL.with(uuid, Clone::clone)
}

pub fn set_sentinel(uuid: &str, sentinel: Option<&Sentinel>) {
    SENTINEL.with_mut(uuid, |kept| *kept = sentinel.cloned());
}
