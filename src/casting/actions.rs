//! The action registry (Hex Casting's `HexActions`, `PatternRegistryManifest`,
//! `ScrungledPatternsSave` and the special handlers).
//!
//! Every pattern is declared once, in an [`actions!`] table next to its
//! implementation (`casting::ops::*`): its id, the direction it's shown
//! drawn from, its angle signature, and what it does. Patterns mean the
//! same whatever way they're drawn — they're looked up by signature.

use std::collections::HashMap;
use yog_api::sync::{Arc, Mutex, OnceLock};

use crate::casting::cont::Cont;
use crate::casting::env::{CastEnv, MOD_ID, random};
use crate::casting::image::Image;
use crate::casting::mishap::{MResult, Mishap};
use crate::casting::vm::{OpResult, SideEffect, Sound, Spell};
use crate::iota::Iota;
use crate::item::pigment::JavaRandom;
use crate::math::{HexAngle, HexCoord, HexDir, HexPattern};
use crate::protocol::ResolvedType;
use crate::text::{Text, fmt_double};

/// `Action.operate`.
pub type OpFn = fn(&mut CastEnv, Image, &Cont) -> MResult<OpResult>;
/// `Action.operateInParens` (with the iota being executed).
pub type ParenFn = fn(&mut CastEnv, Image, &Cont, &Iota) -> MResult<(OpResult, ResolvedType)>;

/// The start directions, as the tables write them.
pub mod dir {
    use crate::math::HexDir;
    pub const NE: HexDir = HexDir::NorthEast;
    pub const E: HexDir = HexDir::East;
    pub const SE: HexDir = HexDir::SouthEast;
    pub const SW: HexDir = HexDir::SouthWest;
    pub const W: HexDir = HexDir::West;
    pub const NW: HexDir = HexDir::NorthWest;
}

/// Great spells: per-world patterns that need enlightenment
/// (`requires_enlightenment` / `per_world_pattern`).
pub const GREAT: [&str; 14] = [
    "lightning",
    "flight",
    "create_lava",
    "teleport/great",
    "sentinel/create/great",
    "dispel_rain",
    "summon_rain",
    "brainsweep",
    "craft/battery",
    "potion/regeneration",
    "potion/night_vision",
    "potion/absorption",
    "potion/haste",
    "potion/strength",
];

pub struct Action {
    /// `hexcasting:<id>`.
    pub id: &'static str,
    pub dir: HexDir,
    pub sig: &'static str,
    pub op: OpFn,
    pub parens: Option<ParenFn>,
}

impl Action {
    pub fn new(
        id: &'static str,
        dir: HexDir,
        sig: &'static str,
        op: OpFn,
        parens: Option<ParenFn>,
    ) -> Self {
        Action {
            id,
            dir,
            sig,
            op,
            parens,
        }
    }

    pub fn great(&self) -> bool {
        GREAT.contains(&self.id)
    }

    /// `hexcasting.action.hexcasting:<id>`.
    pub fn name(&self) -> Text {
        Text::key(format!("hexcasting.action.{MOD_ID}:{}", self.id))
    }

    /// The pattern as the book draws it (none for a signature that isn't
    /// one: every action's is, as a test checks).
    pub fn pattern(&self) -> Option<HexPattern> {
        HexPattern::from_angles(self.sig, self.dir)
    }

    /// `operateInParens`: by default, the pattern is just collected.
    pub fn operate_in_parens(
        &self,
        env: &mut CastEnv,
        image: Image,
        cont: &Cont,
        this: &Iota,
    ) -> MResult<(OpResult, ResolvedType)> {
        match self.parens {
            Some(f) => f(env, image, cont, this),
            None => Ok((
                OpResult::new(image.with_new_parenthesized(this.clone(), false), cont),
                ResolvedType::Escaped,
            )),
        }
    }
}

#[doc(hidden)]
pub const fn one(_: &str) -> usize {
    1
}

