//! Escaping, parentheses, meta-evaluation and the Ravenmind
//! (`casting/actions/{escaping,eval,local}`).

use yog_api::sync::Arc;

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Arg, PosInt, evaluatable};
use crate::casting::cont::{Cont, Frame};
use crate::casting::env::{CastEnv, MAX_OP_COUNT};
use crate::casting::image::{Image, ParenIota};
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::{OpResult, Sound};
use crate::iota::Iota;
use crate::protocol::ResolvedType;

type ParenResult = MResult<(OpResult, ResolvedType)>;

pub fn actions() -> Vec<Action> {
    actions! {
        "escape" W "qqqaw" => raw(escape, escape_in_parens);
        "runtime_escape" SE "wdeee" => raw(escape);
        "open_paren" W "qqq" => raw(open_paren, open_paren_in_parens);
        "close_paren" E "eee" => raw(needs_parens, close_paren);
        "open_n_parens" W "qdaqadq" => raw(open_n_parens);
        "close_all_parens" E "eadedae" => raw(needs_parens, close_all_parens);
        "read_into_parens" E "aqqqqqwded" => raw(needs_parens, read_into_parens);
        "undo" E "eeedw" => raw(needs_parens, undo);
        "eval" SE "deaqq" => raw(eval);
        "eval/cc" NW "qwaqde" => raw(eval_cc);
        "halt" SW "aqdee" => raw(halt);
        "for_each" NE "dadad" => raw(for_each);
        "thanatos" SE "qqaed" => raw(thanatos);
        "read/local" NE "qeewdweddw" => raw(read_local);
        "write/local" NW "eqqwawqaaw" => raw(write_local);
    }
}

fn needs_parens(_: &mut CastEnv, _: Image, _: &Cont) -> MResult<OpResult> {
    Err(Mishap::NeedsParens)
}

/// `OpEscape` / `OpRuntimeEscape`.
fn escape(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    image.escape_next = true;
    Ok(OpResult::new(image.with_used_op(), cont))
}

fn escape_in_parens(env: &mut CastEnv, image: Image, cont: &Cont, _: &Iota) -> ParenResult {
    Ok((escape(env, image, cont)?, ResolvedType::Evaluated))
}

/// `OpOpenParen`.
fn open_paren(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    image.paren_count += 1;
    Ok(OpResult::new(image.with_used_op(), cont))
}

fn open_paren_in_parens(
    _: &mut CastEnv,
    mut image: Image,
    cont: &Cont,
    this: &Iota,
) -> ParenResult {
    image.parenthesized.push(ParenIota {
        iota: this.clone(),
        escaped: false,
    });
    image.paren_count += 1;
    Ok((OpResult::new(image, cont), ResolvedType::Escaped))
}

/// `OpCloseParen`.
fn close_paren(_: &mut CastEnv, mut image: Image, cont: &Cont, this: &Iota) -> ParenResult {
    image.paren_count -= 1;
    if image.paren_count == 0 {
        let list = std::mem::take(&mut image.parenthesized)
            .into_iter()
            .map(|p| p.iota)
            .collect();
        image.stack.push(Iota::List(list));
        Ok((
            OpResult::new(image.with_used_op(), cont),
            ResolvedType::Evaluated,
        ))
    } else {
        image.parenthesized.push(ParenIota {
            iota: this.clone(),
            escaped: false,
        });
        Ok((OpResult::new(image, cont), ResolvedType::Escaped))
    }
}

