//! Reading and writing iotas in items, entities and the akashic library
//! (`casting/actions/{rw,akashic}`).

use crate::akashic;
use crate::casting::actions::{Action, actions};
use crate::casting::args::{Block, Ent};
use crate::casting::env::{CastEnv, Spray, Stack};
use crate::casting::mishap::{MResult, Mishap, true_name_in};
use crate::casting::vm::Spell;
use crate::iota::Iota;
use crate::item::holder::{self, EntityHolder};
use crate::item::media::DUST;
use crate::math::{HexPattern, Vec3};

pub fn actions() -> Vec<Action> {
    actions! {
        "read" E "aqqqqq" => konst [env] { read_held(env)? };
        "read/entity" E "wawqwqwqwqwqw" => konst [env, e: Ent] {
            e.in_range(env)?;
            entity_holder(env, &e)
                .and_then(|(s, _)| holder::read(&s))
                .ok_or_else(|| Mishap::bad_entity(env, &e.r, "iota.read"))?
        };
        "write" E "deeeee" => spell [env, img; datum: Iota] { write_held(env, datum) };
        "write/entity" E "wdwewewewewew" => spell [env, img; e: Ent, datum: Iota] { write_entity(env, e, datum) };
        "readable" E "aqqqqqe" => konst [env] {
            env.held_item_to_operate_on(holder::is_holder).is_some_and(|(s, _)| holder::read(&s).is_some())
        };
        "readable/entity" E "wawqwqwqwqwqwew" => konst [env, e: Ent] {
            e.in_range(env)?;
            entity_holder(env, &e).is_some_and(|(s, _)| holder::read(&s).is_some())
        };
        "writable" E "deeeeeq" => konst [env] {
            env.held_item_to_operate_on(holder::is_holder).is_some_and(|(s, _)| holder::writeable(&s))
        };
        "writable/entity" E "wdwewewewewewqw" => konst [env, e: Ent] {
            e.in_range(env)?;
            entity_holder(env, &e).is_some_and(|(s, _)| holder::writeable(&s))
        };
        "akashic/read" W "qqqwqqqqqaq" => konst(DUST) [env, pos: Block, key: HexPattern] {
            if env.block_id(pos.0) != akashic::RECORD {
                return Err(Mishap::NoAkashicRecord(pos.0));
            }
            akashic::lookup(env.srv, &env.dim, pos.0, &key)
        };
        "akashic/write" E "eeeweeeeede" => spell [env, img; pos: Block, key: HexPattern, datum: Iota] {
            let pos = pos.0;
            env.assert_pos_in_range(pos)?;
            if env.block_id(pos) != akashic::RECORD {
                return Err(Mishap::NoAkashicRecord(pos));
            }
            others_name(env, &datum, env.caster_uuid().as_deref())?;
            Ok(Spell::new(DUST, vec![], move |env, _| {
                akashic::add_new_datum(env.srv, &env.dim, pos, &key, &datum);
                let c = Vec3::center_of(pos);
                env.srv.play_sound(&env.dim, c.x, c.y, c.z, crate::sounds::SCROLL_SCRIBBLE, 1.0, 0.8);
            }))
        };
    }
}

/// `MishapOthersName`: no writing other players' true names down.
pub fn others_name(env: &CastEnv, datum: &Iota, caster: Option<&str>) -> MResult<()> {
    match true_name_in(env, datum, caster) {
        Some(e) => {
            let is_self = env.caster_uuid().as_deref() == Some(e.uuid.as_str());
            Err(Mishap::OthersName {
                name: e.name,
                is_self,
            })
        }
        None => Ok(()),
    }
}

/// `OpRead` (and `OpReadIntoParens`): what the held item holds.
pub fn read_held(env: &CastEnv) -> MResult<Iota> {
    let (stack, _) = env
        .held_item_to_operate_on(|s| holder::is_holder(s) && holder::read(s).is_some())
        // A holder that can't be read, for a more helpful message.
        .or_else(|| env.held_item_to_operate_on(holder::is_holder))
        .ok_or_else(|| Mishap::bad_offhand(None, "iota.read", vec![]))?;
    holder::read(&stack).ok_or_else(|| Mishap::bad_offhand(stack.shown(), "iota.read", vec![]))
}

fn can_write(stack: &Stack, datum: &Iota) -> bool {
    holder::write(&mut stack.clone(), Some(datum), true)
}

/// `OpWrite`.
fn write_held(env: &mut CastEnv, datum: Iota) -> MResult<Spell> {
    let (stack, _) = env
        .held_item_to_operate_on(|s| can_write(s, &datum))
        .or_else(|| env.held_item_to_operate_on(holder::is_holder))
        .ok_or_else(|| Mishap::bad_offhand(None, "iota.write", vec![]))?;
    if !can_write(&stack, &datum) {
        return Err(Mishap::bad_offhand(
            stack.shown(),
            "iota.readonly",
            vec![datum.display()],
        ));
    }
    others_name(env, &datum, env.caster_uuid().as_deref())?;
    let slot = stack.slot;
    Ok(Spell::new(0, vec![], move |env, _| {
        let mut s = env.stack_at(slot);
        if holder::write(&mut s, Some(&datum), false) {
            env.save(&s);
        }
    }))
}

fn entity_holder(env: &CastEnv, e: &Ent) -> Option<(Stack, EntityHolder)> {
    holder::entity_stack(env.srv, e.uuid(), &e.info.type_id).filter(|(s, _)| holder::is_holder(s))
}

/// `OpTheCoolerWrite`.
fn write_entity(env: &mut CastEnv, e: Ent, datum: Iota) -> MResult<Spell> {
    e.in_range(env)?;
    let bad = || Mishap::bad_entity(env, &e.r, "iota.write");
    let (stack, _) = entity_holder(env, &e).ok_or_else(bad)?;
    if !can_write(&stack, &datum) {
        return Err(bad());
    }
    // Not even the caster's own name: no remote true names via sentinels.
    others_name(env, &datum, None)?;
    let mut burst = e.pos();
    if e.is(yog_api::core::entity_flags::ITEM) {
        // The item is drawn well above the entity's position.
        burst.y += 3.0 / 8.0;
    }
    let spray = Spray {
        pos: burst,
        vel: Vec3::new(1.0, 0.0, 0.0),
        fuzziness: 0.25,
        spread: std::f64::consts::PI,
        count: 40,
    };
    let uuid = e.uuid().to_owned();
    let type_id = e.info.type_id.clone();
    Ok(Spell::new(0, vec![spray], move |env, _| {
        if let Some((mut s, how)) = holder::entity_stack(env.srv, &uuid, &type_id)
            && holder::write(&mut s, Some(&datum), false)
        {
            holder::save_entity_stack(env.srv, &uuid, &s, how);
        }
    }))
}