/// `ConstMediaAction.operate`: takes `argc` iotas (deepest first), pushes
/// what `f` gives, and costs `cost`.
pub fn const_op(
    env: &mut CastEnv,
    mut image: Image,
    cont: &Cont,
    argc: usize,
    cost: i64,
    f: impl FnOnce(Vec<Iota>, &mut CastEnv) -> MResult<Vec<Iota>>,
) -> MResult<OpResult> {
    let len = image.stack.len();
    if argc > len {
        return Err(Mishap::NotEnoughArgs {
            expected: argc,
            got: len,
        });
    }
    let args = image.stack.split_off(len - argc);
    let out = f(args, env)?;
    image.stack.extend(out);
    if env.extract_media(cost, true) > 0 {
        return Err(Mishap::NotEnoughMedia(cost));
    }
    image.ops_consumed += 1;
    let mut r = OpResult::new(image, cont);
    r.effects.push(SideEffect::ConsumeMedia(cost));
    Ok(r)
}

/// `SpellAction.operate`.
pub fn spell_op(
    env: &mut CastEnv,
    mut image: Image,
    cont: &Cont,
    argc: usize,
    f: impl FnOnce(Vec<Iota>, &mut CastEnv, &mut Image) -> MResult<Spell>,
) -> MResult<OpResult> {
    let len = image.stack.len();
    if argc > len {
        return Err(Mishap::NotEnoughArgs {
            expected: argc,
            got: len,
        });
    }
    let args = image.stack.split_off(len - argc);
    let spell = f(args, env, &mut image)?;
    if env.extract_media(spell.cost, true) > 0 {
        return Err(Mishap::NotEnoughMedia(spell.cost));
    }
    let mut effects = Vec::new();
    if spell.cost > 0 {
        effects.push(SideEffect::ConsumeMedia(spell.cost));
    }
    effects.push(SideEffect::AttemptSpell(spell.effect));
    effects.extend(spell.particles.into_iter().map(SideEffect::Particles));
    image.ops_consumed += spell.ops;
    let sound = if spell.sound {
        Sound::Spell
    } else {
        Sound::Mute
    };
    Ok(OpResult {
        image,
        effects,
        cont: cont.clone(),
        sound,
    })
}

/// A table of actions:
///
/// ```ignore
/// actions! {
///     "swap" E "aawdd" => konst [env, a: Iota, b: Iota] { (b, a) };
///     "raycast" E "wqaawdd" => konst(DUST / 100) [env, from: Vec3, dir: Vec3] { ... };
///     "explode" E "aawaawaa" => spell [env, img; pos: Vec3, power: f64] { ... };
///     "add" NE "waaw" => arith;
///     "close_paren" E "eee" => raw(needs_parens, close_paren);
/// }
/// ```
///
/// * `konst(cost)` is a `ConstMediaAction` (cost 0 if left out): the body's
///   value is pushed ([`Ret`](crate::casting::args::Ret));
/// * `spell` is a `SpellAction`: the body gives `MResult<Spell>`, and may
///   look at the image (`img`);
/// * `arith` goes to the arithmetic engine;
/// * `raw(operate[, operate_in_parens])` takes the functions as they are.
///
/// Arguments are taken deepest first and type-checked
/// ([`Arg`](crate::casting::args::Arg)), each mishapping with its index
/// from the top of the stack.
macro_rules! actions {
    ($($id:literal $dir:ident $sig:literal => $kind:ident $(($($p:expr),* $(,)?))?
        $([$($param:tt)*] $body:block)?;)*) => {
        vec![$(
            $crate::casting::actions::action!(@$kind ($id, $dir, $sig) [$($($p),*)?] $([$($param)*] $body)?)
        ),*]
    };
}

