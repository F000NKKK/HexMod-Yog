//! The arithmetic engine (Hex Casting's `ArithmeticEngine` and its
//! `DoubleArithmetic`, `Vec3Arithmetic`, `ListArithmetic`, `BoolArithmetic`,
//! `ListSetArithmetic` and `BitwiseSetArithmetic`, tried in that order).
//!
//! An operator pattern takes a fixed number of iotas; the first arithmetic
//! that accepts their types runs, and if none does it's
//! `MishapInvalidOperatorArgs`.

use crate::casting::args::{pos_int_under, pos_int_under_incl, rounded};
use crate::casting::cont::Cont;
use crate::casting::env::CastEnv;
use crate::casting::image::Image;
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::OpResult;
use crate::iota::{Iota, TOLERANCE};
use crate::math::Vec3;
use crate::text::Text;

/// How many iotas each operator takes.
pub fn arity(op: &str) -> usize {
    match op {
        "abs" | "floor" | "ceil" | "deconstruct_vec" | "not" | "sin" | "cos" | "tan" | "arcsin" | "arccos"
        | "arctan" | "unique" | "unappend" | "reverse" | "deconstruct" => 1,
        "construct_vec" | "slice" | "replace" => 3,
        _ => 2,
    }
}

/// `ArithmeticEngine.run`.
pub fn run(op: &str, _env: &mut CastEnv<'_>, mut image: Image, cont: &Cont) -> MResult<OpResult> {
    let n = arity(op);
    let len = image.stack.len();
    if len < n {
        return Err(Mishap::NotEnoughArgs { expected: n, got: len });
    }
    let args = image.stack.split_off(len - n);
    match apply(op, &args)? {
        Some(out) => {
            image.stack.extend(out);
            Ok(OpResult::new(image.with_used_op(), cont))
        }
        None => Err(Mishap::InvalidOperatorArgs(args)),
    }
}

fn num(i: &Iota) -> Option<f64> {
    match i {
        Iota::Double(d) => Some(*d),
        _ => None,
    }
}

fn nums(a: &[Iota]) -> Option<Vec<f64>> {
    a.iter().map(num).collect()
}

fn bools(a: &[Iota]) -> Option<Vec<bool>> {
    a.iter()
        .map(|i| match i {
            Iota::Bool(b) => Some(*b),
            _ => None,
        })
        .collect()
}

/// `IotaMultiPredicate.any(VEC3, DOUBLE)`: all vectors or numbers, at least
/// one vector.
fn vec_args(a: &[Iota]) -> Option<Vec<Vec3>> {
    if !a.iter().any(|i| matches!(i, Iota::Vec3(_))) {
        return None;
    }
    a.iter()
        .map(|i| match i {
            Iota::Vec3(v) => Some(*v),
            Iota::Double(d) => Some(Vec3::splat(*d)),
            _ => None,
        })
        .collect()
}

fn one(i: Iota) -> MResult<Option<Vec<Iota>>> {
    Ok(Some(vec![i]))
}

fn dbl(d: f64) -> MResult<Option<Vec<Iota>>> {
    one(Iota::Double(d))
}

fn div0(a: f64, b: f64, suffix: &'static str) -> Mishap {
    Mishap::divide_by_zero(&Iota::Double(a), &Iota::Double(b), suffix)
}

/// `DoubleArithmetic`'s binary operators.
fn double2(op: &str, a: f64, b: f64) -> MResult<Option<f64>> {
    Ok(Some(match op {
        "add" => a + b,
        "sub" => a - b,
        "mul" => a * b,
        "div" => {
            if b == 0.0 {
                return Err(div0(a, b, "divide"));
            }
            a / b
        }
        "pow" => {
            if a < 0.0 && (b.floor() - b).abs() >= TOLERANCE {
                return Err(div0(a, b, "exponent"));
            }
            a.powf(b)
        }
        "arctan2" => a.atan2(b),
        "logarithm" => {
            if a <= 0.0 || b <= 0.0 || b == 1.0 {
                return Err(div0(a, b, "logarithm"));
            }
            a.ln() / b.ln()
        }
        "modulo" => {
            if b == 0.0 {
                return Err(div0(a, b, "divide"));
            }
            a % b
        }
        _ => return Ok(None),
    }))
}

