//! Numbers, vectors, logic and constants (`casting/actions/math`, the
//! arithmetic `OperationAction`s and `Action.makeConstantOp`s).

use crate::casting::actions::{actions, Action};
use crate::casting::args::NumOrVec;
use crate::casting::env::random;
use crate::iota::Iota;
use crate::math::Vec3;

pub fn actions() -> Vec<Action> {
    actions! {
        "add" NE "waaw" => arith;
        "sub" NW "wddw" => arith;
        "mul" SE "waqaw" => arith;
        "div" NE "wdedw" => arith;
        "abs" NE "wqaqw" => arith;
        "pow" NW "wedew" => arith;
        "floor" E "ewq" => arith;
        "ceil" E "qwe" => arith;
        "construct_vec" E "eqqqqq" => arith;
        "deconstruct_vec" E "qeeeee" => arith;
        "coerce_axial" NW "qqqqqaww" => konst [env, v: NumOrVec] {
            match v {
                NumOrVec::Num(n) => Iota::Double(if n == 0.0 || n.is_nan() { n } else { n.signum() }),
                NumOrVec::Vec(v) if v == Vec3::ZERO => Iota::Vec3(v),
                NumOrVec::Vec(v) => Iota::Vec3(v.nearest_axis()),
            }
        };
        "and" NE "wdw" => arith;
        "or" SE "waw" => arith;
        "not" NW "dw" => arith;
        "xor" NW "dwa" => arith;
        "greater" SE "e" => arith;
        "less" SW "q" => arith;
        "greater_eq" SE "ee" => arith;
        "less_eq" SW "qq" => arith;
        "equals" E "ad" => konst [env, a: Iota, b: Iota] { a.tolerates(&b) };
        "not_equals" E "da" => konst [env, a: Iota, b: Iota] { !a.tolerates(&b) };
        "type_equals" E "wawdw" => konst [env, a: Iota, b: Iota] { a.kind() == b.kind() };
        "type_not_equals" E "wdwaw" => konst [env, a: Iota, b: Iota] { a.kind() != b.kind() };
        "bool_coerce" NE "aw" => konst [env, a: Iota] { a.is_truthy() };
        "if" SE "awdd" => konst [env, cond: bool, t: Iota, f: Iota] { if cond { t } else { f } };
        "random" NW "eqqq" => konst [env] { random() };
        "sin" SE "qqqqqaa" => arith;
        "cos" SE "qqqqqad" => arith;
        "tan" SW "wqqqqqadq" => arith;
        "arcsin" SE "ddeeeee" => arith;
        "arccos" NE "adeeeee" => arith;
        "arctan" NE "eadeeeeew" => arith;
        "arctan2" W "deadeeeeewd" => arith;
        "logarithm" NW "eqaqe" => arith;
        "modulo" NE "addwaad" => arith;
        "unique" NE "aweaqa" => arith;

        "const/null" E "d" => konst [env] { Iota::Null };
        "const/true" SE "aqae" => konst [env] { true };
        "const/false" NE "dedq" => konst [env] { false };
        "const/vec/px" NW "qqqqqea" => konst [env] { Vec3::new(1.0, 0.0, 0.0) };
        "const/vec/py" NW "qqqqqew" => konst [env] { Vec3::new(0.0, 1.0, 0.0) };
        "const/vec/pz" NW "qqqqqed" => konst [env] { Vec3::new(0.0, 0.0, 1.0) };
        "const/vec/nx" SW "eeeeeqa" => konst [env] { Vec3::new(-1.0, 0.0, 0.0) };
        "const/vec/ny" SW "eeeeeqw" => konst [env] { Vec3::new(0.0, -1.0, 0.0) };
        "const/vec/nz" SW "eeeeeqd" => konst [env] { Vec3::new(0.0, 0.0, -1.0) };
        "const/vec/0" NW "qqqqq" => konst [env] { Vec3::ZERO };
        "const/double/pi" NE "qdwdq" => konst [env] { std::f64::consts::PI };
        "const/double/tau" NW "eawae" => konst [env] { std::f64::consts::TAU };
        "const/double/e" E "aaq" => konst [env] { std::f64::consts::E };
        "const/double/phi" NE "wdded" => konst [env] { 1.618033988749895 };
    }
}