#[doc(hidden)]
macro_rules! action {
    (@konst $head:tt [] $($rest:tt)*) => {
        $crate::casting::actions::action!(@konst $head [0] $($rest)*)
    };
    (@konst ($id:literal, $dir:ident, $sig:literal) [$cost:expr]
        [$env:ident $(, $arg:ident : $ty:ty)* $(,)?] $body:block) => {{
        // One shape for every action: one without arguments leaves the counter
        // and `out`'s `mut` unused, and a cost already an `i64` is cast again.
        #[allow(unused_variables, unused_mut, unused_assignments, clippy::unnecessary_cast)]
        fn op(
            env: &mut $crate::casting::env::CastEnv,
            image: $crate::casting::image::Image,
            cont: &$crate::casting::cont::Cont,
        ) -> $crate::casting::mishap::MResult<$crate::casting::vm::OpResult> {
            const ARGC: usize = 0 $(+ $crate::casting::actions::one(stringify!($arg)))*;
            $crate::casting::actions::const_op(env, image, cont, ARGC, ($cost) as i64, |args, $env| {
                let mut i = 0usize;
                $(
                    let $arg: $ty = <$ty as $crate::casting::args::Arg>::get(&args[i], ARGC - 1 - i, $env)?;
                    i += 1;
                )*
                let mut out = Vec::new();
                $crate::casting::args::Ret::push($body, &mut out)?;
                Ok(out)
            })
        }
        $crate::casting::actions::Action::new($id, $crate::casting::actions::dir::$dir, $sig, op, None)
    }};
    (@spell ($id:literal, $dir:ident, $sig:literal) []
        [$env:ident, $img:ident $(; $($arg:ident : $ty:ty),* $(,)?)?] $body:block) => {{
        // One shape for every spell: one without arguments leaves the counter unused.
        #[allow(unused_variables, unused_mut, unused_assignments)]
        fn op(
            env: &mut $crate::casting::env::CastEnv,
            image: $crate::casting::image::Image,
            cont: &$crate::casting::cont::Cont,
        ) -> $crate::casting::mishap::MResult<$crate::casting::vm::OpResult> {
            const ARGC: usize = 0 $($(+ $crate::casting::actions::one(stringify!($arg)))*)?;
            $crate::casting::actions::spell_op(env, image, cont, ARGC, |args, $env, $img| {
                let mut i = 0usize;
                $($(
                    let $arg: $ty = <$ty as $crate::casting::args::Arg>::get(&args[i], ARGC - 1 - i, $env)?;
                    i += 1;
                )*)?
                $body
            })
        }
        $crate::casting::actions::Action::new($id, $crate::casting::actions::dir::$dir, $sig, op, None)
    }};
    (@arith ($id:literal, $dir:ident, $sig:literal) []) => {{
        fn op(
            env: &mut $crate::casting::env::CastEnv,
            image: $crate::casting::image::Image,
            cont: &$crate::casting::cont::Cont,
        ) -> $crate::casting::mishap::MResult<$crate::casting::vm::OpResult> {
            $crate::casting::arith::run($id, env, image, cont)
        }
        $crate::casting::actions::Action::new($id, $crate::casting::actions::dir::$dir, $sig, op, None)
    }};
    (@raw ($id:literal, $dir:ident, $sig:literal) [$op:expr]) => {
        $crate::casting::actions::Action::new($id, $crate::casting::actions::dir::$dir, $sig, $op, None)
    };
    (@raw ($id:literal, $dir:ident, $sig:literal) [$op:expr, $parens:expr]) => {
        $crate::casting::actions::Action::new($id, $crate::casting::actions::dir::$dir, $sig, $op, Some($parens))
    };
}

pub(crate) use {action, actions};

// ── the registry ──────────────────────────────────────────────────────────

struct Registry {
    all: Vec<Action>,
    /// Signature → index, for everything but the great spells.
    normal: HashMap<&'static str, usize>,
    /// Id → index.
    by_id: HashMap<&'static str, usize>,
}

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| {
        use crate::casting::ops;
        let mut all = Vec::new();
        all.extend(ops::query::actions());
        all.extend(ops::stack::actions());
        all.extend(ops::math::actions());
        all.extend(ops::spells::actions());
        all.extend(ops::great::actions());
        all.extend(ops::meta::actions());
        all.extend(ops::rw::actions());
        all.extend(ops::lists::actions());
        // Other mods' patterns, when they're loaded.
        all.extend(ops::interop::actions());
        let mut normal = HashMap::new();
        for (i, a) in all.iter().enumerate() {
            if !a.great() {
                normal.insert(a.sig, i);
            }
        }
        let by_id = all.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        Registry { all, normal, by_id }
    })
}

/// Every action, in registration order.
pub fn all() -> &'static [Action] {
    &registry().all
}

pub fn by_id(id: &str) -> Option<&'static Action> {
    let reg = registry();
    reg.by_id.get(id).map(|&i| &reg.all[i])
}

