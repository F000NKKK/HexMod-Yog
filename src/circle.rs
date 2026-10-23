//! Spell circles: Hex Casting's `ICircleComponent`, `BlockCircleComponent`,
//! `CircleExecutionState`, `BlockEntityAbstractImpetus`, the impetuses,
//! directrices and slates.
//!
//! The blocks are the loader's stateful blocks (`facing`, `energized`, …);
//! what the original keeps in block entities — an impetus' media, its run,
//! its display, a slate's pattern — is kept in the chunk's mod storage,
//! keyed by position.

use std::collections::{HashMap, HashSet};
use yog_api::sync::{LazyLock, Mutex};

use yog_api::storage::Store;
use yog_api::vanilla;
use yog_api::{EventPhase, Registry, split_state};

use crate::casting::env::{
    CastEnv, Caster, CircleRun, Source, Spray, from_ypos, get_pigment, random, to_ypos,
};
use crate::casting::image::Image;
use crate::casting::mishap::{BlockPos, Context, Mishap, dye};
use crate::casting::vm::Vm;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::item::holder;
use crate::item::pigment::{Pigment, dye_pigment};
use crate::math::{HexPattern, Vec3};
use crate::protocol::Hand;
use crate::res::Lang;
use crate::text::Text;

/// `HexConfig.server().maxSpellCircleLength()` (`DEFAULT_MAX_SPELL_CIRCLE_LENGTH`).
const MAX_LENGTH: usize = 1024;
/// `BlockEntityAbstractImpetus.MAX_CAPACITY`.
const MAX_MEDIA: i64 = 9_000_000_000_000_000_000;
/// `BlockEntityLookingImpetus.MAX_LOOK_AMOUNT`.
const MAX_LOOK: i32 = 30;
const LOOK_RANGE: f64 = 20.0;

const SOUND_FIND_BLOCK: &str = crate::res::SoundEvents::SPELLCIRCLE_FIND_BLOCK;
const SOUND_FAIL: &str = crate::res::SoundEvents::SPELLCIRCLE_FAIL;
const SOUND_LOOK_TICK: &str = crate::res::SoundEvents::IMPETUS_FLETCHER_TICK;
const SOUND_REDSTONE_DING: &str = crate::res::SoundEvents::IMPETUS_REDSTONE_REGISTER;
const SOUND_REDSTONE_CLEAR: &str = crate::res::SoundEvents::IMPETUS_REDSTONE_CLEAR;

// ── directions ──────────────────────────────────────────────────────────────

/// A block direction, `Direction`'s order (down, up, north, south, west, east).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];

    pub fn name(self) -> &'static str {
        ["down", "up", "north", "south", "west", "east"][self as usize]
    }

    pub fn parse(s: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| d.name() == s)
    }

    pub fn step(self) -> (i32, i32, i32) {
        match self {
            Dir::Down => (0, -1, 0),
            Dir::Up => (0, 1, 0),
            Dir::North => (0, 0, -1),
            Dir::South => (0, 0, 1),
            Dir::West => (-1, 0, 0),
            Dir::East => (1, 0, 0),
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }
}

fn relative(p: BlockPos, d: Dir) -> BlockPos {
    let s = d.step();
    (p.0 + s.0, p.1 + s.1, p.2 + s.2)
}

// ── components ──────────────────────────────────────────────────────────────

/// What a circle block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An impetus that starts circles (`BlockAbstractImpetus`): right-click,
    /// look (fletcher) or redstone (cleric).
    Impetus,
    /// `BlockEmptyImpetus`: media passes through it the way it faces.
    EmptyImpetus,
    Slate,
    DirEmpty,
    DirRedstone,
    DirBoolean,
}

pub fn kind(id: &str) -> Option<Kind> {
    Some(match id {
        Blocks::IMPETUS_RIGHTCLICK | Blocks::IMPETUS_LOOK | Blocks::IMPETUS_REDSTONE => {
            Kind::Impetus
        }
        Blocks::IMPETUS_EMPTY => Kind::EmptyImpetus,
        Blocks::SLATE => Kind::Slate,
        Blocks::DIRECTRIX_EMPTY => Kind::DirEmpty,
        Blocks::DIRECTRIX_REDSTONE => Kind::DirRedstone,
        Blocks::DIRECTRIX_BOOLEAN => Kind::DirBoolean,
        _ => return None,
    })
}

