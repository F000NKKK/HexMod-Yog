//! Typed action arguments and results (Hex Casting's `OperatorUtils`
//! getters and `asActionResult`).
//!
//! An argument type checks the iota and says what it wanted when it isn't
//! one (`MishapInvalidIota`, with the argument's index counted from the top
//! of the stack).

use yog_api::core::{entity_flags, EntityInfo};

use crate::casting::env::CastEnv;
use crate::casting::mishap::{BlockPos, MResult, Mishap};
use crate::iota::{EntityRef, Iota, TOLERANCE};
use crate::math::{HexPattern, Vec3};

pub trait Arg: Sized {
    fn get(iota: &Iota, rev: usize, env: &CastEnv) -> MResult<Self>;
}

impl Arg for Iota {
    fn get(iota: &Iota, _: usize, _: &CastEnv) -> MResult<Self> {
        Ok(iota.clone())
    }
}

impl Arg for f64 {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Double(d) => Ok(*d),
            _ => Err(Mishap::of_type(iota, rev, "double")),
        }
    }
}

impl Arg for bool {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Bool(b) => Ok(*b),
            _ => Err(Mishap::of_type(iota, rev, "boolean")),
        }
    }
}

impl Arg for Vec3 {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Vec3(v) => Ok(*v),
            _ => Err(Mishap::of_type(iota, rev, "vector")),
        }
    }
}

impl Arg for Vec<Iota> {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::List(l) => Ok(l.clone()),
            _ => Err(Mishap::of_type(iota, rev, "list")),
        }
    }
}

impl Arg for HexPattern {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Pattern(p) => Ok(p.clone()),
            _ => Err(Mishap::of_type(iota, rev, "pattern")),
        }
    }
}

/// `getBlockPos`: the block a vector is in.
pub struct Block(pub BlockPos);

impl Arg for Block {
    fn get(iota: &Iota, rev: usize, env: &CastEnv) -> MResult<Self> {
        Vec3::get(iota, rev, env).map(|v| Block(v.block()))
    }
}

/// The integer a number iota is (within tolerance), rounded like Kotlin's
/// `roundToInt`.
pub fn rounded(iota: &Iota) -> Option<i64> {
    match iota {
        Iota::Double(d) => {
            // Kotlin's `roundToInt`: ties go up.
            let r = (d + 0.5).floor();
            ((d - r).abs() <= TOLERANCE && r.abs() < 9.0e18).then_some(r as i64)
        }
        _ => None,
    }
}

/// `getInt` / `getLong`.
pub struct Int(pub i64);

impl Arg for Int {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        rounded(iota).map(Int).ok_or_else(|| Mishap::invalid(iota, rev, "int", vec![]))
    }
}

/// `getPositiveInt` / `getPositiveLong`.
pub struct PosInt(pub i64);

impl Arg for PosInt {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        rounded(iota)
            .filter(|&n| n >= 0)
            .map(PosInt)
            .ok_or_else(|| Mishap::invalid(iota, rev, "int.positive", vec![]))
    }
}

/// `getPositiveDouble`.
pub struct PosDouble(pub f64);

impl Arg for PosDouble {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Double(d) if *d >= 0.0 => Ok(PosDouble(*d)),
            _ => Err(Mishap::invalid(iota, rev, "double.positive", vec![])),
        }
    }
}

/// `getIntBetween(min, max)` for an iota already taken off the stack.
pub fn int_between(iota: &Iota, rev: usize, min: i64, max: i64) -> MResult<i64> {
    rounded(iota).filter(|n| (min..=max).contains(n)).ok_or_else(|| {
        Mishap::invalid(iota, rev, "int.between", vec![crate::text::Text::int(min), crate::text::Text::int(max)])
    })
}

/// `getPositiveIntUnder(max)` (`max` excluded).
pub fn pos_int_under(iota: &Iota, rev: usize, max: i64) -> MResult<i64> {
    rounded(iota)
        .filter(|n| (0..max).contains(n))
        .ok_or_else(|| Mishap::invalid(iota, rev, "int.positive.less", vec![crate::text::Text::int(max)]))
}

/// `getPositiveIntUnderInclusive(max)`.
pub fn pos_int_under_incl(iota: &Iota, rev: usize, max: i64) -> MResult<i64> {
    rounded(iota).filter(|n| (0..=max).contains(n)).ok_or_else(|| {
        Mishap::invalid(iota, rev, "int.positive.less.equal", vec![crate::text::Text::int(max)])
    })
}

/// `getDoubleBetween(min, max)`.
pub fn double_between(iota: &Iota, rev: usize, min: f64, max: f64) -> MResult<f64> {
    match iota {
        Iota::Double(d) if (min..=max).contains(d) => Ok(*d),
        _ => Err(Mishap::invalid(
            iota,
            rev,
            "double.between",
            vec![crate::text::Text::num(min), crate::text::Text::num(max)],
        )),
    }
}

/// `getNumOrVec`.
pub enum NumOrVec {
    Num(f64),
    Vec(Vec3),
}

impl Arg for NumOrVec {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        match iota {
            Iota::Double(d) => Ok(NumOrVec::Num(*d)),
            Iota::Vec3(v) => Ok(NumOrVec::Vec(*v)),
            _ => Err(Mishap::invalid(iota, rev, "numvec", vec![])),
        }
    }
}