/// `PatternShapeMatch`.
pub enum Lookup {
    Action(&'static Action),
    Special(Special),
    Nothing,
}

/// `PatternRegistryManifest.matchPattern`: normal actions, then this
/// world's great spells, then the special handlers.
pub fn lookup(pattern: &HexPattern) -> Lookup {
    let sig = pattern.signature();
    let reg = registry();
    if let Some(&i) = reg.normal.get(sig.as_str()) {
        return Lookup::Action(&reg.all[i]);
    }
    if let Some(a) = per_world().get(&sig) {
        return Lookup::Action(a);
    }
    match Special::number(pattern).or_else(|| Special::mask(pattern)) {
        Some(s) => Lookup::Special(s),
        None => Lookup::Nothing,
    }
}

// ── great spells: drawn differently in every world ────────────────────────

const SEED_KEY: &str = "scrungled_patterns.seed";

/// This world's great spells, by the signature they're drawn with here.
pub struct PerWorld {
    world_dir: String,
    pub seed: i64,
    by_sig: HashMap<String, &'static Action>,
}

impl PerWorld {
    pub fn get(&self, sig: &str) -> Option<&'static Action> {
        self.by_sig.get(sig).copied()
    }
}

/// This world's great spells. The seed is read (or made) once per world;
/// lookups after that never touch the disk.
pub fn per_world() -> Arc<PerWorld> {
    static CACHE: Mutex<Option<Arc<PerWorld>>> = Mutex::new(None);
    let world_dir = yog_api::event::game::world_dir().unwrap_or_default();
    let mut cache = CACHE.lock();
    if let Some(pw) = cache.as_ref()
        && pw.world_dir == world_dir
    {
        return pw.clone();
    }
    let store = yog_api::storage::world();
    let seed = store
        .as_ref()
        .and_then(|s| s.get::<i64>(SEED_KEY))
        .unwrap_or_else(|| {
            let s =
                (random() * (1u64 << 52) as f64) as i64 ^ (random() * (1u64 << 52) as f64) as i64;
            if let Some(store) = &store {
                store.set(SEED_KEY, &s);
            }
            s
        });
    let by_sig = all()
        .iter()
        .filter(|a| a.great())
        .filter_map(|a| Some((alt_drawing(&a.pattern()?, seed).signature(), a)))
        .collect();
    let pw = Arc::new(PerWorld {
        world_dir,
        seed,
        by_sig,
    });
    *cache = Some(pw.clone());
    pw
}

/// How a great spell is drawn in this world (`getCanonicalStrokesPerWorld`).
pub fn per_world_pattern(id: &str) -> Option<HexPattern> {
    let a = by_id(id)?;
    let pattern = a.pattern().filter(|_| a.great())?;
    Some(alt_drawing(&pattern, per_world().seed))
}

/// `EulerPathFinder.findAltDrawing`: another way to draw the same shape,
/// picked by `seed`.
pub fn alt_drawing(original: &HexPattern, seed: i64) -> HexPattern {
    let mut rand = JavaRandom::new(seed);
    walk_path(original, &mut rand).unwrap_or_else(|| original.clone())
}