/// A circle block as it stands: its id and properties.
#[derive(Debug, Clone)]
struct Part {
    id: String,
    kind: Kind,
    props: Vec<(String, String)>,
}

impl Part {
    fn at(dim: &str, pos: BlockPos) -> Option<Part> {
        let state = yog_api::event::block::state(dim, to_ypos(pos))?;
        let (id, props) = split_state(&state);
        Some(Part {
            kind: kind(id)?,
            id: id.to_owned(),
            props: props
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        })
    }

    fn prop(&self, name: &str) -> &str {
        self.props
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v)
    }

    fn facing(&self) -> Dir {
        Dir::parse(self.prop("facing")).unwrap_or(Dir::North)
    }

    /// `BlockRedstoneDirectrix.getRealFacing`.
    fn real_facing(&self) -> Dir {
        if self.prop("powered") == "true" {
            self.facing()
        } else {
            self.facing().opposite()
        }
    }

    /// `canEnterFromDirection`.
    fn can_enter(&self, dim: &str, enter: Dir, pos: BlockPos) -> bool {
        let f = self.facing();
        match self.kind {
            // FACING is where media leaves: it can't come in that way.
            Kind::Impetus | Kind::EmptyImpetus => enter != f.opposite(),
            Kind::DirEmpty | Kind::DirRedstone | Kind::DirBoolean => {
                enter != f.opposite() && enter != f
            }
            Kind::Slate => enter != self.normal(dim, pos, 16).opposite(),
        }
    }

    /// `possibleExitDirections`.
    fn exits(&self, dim: &str, pos: BlockPos) -> Vec<Dir> {
        let f = self.facing();
        match self.kind {
            Kind::Impetus | Kind::EmptyImpetus => vec![f],
            Kind::DirEmpty | Kind::DirRedstone | Kind::DirBoolean => vec![f, f.opposite()],
            Kind::Slate => {
                let normal = self.normal(dim, pos, 16);
                Dir::ALL.into_iter().filter(|&d| d != normal).collect()
            }
        }
    }

    /// `normalDir`: which way is "up" for the particles here.
    fn normal(&self, dim: &str, pos: BlockPos, left: i32) -> Dir {
        match self.kind {
            Kind::Impetus | Kind::EmptyImpetus | Kind::DirBoolean => {
                normal_of_other(dim, relative(pos, self.facing()), left)
            }
            Kind::DirRedstone => normal_of_other(dim, relative(pos, self.real_facing()), left),
            Kind::DirEmpty => Dir::Up,
            Kind::Slate => match self.prop("face") {
                "floor" => Dir::Up,
                "ceiling" => Dir::Down,
                _ => self.facing(),
            },
        }
    }

    /// `particleHeight`.
    fn particle_height(&self) -> f64 {
        if self.kind == Kind::Slate {
            0.5 - 15.0 / 16.0
        } else {
            0.5
        }
    }
}

/// `BlockCircleComponent.normalDirOfOther`.
fn normal_of_other(dim: &str, other: BlockPos, left: i32) -> Dir {
    if left <= 0 {
        return Dir::Up;
    }
    match Part::at(dim, other) {
        Some(p) => p.normal(dim, other, left - 1),
        None => Dir::Up,
    }
}

/// `ICircleComponent.ControlFlow`.
enum Flow {
    Continue(Image, Vec<(BlockPos, Dir)>),
    Stop,
}

/// `startEnergized` / `endEnergized`.
fn set_energized(dim: &str, pos: BlockPos, part: &Part, on: bool) {
    let mut props = format!("energized={on}");
    if !on && part.kind == Kind::DirBoolean {
        props.push_str(",state=neither");
    }
    yog_api::event::block::set_properties(dim, to_ypos(pos), &props);
}

// ── an impetus' state (`BlockEntityAbstractImpetus`) ────────────────────────

/// The chunk's store `pos` is in, with a world open.
fn store(dim: &str, pos: BlockPos) -> Option<Store> {
    yog_api::storage::chunk(dim, pos.0 >> 4, pos.2 >> 4)
}

