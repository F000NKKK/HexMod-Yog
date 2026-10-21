//! The casting hot paths: pattern parsing and lookup, the VM running long
//! hexes, and iotas going to and from their stored forms.
//!
//! Every benchmark first checks that what it times gives the right answer —
//! a hex leaves the stack it should, a round trip gives back what went in —
//! so a regression shows as a failure here, not as a fast wrong result.

use std::hint::black_box;

use criterion::{BatchSize, Criterion};
use hexcasting::casting::actions::{self, Lookup, dir};
use hexcasting::casting::env::{CastEnv, Source, Stack};
use hexcasting::casting::image::Image;
use hexcasting::casting::vm::Vm;
use hexcasting::iota::Iota;
use hexcasting::item::holder;
use hexcasting::math::{HexDir, HexPattern};
use hexcasting::protocol::Hand;
use hexcasting::snbt::Nbt;
use yog_api::yog_ids::game::OVERWORLD;

/// The patterns the hexes are made of: signature and start direction.
mod sig {
    /// Numerical Reflection's prefix (a positive number); then `e` adds 10, `w` adds 1.
    pub const NUMBER: &str = "aqaa";
    pub const TEN: char = 'e';
    pub const ONE: char = 'w';
    /// Additive Distillation.
    pub const ADD: &str = "waaw";
    /// Multiplicative Distillation.
    pub const MUL: &str = "waqaw";
    /// Gemini Decomposition: duplicate the top.
    pub const DUPLICATE: &str = "aadaa";
    /// Jester's Gambit: swap the top two.
    pub const SWAP: &str = "aawdd";
    /// Bookkeeper's Gambit `v`: drop the top.
    pub const DROP: &str = "a";
    /// Introspection / Retrospection: quote the patterns between them.
    pub const OPEN: &str = "qqq";
    pub const CLOSE: &str = "eee";
    /// Flock's Gambit: the top n iotas as a list.
    pub const FLOCK: &str = "ewdqdwe";
    /// Thoth's Gambit: a hex run on each of a list's iotas.
    pub const THOTH: &str = "dadad";
    /// No pattern of the game's.
    pub const UNKNOWN: &str = "qqqqqqwwwwwweeeee";
    /// A long pattern, for parsing.
    pub const LONG: &str = "qaqwedsaeqd";
}

/// The largest number [`num`] writes.
const MAX_NUM: u32 = 100;
/// Rounds of the long hex: one more each.
const LONG_ROUNDS: u32 = 250;
/// Numbers summed by the deep-stack hex.
const DEEP: u32 = 200;
/// Numbers doubled by Thoth's Gambit.
const THOTH_COUNT: u32 = 100;
/// Entries of each kind in the serialised list.
const LIST_ENTRIES: u32 = 60;
/// The player stored as an entity iota.
const PLAYER_UUID: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
const PLAYER_NAME: &str = "Notch";
/// The action looked up by id.
const ACTION_ID: &str = "for_each";

fn env() -> CastEnv {
    CastEnv::new(None, Hand::Main, Source::Staff, OVERWORLD.into())
}

fn pattern(sig: &str, d: HexDir) -> HexPattern {
    HexPattern::from_angles(sig, d).unwrap_or_else(|| panic!("`{sig}` is a valid pattern"))
}

fn pat(sig: &str, d: HexDir) -> Iota {
    Iota::Pattern(pattern(sig, d))
}

/// Numerical Reflection for a whole number in `0..=MAX_NUM`.
fn num(n: u32) -> Iota {
    assert!(n <= MAX_NUM, "{n} is above {MAX_NUM}");
    let tens = sig::TEN.to_string().repeat((n / 10) as usize);
    let ones = sig::ONE.to_string().repeat((n % 10) as usize);
    pat(&format!("{}{tens}{ones}", sig::NUMBER), dir::E)
}

fn add() -> Iota {
    pat(sig::ADD, dir::NE)
}

fn mul() -> Iota {
    pat(sig::MUL, dir::SE)
}

/// `n`, as the VM leaves numbers.
fn double(n: u32) -> Iota {
    Iota::Double(f64::from(n))
}

/// A hex and the stack it leaves.
struct Hex {
    name: &'static str,
    patterns: Vec<Iota>,
    leaves: Vec<Iota>,
}

/// 1250 patterns on a small stack: each round adds 1, duplicates, swaps and
/// drops — the stack ends as `1 + rounds`.
fn long_hex() -> Hex {
    let mut patterns = vec![num(1)];
    for _ in 0..LONG_ROUNDS {
        patterns.extend([
            num(1),
            add(),
            pat(sig::DUPLICATE, dir::E),
            pat(sig::SWAP, dir::E),
            pat(sig::DROP, dir::SE),
        ]);
    }
    Hex {
        name: "vm/long_hex",
        patterns,
        leaves: vec![double(1 + LONG_ROUNDS)],
    }
}

/// `DEEP` numbers pushed, then summed: every pattern sees a deep stack.
fn deep_stack_hex() -> Hex {
    let numbers: Vec<u32> = (0..DEEP).map(|i| i % MAX_NUM).collect();
    let mut patterns: Vec<Iota> = numbers.iter().map(|&n| num(n)).collect();
    patterns.extend((1..DEEP).map(|_| add()));
    Hex {
        name: "vm/deep_stack",
        patterns,
        leaves: vec![double(numbers.iter().sum())],
    }
}

