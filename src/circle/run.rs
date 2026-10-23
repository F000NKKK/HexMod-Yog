//! A run: from the impetus round its slates, a pattern each, back to it.

use super::impetus::{Impetus, KNOWN, Run, TICKS, impetus, save_impetus};
use super::triggers::slate_pattern;
use super::{
    Dir, Flow, Kind, MAX_LENGTH, Part, SOUND_FAIL, SOUND_FIND_BLOCK, relative, set_energized,
};
use crate::casting::env::{
    CastEnv, Caster, CircleRun, Source, Spray, get_pigment, random, to_ypos,
};
use crate::casting::image::Image;
use crate::casting::mishap::{BlockPos, Context, Mishap, dye};
use crate::casting::vm::Vm;
use crate::client::text::Text;
use crate::encoding::protocol::Hand;
use crate::iota::Iota;
use crate::item::pigment::{Pigment, dye_pigment};
use crate::math::Vec3;
use crate::res::Lang;
use std::collections::HashSet;
use yog_api::vanilla;

/// The spray a slate gives as the run reaches it, or as it fails: its
/// speed out, fuzziness, spread (π over it) and particles.
const SPRAY_FOUND: (f64, f64, f64, u32) = (1.0, 0.1, 4.0, 30);
const SPRAY_FAIL: (f64, f64, f64, u32) = (1.5, 0.5, 2.0, 100);
/// The semitone heard at pitch 1 (a note block's), and an octave's.
const NOTE_ONE: i32 = 8;
const SEMITONES: i32 = 12;

/// `CircleExecutionState.createNew`: flood-fills the circle from the
/// impetus. `Err(None)`: nowhere to go; `Err(Some(pos))`: the media can't
/// get back to the impetus, `pos` the last block it reached.
pub(super) fn create_run(
    dim: &str,
    pos: BlockPos,
    dir: Dir,
    caster: Option<&Caster>,
) -> Result<Run, Option<BlockPos>> {
    let mut todo = vec![(dir, relative(pos, dir))];
    let mut seen = HashSet::new();
    let (mut lo, mut hi, mut last) = (pos, pos, pos);
    while let Some((enter, here)) = todo.pop() {
        let Some(part) = Part::at(dim, here) else {
            continue;
        };
        if !part.can_enter(dim, enter, here) {
            continue;
        }
        if seen.insert(here) {
            last = here;
            hi = (hi.0.max(here.0), hi.1.max(here.1), hi.2.max(here.2));
            lo = (lo.0.min(here.0), lo.1.min(here.1), lo.2.min(here.2));
            for out in part.exits(dim, here) {
                todo.push((out, relative(here, out)));
            }
        }
        if seen.len() >= MAX_LENGTH {
            return Err(None);
        }
    }
    if last == pos {
        return Err(None);
    } else if !seen.contains(&pos) {
        // It can't come back in the way the impetus sends it out: the last
        // block it tried is a dead end.
        return Err(Some(last));
    }
    Ok(Run {
        impetus: pos,
        impetus_dir: dir,
        reached: HashSet::from([pos]),
        current: relative(pos, dir),
        entered_from: dir,
        image: Image::default(),
        caster: caster.map(|c| c.uuid.clone()),
        caster_pigment: caster.map(|c| get_pigment(&c.uuid)),
        reached_slate: 0,
        min: lo,
        max: hi,
    })
}