fn walk_path(original: &HexPattern, rand: &mut JavaRandom) -> Option<HexPattern> {
    // Each point and the strokes leaving it, in first-visited order.
    let mut graph: Vec<(HexCoord, Vec<HexDir>)> = Vec::new();
    fn exits(graph: &mut Vec<(HexCoord, Vec<HexDir>)>, at: HexCoord) -> &mut Vec<HexDir> {
        let i = match graph.iter().position(|(c, _)| *c == at) {
            Some(i) => i,
            None => {
                graph.push((at, Vec::new()));
                graph.len() - 1
            }
        };
        &mut graph[i].1
    }
    fn add(graph: &mut Vec<(HexCoord, Vec<HexDir>)>, at: HexCoord, d: HexDir) {
        let e = exits(graph, at);
        if !e.contains(&d) {
            e.push(d);
            e.sort_by_key(|d| d.ordinal());
        }
    }
    let mut cursor = HexCoord::default();
    for d in original.directions() {
        add(&mut graph, cursor, d);
        add(
            &mut graph,
            cursor + d.as_delta(),
            d.rotated_by(HexAngle::OneEighty),
        );
        cursor = cursor + d.as_delta();
    }
    let odd: Vec<HexCoord> = graph
        .iter()
        .filter(|(_, e)| e.len() % 2 == 1)
        .map(|(c, _)| *c)
        .collect();
    let mut current = match odd.len() {
        0 => graph[rand.next_int(graph.len() as i32) as usize].0,
        2 => odd[rand.next_int(2) as usize],
        _ => return None,
    };
    let mut stack = Vec::new();
    let mut out = Vec::new();
    loop {
        let here = exits(&mut graph, current);
        if here.is_empty() {
            out.push(current);
            current = stack.pop()?;
        } else {
            stack.push(current);
            let burn = here.remove(rand.next_int(here.len() as i32) as usize);
            let next = current + burn.as_delta();
            let back = burn.rotated_by(HexAngle::OneEighty);
            exits(&mut graph, next).retain(|d| *d != back);
            current = next;
        }
        if exits(&mut graph, current).is_empty() && stack.is_empty() {
            break;
        }
    }
    out.push(current);
    let dirs: Option<Vec<HexDir>> = out
        .windows(2)
        .map(|w| {
            HexDir::ALL
                .into_iter()
                .find(|d| w[0] + d.as_delta() == w[1])
        })
        .collect();
    let dirs = dirs?;
    (!dirs.is_empty()).then(|| HexPattern::from_list(dirs))
}

// ── special handlers ──────────────────────────────────────────────────────

/// `SpecialHandler`s: Numerical Reflection and Bookkeeper's Gambit.
#[derive(Debug, Clone, PartialEq)]
pub enum Special {
    Number(f64),
    /// `true` keeps the iota, `false` drops it (deepest first).
    Mask(Vec<bool>),
}

impl Special {
    /// `SpecialHandlerNumberLiteral.Factory.tryMatch`.
    pub fn number(p: &HexPattern) -> Option<Self> {
        let sig = p.signature();
        let negate = sig.starts_with("dedd");
        if !negate && !sig.starts_with("aqaa") {
            return None;
        }
        let mut acc = 0.0f64;
        for c in sig[4..].chars() {
            match c {
                'w' => acc += 1.0,
                'q' => acc += 5.0,
                'e' => acc += 10.0,
                'a' => acc *= 2.0,
                'd' => acc /= 2.0,
                _ => {}
            }
        }
        Some(Special::Number(if negate { -acc } else { acc }))
    }

    /// `SpecialHandlerMask.Factory.tryMatch`.
    pub fn mask(p: &HexPattern) -> Option<Self> {
        let dirs = p.directions();
        let angles = p.angles();
        let mut flat = p.start;
        if angles.first() == Some(&HexAngle::TwoForty) {
            flat = dirs[0].rotated_by(HexAngle::ThreeHundred);
        }
        let mut mask = Vec::new();
        let mut i = 0;
        while i < dirs.len() {
            let a = dirs[i].angle_from(flat);
            if a == HexAngle::Zero {
                mask.push(true);
                i += 1;
                continue;
            }
            if i + 1 >= dirs.len() {
                return None;
            }
            let a2 = dirs[i + 1].angle_from(flat);
            if a == HexAngle::Sixty && a2 == HexAngle::ThreeHundred {
                mask.push(false);
                i += 2;
                continue;
            }
            return None;
        }
        Some(Special::Mask(mask))
    }

    /// `hexcasting.special.hexcasting:<handler>`.
    pub fn name(&self) -> Text {
        match self {
            Special::Number(x) => Text::key_args(
                format!("hexcasting.special.{MOD_ID}:number"),
                vec![Text::lit(fmt_double(*x))],
            ),
            Special::Mask(m) => {
                let fp: String = m.iter().map(|&k| if k { '-' } else { 'v' }).collect();
                Text::key_args(
                    format!("hexcasting.special.{MOD_ID}:mask"),
                    vec![Text::lit(fp)],
                )
            }
        }
    }