/// Thoth's Gambit doubling `THOTH_COUNT` numbers.
fn thoth_hex() -> Hex {
    let numbers: Vec<u32> = (0..THOTH_COUNT).map(|i| i % MAX_NUM).collect();
    let mut patterns = vec![
        pat(sig::OPEN, dir::W),
        num(2),
        mul(),
        pat(sig::CLOSE, dir::E),
    ];
    patterns.extend(numbers.iter().map(|&n| num(n)));
    patterns.extend([
        num(THOTH_COUNT),
        pat(sig::FLOCK, dir::SW),
        pat(sig::THOTH, dir::NE),
    ]);
    let doubled = numbers.iter().map(|&n| double(2 * n)).collect();
    Hex {
        name: "vm/thoth",
        patterns,
        leaves: vec![Iota::List(doubled)],
    }
}

/// The stack `patterns` leave, or why they didn't run.
fn run(patterns: Vec<Iota>) -> Result<Vec<Iota>, String> {
    let mut vm = Vm::new(Image::default(), env());
    let kind = vm.queue_execute(patterns).kind;
    if kind.success() {
        Ok(vm.image.stack)
    } else {
        Err(format!("{kind:?}"))
    }
}

fn big_list() -> Iota {
    let mut items = Vec::new();
    for i in 0..LIST_ENTRIES {
        let x = f64::from(i);
        items.push(Iota::Double(x * 1.5));
        items.push(Iota::vec(x, -2.0, 3.25));
        items.push(mul());
        items.push(Iota::entity(PLAYER_UUID, PLAYER_NAME));
        items.push(Iota::List(vec![
            Iota::Bool(true),
            Iota::Null,
            Iota::Double(0.1),
        ]));
    }
    Iota::List(items)
}

#[yog_api::bench]
fn vm(c: &mut Criterion) {
    for hex in [long_hex(), deep_stack_hex(), thoth_hex()] {
        assert_eq!(
            run(hex.patterns.clone()).as_deref(),
            Ok(hex.leaves.as_slice()),
            "{}",
            hex.name
        );
        c.bench_function(hex.name, |b| {
            b.iter_batched(
                || hex.patterns.clone(),
                run,
                BatchSize::SmallInput,
            )
        });
    }
}

#[yog_api::bench]
fn patterns(c: &mut Criterion) {
    let long = pattern(&sig::LONG.repeat(10), dir::E);
    let doubled = sig::LONG.repeat(2);
    assert_eq!(
        HexPattern::from_serialized(&long.serialized_form()).as_ref(),
        Some(&long)
    );
    c.bench_function("pattern/signature", |b| {
        b.iter(|| black_box(&long).signature())
    });
    c.bench_function("pattern/from_angles", |b| {
        b.iter(|| HexPattern::from_angles(black_box(&doubled), dir::E))
    });
    c.bench_function("pattern/serialized_round_trip", |b| {
        b.iter(|| HexPattern::from_serialized(&black_box(&long).serialized_form()))
    });
}

#[yog_api::bench]
fn registry(c: &mut Criterion) {
    let lookups = [
        ("registry/lookup_action", pattern(sig::ADD, dir::NE), true),
        (
            "registry/lookup_number",
            match num(37) {
                Iota::Pattern(p) => p,
                _ => unreachable!("num makes a pattern"),
            },
            true,
        ),
        (
            "registry/lookup_unknown",
            pattern(sig::UNKNOWN, dir::E),
            false,
        ),
    ];
    for (name, p, known) in lookups {
        assert_eq!(
            !matches!(actions::lookup(&p), Lookup::Nothing),
            known,
            "{name}"
        );
        c.bench_function(name, |b| b.iter(|| actions::lookup(black_box(&p))));
    }
    assert!(
        actions::by_id(ACTION_ID).is_some(),
        "{ACTION_ID} is an action"
    );
    c.bench_function("registry/by_id", |b| {
        b.iter(|| actions::by_id(black_box(ACTION_ID)))
    });
}

#[yog_api::bench]
fn serialization(c: &mut Criterion) {
    let list = big_list();
    let text = list.serialize();
    assert_eq!(
        Iota::deserialize(&text),
        list,
        "an iota survives its serialised form"
    );
    c.bench_function("iota/serialize", |b| {
        b.iter(|| black_box(&list).serialize())
    });
    c.bench_function("iota/deserialize", |b| {
        b.iter(|| Iota::deserialize(black_box(&text)))
    });

    let image = Image {
        stack: vec![list.clone(), double(2)],
        ..Image::default()
    };
    let encoded = image.encode();
    assert_eq!(
        Image::decode(&encoded),
        image,
        "an image survives its encoding"
    );
    c.bench_function("image/encode", |b| b.iter(|| black_box(&image).encode()));
    c.bench_function("image/decode", |b| {
        b.iter(|| Image::decode(black_box(&encoded)))
    });

    let mut focus = Stack {
        slot: 0,
        id: holder::FOCUS.into(),
        count: 1,
        nbt: Nbt::default(),
    };
    assert!(
        holder::write(&mut focus, Some(&list), false),
        "a focus holds a list"
    );
    assert_eq!(
        holder::read(&focus).as_ref(),
        Some(&list),
        "a focus gives back what it holds"
    );
    let snbt = focus.nbt.to_snbt();
    assert_eq!(Nbt::parse(&snbt), focus.nbt, "NBT survives SNBT");
    c.bench_function("nbt/focus_write", |b| {
        b.iter_batched(
            || Stack {
                nbt: Nbt::default(),
                ..focus.clone()
            },
            |mut s| holder::write(&mut s, Some(&list), false),
            BatchSize::SmallInput,
        )
    });
    c.bench_function("nbt/focus_read", |b| {
        b.iter(|| holder::read(black_box(&focus)))
    });
    c.bench_function("nbt/parse", |b| b.iter(|| Nbt::parse(black_box(&snbt))));
    c.bench_function("nbt/to_snbt", |b| {
        b.iter(|| black_box(&focus.nbt).to_snbt())
    });
}