/// Where a chunk's impetuses and slates are kept in its store.
const IMPETUS_KEY: &str = "impetus";
const SLATES_KEY: &str = "slates";

/// What an impetus keeps, as it's kept with its chunk (its message and
/// run in the casting's own text forms).
#[derive(Default, yog_api::Schema)]
struct KeptImpetus {
    pos: BlockPos,
    media: i64,
    /// The item beside its message, and the message.
    display: Option<(String, String)>,
    pigment: Option<Pigment>,
    run: Option<String>,
    look: i32,
    /// The cleric impetus' player: uuid and name.
    player: Option<(String, String)>,
}

/// A slate's pattern, kept with its chunk.
#[derive(yog_api::Schema)]
struct KeptSlate {
    pos: BlockPos,
    pattern: String,
}

/// `CircleExecutionState`.
#[derive(Debug, Clone)]
struct Run {
    impetus: BlockPos,
    impetus_dir: Dir,
    reached: HashSet<BlockPos>,
    current: BlockPos,
    entered_from: Dir,
    image: Image,
    caster: Option<String>,
    caster_pigment: Option<Pigment>,
    reached_slate: i64,
    /// The circle's bounds, both inclusive.
    min: BlockPos,
    max: BlockPos,
}

fn pos_str(p: BlockPos) -> String {
    format!("{},{},{}", p.0, p.1, p.2)
}