    pub fn operate(&self, env: &mut CastEnv, image: Image, cont: &Cont) -> MResult<OpResult> {
        match self {
            Special::Number(x) => {
                const_op(env, image, cont, 0, 0, |_, _| Ok(vec![Iota::Double(*x)]))
            }
            Special::Mask(m) => const_op(env, image, cont, m.len(), 0, |args, _| {
                Ok(args
                    .into_iter()
                    .zip(m)
                    .filter(|(_, k)| **k)
                    .map(|(a, _)| a)
                    .collect())
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(sig: &str, d: HexDir) -> HexPattern {
        HexPattern::from_angles(sig, d).unwrap()
    }

    #[test]
    fn numbers() {
        assert_eq!(
            Special::number(&pat("aqaaw", dir::E)),
            Some(Special::Number(1.0))
        );
        assert_eq!(
            Special::number(&pat("deddqa", dir::E)),
            Some(Special::Number(-10.0))
        );
        assert_eq!(
            Special::number(&pat("aqaaeda", dir::E)),
            Some(Special::Number(10.0))
        );
        assert_eq!(Special::number(&pat("qaq", dir::E)), None);
    }

    #[test]
    fn masks() {
        // Bookkeeper's: "-" is a straight stroke, "v" a dip.
        assert_eq!(
            Special::mask(&HexPattern::from_list(vec![dir::E])),
            Some(Special::Mask(vec![true]))
        );
        assert_eq!(
            Special::mask(&pat("ea", dir::E)),
            Some(Special::Mask(vec![true, false]))
        );
        assert_eq!(
            Special::mask(&pat("ada", dir::SE)),
            Some(Special::Mask(vec![false, false]))
        );
    }

    #[test]
    fn pehkuis_patterns_fit_in() {
        // Without Pehkui (no runtime here) they aren't registered; their signatures
        // still mustn't clash with the rest.
        let sigs: std::collections::HashSet<&str> = all().iter().map(|a| a.sig).collect();
        for a in crate::casting::ops::interop::pehkui() {
            assert!(HexPattern::from_angles(a.sig, a.dir).is_some(), "{}", a.id);
            assert!(!sigs.contains(a.sig), "{} clashes", a.id);
        }
    }

    #[test]
    fn every_signature_is_a_pattern() {
        for a in all() {
            assert!(a.pattern().is_some(), "{}: {}", a.id, a.sig);
        }
    }

    #[test]
    fn registry_is_consistent() {
        let mut sigs = std::collections::HashSet::new();
        for a in all() {
            assert!(HexPattern::from_angles(a.sig, a.dir).is_some(), "{}", a.id);
            assert!(sigs.insert(a.sig), "duplicate signature {}", a.id);
        }
        for g in GREAT {
            assert!(by_id(g).is_some(), "{g}");
        }
    }

    #[test]
    fn alt_drawings_keep_the_shape() {
        let a = pat("waadwawdaaweewq", dir::E);
        let b = alt_drawing(&a, 12345);
        let edges = |p: &HexPattern| {
            let pts = p.positions();
            let mut e: Vec<_> = pts
                .windows(2)
                .map(|w| {
                    if (w[0].0, w[0].1) < (w[1].0, w[1].1) {
                        (w[0], w[1])
                    } else {
                        (w[1], w[0])
                    }
                })
                .map(|(x, y)| (x - pts[0], y - pts[0]))
                .collect();
            e.sort_by_key(|(x, y)| (x.0, x.1, y.0, y.1));
            e
        };
        assert_eq!(b.steps.len(), a.steps.len());
        // Same edges, up to where the drawing starts.
        let (ea, eb) = (edges(&a), edges(&b));
        let shift = |e: &[(HexCoord, HexCoord)], by: HexCoord| -> Vec<(HexCoord, HexCoord)> {
            let mut v: Vec<_> = e.iter().map(|(x, y)| (*x - by, *y - by)).collect();
            v.sort_by_key(|(x, y)| (x.0, x.1, y.0, y.1));
            v
        };
        let min = |e: &[(HexCoord, HexCoord)]| e.iter().map(|(x, _)| (x.0, x.1)).min().unwrap();
        let (ma, mb) = (min(&ea), min(&eb));
        assert_eq!(
            shift(&ea, HexCoord(ma.0, ma.1)),
            shift(&eb, HexCoord(mb.0, mb.1))
        );
    }
}