/// `ICircleComponent.sfx`: particles and a sound where media moves (or fails).
pub(super) fn sfx(dim: &str, pos: BlockPos, imp: Option<(&Impetus, BlockPos)>, success: bool) {
    let (center, out) = match Part::at(dim, pos) {
        Some(part) => {
            let n = part.normal(dim, pos, 16).step();
            let out = Vec3::new(n.0 as f64, n.1 as f64, n.2 as f64);
            (Vec3::center_of(pos) + out * part.particle_height(), out)
        }
        None => (Vec3::center_of(pos), Vec3::new(0.0, 0.0, 0.0)),
    };
    let activator = imp
        .and_then(|(i, _)| i.run.as_ref()?.caster.clone())
        .unwrap_or_else(|| Pigment::default().owner);
    let pigment = match imp {
        Some((i, _)) if success && i.run.is_some() => i.pigment(),
        _ => Pigment {
            owner: activator,
            ..dye_pigment(dye::RED)
        },
    };
    let (speed, fuzziness, spread, count) = if success { SPRAY_FOUND } else { SPRAY_FAIL };
    let spray = Spray {
        pos: center,
        vel: out * speed,
        fuzziness,
        spread: std::f64::consts::PI / spread,
        count,
    };
    spray.spawn(dim, &pigment);
    let (mut sound, mut pitch) = (SOUND_FAIL, 1.0f32);
    if success && let Some((i, at)) = imp {
        sound = SOUND_FIND_BLOCK;
        let note = i.run.as_ref().map_or(0, |r| r.reached_slate - 1);
        let semitone = semitone_from_scale(dim, at, note);
        pitch = 2f32.powf((semitone - NOTE_ONE) as f32 / SEMITONES as f32);
    }
    yog_api::event::sound::play(dim, (center.x, center.y, center.z), sound, 1.0, pitch);
}

/// `semitoneFromScale`: the block under the impetus picks the scale.
pub(super) fn semitone_from_scale(dim: &str, impetus: BlockPos, note: i64) -> i32 {
    const MAJOR: &[i32] = &[0, 2, 4, 5, 7, 9, 11, 12];
    const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 11, 12];
    const DORIAN: &[i32] = &[0, 2, 3, 5, 7, 9, 10, 12];
    const MIXOLYDIAN: &[i32] = &[0, 2, 4, 5, 7, 9, 10, 12];
    const BLUES: &[i32] = &[0, 3, 5, 6, 7, 10, 12];
    const BAD_TIME: &[i32] = &[0, 0, 12, 7, 6, 5, 3, 0, 3, 5];
    const SUSSY_BAKA: &[i32] = &[5, 8, 10, 11, 10, 8, 5, 3, 7, 5];
    let below = (impetus.0, impetus.1 - 1, impetus.2);
    let id = yog_api::event::block::info(dim, to_ypos(below))
        .map(|b| b.block_id().to_owned())
        .unwrap_or_default();
    let tagged = |tag: &str| yog_api::event::tag::has_block(&id, tag);
    let scale = match id.as_str() {
        vanilla::blocks::CRYING_OBSIDIAN => MINOR,
        _ if tagged(vanilla::block_tags::DOORS) || tagged(vanilla::block_tags::TRAPDOORS) => DORIAN,
        vanilla::blocks::PISTON | vanilla::blocks::STICKY_PISTON => MIXOLYDIAN,
        vanilla::blocks::BLUE_WOOL
        | vanilla::blocks::BLUE_CONCRETE
        | vanilla::blocks::BLUE_CONCRETE_POWDER
        | vanilla::blocks::BLUE_TERRACOTTA
        | vanilla::blocks::BLUE_GLAZED_TERRACOTTA
        | vanilla::blocks::BLUE_STAINED_GLASS
        | vanilla::blocks::BLUE_STAINED_GLASS_PANE => BLUES,
        vanilla::blocks::BONE_BLOCK => BAD_TIME,
        vanilla::blocks::COMPOSTER => SUSSY_BAKA,
        _ => MAJOR,
    };
    scale[note.clamp(0, scale.len() as i64 - 1) as usize]
}

/// `BlockEntityAbstractImpetus.startExecution`.
pub fn start(dim: &str, pos: BlockPos, caster: Option<Caster>) {
    let Some(part) = Part::at(dim, pos).filter(|p| p.kind == Kind::Impetus) else {
        return;
    };
    let mut imp = impetus(dim, pos);
    if imp.run.is_some() {
        return;
    }
    match create_run(dim, pos, part.facing(), caster.as_ref()) {
        Err(None) => {
            sfx(dim, pos, None, false);
            imp.post_no_exits(pos);
        }
        Err(Some(at)) => {
            sfx(dim, at, None, false);
            let shown = Text::lit(format!("{}, {}, {}", at.0, at.1, at.2));
            imp.post(
                &Text::key_args(Lang::TOOLTIP_CIRCLE_NO_CLOSURE_KEY, vec![shown]),
                vanilla::items::LEAD,
            );
        }
        Ok(run) => {
            let speed = run.tick_speed();
            imp.run = Some(run);
            imp.display = None;
            set_energized(dim, pos, &part, true);
            if let Some(k) = KNOWN.lock().get_mut(&(dim.to_owned(), pos)) {
                k.due = Some(*TICKS.lock() + speed);
            }
        }
    }
    save_impetus(dim, pos, &imp);
}