fn parse_pos(s: &str) -> Option<BlockPos> {
    let mut it = s.split(',').map(|n| n.parse::<i32>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

impl Run {
    /// One `key\tvalue` a line.
    fn encode(&self) -> String {
        let reached: Vec<String> = self.reached.iter().map(|&p| pos_str(p)).collect();
        let mut lines = vec![
            format!("impetus\t{}", pos_str(self.impetus)),
            format!("impetus_dir\t{}", self.impetus_dir.name()),
            format!("reached\t{}", reached.join(";")),
            format!("current\t{}", pos_str(self.current)),
            format!("entered_from\t{}", self.entered_from.name()),
            format!("reached_slate\t{}", self.reached_slate),
            format!("min\t{}", pos_str(self.min)),
            format!("max\t{}", pos_str(self.max)),
            format!("image\t{}", self.image.encode().replace('\n', "\u{1}")),
        ];
        if let Some(c) = &self.caster {
            lines.push(format!("caster\t{c}"));
        }
        if let Some(p) = &self.caster_pigment {
            lines.push(format!("pigment\t{}\t{}", p.item, p.owner));
        }
        lines.join("\n")
    }

    fn decode(s: &str) -> Option<Run> {
        let get = |k: &str| {
            s.lines()
                .find_map(|l| l.strip_prefix(k)?.strip_prefix('\t'))
        };
        Some(Run {
            impetus: parse_pos(get("impetus")?)?,
            impetus_dir: Dir::parse(get("impetus_dir")?)?,
            reached: get("reached")?.split(';').filter_map(parse_pos).collect(),
            current: parse_pos(get("current")?)?,
            entered_from: Dir::parse(get("entered_from")?)?,
            reached_slate: get("reached_slate")?.parse().ok()?,
            min: parse_pos(get("min")?)?,
            max: parse_pos(get("max")?)?,
            image: Image::decode(&get("image")?.replace('\u{1}', "\n")),
            caster: get("caster").map(str::to_owned),
            caster_pigment: get("pigment").and_then(|p| {
                let (item, owner) = p.split_once('\t')?;
                Some(Pigment {
                    item: item.into(),
                    owner: owner.into(),
                })
            }),
        })
    }

    /// `getTickSpeed`: ticks between steps, faster as the circle grows.
    fn tick_speed(&self) -> u64 {
        (10 - (self.reached_slate - 1) / 3).max(2) as u64
    }
}

/// What an impetus keeps.
#[derive(Debug, Clone, Default)]
pub struct Impetus {
    /// `-1`: infinite (a creative unlocker).
    pub media: i64,
    /// What it shows (`postDisplay`): a message and the item beside it.
    pub display: Option<(Text, String)>,
    pub pigment: Option<Pigment>,
    run: Option<Run>,
    /// The fletcher impetus' gaze count.
    look: i32,
    /// The cleric impetus' player: uuid and name.
    pub player: Option<(String, String)>,
}

pub fn impetus(dim: &str, pos: BlockPos) -> Impetus {
    let all: Vec<KeptImpetus> = store(dim, pos)
        .map(|s| s.get_or_default(IMPETUS_KEY))
        .unwrap_or_default();
    let Some(k) = all.into_iter().find(|k| k.pos == pos) else {
        return Impetus::default();
    };
    Impetus {
        media: k.media,
        display: k
            .display
            .and_then(|(item, text)| Some((Text::decode(&text)?, item))),
        pigment: k.pigment,
        run: k.run.and_then(|r| Run::decode(&r)),
        look: k.look,
        player: k.player,
    }
}

fn save_impetus(dim: &str, pos: BlockPos, i: &Impetus) {
    let kept = KeptImpetus {
        pos,
        media: i.media,
        display: i
            .display
            .as_ref()
            .map(|(text, item)| (item.clone(), text.encode())),
        pigment: i.pigment.clone(),
        run: i.run.as_ref().map(Run::encode),
        look: i.look,
        player: i.player.clone(),
    };
    if let Some(s) = store(dim, pos) {
        s.update(IMPETUS_KEY, |all: &mut Vec<KeptImpetus>| {
            all.retain(|k| k.pos != pos);
            all.push(kept);
        });
    }
}

fn forget_impetus(dim: &str, pos: BlockPos) {
    if let Some(s) = store(dim, pos) {
        s.update(IMPETUS_KEY, |all: &mut Vec<KeptImpetus>| {
            all.retain(|k| k.pos != pos)
        });
    }
}

impl Impetus {
    /// `getPigment`.
    fn pigment(&self) -> Pigment {
        self.pigment
            .clone()
            .or_else(|| self.run.as_ref().and_then(|r| r.caster_pigment.clone()))
            .unwrap_or_default()
    }

    /// `postDisplay`.
    fn post(&mut self, msg: &Text, item: &str) {
        self.display = Some((msg.clone(), item.to_owned()));
    }

    /// `postNoExits`.
    fn post_no_exits(&mut self, pos: BlockPos) {
        let at = Text::lit(format!("{}, {}, {}", pos.0, pos.1, pos.2));
        self.post(
            &Text::key_args(Lang::TOOLTIP_CIRCLE_NO_EXIT_KEY, vec![at]),
            vanilla::items::OAK_SIGN,
        );
    }

    /// `remainingMediaCapacity`.
    pub fn room(&self) -> i64 {
        if self.media < 0 {
            0
        } else {
            (MAX_MEDIA - self.media).max(0)
        }
    }
}

// ── the impetuses in the world ──────────────────────────────────────────────

/// Every impetus there is, by dimension and position: its block id, when
/// its run steps next (a tick count), and the redstone it last had.
#[derive(Debug, Clone)]
struct Known {
    id: String,
    due: Option<u64>,
    powered: bool,
}

static KNOWN: LazyLock<Mutex<HashMap<(String, BlockPos), Known>>> = LazyLock::new(Default::default);
static TICKS: Mutex<u64> = Mutex::new(0);
/// The slate a player is about to place: its pattern (from the item).
static PLACING: LazyLock<Mutex<HashMap<String, Option<HexPattern>>>> =
    LazyLock::new(Default::default);

/// Where the impetuses there are, are kept in the world store.
const KNOWN_KEY: &str = "circles";

/// An impetus there is, as it's kept with the world.
#[derive(yog_api::Schema)]
struct KeptKnown {
    dim: String,
    pos: BlockPos,
    id: String,
}

fn save_known() {
    let known: Vec<KeptKnown> = KNOWN
        .lock()
        .iter()
        .map(|((dim, pos), k)| KeptKnown {
            dim: dim.clone(),
            pos: *pos,
            id: k.id.clone(),
        })
        .collect();
    if let Some(store) = yog_api::storage::world() {
        store.set(KNOWN_KEY, &known);
    }
}

fn load_known() {
    let kept: Vec<KeptKnown> = yog_api::storage::world()
        .map(|s| s.get_or_default(KNOWN_KEY))
        .unwrap_or_default();
    let mut known = KNOWN.lock();
    known.clear();
    for KeptKnown { dim, pos, id } in kept {
        let i = impetus(&dim, pos);
        let powered = yog_api::event::block::property(&dim, to_ypos(pos), "powered").as_deref()
            == Some("true");
        known.insert(
            (dim, pos),
            Known {
                id,
                due: i.run.as_ref().map(|_| 1),
                powered,
            },
        );
    }
}

/// The impetuses: the blocks that start a circle.
const IMPETUSES: [&str; 3] = [
    Blocks::IMPETUS_RIGHTCLICK,
    Blocks::IMPETUS_LOOK,
    Blocks::IMPETUS_REDSTONE,
];

fn is_impetus(id: &str) -> bool {
    IMPETUSES.contains(&id)
}

// ── a run ───────────────────────────────────────────────────────────────────

/// `CircleExecutionState.createNew`: flood-fills the circle from the
/// impetus. `Err(None)`: nowhere to go; `Err(Some(pos))`: the media can't
/// get back to the impetus, `pos` the last block it reached.
fn create_run(
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
fn sfx(dim: &str, pos: BlockPos, imp: Option<(&Impetus, BlockPos)>, success: bool) {
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
    let spray = Spray {
        pos: center,
        vel: out * if success { 1.0 } else { 1.5 },
        fuzziness: if success { 0.1 } else { 0.5 },
        spread: std::f64::consts::PI / if success { 4.0 } else { 2.0 },
        count: if success { 30 } else { 100 },
    };
    spray.spawn(dim, &pigment);
    let (mut sound, mut pitch) = (SOUND_FAIL, 1.0f32);
    if success && let Some((i, at)) = imp {
        sound = SOUND_FIND_BLOCK;
        let note = i.run.as_ref().map_or(0, |r| r.reached_slate - 1);
        let semitone = semitone_from_scale(dim, at, note);
        pitch = 2f32.powf((semitone as f32 - 8.0) / 12.0);
    }
    yog_api::event::sound::play(dim, (center.x, center.y, center.z), sound, 1.0, pitch);
}

/// `semitoneFromScale`: the block under the impetus picks the scale.
fn semitone_from_scale(dim: &str, impetus: BlockPos, note: i64) -> i32 {
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
fn step(dim: &str, imp: &mut Impetus, at: BlockPos) -> bool {
    let Some(run) = imp.run.clone() else {
        return false;
    };
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

    let Some(part) = Part::at(dim, run.current) else {
        sfx(dim, run.current, Some((imp, at)), false);
        return false;
    };
    set_energized(dim, run.current, &part, true);
    let mut run = run;
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
        Flow::Continue(image, exits) => {
            let mut found: Option<(BlockPos, Dir)> = None;
            for (pos, dir) in exits {
                let ok = Part::at(dim, pos).is_some_and(|p| p.can_enter(dim, dir, pos));
                if !ok {
                    continue;
                }
                if found.is_some() {
                    let shown = Text::lit(format!(
                        "{}, {}, {}",
                        run.current.0, run.current.1, run.current.2
                    ));
                    imp.post(
                        &Text::key_args(Lang::TOOLTIP_CIRCLE_MANY_EXITS_KEY, vec![shown]),
                        vanilla::items::COMPASS,
                    );
                    sfx(dim, run.current, Some((imp, at)), false);
                    return false;
                }
                found = Some((pos, dir));
            }
            match found {
                None => {
                    sfx(dim, run.current, Some((imp, at)), false);
                    imp.post_no_exits(run.current);
                    false
                }
                Some((pos, dir)) => {
                    sfx(dim, run.current, Some((imp, at)), true);
                    let Some(run) = imp.run.as_mut() else {
                        return false;
                    };
                    run.current = pos;
                    run.entered_from = dir;
                    // Ops count afresh after each slate.
                    run.image = image.with_used_ops(0);
                    true
                }
            }
        }
    }
}

/// `acceptControlFlow`.
fn accept(part: &Part, image: Image, env: &mut CastEnv, enter: Dir, pos: BlockPos) -> Flow {
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
fn fake_throw_mishap(part: &Part, pos: BlockPos, image: Image, env: &mut CastEnv, mishap: Mishap) {
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
fn end(dim: &str, imp: &mut Impetus) {
    if let Some(run) = imp.run.take() {
        for pos in run.reached {
            if let Some(part) = Part::at(dim, pos) {
                set_energized(dim, pos, &part, false);
            }
        }
    }
}

/// `tickExecution`.
fn tick_run(dim: &str, pos: BlockPos, now: u64) -> Option<u64> {
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

// ── the impetuses' triggers ─────────────────────────────────────────────────

/// `BlockEntityLookingImpetus.serverTick`: someone staring at it fills it.
fn tick_look(dim: &str, pos: BlockPos) {
    if yog_api::event::block::property(dim, to_ypos(pos), "energized").as_deref() == Some("true") {
        return;
    }
    let mut looker = None;
    for name in yog_api::event::player::online() {
        let Some(info) = yog_api::event::player::info(&name)
            .and_then(|p| Some((p.uuid.clone(), yog_api::event::entity::info(&p.uuid)?)))
        else {
            continue;
        };
        let (uuid, e) = info;
        let eye = Vec3::new(e.pos.x, e.pos.y + e.eye_height, e.pos.z);
        if e.dimension != dim
            || eye.distance_sqr(Vec3::center_of(pos)) > 3.0 * LOOK_RANGE * LOOK_RANGE
        {
            continue;
        }
        // A carved pumpkin on the head hides the gaze.
        if crate::casting::env::Stack::read(&name, 39).id == vanilla::items::CARVED_PUMPKIN {
            continue;
        }
        let end = eye + crate::casting::env::look_vec(&uuid) * (LOOK_RANGE / 1.5);
        if yog_api::event::world::raycast_block(dim, eye.tuple(), end.tuple())
            .is_some_and(|h| from_ypos(h.pos) == pos)
        {
            looker = Caster::by_name(&name);
            break;
        }
    }
    let mut imp = impetus(dim, pos);
    let prev = imp.look;
    let new = (prev + if looker.is_some() { 1 } else { -1 }).clamp(0, MAX_LOOK);
    if new == prev {
        return;
    }
    if new == MAX_LOOK {
        imp.look = 0;
        save_impetus(dim, pos, &imp);
        start(dim, pos, looker);
    } else {
        if new % 5 == 1 {
            let t = new as f32 / MAX_LOOK as f32;
            let (pitch, volume) = (0.5 + (1.2 - 0.5) * t, 0.2 + (1.2 - 0.2) * t);
            let c = Vec3::center_of(pos);
            yog_api::event::sound::play(dim, (c.x, c.y, c.z), SOUND_LOOK_TICK, volume, pitch);
        }
        imp.look = new;
        save_impetus(dim, pos, &imp);
    }
}

pub fn tick() {
    let now = {
        let mut t = TICKS.lock();
        *t += 1;
        *t
    };
    let known: Vec<((String, BlockPos), Known)> = KNOWN
        .lock()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut changed = false;
    for ((dim, pos), k) in known {
        // Gone some way a break didn't see (an explosion, a piston).
        if now.is_multiple_of(20)
            && yog_api::event::block::info(&dim, to_ypos(pos)).is_some_and(|b| b.block_id() != k.id)
        {
            KNOWN.lock().remove(&(dim.clone(), pos));
            forget_impetus(&dim, pos);
            changed = true;
            continue;
        }
        if k.due.is_some_and(|d| now >= d) {
            let due = tick_run(&dim, pos, now);
            if let Some(e) = KNOWN.lock().get_mut(&(dim.clone(), pos)) {
                e.due = due;
            }
        }
        match k.id.as_str() {
            Blocks::IMPETUS_LOOK => tick_look(&dim, pos),
            // `BlockRedstoneImpetus.neighborChanged`: power coming on starts it.
            Blocks::IMPETUS_REDSTONE => {
                let powered = yog_api::event::block::property(&dim, to_ypos(pos), "powered")
                    .as_deref()
                    == Some("true");
                if powered != k.powered {
                    if let Some(e) = KNOWN.lock().get_mut(&(dim.clone(), pos)) {
                        e.powered = powered;
                    }
                    if powered {
                        let player = impetus(&dim, pos)
                            .player
                            .and_then(|(_, name)| Caster::by_name(&name));
                        start(&dim, pos, player);
                    }
                }
            }
            _ => {}
        }
    }
    if changed {
        save_known();
    }
}

// ── slates ──────────────────────────────────────────────────────────────────

/// `BlockEntitySlate.pattern`.
pub fn slate_pattern(dim: &str, pos: BlockPos) -> Option<HexPattern> {
    let slates: Vec<KeptSlate> = store(dim, pos)?.get_or_default(SLATES_KEY);
    let slate = slates.into_iter().find(|s| s.pos == pos)?;
    HexPattern::from_serialized(&slate.pattern)
}

fn set_slate_pattern(dim: &str, pos: BlockPos, pattern: Option<&HexPattern>) {
    let Some(s) = store(dim, pos) else {
        return;
    };
    s.update(SLATES_KEY, |slates: &mut Vec<KeptSlate>| {
        slates.retain(|s| s.pos != pos);
        if let Some(p) = pattern {
            slates.push(KeptSlate {
                pos,
                pattern: p.serialized_form(),
            });
        }
    });
}

/// The slate item, written with `pattern` if it has one.
fn slate_item_nbt(pattern: Option<&HexPattern>) -> String {
    match pattern {
        Some(p) => format!("{{BlockEntityTag:{{pattern:\"{}\"}}}}", p.serialized_form()),
        None => String::new(),
    }
}

// ── events ──────────────────────────────────────────────────────────────────

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::life::Started, _>(|_, _| {
        load_known();
        *TICKS.lock() = 0;
    });
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());

    registry.on::<yog_api::event::block::Place, _>(|ev, cx| {
        let phase = cx.phase();
        let pos = from_ypos(ev.pos);
        let Some(k) = kind(&ev.block_id) else {
            return true;
        };
        match phase {
            EventPhase::Pre => {
                if k == Kind::Slate {
                    // The item placing it is in a hand now; it may be used up after.
                    let selected = yog_api::event::player::info(&ev.player_name)
                        .map_or(0, |p| p.selected_slot);
                    let pattern = [selected, crate::casting::env::SLOT_OFFHAND]
                        .into_iter()
                        .map(|slot| crate::casting::env::Stack::read(&ev.player_name, slot))
                        .find(|s| s.id == Blocks::SLATE)
                        .and_then(|s| holder::written_pattern(&s.id, &s.nbt));
                    PLACING.lock().insert(ev.player_name.clone(), pattern);
                }
            }
            _ => {
                // `placeStateDirAndSneak`: sneaking turns it the other way.
                if matches!(
                    k,
                    Kind::Impetus
                        | Kind::EmptyImpetus
                        | Kind::DirEmpty
                        | Kind::DirRedstone
                        | Kind::DirBoolean
                ) && ev.sneaking
                    && let Some(f) =
                        yog_api::event::block::property(&ev.dimension, ev.pos, "facing")
                            .as_deref()
                            .and_then(Dir::parse)
                {
                    yog_api::event::block::set_properties(
                        &ev.dimension,
                        ev.pos,
                        &format!("facing={}", f.opposite().name()),
                    );
                }
                if k == Kind::Slate {
                    let pattern = PLACING.lock().remove(&ev.player_name).flatten();
                    set_slate_pattern(&ev.dimension, pos, pattern.as_ref());
                    crate::render::slate_changed(&ev.dimension, pos, pattern.as_ref());
                }
                if is_impetus(&ev.block_id) {
                    forget_impetus(&ev.dimension, pos);
                    let powered = yog_api::event::block::property(&ev.dimension, ev.pos, "powered")
                        .as_deref()
                        == Some("true");
                    KNOWN.lock().insert(
                        (ev.dimension.clone(), pos),
                        Known {
                            id: ev.block_id.clone(),
                            due: None,
                            powered,
                        },
                    );
                    save_known();
                }
            }
        }
        true
    });

    // `BlockEntityAbstractImpetus` as a container: hoppers and droppers
    // feed it media (`insertMedia`); a creative unlocker makes it endless.
    for id in IMPETUSES {
        registry.provide_key::<yog_api::event::block::Insert>(id, |ev, _| {
            let pos = from_ypos(ev.pos);
            let mut imp = impetus(&ev.dimension, pos);
            if imp.room() == 0 {
                return false;
            }
            if ev.item_id == crate::content::Items::CREATIVE_UNLOCKER {
                if !ev.simulate {
                    imp.media = -1;
                    save_impetus(&ev.dimension, pos, &imp);
                }
                return true;
            }
            let mut stack = crate::casting::env::Stack {
                slot: 0,
                id: ev.item_id.clone(),
                count: ev.count,
                nbt: crate::snbt::Nbt::parse(&ev.nbt),
            };
            let taken = crate::item::media::withdraw(&mut stack, imp.room(), ev.simulate);
            if taken <= 0 {
                return false;
            }
            if !ev.simulate {
                imp.media = (imp.media + taken).min(MAX_MEDIA);
                save_impetus(&ev.dimension, pos, &imp);
            }
            true
        });
    }

    registry.on::<yog_api::event::block::Break, _>(|ev, cx| {
        let phase = cx.phase();
        if phase != EventPhase::Post {
            return true;
        }
        let pos = from_ypos(ev.pos);
        if ev.block_id == Blocks::SLATE {
            // The slate keeps its pattern (the original's loot table copies it).
            let pattern = slate_pattern(&ev.dimension, pos);
            set_slate_pattern(&ev.dimension, pos, None);
            crate::render::slate_changed(&ev.dimension, pos, None);
            let creative = yog_api::event::player::info(&ev.player_name)
                .is_some_and(|p| p.gamemode == "creative");
            if !creative {
                let c = Vec3::center_of(pos);
                yog_api::event::item::spawn_item(
                    &ev.dimension,
                    c.tuple(),
                    (0.0, 0.1, 0.0),
                    Blocks::SLATE,
                    1,
                    slate_item_nbt(pattern.as_ref()),
                );
            }
        }
        if is_impetus(&ev.block_id) {
            let mut imp = impetus(&ev.dimension, pos);
            end(&ev.dimension, &mut imp);
            forget_impetus(&ev.dimension, pos);
            KNOWN.lock().remove(&(ev.dimension.clone(), pos));
            save_known();
        }
        true
    });

    registry.on::<yog_api::event::block::Use, _>(|ev, cx| {
        let phase = cx.phase();
        if phase != EventPhase::Pre {
            return true;
        }
        let pos = from_ypos(ev.pos);
        match ev.block_id.as_str() {
            // `BlockRightClickImpetus.use`.
            Blocks::IMPETUS_RIGHTCLICK if !ev.sneaking => {
                start(&ev.dimension, pos, Caster::by_name(&ev.player_name));
                false
            }
            // `BlockRedstoneImpetus.use`: bind the player a held iota names, or
            // (sneaking, empty-handed) unbind.
            Blocks::IMPETUS_REDSTONE => {
                let selected =
                    yog_api::event::player::info(&ev.player_name).map_or(0, |p| p.selected_slot);
                let held = crate::casting::env::Stack::read(&ev.player_name, selected);
                let c = Vec3::center_of(pos);
                if held.is_empty() && ev.sneaking {
                    let mut imp = impetus(&ev.dimension, pos);
                    imp.player = None;
                    save_impetus(&ev.dimension, pos, &imp);
                    yog_api::event::sound::play(
                        &ev.dimension,
                        (c.x, c.y, c.z),
                        SOUND_REDSTONE_CLEAR,
                        1.0,
                        1.0,
                    );
                    return false;
                }
                let iota = holder::read(&held);
                if let Some(Iota::Entity(e)) = iota
                    && let Some(info) = yog_api::event::entity::info(&e.uuid)
                    && info.has(yog_api::core::entity_flags::PLAYER)
                {
                    let mut imp = impetus(&ev.dimension, pos);
                    imp.player = Some((e.uuid.clone(), info.name));
                    save_impetus(&ev.dimension, pos, &imp);
                    yog_api::event::sound::play(
                        &ev.dimension,
                        (c.x, c.y, c.z),
                        SOUND_REDSTONE_DING,
                        1.0,
                        1.0,
                    );
                    return false;
                }
                true
            }
            _ => true,
        }
    });
}