/// `OpOpenNParens`.
fn open_n_parens(env: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let Some(top) = image.stack.last() else {
        return Err(Mishap::NotEnoughArgs {
            expected: 1,
            got: 0,
        });
    };
    let layers = PosInt::get(top, 0, env)?.0;
    image.stack.pop();
    image.paren_count = layers;
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpCloseAllParens`.
fn close_all_parens(_: &mut CastEnv, mut image: Image, cont: &Cont, _: &Iota) -> ParenResult {
    image.stack.push(Iota::Double(image.paren_count as f64));
    let list = std::mem::take(&mut image.parenthesized)
        .into_iter()
        .map(|p| p.iota)
        .collect();
    image.stack.push(Iota::List(list));
    image.paren_count = 0;
    Ok((
        OpResult::new(image.with_used_op(), cont),
        ResolvedType::Evaluated,
    ))
}

/// `OpReadIntoParens`: what the held item holds, escaped into the parens.
fn read_into_parens(env: &mut CastEnv, image: Image, cont: &Cont, _: &Iota) -> ParenResult {
    let datum = super::rw::read_held(env)?;
    Ok((
        OpResult::new(
            image.with_used_op().with_new_parenthesized(datum, true),
            cont,
        ),
        ResolvedType::Evaluated,
    ))
}

/// `OpUndo`: takes back the last pattern drawn inside the parens.
fn undo(_: &mut CastEnv, mut image: Image, cont: &Cont, _: &Iota) -> ParenResult {
    match image.parenthesized.pop() {
        None => image.paren_count = 0,
        Some(ParenIota {
            iota: Iota::Pattern(p),
            escaped: false,
        }) => match p.signature().as_str() {
            "qqq" => image.paren_count -= 1,
            "eee" => image.paren_count += 1,
            _ => {}
        },
        Some(_) => {}
    }
    Ok((
        OpResult::new(image.with_used_op(), cont),
        ResolvedType::Undone,
    ))
}

/// `OpEval.exec`.
fn exec(image: Image, cont: &Cont, iota: &Iota) -> MResult<OpResult> {
    let single = !matches!(iota, Iota::List(_));
    let list = evaluatable(iota, 0)?;
    let cont = if single || matches!(cont.frame(), Some(Frame::FinishEval)) {
        cont.clone()
    } else {
        // A boundary for Charon's Gambit.
        cont.push(Frame::FinishEval)
    };
    let cont = cont.push(Frame::Evaluate {
        list: Arc::new(list),
        idx: 0,
        metacasting: true,
    });
    Ok(OpResult::new(image.with_used_op(), &cont).with_sound(Sound::Hermes))
}

/// `OpEval` (Hermes' Gambit).
fn eval(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let iota = image.stack.pop().ok_or(Mishap::NotEnoughArgs {
        expected: 1,
        got: 0,
    })?;
    exec(image, cont, &iota)
}

/// `OpEvalBreakable` (Iris' Gambit): the rest of the hex goes on the stack.
fn eval_cc(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let iota = image.stack.pop().ok_or(Mishap::NotEnoughArgs {
        expected: 1,
        got: 0,
    })?;
    image.stack.push(Iota::Continuation(cont.clone()));
    exec(image, cont, &iota)
}

/// `OpHalt` (Charon's Gambit).
fn halt(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let mut stack = std::mem::take(&mut image.stack);
    let mut done = false;
    let mut cont = cont.clone();
    while !done {
        let Some(frame) = cont.frame() else { break };
        let (d, s) = frame.break_downwards(stack);
        done = d;
        stack = s;
        cont = cont.next();
    }
    if !done {
        stack.clear();
    }
    image.stack = stack;
    Ok(OpResult::new(image.with_used_op(), &cont).with_sound(Sound::Spell))
}

/// `OpForEach` (Thoth's Gambit).
fn for_each(env: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let n = image.stack.len();
    if n < 2 {
        return Err(Mishap::NotEnoughArgs {
            expected: 2,
            got: n,
        });
    }
    let code = <Vec<Iota>>::get(&image.stack[n - 2], 1, env)?;
    let data = <Vec<Iota>>::get(&image.stack[n - 1], 0, env)?;
    image.stack.truncate(n - 2);
    let frame = Frame::ForEach {
        data: Arc::new(data),
        data_idx: 0,
        code: Arc::new(code),
        base: None,
        acc: Vec::new(),
    };
    Ok(OpResult::new(image.with_used_op(), &cont.push(frame)).with_sound(Sound::Thoth))
}

/// `OpThanos`: how many more operations the hex may take.
fn thanatos(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    image
        .stack
        .push(Iota::Double((MAX_OP_COUNT - image.ops_consumed) as f64));
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpPeekLocal`.
fn read_local(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    image
        .stack
        .push(image.ravenmind.clone().unwrap_or(Iota::Null));
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpPushLocal`.
fn write_local(_: &mut CastEnv, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let iota = image.stack.pop().ok_or(Mishap::NotEnoughArgs {
        expected: 1,
        got: 0,
    })?;
    image.ravenmind = (!matches!(iota, Iota::Null)).then_some(iota);
    Ok(OpResult::new(image.with_used_op(), cont))
}