/// `CircleExecutionState.tick`: one step of a run; whether to go on.
pub(super) fn step(dim: &str, imp: &mut Impetus, at: BlockPos) -> bool {
    let Some(mut run) = imp.run.clone() else {
        return false;
    };
    let mut env = circle_env(dim, imp, &run);
    let Some(part) = Part::at(dim, run.current) else {
        sfx(dim, run.current, Some((imp, at)), false);
        return false;
    };
    set_energized(dim, run.current, &part, true);
    run.reached.insert(run.current);
    run.reached_slate += 1;
    imp.run = Some(run.clone());
    let flow = accept(
        &part,
        run.image.clone(),
        &mut env,
        run.entered_from,
        run.current,
    );
    // What the cast spent, recolored and said.
    if imp.media >= 0 {
        imp.media = env.circle_media.max(0);
    }
    if let Some(p) = env.circle_pigment.clone().filter(|p| *p != imp.pigment()) {
        imp.pigment = Some(p);
    }
    if let Some((msg, item)) = env.circle_posts.last() {
        imp.post(msg, item);
    }
    if Part::at(dim, at).is_none_or(|p| p.kind != Kind::Impetus) {
        return false; // the impetus is gone
    }
    match flow {
        Flow::Stop => false,
        Flow::Continue(image, exits) => go_on(dim, imp, at, image, exits),
    }
}

/// The casting environment of a run's slate: its caster (if still about),
/// the circle's bounds, the impetus' media and pigment.
fn circle_env(dim: &str, imp: &Impetus, run: &Run) -> CastEnv {
    let caster = run.caster.as_ref().and_then(|uuid| {
        let info = yog_api::event::entity::info(uuid)?;
        Some(Caster {
            name: info.name,
            uuid: uuid.clone(),
        })
    });
    let circle = CircleRun {
        impetus: run.impetus,
        facing: run.impetus_dir.step(),
        min: run.min,
        max: run.max,
        current: run.current,
    };
    let mut env = CastEnv::new(caster, Hand::Off, Source::Circle(circle), dim.to_owned());
    env.circle_media = if imp.media < 0 { i64::MAX } else { imp.media };
    env.circle_pigment = Some(imp.pigment());
    env
}

/// The run goes on to the one slate among `exits` it can enter, with
/// `image`; more than one, or none, ends it. Whether it goes on.
fn go_on(
    dim: &str,
    imp: &mut Impetus,
    at: BlockPos,
    image: Image,
    exits: Vec<(BlockPos, Dir)>,
) -> bool {
    let Some(current) = imp.run.as_ref().map(|r| r.current) else {
        return false;
    };
    let open: Vec<(BlockPos, Dir)> = exits
        .into_iter()
        .filter(|&(pos, dir)| Part::at(dim, pos).is_some_and(|p| p.can_enter(dim, dir, pos)))
        .collect();
    let &[(pos, dir)] = open.as_slice() else {
        if open.is_empty() {
            sfx(dim, current, Some((imp, at)), false);
            imp.post_no_exits(current);
        } else {
            let shown = Text::lit(format!("{}, {}, {}", current.0, current.1, current.2));
            let msg = Text::key_args(Lang::TOOLTIP_CIRCLE_MANY_EXITS_KEY, vec![shown]);
            imp.post(&msg, vanilla::items::COMPASS);
            sfx(dim, current, Some((imp, at)), false);
        }
        return false;
    };
    sfx(dim, current, Some((imp, at)), true);
    let Some(run) = imp.run.as_mut() else {
        return false;
    };
    run.current = pos;
    run.entered_from = dir;
    // Ops count afresh after each slate.
    run.image = image.with_used_ops(0);
    true
}

