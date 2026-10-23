//! Particle sprays (Hex Casting's `ParticleSpray`) and dust.

use super::random;
use crate::item::pigment::Pigment;
use crate::math::Vec3;
use yog_api::event::particle::ParticleOptions;

use yog_api::yog_ids::time::TICKS_PER_SECOND;

/// A cloud's particles' drift up (a cloud barely moves).
const CLOUD_DRIFT: f64 = 0.001;

/// Hex Casting's `ParticleSpray`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spray {
    pub pos: Vec3,
    pub vel: Vec3,
    /// The radius of the sphere a particle may start in.
    pub fuzziness: f64,
    /// The largest angle (radians) a particle may stray from `vel`.
    pub spread: f64,
    pub count: u32,
}

impl Spray {
    /// A spray's look — its velocity, fuzziness, spread and count — to be
    /// placed where it goes (`Spray { pos, ..LOOK }`).
    pub const fn look((x, y, z): (f64, f64, f64), fuzziness: f64, spread: f64, count: u32) -> Self {
        Spray {
            pos: Vec3::new(0.0, 0.0, 0.0),
            vel: Vec3::new(x, y, z),
            fuzziness,
            spread,
            count,
        }
    }

    pub fn burst(pos: Vec3, size: f64) -> Self {
        Self::burst_n(pos, size, 20)
    }

    pub fn burst_n(pos: Vec3, size: f64, count: u32) -> Self {
        Spray {
            pos,
            vel: Vec3::new(size, 0.0, 0.0),
            fuzziness: 0.0,
            spread: std::f64::consts::PI,
            count,
        }
    }

    pub fn cloud(pos: Vec3, size: f64) -> Self {
        Self::cloud_n(pos, size, 20)
    }

    pub fn cloud_n(pos: Vec3, size: f64, count: u32) -> Self {
        Spray {
            pos,
            vel: Vec3::new(0.0, CLOUD_DRIFT, 0.0),
            fuzziness: size,
            spread: 0.0,
            count,
        }
    }

    /// `MsgCastParticleS2C.handle`, done on the server with colored dust
    /// (Hex Casting's conjure particles are client-side).
    pub fn spawn(&self, dim: &str, pigment: &Pigment) {
        let time = yog_api::event::world::time(dim).unwrap_or(0) as f32;
        let speed = self.vel.length() / f64::from(TICKS_PER_SECOND);
        let v = self.vel.normalize();
        let k = if v.x == 0.0 && v.y == 0.0 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            v.cross(Vec3::new(0.0, 0.0, 1.0))
        };
        for _ in 0..self.count.min(64) {
            let offset = random_on_sphere() * (random() * self.fuzziness / 2.0);
            let pos = self.pos + offset;
            let phi = (1.0 - random() * (1.0 - self.spread.cos())).acos();
            let theta = std::f64::consts::TAU * random();
            let unlen = v * phi.cos()
                + k * (phi.sin() * theta.cos())
                + v.cross(k) * (phi.sin() * theta.sin());
            let vel = unlen * speed;
            let color = pigment.color(time, unlen);
            spawn_dust(dim, pos, vel, color);
        }
    }
}

pub(super) fn random_on_sphere() -> Vec3 {
    let th = random() * std::f64::consts::TAU;
    let y = random() * 2.0 - 1.0;
    let r = (1.0 - y * y).sqrt();
    Vec3::new(r * th.cos(), y, r * th.sin())
}

/// One conjured particle (`ConjureParticleOptions`) in `color` moving along `vel`.
pub fn spawn_dust(dim: &str, pos: Vec3, vel: Vec3, color: u32) {
    yog_api::event::particle::spawn_particles(
        dim,
        (pos.x, pos.y, pos.z),
        ParticleOptions::colored(crate::res::Particles::CONJURE_PARTICLE, color),
        0,
        (vel.x, vel.y, vel.z),
        1.0,
    );
}
