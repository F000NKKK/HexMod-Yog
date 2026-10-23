//! What a mishap does: to the world and the caster, and to the stack.

use yog_api::yog_ids::time::TICKS_PER_SECOND;

use super::{Context, Mishap};
use crate::casting::env::{CastEnv, to_ypos};
use crate::iota::Iota;
use crate::math::Vec3;
use crate::res::Lang;
use crate::text::Text;

/// A bad block's little explosion.
const BAD_BLOCK_BLAST: f32 = 0.25;
/// What a bad brainsweep hurts, and a division by zero.
const BRAINSWEEP_HURT: f32 = 1.0;
const DIVIDE_HURT: f32 = 0.5;
/// How a bad item entity is knocked: up, and this much aside at most.
const KNOCK_UP: f64 = 0.75;
const KNOCK_ASIDE: f64 = 0.05;
/// A second knock's direction from the first's random number.
const KNOCK_SPIN: f64 = 7.0;
/// The experience a missing Akashic record takes.
const NO_RECORD_XP: i32 = 100;
/// How long a true name blinds: one's own, another's (seconds).
const OWN_NAME_BLIND: i32 = 5;
const OTHERS_NAME_BLIND: i32 = 60;
/// The glass breaking as a great spell fails: its volume and pitch.
const GLASS: (f32, f32) = (0.5, 0.7);

impl Mishap {
    /// The effect on the world and the stack (no sound or particles).
    pub fn execute(&self, env: &mut CastEnv, ctx: &Context, stack: &mut Vec<Iota>) {
        self.punish(env);
        self.garble(ctx, stack);
    }

    /// What it does to the world and the caster.
    fn punish(&self, env: &mut CastEnv) {
        match self {
            Mishap::AlreadyBrainswept { mob, .. } => {
                let health = yog_api::event::entity::health(&mob.uuid)
                    .map(|h| h as f32)
                    .unwrap_or(0.0);
                let caster = env.caster_uuid().unwrap_or_default();
                let overcast = crate::ids::damage::OVERCAST;
                yog_api::event::entity::damage(&mob.uuid, health, overcast, &caster);
            }
            Mishap::BadBlock { pos, .. } => {
                let p = Vec3::center_of(*pos);
                yog_api::event::world::explode(
                    &env.dim,
                    (p.x, p.y, p.z),
                    BAD_BLOCK_BLAST,
                    false,
                    "",
                );
            }
            Mishap::BadBrainsweep { mob, .. } => env.truly_hurt(&mob.uuid, BRAINSWEEP_HURT),
            Mishap::BadEntity { pos, .. }
            | Mishap::EntityTooFar { pos, .. }
            | Mishap::ImmuneEntity { pos, .. } => env.yeet_held_items_towards(*pos),
            Mishap::BadLocation { pos, .. } => env.yeet_held_items_towards(*pos),
            Mishap::BadItem { entity, .. } => {
                let r = crate::casting::env::random();
                let aside = |v: f64| (v - 0.5) * KNOCK_ASIDE;
                let knock = (aside(r), KNOCK_UP, aside(r * KNOCK_SPIN % 1.0));
                yog_api::event::entity::add_velocity(&entity.uuid, knock);
            }
            Mishap::BadOffhandItem { .. } | Mishap::LackingHotbarItem(_) => env.drop_held_items(),
            Mishap::DivideByZero { .. } => env.mishap_damage(DIVIDE_HURT),
            Mishap::EvalTooMuch => env.drown(),
            Mishap::NoAkashicRecord(_) => env.remove_xp(NO_RECORD_XP),
            // The directrix breaks.
            Mishap::BoolDirectrixEmptyStack(pos) | Mishap::BoolDirectrixNotBool(_, pos) => {
                yog_api::event::block::destroy(&env.dim, to_ypos(*pos), true, "");
            }
            Mishap::NoSpellCircle => env.drop_everything(),
            Mishap::NotEnoughMedia(cost) => {
                env.extract_media(*cost, false);
            }
            Mishap::OthersName { is_self, .. } => {
                let seconds = if *is_self {
                    OWN_NAME_BLIND
                } else {
                    OTHERS_NAME_BLIND
                };
                env.blind(seconds * TICKS_PER_SECOND as i32);
            }
            Mishap::Unenlightened => unenlightened(env),
            _ => {}
        }
    }

    /// What it does to the stack.
    fn garble(&self, ctx: &Context, stack: &mut Vec<Iota>) {
        match self {
            Mishap::DivideByZero { .. } | Mishap::InvalidPattern(_) | Mishap::WrongDimension(_) => {
                stack.push(Iota::Garbage)
            }
            Mishap::InvalidIota { rev_idx, .. } => {
                if let Some(i) = stack.len().checked_sub(1 + rev_idx) {
                    stack[i] = Iota::Garbage;
                }
            }
            Mishap::InvalidOperatorArgs(perps) => {
                for i in 0..perps.len() {
                    if let Some(j) = stack.len().checked_sub(1 + i) {
                        stack[j] = Iota::Garbage;
                    }
                }
            }
            Mishap::NeedsParens => {
                if let Some(p) = &ctx.pattern {
                    stack.push(Iota::Pattern(p.clone()));
                }
            }
            Mishap::NotEnoughArgs { expected, got } => {
                for _ in *got..*expected {
                    stack.push(Iota::Garbage);
                }
            }
            Mishap::StackSize => {
                stack.clear();
                stack.push(Iota::Garbage);
            }
            _ => {}
        }
    }
}

/// A great spell cast by one who can't: their hands empty, glass breaks,
/// and `HexAdvancementTriggers.FAIL_GREAT_SPELL_TRIGGER` unlocks
/// overcasting ("Blind Diversion").
fn unenlightened(env: &mut CastEnv) {
    env.drop_held_items();
    env.print_message(&Text::key(Lang::MESSAGE_CANT_GREAT_SPELL_KEY));
    let p = env.mishap_spray_pos();
    let glass = yog_api::vanilla::sounds::BLOCK_GLASS_BREAK;
    yog_api::event::sound::play(&env.dim, (p.x, p.y, p.z), glass, GLASS.0, GLASS.1);
    env.grant_advancement(crate::content::Advancements::Y_U_NO_CAST_ANGY);
}