/// `acceptControlFlow`.
pub(super) fn accept(
    part: &Part,
    image: Image,
    env: &mut CastEnv,
    enter: Dir,
    pos: BlockPos,
) -> Flow {
    let dim = env.dim.clone();
    let exit = |d: Dir| (relative(pos, d), d);
    match part.kind {
        Kind::Impetus => Flow::Stop,
        Kind::EmptyImpetus => Flow::Continue(image, vec![exit(part.facing())]),
        Kind::DirEmpty => {
            let d = if random() < 0.5 {
                part.facing()
            } else {
                part.facing().opposite()
            };
            Flow::Continue(image, vec![exit(d)])
        }
        Kind::DirRedstone => Flow::Continue(image, vec![exit(part.real_facing())]),
        Kind::DirBoolean => {
            let mut stack = image.stack.clone();
            let mishap = match stack.pop() {
                None => Some(Mishap::BoolDirectrixEmptyStack(pos)),
                Some(Iota::Bool(b)) => {
                    yog_api::event::block::set_properties(
                        &dim,
                        to_ypos(pos),
                        if b { "state=true" } else { "state=false" },
                    );
                    let out = if b {
                        part.facing().opposite()
                    } else {
                        part.facing()
                    };
                    return Flow::Continue(Image { stack, ..image }, vec![exit(out)]);
                }
                Some(other) => Some(Mishap::BoolDirectrixNotBool(other, pos)),
            };
            if let Some(m) = mishap {
                fake_throw_mishap(part, pos, image, env, m);
            }
            Flow::Stop
        }
        Kind::Slate => {
            let exits: Vec<(BlockPos, Dir)> = part
                .exits(&dim, pos)
                .into_iter()
                .filter(|&d| d != enter.opposite())
                .map(exit)
                .collect();
            let Some(pattern) = slate_pattern(&dim, pos) else {
                return Flow::Continue(image, exits);
            };
            // A slate's cast runs in its own env, one that shares the impetus' state.
            let mut vm = Vm::new(
                image,
                std::mem::replace(
                    env,
                    CastEnv::new(None, Hand::Off, Source::Staff, dim.clone()),
                ),
            );
            let view = vm.queue_execute(vec![Iota::Pattern(pattern)]);
            *env = vm.env;
            if view.kind.success() {
                Flow::Continue(vm.image, exits)
            } else {
                Flow::Stop
            }
        }
    }
}

/// `fakeThrowMishap`: a component's own mishap, named after the block.
pub(super) fn fake_throw_mishap(
    part: &Part,
    pos: BlockPos,
    image: Image,
    env: &mut CastEnv,
    mishap: Mishap,
) {
    let (ns, path) = part.id.split_once(':').unwrap_or(("hexcasting", &part.id));
    let name = Text::Join(
        vec![
            Text::key(format!("block.{ns}.{}", path.replace('/', "."))),
            Text::lit(format!(" ({}, {}, {})", pos.0, pos.1, pos.2)),
        ],
        String::new(),
    );
    let ctx = Context {
        pattern: None,
        name: Some(name),
    };
    if let Some(msg) = mishap.message_with_name(env, &ctx) {
        env.report_mishap(&msg);
    }
    let spray = mishap.particle_spray(env);
    env.mishap_spray(&spray, mishap.accent_color());
    let mut stack = image.stack;
    mishap.execute(env, &ctx, &mut stack);
}

/// `endExecution`: every block the run reached stops glowing.
pub(super) fn end(dim: &str, imp: &mut Impetus) {
    if let Some(run) = imp.run.take() {
        for pos in run.reached {
            if let Some(part) = Part::at(dim, pos) {
                set_energized(dim, pos, &part, false);
            }
        }
    }
}

/// `tickExecution`.
pub(super) fn tick_run(dim: &str, pos: BlockPos, now: u64) -> Option<u64> {
    let mut imp = impetus(dim, pos);
    imp.run.as_ref()?;
    let go_on = step(dim, &mut imp, pos);
    let due = if go_on {
        Some(now + imp.run.as_ref().map_or(10, Run::tick_speed))
    } else {
        end(dim, &mut imp);
        None
    };
    save_impetus(dim, pos, &imp);
    due
}
