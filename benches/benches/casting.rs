//! The casting hot paths: pattern parsing and lookup, the VM running long
//! hexes, and iotas going to and from their stored forms.

mod stub_server;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use hexcasting::casting::actions::{self, Lookup, dir};
use hexcasting::casting::env::{CastEnv, Source, Stack};
use hexcasting::casting::image::Image;
use hexcasting::casting::vm::Vm;
use hexcasting::iota::Iota;
use hexcasting::item::holder;
use hexcasting::math::{HexDir, HexPattern};
use hexcasting::protocol::{Hand, ResolvedType};
use hexcasting::snbt::Nbt;
use stub_server::StubServer;

fn server() -> StubServer {
    let dir = std::env::temp_dir().join("hexcasting-bench");
    std::fs::create_dir_all(&dir).unwrap();
    StubServer { game_dir: dir.to_string_lossy().into_owned() }
}

fn env(srv: &StubServer) -> CastEnv<'_> {
    CastEnv::new(srv, None, Hand::Main, Source::Staff, "minecraft:overworld".into())
}

fn pat(sig: &str, d: HexDir) -> Iota {
    Iota::Pattern(HexPattern::from_angles(sig, d).unwrap())
}

/// Numerical Reflection for a whole number in `0..=100`.
fn num(n: u32) -> Iota {
    let sig = format!("aqaa{}{}", "e".repeat((n / 10) as usize), "w".repeat((n % 10) as usize));
    pat(&sig, dir::E)
}

fn add() -> Iota {
    pat("waaw", dir::NE)
}

fn mul() -> Iota {
    pat("waqaw", dir::SE)
}

/// 1250 patterns on a small stack: arithmetic, Gemini, Jester's and a
/// Bookkeeper's pop.
fn long_hex() -> Vec<Iota> {
    let mut hex = vec![num(1)];
    for _ in 0..250 {
        hex.extend([num(1), add(), pat("aadaa", dir::E), pat("aawdd", dir::E), pat("a", dir::SE)]);
    }
    hex
}

/// 200 numbers pushed, then summed: every pattern sees a deep stack.
fn deep_stack_hex() -> Vec<Iota> {
    let mut hex: Vec<Iota> = (0..200).map(|i| num(i % 100)).collect();
    hex.extend((0..199).map(|_| add()));
    hex
}

/// Thoth's Gambit doubling 100 numbers.
fn thoth_hex() -> Vec<Iota> {
    let mut hex = vec![pat("qqq", dir::W), num(2), mul(), pat("eee", dir::E)];
    hex.extend((0..100).map(|i| num(i % 100)));
    hex.extend([num(100), pat("ewdqdwe", dir::SW), pat("dadad", dir::NE)]);
    hex
}

fn run(srv: &StubServer, hex: Vec<Iota>) -> ResolvedType {
    let mut vm = Vm::new(Image::default(), env(srv));
    vm.queue_execute(hex).kind
}

fn big_list() -> Iota {
    let mut items = Vec::new();
    for i in 0..60 {
        items.push(Iota::Double(i as f64 * 1.5));
        items.push(Iota::vec(i as f64, -2.0, 3.25));
        items.push(pat("waqaw", dir::SE));
        items.push(Iota::entity("069a79f4-44e9-4726-a5be-fca90e38aaf5", "Notch"));
        items.push(Iota::List(vec![Iota::Bool(true), Iota::Null, Iota::Double(0.1)]));
    }
    Iota::List(items)
}

fn vm(c: &mut Criterion) {
    let srv = server();
    for (name, hex) in [
        ("vm/long_hex", long_hex()),
        ("vm/deep_stack", deep_stack_hex()),
        ("vm/thoth", thoth_hex()),
    ] {
        assert!(run(&srv, hex.clone()).success(), "{name} mishapped");
        c.bench_function(name, |b| {
            b.iter_batched(|| hex.clone(), |h| run(&srv, h), BatchSize::SmallInput)
        });
    }
}

fn patterns(c: &mut Criterion) {
    let long = HexPattern::from_angles(&"qaqwedsaeqd".repeat(10), dir::E).unwrap();
    c.bench_function("pattern/signature", |b| b.iter(|| black_box(&long).signature()));
    c.bench_function("pattern/from_angles", |b| {
        b.iter(|| HexPattern::from_angles(black_box("qaqwedsaeqdqaqwedsaeqd"), dir::E))
    });
    c.bench_function("pattern/serialized_round_trip", |b| {
        b.iter(|| HexPattern::from_serialized(&black_box(&long).serialized_form()))
    });
}

fn registry(c: &mut Criterion) {
    let srv = server();
    let env = env(&srv);
    let lookups = [
        ("registry/lookup_action", add()),
        ("registry/lookup_number", num(37)),
        ("registry/lookup_unknown", pat("qqqqqqwwwwwweeeee", dir::E)),
    ];
    for (name, iota) in lookups {
        let Iota::Pattern(p) = iota else { unreachable!() };
        assert!(!matches!(actions::lookup(&p, &env), Lookup::Nothing) || name.ends_with("unknown"));
        c.bench_function(name, |b| b.iter(|| actions::lookup(black_box(&p), &env)));
    }
    c.bench_function("registry/by_id", |b| b.iter(|| actions::by_id(black_box("for_each"))));
}

fn serialization(c: &mut Criterion) {
    let list = big_list();
    let text = list.serialize();
    c.bench_function("iota/serialize", |b| b.iter(|| black_box(&list).serialize()));
    c.bench_function("iota/deserialize", |b| b.iter(|| Iota::deserialize(black_box(&text))));

    let image = Image { stack: vec![list.clone(), Iota::Double(2.0)], ..Image::default() };
    let encoded = image.encode();
    c.bench_function("image/encode", |b| b.iter(|| black_box(&image).encode()));
    c.bench_function("image/decode", |b| b.iter(|| Image::decode(black_box(&encoded))));

    let mut focus = Stack { slot: 0, id: holder::FOCUS.into(), count: 1, nbt: Nbt::default() };
    assert!(holder::write(&mut focus, Some(&list), false));
    let snbt = focus.nbt.to_snbt();
    c.bench_function("nbt/focus_write", |b| {
        b.iter_batched(
            || Stack { nbt: Nbt::default(), ..focus.clone() },
            |mut s| holder::write(&mut s, Some(&list), false),
            BatchSize::SmallInput,
        )
    });
    c.bench_function("nbt/focus_read", |b| b.iter(|| holder::read(black_box(&focus))));
    c.bench_function("nbt/parse", |b| b.iter(|| Nbt::parse(black_box(&snbt))));
    c.bench_function("nbt/to_snbt", |b| b.iter(|| black_box(&focus.nbt).to_snbt()));
}

criterion_group!(benches, vm, patterns, registry, serialization);
criterion_main!(benches);