/// `getLongOrList`.
pub enum LongOrList {
    Long(i64),
    List(Vec<Iota>),
}

impl Arg for LongOrList {
    fn get(iota: &Iota, rev: usize, _: &CastEnv) -> MResult<Self> {
        if let Iota::List(l) = iota {
            return Ok(LongOrList::List(l.clone()));
        }
        rounded(iota).map(LongOrList::Long).ok_or_else(|| Mishap::invalid(iota, rev, "numlist", vec![]))
    }
}

/// `evaluatable`: a list of iotas, or one executable iota.
pub fn evaluatable(iota: &Iota, rev: usize) -> MResult<Vec<Iota>> {
    match iota {
        Iota::List(l) => Ok(l.clone()),
        i if i.executable() => Ok(vec![i.clone()]),
        i => Err(Mishap::invalid(i, rev, "evaluatable", vec![])),
    }
}

// ── entities ───────────────────────────────────────────────────────────────

/// A loaded entity, with what the server says about it.
#[derive(Debug, Clone)]
pub struct Ent {
    pub r: EntityRef,
    pub info: EntityInfo,
}

impl Ent {
    pub fn uuid(&self) -> &str {
        &self.r.uuid
    }

    pub fn pos(&self) -> Vec3 {
        Vec3::from_tuple(self.info.pos)
    }

    pub fn eye(&self) -> Vec3 {
        Vec3::from_tuple(self.info.eye_pos())
    }

    pub fn is(&self, flag: u32) -> bool {
        self.info.has(flag)
    }

    pub fn iota(&self) -> Iota {
        Iota::Entity(self.r.clone())
    }

    /// `env.assertEntityInRange`.
    pub fn in_range(&self, env: &CastEnv) -> MResult<()> {
        env.assert_entity_in_range(&self.r, &self.info)
    }
}

fn entity(iota: &Iota, rev: usize, env: &CastEnv, flags: u32, deny: u32, name: &str) -> MResult<Ent> {
    if let Iota::Entity(r) = iota {
        if let Some(info) = env.srv.entity_info(&r.uuid) {
            if info.flags & flags == flags && info.flags & deny == 0 {
                // The name it's shown with is whatever it's called now.
                let r = EntityRef { uuid: r.uuid.clone(), name: info.name.clone() };
                return Ok(Ent { r, info });
            }
        }
    }
    Err(Mishap::of_type(iota, rev, name))
}

impl Arg for Ent {
    fn get(iota: &Iota, rev: usize, env: &CastEnv) -> MResult<Self> {
        entity(iota, rev, env, 0, 0, "entity")
    }
}

macro_rules! entity_kind {
    ($(#[$m:meta])* $name:ident, $flags:expr, $deny:expr, $class:literal) => {
        $(#[$m])*
        pub struct $name(pub Ent);

        impl Arg for $name {
            fn get(iota: &Iota, rev: usize, env: &CastEnv) -> MResult<Self> {
                entity(iota, rev, env, $flags, $deny, $class).map($name)
            }
        }

        impl std::ops::Deref for $name {
            type Target = Ent;
            fn deref(&self) -> &Ent {
                &self.0
            }
        }
    };
}

entity_kind!(
    /// `getLivingEntityButNotArmorStand`.
    Living, entity_flags::LIVING, entity_flags::ARMOR_STAND, "entity.living"
);
entity_kind!(
    /// `getPlayer`.
    Player, entity_flags::PLAYER, 0, "entity.player"
);
entity_kind!(
    /// `getItemEntity`.
    ItemEnt, entity_flags::ITEM, 0, "entity.item"
);
entity_kind!(
    /// `getMob`.
    Mob, entity_flags::MOB, 0, "entity.mob"
);

// ── results ────────────────────────────────────────────────────────────────

/// What an action pushes (`asActionResult`).
pub trait Ret {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()>;
}

impl Ret for Iota {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        stack.push(self);
        Ok(())
    }
}

impl Ret for () {
    fn push(self, _: &mut Vec<Iota>) -> MResult<()> {
        Ok(())
    }
}

impl Ret for f64 {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        Iota::Double(self).push(stack)
    }
}

impl Ret for bool {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        Iota::Bool(self).push(stack)
    }
}

impl Ret for Vec3 {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        Iota::Vec3(self).push(stack)
    }
}

/// `null` for `None`.
impl<T: Ret> Ret for Option<T> {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        match self {
            Some(v) => v.push(stack),
            None => Iota::Null.push(stack),
        }
    }
}

/// Several iotas, pushed one after another (not as a list).
pub struct Many(pub Vec<Iota>);

impl Ret for Many {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        stack.extend(self.0);
        Ok(())
    }
}

impl<T: Ret> Ret for MResult<T> {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        self?.push(stack)
    }
}

impl<A: Ret, B: Ret> Ret for (A, B) {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        self.0.push(stack)?;
        self.1.push(stack)
    }
}

impl<A: Ret, B: Ret, C: Ret> Ret for (A, B, C) {
    fn push(self, stack: &mut Vec<Iota>) -> MResult<()> {
        self.0.push(stack)?;
        self.1.push(stack)?;
        self.2.push(stack)
    }
}

/// `BlockPos.asActionResult`: the block's centre.
pub fn block_center(pos: BlockPos) -> Iota {
    Iota::Vec3(Vec3::center_of(pos))
}
