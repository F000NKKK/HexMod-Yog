//! What a mishap says.

use super::{Mishap, entity_name, error, pos_text, vec_text};
use crate::casting::env::CastEnv;
use crate::client::text::Text;
use crate::iota::Iota;
use crate::res::Lang;

impl Mishap {
    /// The message, without the action's name.
    pub(super) fn error_message(&self, env: &CastEnv) -> Option<Text> {
        match self {
            Mishap::Unenlightened => None,
            _ => self.world_message(env).or_else(|| self.stack_message()),
        }
    }

    /// The message of a mishap about the world: blocks, entities, items,
    /// places, circles, media.
    fn world_message(&self, env: &CastEnv) -> Option<Text> {
        Some(match self {
            Mishap::AlreadyBrainswept { .. } => error("already_brainswept", vec![]),
            Mishap::BadBlock { pos, expected } => error(
                "bad_block",
                vec![expected.clone(), pos_text(*pos), env.block_name(*pos)],
            ),
            Mishap::BadBrainsweep { pos, .. } => {
                error("bad_brainsweep", vec![env.block_name(*pos)])
            }
            Mishap::BadCaster => error("bad_caster", vec![]),
            Mishap::BadEntity { entity, wanted, .. } => {
                error("bad_entity", vec![wanted.clone(), entity_name(entity)])
            }
            Mishap::BadItem {
                item: None, wanted, ..
            } => error("no_item", vec![wanted.clone()]),
            Mishap::BadItem {
                item: Some(i),
                wanted,
                ..
            } => error(
                "bad_item",
                vec![wanted.clone(), Text::int(i.count as i64), i.name.clone()],
            ),
            Mishap::BadLocation { pos, kind } => {
                error(&format!("location_{kind}"), vec![vec_text(*pos)])
            }
            Mishap::BadOffhandItem {
                item: Some(i),
                wanted,
            } => error(
                "bad_item.offhand",
                vec![wanted.clone(), Text::int(i.count as i64), i.name.clone()],
            ),
            Mishap::BadOffhandItem { item: None, wanted } => {
                error("no_item.offhand", vec![wanted.clone()])
            }
            Mishap::EntityTooFar { entity, .. } => {
                error("entity_too_far", vec![entity_name(entity)])
            }
            Mishap::ImmuneEntity { entity, .. } => {
                error("immune_entity", vec![entity_name(entity)])
            }
            Mishap::LackingHotbarItem(wanted) => error("bad_item.hotbar", vec![wanted.clone()]),
            Mishap::WrongDimension(proper) => error(
                "wrong_dimension",
                vec![Text::lit(proper.clone()), Text::lit(env.dim.clone())],
            ),
            Mishap::NoAkashicRecord(pos) => error("no_akashic_record", vec![pos_text(*pos)]),
            Mishap::BoolDirectrixEmptyStack(pos) => {
                error("circle.bool_directrix.empty_stack", vec![pos_text(*pos)])
            }
            Mishap::BoolDirectrixNotBool(perp, pos) => error(
                "circle.bool_directrix.no_bool",
                vec![pos_text(*pos), perp.display()],
            ),
            Mishap::NoSpellCircle => error("no_spell_circle", vec![]),
            Mishap::NotEnoughMedia(_) => Text::key(Lang::MESSAGE_CANT_OVERCAST_KEY),
            Mishap::OthersName { is_self: true, .. } => error("others_name.self", vec![]),
            Mishap::OthersName { name, .. } => error("others_name", vec![Text::lit(name.clone())]),
            _ => return None,
        })
    }

    /// The message of a mishap about the stack and its iotas.
    fn stack_message(&self) -> Option<Text> {
        Some(match self {
            Mishap::DivideByZero { a, b, suffix } => error(
                &format!("divide_by_zero.{suffix}"),
                vec![a.clone(), b.clone()],
            ),
            Mishap::EvalTooMuch => error("eval_too_much", vec![]),
            Mishap::InvalidIota {
                perp,
                rev_idx,
                expected,
            } => error(
                "invalid_value",
                vec![
                    expected.clone(),
                    Text::int(*rev_idx as i64),
                    perp.kind().desc(),
                    perp.display(),
                ],
            ),
            Mishap::InvalidOperatorArgs(perps) if perps.len() == 1 => error(
                "invalid_operator_args.one",
                vec![Text::int(0), perps[0].display()],
            ),
            Mishap::InvalidOperatorArgs(perps) => error(
                "invalid_operator_args.many",
                vec![
                    Text::int(perps.len() as i64),
                    Text::int(0),
                    Text::int(perps.len() as i64 - 1),
                    Text::Join(perps.iter().map(Iota::display).collect(), ", ".into()),
                ],
            ),
            Mishap::InvalidPattern(None) => error("invalid_pattern_generic", vec![]),
            Mishap::InvalidPattern(Some(p)) => {
                error("invalid_pattern", vec![Iota::Pattern(p.clone()).display()])
            }
            Mishap::NeedsParens => error("needs_parens", vec![]),
            Mishap::NotEnoughArgs { expected, got: 0 } => {
                error("no_args", vec![Text::int(*expected as i64)])
            }
            Mishap::NotEnoughArgs { expected, got } => error(
                "not_enough_args",
                vec![Text::int(*expected as i64), Text::int(*got as i64)],
            ),
            Mishap::StackSize => error("stack_size", vec![]),
            Mishap::Unescaped(perp) => error("unescaped", vec![perp.display()]),

            _ => return None,
        })
    }
}