/// `DoubleArithmetic`'s unary operators.
fn double1(op: &str, a: f64) -> MResult<Option<f64>> {
    let between = |a: f64| {
        if (-1.0..=1.0).contains(&a) {
            Ok(a)
        } else {
            Err(Mishap::invalid(&Iota::Double(a), 0, "double.between", vec![Text::num(-1.0), Text::num(1.0)]))
        }
    };
    Ok(Some(match op {
        "abs" => a.abs(),
        "floor" => a.floor(),
        "ceil" => a.ceil(),
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => {
            if a.cos() == 0.0 {
                return Err(Mishap::tan(a));
            }
            a.tan()
        }
        "arcsin" => between(a)?.asin(),
        "arccos" => between(a)?.acos(),
        "arctan" => a.atan(),
        _ => return Ok(None),
    }))
}

/// Kotlin's `roundToLong` (ties up).
fn round_long(d: f64) -> i64 {
    (d + 0.5).floor() as i64
}

fn tolerates_any(x: &Iota, l: &[Iota]) -> bool {
    l.iter().any(|y| x.tolerates(y))
}

/// The operator `op` on `a` (deepest first); `Ok(None)` if no arithmetic
/// takes these types.
pub fn apply(op: &str, a: &[Iota]) -> MResult<Option<Vec<Iota>>> {
    // DoubleArithmetic
    if let Some(d) = nums(a) {
        match d.len() {
            1 => {
                if let Some(r) = double1(op, d[0])? {
                    return dbl(r);
                }
            }
            2 => {
                if let Some(r) = double2(op, d[0], d[1])? {
                    return dbl(r);
                }
            }
            _ => {}
        }
    }

    // Vec3Arithmetic
    if op == "construct_vec" {
        if let Some(d) = nums(a) {
            return one(Iota::vec(d[0], d[1], d[2]));
        }
    }
    if let Some(v) = vec_args(a) {
        match (op, v.as_slice()) {
            ("deconstruct_vec", [v]) => {
                return Ok(Some(vec![Iota::Double(v.x), Iota::Double(v.y), Iota::Double(v.z)]));
            }
            ("abs", [v]) => return dbl(v.length()),
            ("floor", [v]) => return one(Iota::Vec3(v.map(f64::floor))),
            ("ceil", [v]) => return one(Iota::Vec3(v.map(f64::ceil))),
            ("add" | "sub" | "mul" | "div" | "pow" | "modulo", [l, r]) => {
                let both = matches!((&a[0], &a[1]), (Iota::Vec3(_), Iota::Vec3(_)));
                if both {
                    match op {
                        "mul" => return dbl(l.dot(*r)),
                        "div" => return one(Iota::Vec3(l.cross(*r))),
                        "pow" => {
                            let n = r.normalize();
                            return one(Iota::Vec3(n * l.dot(n)));
                        }
                        _ => {}
                    }
                }
                // OperatorVec3Delegating: component by component, a
                // division by zero blamed on the whole operands.
                let comp = |x: f64, y: f64| -> MResult<f64> {
                    double2(op, x, y)
                        .map(|r| r.unwrap_or(f64::NAN))
                        .map_err(|e| match e {
                            Mishap::DivideByZero { suffix, .. } => Mishap::divide_by_zero(&a[0], &a[1], suffix),
                            other => other,
                        })
                };
                return one(Iota::vec(comp(l.x, r.x)?, comp(l.y, r.y)?, comp(l.z, r.z)?));
            }
            _ => {}
        }
    }

    // ListArithmetic
    match (op, a) {
        ("index", [Iota::List(l), Iota::Double(i)]) => {
            let idx = round_long(*i);
            return one(usize::try_from(idx).ok().and_then(|i| l.get(i).cloned()).unwrap_or(Iota::Null));
        }
        ("slice", [Iota::List(l), Iota::Double(_), Iota::Double(_)]) => {
            let i0 = pos_int_under_incl(&a[1], 1, l.len() as i64)? as usize;
            let i1 = pos_int_under_incl(&a[2], 0, l.len() as i64)? as usize;
            return one(Iota::List(l[i0.min(i1)..i0.max(i1)].to_vec()));
        }
        ("append", [Iota::List(l), x]) => {
            let mut l = l.clone();
            l.push(x.clone());
            return one(Iota::List(l));
        }
        ("unappend", [Iota::List(l)]) => {
            let mut l = l.clone();
            let last = l.pop().unwrap_or(Iota::Null);
            return Ok(Some(vec![Iota::List(l), last]));
        }
        ("add", [Iota::List(x), Iota::List(y)]) => {
            let mut l = x.clone();
            l.extend(y.iter().cloned());
            return one(Iota::List(l));
        }
        ("abs", [Iota::List(l)]) => return dbl(l.len() as f64),
        ("reverse", [Iota::List(l)]) => return one(Iota::List(l.iter().rev().cloned().collect())),
        ("index_of", [Iota::List(l), x]) => {
            return dbl(l.iter().position(|y| x.tolerates(y)).map_or(-1.0, |i| i as f64));
        }
        ("remove_from", [Iota::List(l), Iota::Double(_)]) => {
            let i = rounded(&a[1]).ok_or_else(|| Mishap::invalid(&a[1], 0, "int", vec![]))?;
            let mut l = l.clone();
            if (0..l.len() as i64).contains(&i) {
                l.remove(i as usize);
            }
            return one(Iota::List(l));
        }
        ("replace", [Iota::List(l), Iota::Double(_), x]) => {
            let i = pos_int_under(&a[1], 1, l.len() as i64)? as usize;
            let mut l = l.clone();
            l[i] = x.clone();
            return one(Iota::List(l));
        }
        ("construct", [Iota::List(l), x]) => {
            let mut out = Vec::with_capacity(l.len() + 1);
            out.push(x.clone());
            out.extend(l.iter().cloned());
            return one(Iota::List(out));
        }
        ("deconstruct", [Iota::List(l)]) => {
            return Ok(Some(match l.split_first() {
                Some((car, cdr)) => vec![Iota::List(cdr.to_vec()), car.clone()],
                None => vec![Iota::List(Vec::new()), Iota::Null],
            }));
        }
        _ => {}
    }

    // BoolArithmetic
    if let Some(b) = bools(a) {
        match (op, b.as_slice()) {
            ("and", [x, y]) => return one(Iota::Bool(*x && *y)),
            ("or", [x, y]) => return one(Iota::Bool(*x || *y)),
            ("xor", [x, y]) => return one(Iota::Bool(x ^ y)),
            ("not", [x]) => return one(Iota::Bool(!x)),
            ("abs", [x]) => return dbl(if *x { 1.0 } else { 0.0 }),
            _ => {}
        }
    }
    if let Some(d) = nums(a) {
        if let [x, y] = d.as_slice() {
            let (x, y) = (*x, *y);
            let eq = (x - y).abs() < TOLERANCE;
            match op {
                "greater" => return one(Iota::Bool(x > y)),
                "less" => return one(Iota::Bool(x < y)),
                "greater_eq" => return one(Iota::Bool(eq || x >= y)),
                "less_eq" => return one(Iota::Bool(eq || x <= y)),
                _ => {}
            }
        }
    }

    // ListSetArithmetic
    match (op, a) {
        ("and", [Iota::List(x), Iota::List(y)]) => {
            return one(Iota::List(x.iter().filter(|i| tolerates_any(i, y)).cloned().collect()));
        }
        ("or", [Iota::List(x), Iota::List(y)]) => {
            let mut out = x.clone();
            out.extend(y.iter().filter(|i| !tolerates_any(i, x)).cloned());
            return one(Iota::List(out));
        }
        ("xor", [Iota::List(x), Iota::List(y)]) => {
            let mut out: Vec<Iota> = x.iter().filter(|i| !tolerates_any(i, y)).cloned().collect();
            out.extend(y.iter().filter(|i| !tolerates_any(i, x)).cloned());
            return one(Iota::List(out));
        }
        ("unique", [Iota::List(l)]) => {
            let mut out: Vec<Iota> = Vec::new();
            for i in l {
                if !tolerates_any(i, &out) {
                    out.push(i.clone());
                }
            }
            return one(Iota::List(out));
        }
        _ => {}
    }

    // BitwiseSetArithmetic
    if let Some(d) = nums(a) {
        let l: Vec<i64> = d.iter().map(|x| round_long(*x)).collect();
        match (op, l.as_slice()) {
            ("and", [x, y]) => return dbl((x & y) as f64),
            ("or", [x, y]) => return dbl((x | y) as f64),
            ("xor", [x, y]) => return dbl((x ^ y) as f64),
            ("not", [x]) => return dbl(!x as f64),
            _ => {}
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(x: f64) -> Iota {
        Iota::Double(x)
    }

    #[test]
    fn numbers_and_vectors() {
        assert_eq!(apply("add", &[d(1.0), d(2.0)]).unwrap(), Some(vec![d(3.0)]));
        assert_eq!(apply("mul", &[Iota::vec(1.0, 2.0, 3.0), d(2.0)]).unwrap(), Some(vec![Iota::vec(2.0, 4.0, 6.0)]));
        assert_eq!(apply("mul", &[Iota::vec(1.0, 2.0, 3.0), Iota::vec(1.0, 1.0, 1.0)]).unwrap(), Some(vec![d(6.0)]));
        assert!(matches!(apply("div", &[d(1.0), d(0.0)]), Err(Mishap::DivideByZero { .. })));
        assert!(matches!(
            apply("div", &[Iota::vec(1.0, 1.0, 1.0), d(0.0)]),
            Err(Mishap::DivideByZero { suffix: "divide", .. })
        ));
        assert_eq!(apply("abs", &[Iota::vec(3.0, 4.0, 0.0)]).unwrap(), Some(vec![d(5.0)]));
        assert_eq!(apply("construct_vec", &[d(1.0), d(2.0), d(3.0)]).unwrap(), Some(vec![Iota::vec(1.0, 2.0, 3.0)]));
    }

    #[test]
    fn lists_bools_and_bits() {
        let l = Iota::List(vec![d(1.0), d(2.0), d(1.0)]);
        assert_eq!(apply("abs", std::slice::from_ref(&l)).unwrap(), Some(vec![d(3.0)]));
        assert_eq!(apply("unique", std::slice::from_ref(&l)).unwrap(), Some(vec![Iota::List(vec![d(1.0), d(2.0)])]));
        assert_eq!(apply("index", &[l.clone(), d(5.0)]).unwrap(), Some(vec![Iota::Null]));
        assert_eq!(apply("and", &[d(6.0), d(3.0)]).unwrap(), Some(vec![d(2.0)]));
        assert_eq!(apply("and", &[Iota::Bool(true), Iota::Bool(false)]).unwrap(), Some(vec![Iota::Bool(false)]));
        assert_eq!(apply("greater_eq", &[d(1.0), d(1.00001)]).unwrap(), Some(vec![Iota::Bool(true)]));
        assert_eq!(apply("add", &[d(1.0), Iota::Bool(true)]).unwrap(), None);
        assert!(matches!(apply("slice", &[l, d(0.0), d(9.0)]), Err(Mishap::InvalidIota { rev_idx: 0, .. })));
    }
}
