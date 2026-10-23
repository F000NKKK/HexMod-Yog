//! What mishaps do to the caster (`MishapEnvironment`).

use super::player::look_vec;
use super::{CastEnv, SLOT_OFFHAND, Source, Stack, random};
use crate::math::Vec3;
use crate::protocol::Hand;
use yog_api::vanilla;

/// A player's armor slots.
pub const ARMOR_SLOTS: std::ops::Range<u32> = 36..40;
/// How `Player.drop(stack, dropAround = true)` tosses: the most a stack
/// flies aside, how high, from how far below the eyes.
const DROP_ASIDE: f64 = 0.5;
const DROP_UP: f64 = 0.2;
const DROP_BELOW_EYES: f64 = 0.3;
/// How much a yeeted item strays from its way.
const YEET_JITTER: f64 = 0.1;

/// Whether `stack` stays on: armor with Curse of Binding.
pub fn bound(stack: &Stack) -> bool {
    ARMOR_SLOTS.contains(&stack.slot)
        && stack
            .nbt
            .to_snbt()
            .contains(vanilla::enchantments::BINDING_CURSE)
}

impl CastEnv {
    // ── what mishaps do to the caster (`MishapEnvironment`) ────────────────

    /// `Mishap.trulyHurt`: damage that lands even through invulnerability.
    pub fn truly_hurt(&self, uuid: &str, amount: f32) {
        let before = yog_api::event::entity::health(uuid)
            .map(|h| h as f32)
            .unwrap_or(0.0);
        let attacker = self.caster_uuid().unwrap_or_default();
        yog_api::event::entity::damage(uuid, amount, crate::ids::damage::OVERCAST, &attacker);
        let after = yog_api::event::entity::health(uuid)
            .map(|h| h as f32)
            .unwrap_or(0.0);
        if before > 0.0 && after >= before {
            let target = before - amount;
            if target <= 0.0 {
                yog_api::event::entity::kill(uuid);
            } else {
                yog_api::event::entity::set_health(uuid, f64::from(target));
            }
        }
    }

    pub fn yeet_held_items_towards(&self, target: Vec3) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let Some(pos) = self.caster_pos() else { return };
        let delta = (target - pos).normalize() * 0.5;
        for h in [Hand::Main, Hand::Off] {
            let Some(stack) = self.held(h) else { continue };
            if stack.is_empty() {
                continue;
            }
            self.save(&Stack {
                slot: stack.slot,
                ..Stack::default()
            });
            self.yeet_item(&stack, pos, delta);
        }
    }

    pub(super) fn yeet_item(&self, stack: &Stack, src: Vec3, delta: Vec3) {
        let vel = Vec3::new(
            delta.x + (random() - 0.5) * YEET_JITTER,
            delta.y + (random() - 0.5) * YEET_JITTER,
            delta.z + (random() - 0.5) * YEET_JITTER,
        );
        yog_api::event::item::spawn_item(
            &self.dim,
            (src.x, src.y + 1.0, src.z),
            vel.tuple(),
            &stack.id,
            stack.count,
            stack.nbt.to_snbt(),
        );
    }

    pub fn drop_held_items(&self) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let Some(c) = self.caster_info() else { return };
        let look = self.caster_uuid().map(|u| look_vec(&u)).unwrap_or_default();
        self.yeet_held_items_towards(Vec3::from_tuple(c.pos) + look);
    }

    /// `MishapNoSpellCircle`: the whole inventory (but armour with Curse of
    /// Binding) is thrown about.
    pub fn drop_everything(&self) {
        let Some(name) = self.caster_name() else {
            return;
        };
        for slot in 0..=SLOT_OFFHAND {
            let stack = self.stack_at(slot);
            if stack.is_empty() || bound(&stack) {
                continue;
            }
            Stack {
                slot,
                ..Stack::default()
            }
            .save(name);
            self.drop_from_eyes(&stack);
        }
    }

    /// `Player.drop(stack, dropAround = true)`: `stack` tossed from just
    /// below the caster's eyes, a random way.
    pub fn drop_from_eyes(&self, stack: &Stack) {
        let Some(info) = self.caster_info() else {
            return;
        };
        let (f, a) = (random() * DROP_ASIDE, random() * std::f64::consts::TAU);
        let delta = Vec3::new(-a.sin() * f, DROP_UP, a.cos() * f);
        let eye = info.eye_pos();
        let from = (eye.x, eye.y - DROP_BELOW_EYES, eye.z);
        let nbt = stack.nbt.to_snbt();
        yog_api::event::item::spawn_item(
            &self.dim,
            from,
            delta.tuple(),
            &stack.id,
            stack.count,
            nbt,
        );
    }

    /// `damage(healthProportion)`.
    pub fn mishap_damage(&self, proportion: f32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(uuid) = self.caster_uuid() {
            let health = yog_api::event::entity::health(&uuid)
                .map(|h| h as f32)
                .unwrap_or(0.0);
            self.truly_hurt(&uuid, health * proportion);
        }
    }

    pub fn drown(&self) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let (Some(uuid), Some(name)) = (self.caster_uuid(), self.caster_name()) else {
            return;
        };
        let air = yog_api::event::player::info(name).map_or(300, |p| p.air);
        if air < 200 {
            yog_api::event::entity::damage(&uuid, 2.0, vanilla::damage_types::DROWN, "");
        }
        yog_api::event::entity::set_nbt(&uuid, "{Air:0s}");
    }

    pub fn remove_xp(&self, amount: i32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(name) = self.caster_name() {
            yog_api::event::player::add_xp(name, -amount);
        }
    }

    pub fn blind(&self, ticks: i32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(uuid) = self.caster_uuid() {
            yog_api::event::entity::add_effect(&uuid, vanilla::effects::BLINDNESS, ticks, 0, true);
        }
    }
}
