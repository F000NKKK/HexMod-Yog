//! Lists (`casting/actions/lists` and the list `OperationAction`s).

use crate::casting::actions::{actions, Action};
use crate::casting::args::{pos_int_under_incl, Many};
use crate::casting::cont::Cont;
use crate::casting::env::CastEnv;
use crate::casting::image::Image;
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::OpResult;
use crate::iota::Iota;

pub fn actions() -> Vec<Action> {
    actions! {
        "append" SW "edqde" => arith;
        "unappend" NW "qaeaq" => arith;
        "index" NW "deeed" => arith;
        "singleton" E "adeeed" => konst [env, x: Iota] { Iota::List(vec![x]) };
        "empty_list" NE "qqaeaae" => konst [env] { Iota::List(Vec::new()) };
        "reverse" E "qqqaede" => arith;
        "last_n_list" SW "ewdqdwe" => raw(last_n_list);
        "splat" NW "qwaeawq" => konst [env, l: Vec<Iota>] { Many(l) };
        "index_of" E "dedqde" => arith;
        "remove_from" SW "edqdewaqa" => arith;
        "slice" NW "qaeaqwded" => arith;
        "replace" NW "wqaeaqw" => arith;
        "construct" SE "ddewedd" => arith;
        "deconstruct" SW "aaqwqaa" => arith;
    }
}

/// `OpLastNToList` (Flock's Gambit).
fn last_n_list(_: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let stack = &mut image.stack;
    let Some(top) = stack.last() else {
        return Err(Mishap::NotEnoughArgs { expected: 1, got: 0 });
    };
    let n = pos_int_under_incl(top, 0, stack.len() as i64 - 1)? as usize;
    stack.pop();
    let list = stack.split_off(stack.len() - n);
    stack.push(Iota::List(list));
    Ok(OpResult::new(image.with_used_op(), cont))
}
