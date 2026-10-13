//! Stack manipulation (`casting/actions/stack`).

use crate::casting::actions::{actions, Action};
use crate::casting::args::{int_between, Arg, Many, PosInt};
use crate::casting::cont::Cont;
use crate::casting::env::CastEnv;
use crate::casting::image::Image;
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::OpResult;
use crate::iota::{Iota, MAX_SERIALIZATION_TOTAL};

pub fn actions() -> Vec<Action> {
    actions! {
        // OpTwiddling
        "swap" E "aawdd" => konst [env, a: Iota, b: Iota] { (b, a) };
        "rotate" E "aaeaa" => konst [env, a: Iota, b: Iota, c: Iota] { (b, c, a) };
        "rotate_reverse" NE "ddqdd" => konst [env, a: Iota, b: Iota, c: Iota] { (c, a, b) };
        "duplicate" E "aadaa" => konst [env, a: Iota] { (a.clone(), a) };
        "over" E "aaedd" => konst [env, a: Iota, b: Iota] { (a.clone(), b, a) };
        "tuck" E "ddqaa" => konst [env, a: Iota, b: Iota] { (b.clone(), a, b) };
        "2dup" E "aadadaaw" => konst [env, a: Iota, b: Iota] { Many(vec![a.clone(), b.clone(), a, b]) };
        "stack_len" NW "qwaeawqaeaqa" => raw(stack_len);
        "duplicate_n" E "aadaadaa" => konst [env, x: Iota, n: PosInt] {
            Many(vec![x; n.0.min(MAX_SERIALIZATION_TOTAL as i64) as usize])
        };
        "fisherman" W "ddad" => raw(fisherman);
        "fisherman/copy" E "aada" => raw(fisherman_copy);
        "swizzle" SE "qaawdde" => raw(swizzle);
    }
}

/// `OpStackSize`.
fn stack_len(_: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    image.stack.push(Iota::Double(image.stack.len() as f64));
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpFisherman`: moves the iota `n` down to the top (or, negative, the
/// top down to `-n`).
fn fisherman(_: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let stack = &mut image.stack;
    if stack.len() < 2 {
        return Err(Mishap::NotEnoughArgs { expected: 2, got: stack.len() });
    }
    let x = stack.pop().unwrap();
    let max = stack.len() as i64 - 1;
    let depth = int_between(&x, 0, -max, max)?;
    if depth >= 0 {
        let fish = stack.remove(stack.len() - 1 - depth as usize);
        stack.push(fish);
    } else {
        let lure = stack.pop().unwrap();
        let at = (stack.len() as i64 + depth) as usize;
        stack.insert(at, lure);
    }
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpFishermanButItCopies`.
fn fisherman_copy(_: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let stack = &mut image.stack;
    if stack.len() < 2 {
        return Err(Mishap::NotEnoughArgs { expected: 2, got: stack.len() });
    }
    let max = stack.len() as i64 - 2;
    let depth = int_between(stack.last().unwrap(), 0, -max, max)?;
    stack.pop();
    if depth >= 0 {
        let fish = stack[stack.len() - 1 - depth as usize].clone();
        stack.push(fish);
    } else {
        let lure = stack.last().unwrap().clone();
        let at = (stack.len() as i64 - 1 + depth) as usize;
        stack.insert(at, lure);
    }
    Ok(OpResult::new(image.with_used_op(), cont))
}

/// `OpAlwinfyHasAscendedToABeingOfPureMath`: permutes the top of the stack
/// by the permutation numbered `code` (in the factorial number system).
fn swizzle(env: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let stack = &mut image.stack;
    let Some(top) = stack.last() else {
        return Err(Mishap::NotEnoughArgs { expected: 1, got: 0 });
    };
    let code = PosInt::get(top, 0, env)?.0;
    stack.pop();
    permute(stack, code)?;
    Ok(OpResult::new(image.with_used_op(), cont))
}

fn permute(stack: &mut [Iota], code: i64) -> MResult<()> {
    // 0!, 1!, 2!, … up to `code`.
    let mut strides = Vec::new();
    let (mut acc, mut n) = (1i64, 1i64);
    while acc <= code {
        strides.push(acc);
        match acc.checked_mul(n) {
            Some(next) => acc = next,
            None => break,
        }
        n += 1;
    }
    if strides.len() > stack.len() {
        return Err(Mishap::NotEnoughArgs { expected: strides.len() + 1, got: stack.len() + 1 });
    }
    let base = stack.len() - strides.len();
    let mut swap: Vec<Iota> = stack[base..].to_vec();
    let mut radix = code;
    for (k, divisor) in strides.iter().rev().enumerate() {
        let index = (radix / divisor) as usize;
        radix %= divisor;
        stack[base + k] = swap.remove(index);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums(v: &[f64]) -> Vec<Iota> {
        v.iter().map(|&d| Iota::Double(d)).collect()
    }

    #[test]
    fn swizzle_codes() {
        // Code 1 swaps the top two; 5 reverses the top three.
        let mut s = nums(&[0.0, 1.0, 2.0]);
        permute(&mut s, 1).unwrap();
        assert_eq!(s, nums(&[0.0, 2.0, 1.0]));
        let mut s = nums(&[0.0, 1.0, 2.0]);
        permute(&mut s, 5).unwrap();
        assert_eq!(s, nums(&[2.0, 1.0, 0.0]));
        let mut s = nums(&[0.0]);
        assert!(permute(&mut s, 2).is_err());
    }
}
