//! Spell circles: Hex Casting's `ICircleComponent`, `BlockCircleComponent`,
//! `CircleExecutionState`, `BlockEntityAbstractImpetus`, the impetuses,
//! directrices and slates.
//!
//! The blocks are the loader's stateful blocks (`facing`, `energized`, …);
//! what the original keeps in block entities — an impetus' media, its run,
//! its display, a slate's pattern — is kept in the chunk's mod storage,
//! keyed by position.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use yog_api::vanilla;
use yog_api::{EventPhase, Registry, Server, Storage, split_state};

use crate::casting::env::{
    CastEnv, Caster, CircleRun, MOD_ID, Source, Spray, from_ypos, get_pigment, random, to_ypos,
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

const SOUND_FIND_BLOCK: &str = crate::sounds::SPELLCIRCLE_FIND_BLOCK;
const SOUND_FAIL: &str = crate::sounds::SPELLCIRCLE_FAIL;
const SOUND_LOOK_TICK: &str = crate::sounds::IMPETUS_FLETCHER_TICK;
const SOUND_REDSTONE_DING: &str = crate::sounds::IMPETUS_REDSTONE_REGISTER;
const SOUND_REDSTONE_CLEAR: &str = crate::sounds::IMPETUS_REDSTONE_CLEAR;

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
    fn at(srv: &dyn Server, dim: &str, pos: BlockPos) -> Option<Part> {
        let state = srv.get_block_state(dim, to_ypos(pos))?;
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
    fn can_enter(&self, srv: &dyn Server, dim: &str, enter: Dir, pos: BlockPos) -> bool {
        let f = self.facing();
        match self.kind {
            // FACING is where media leaves: it can't come in that way.
            Kind::Impetus | Kind::EmptyImpetus => enter != f.opposite(),
            Kind::DirEmpty | Kind::DirRedstone | Kind::DirBoolean => {
                enter != f.opposite() && enter != f
            }
            Kind::Slate => enter != self.normal(srv, dim, pos, 16).opposite(),
        }
    }

    /// `possibleExitDirections`.
    fn exits(&self, srv: &dyn Server, dim: &str, pos: BlockPos) -> Vec<Dir> {
        let f = self.facing();
        match self.kind {
            Kind::Impetus | Kind::EmptyImpetus => vec![f],
            Kind::DirEmpty | Kind::DirRedstone | Kind::DirBoolean => vec![f, f.opposite()],
            Kind::Slate => {
                let normal = self.normal(srv, dim, pos, 16);
                Dir::ALL.into_iter().filter(|&d| d != normal).collect()
            }
        }
    }

    /// `normalDir`: which way is "up" for the particles here.
    fn normal(&self, srv: &dyn Server, dim: &str, pos: BlockPos, left: i32) -> Dir {
        match self.kind {
            Kind::Impetus | Kind::EmptyImpetus | Kind::DirBoolean => {
                normal_of_other(srv, dim, relative(pos, self.facing()), left)
            }
            Kind::DirRedstone => normal_of_other(srv, dim, relative(pos, self.real_facing()), left),
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
fn normal_of_other(srv: &dyn Server, dim: &str, other: BlockPos, left: i32) -> Dir {
    if left <= 0 {
        return Dir::Up;
    }
    match Part::at(srv, dim, other) {
        Some(p) => p.normal(srv, dim, other, left - 1),
        None => Dir::Up,
    }
}

/// `ICircleComponent.ControlFlow`.
enum Flow {
    Continue(Image, Vec<(BlockPos, Dir)>),
    Stop,
}

/// `startEnergized` / `endEnergized`.
fn set_energized(srv: &dyn Server, dim: &str, pos: BlockPos, part: &Part, on: bool) {
    let mut props = format!("energized={on}");
    if !on && part.kind == Kind::DirBoolean {
        props.push_str(",state=neither");
    }
    srv.set_block_properties(dim, to_ypos(pos), &props);
}

// ── an impetus' state (`BlockEntityAbstractImpetus`) ────────────────────────

fn store(srv: &dyn Server, dim: &str, pos: BlockPos) -> Storage {
    Storage::open_chunk(&srv.world_dir(), MOD_ID, dim, pos.0 >> 4, pos.2 >> 4)
}

fn key(what: &str, pos: BlockPos) -> String {
    format!("{what}.{}.{}.{}", pos.0, pos.1, pos.2)
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

pub fn impetus(srv: &dyn Server, dim: &str, pos: BlockPos) -> Impetus {
    let s = store(srv, dim, pos);
    let get = |what: &str| s.get_str(&key(what, pos)).map(str::to_owned);
    Impetus {
        media: s.get_int(&key("impetus.media", pos)).unwrap_or(0),
        display: get("impetus.display").and_then(|d| {
            let (item, text) = d.split_once('\t')?;
            Some((Text::decode(text)?, item.to_owned()))
        }),
        pigment: get("impetus.pigment").and_then(|p| {
            let (item, owner) = p.split_once('\t')?;
            Some(Pigment {
                item: item.into(),
                owner: owner.into(),
            })
        }),
        run: get("impetus.run").and_then(|r| Run::decode(&r)),
        look: s.get_int(&key("impetus.look", pos)).unwrap_or(0) as i32,
        player: get("impetus.player").and_then(|p| {
            let (uuid, name) = p.split_once('\t')?;
            Some((uuid.into(), name.into()))
        }),
    }
}

fn save_impetus(srv: &dyn Server, dim: &str, pos: BlockPos, i: &Impetus) {
    let mut s = store(srv, dim, pos);
    s.set(key("impetus.media", pos), i.media);
    let mut put = |what: &str, v: Option<String>| match v {
        Some(v) => s.set(key(what, pos), v),
        None => {
            s.remove(&key(what, pos));
        }
    };
    put(
        "impetus.display",
        i.display
            .as_ref()
            .map(|(text, item)| format!("{item}\t{}", text.encode())),
    );
    put(
        "impetus.pigment",
        i.pigment
            .as_ref()
            .map(|p| format!("{}\t{}", p.item, p.owner)),
    );
    put("impetus.run", i.run.as_ref().map(Run::encode));
    put(
        "impetus.player",
        i.player.as_ref().map(|(u, n)| format!("{u}\t{n}")),
    );
    s.set(key("impetus.look", pos), i.look as i64);
}

fn forget_impetus(srv: &dyn Server, dim: &str, pos: BlockPos) {
    let mut s = store(srv, dim, pos);
    for what in [
        "impetus.media",
        "impetus.display",
        "impetus.pigment",
        "impetus.run",
        "impetus.player",
        "impetus.look",
    ] {
        s.remove(&key(what, pos));
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

const KNOWN_KEY: &str = "circles.impetuses";

fn global(srv: &dyn Server) -> Storage {
    Storage::open(&srv.world_dir(), MOD_ID)
}

fn save_known(srv: &dyn Server) {
    let known = KNOWN.lock().unwrap();
    let list: Vec<String> = known
        .iter()
        .map(|((dim, p), k)| format!("{dim}|{}|{}", pos_str(*p), k.id))
        .collect();
    global(srv).set(KNOWN_KEY, list.join("\n"));
}

fn load_known(srv: &dyn Server) {
    let text = global(srv).get_str(KNOWN_KEY).unwrap_or("").to_owned();
    let mut known = KNOWN.lock().unwrap();
    known.clear();
    for line in text.lines() {
        let mut it = line.split('|');
        let (Some(dim), Some(pos), Some(id)) =
            (it.next(), it.next().and_then(parse_pos), it.next())
        else {
            continue;
        };
        let i = impetus(srv, dim, pos);
        let powered = srv.block_property(dim, to_ypos(pos), "powered").as_deref() == Some("true");
        known.insert(
            (dim.to_owned(), pos),
            Known {
                id: id.to_owned(),
                due: i.run.as_ref().map(|_| 1),
                powered,
            },
        );
    }
}

fn is_impetus(id: &str) -> bool {
    matches!(
        id,
        Blocks::IMPETUS_RIGHTCLICK | Blocks::IMPETUS_LOOK | Blocks::IMPETUS_REDSTONE
    )
}

// ── a run ───────────────────────────────────────────────────────────────────

/// `CircleExecutionState.createNew`: flood-fills the circle from the
/// impetus. `Err(None)`: nowhere to go; `Err(Some(pos))`: the media can't
/// get back to the impetus, `pos` the last block it reached.
fn create_run(
    srv: &dyn Server,
    dim: &str,
    pos: BlockPos,
    dir: Dir,
    caster: Option<&Caster>,
) -> Result<Run, Option<BlockPos>> {
    let mut todo = vec![(dir, relative(pos, dir))];
    let mut seen = HashSet::new();
    let (mut lo, mut hi, mut last) = (pos, pos, pos);
    while let Some((enter, here)) = todo.pop() {
        let Some(part) = Part::at(srv, dim, here) else {
            continue;
        };
        if !part.can_enter(srv, dim, enter, here) {
            continue;
        }
        if seen.insert(here) {
            last = here;
            hi = (hi.0.max(here.0), hi.1.max(here.1), hi.2.max(here.2));
            lo = (lo.0.min(here.0), lo.1.min(here.1), lo.2.min(here.2));
            for out in part.exits(srv, dim, here) {
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
        caster_pigment: caster.map(|c| get_pigment(srv, &c.uuid)),
        reached_slate: 0,
        min: lo,
        max: hi,
    })
}

/// `ICircleComponent.sfx`: particles and a sound where media moves (or fails).
fn sfx(
    srv: &dyn Server,
    dim: &str,
    pos: BlockPos,
    imp: Option<(&Impetus, BlockPos)>,
    success: bool,
) {
    let (center, out) = match Part::at(srv, dim, pos) {
        Some(part) => {
            let n = part.normal(srv, dim, pos, 16).step();
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
    spray.spawn(srv, dim, &pigment);
    let (mut sound, mut pitch) = (SOUND_FAIL, 1.0f32);
    if success && let Some((i, at)) = imp {
        sound = SOUND_FIND_BLOCK;
        let note = i.run.as_ref().map_or(0, |r| r.reached_slate - 1);
        let semitone = semitone_from_scale(srv, dim, at, note);
        pitch = 2f32.powf((semitone as f32 - 8.0) / 12.0);
    }
    srv.play_sound(dim, center.x, center.y, center.z, sound, 1.0, pitch);
}

/// `semitoneFromScale`: the block under the impetus picks the scale.
fn semitone_from_scale(srv: &dyn Server, dim: &str, impetus: BlockPos, note: i64) -> i32 {
    const MAJOR: &[i32] = &[0, 2, 4, 5, 7, 9, 11, 12];
    const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 11, 12];
    const DORIAN: &[i32] = &[0, 2, 3, 5, 7, 9, 10, 12];
    const MIXOLYDIAN: &[i32] = &[0, 2, 4, 5, 7, 9, 10, 12];
    const BLUES: &[i32] = &[0, 3, 5, 6, 7, 10, 12];
    const BAD_TIME: &[i32] = &[0, 0, 12, 7, 6, 5, 3, 0, 3, 5];
    const SUSSY_BAKA: &[i32] = &[5, 8, 10, 11, 10, 8, 5, 3, 7, 5];
    let below = (impetus.0, impetus.1 - 1, impetus.2);
    let id = srv
        .block_info(dim, to_ypos(below))
        .map(|b| b.block_id().to_owned())
        .unwrap_or_default();
    let tagged = |tag: &str| srv.has_block_tag(&id, tag);
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
pub fn start(srv: &dyn Server, dim: &str, pos: BlockPos, caster: Option<Caster>) {
    let Some(part) = Part::at(srv, dim, pos).filter(|p| p.kind == Kind::Impetus) else {
        return;
    };
    let mut imp = impetus(srv, dim, pos);
    if imp.run.is_some() {
        return;
    }
    match create_run(srv, dim, pos, part.facing(), caster.as_ref()) {
        Err(None) => {
            sfx(srv, dim, pos, None, false);
            imp.post_no_exits(pos);
        }
        Err(Some(at)) => {
            sfx(srv, dim, at, None, false);
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
            set_energized(srv, dim, pos, &part, true);
            if let Some(k) = KNOWN.lock().unwrap().get_mut(&(dim.to_owned(), pos)) {
                k.due = Some(*TICKS.lock().unwrap() + speed);
            }
        }
    }
    save_impetus(srv, dim, pos, &imp);
}

/// `CircleExecutionState.tick`: one step of a run; whether to go on.
fn step(srv: &dyn Server, dim: &str, imp: &mut Impetus, at: BlockPos) -> bool {
    let Some(run) = imp.run.clone() else {
        return false;
    };
    let caster = run.caster.as_ref().and_then(|uuid| {
        let info = srv.entity_info(uuid)?;
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
    let mut env = CastEnv::new(
        srv,
        caster,
        Hand::Off,
        Source::Circle(circle),
        dim.to_owned(),
    );
    env.circle_media = if imp.media < 0 { i64::MAX } else { imp.media };
    env.circle_pigment = Some(imp.pigment());

    let Some(part) = Part::at(srv, dim, run.current) else {
        sfx(srv, dim, run.current, Some((imp, at)), false);
        return false;
    };
    set_energized(srv, dim, run.current, &part, true);
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
    if Part::at(srv, dim, at).is_none_or(|p| p.kind != Kind::Impetus) {
        return false; // the impetus is gone
    }
    match flow {
        Flow::Stop => false,
        Flow::Continue(image, exits) => {
            let mut found: Option<(BlockPos, Dir)> = None;
            for (pos, dir) in exits {
                let ok = Part::at(srv, dim, pos).is_some_and(|p| p.can_enter(srv, dim, dir, pos));
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
                    sfx(srv, dim, run.current, Some((imp, at)), false);
                    return false;
                }
                found = Some((pos, dir));
            }
            match found {
                None => {
                    sfx(srv, dim, run.current, Some((imp, at)), false);
                    imp.post_no_exits(run.current);
                    false
                }
                Some((pos, dir)) => {
                    sfx(srv, dim, run.current, Some((imp, at)), true);
                    let run = imp.run.as_mut().expect("running");
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
fn accept(part: &Part, image: Image, env: &mut CastEnv<'_>, enter: Dir, pos: BlockPos) -> Flow {
    let (srv, dim) = (env.srv, env.dim.clone());
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
                    srv.set_block_properties(
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
                .exits(srv, &dim, pos)
                .into_iter()
                .filter(|&d| d != enter.opposite())
                .map(exit)
                .collect();
            let Some(pattern) = slate_pattern(srv, &dim, pos) else {
                return Flow::Continue(image, exits);
            };
            // A slate's cast runs in its own env, one that shares the impetus' state.
            let mut vm = Vm::new(
                image,
                std::mem::replace(
                    env,
                    CastEnv::new(srv, None, Hand::Off, Source::Staff, dim.clone()),
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
fn fake_throw_mishap(
    part: &Part,
    pos: BlockPos,
    image: Image,
    env: &mut CastEnv<'_>,
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
fn end(srv: &dyn Server, dim: &str, imp: &mut Impetus) {
    if let Some(run) = imp.run.take() {
        for pos in run.reached {
            if let Some(part) = Part::at(srv, dim, pos) {
                set_energized(srv, dim, pos, &part, false);
            }
        }
    }
}

/// `tickExecution`.
fn tick_run(srv: &dyn Server, dim: &str, pos: BlockPos, now: u64) -> Option<u64> {
    let mut imp = impetus(srv, dim, pos);
    imp.run.as_ref()?;
    let go_on = step(srv, dim, &mut imp, pos);
    let due = if go_on {
        Some(now + imp.run.as_ref().map_or(10, Run::tick_speed))
    } else {
        end(srv, dim, &mut imp);
        None
    };
    save_impetus(srv, dim, pos, &imp);
    due
}

// ── the impetuses' triggers ─────────────────────────────────────────────────

/// `BlockEntityLookingImpetus.serverTick`: someone staring at it fills it.
fn tick_look(srv: &dyn Server, dim: &str, pos: BlockPos) {
    if srv
        .block_property(dim, to_ypos(pos), "energized")
        .as_deref()
        == Some("true")
    {
        return;
    }
    let mut looker = None;
    for name in srv.online_players() {
        let Some(info) = srv
            .player_info(&name)
            .and_then(|p| Some((p.uuid.clone(), srv.entity_info(&p.uuid)?)))
        else {
            continue;
        };
        let (uuid, e) = info;
        let eye = Vec3::new(e.pos.0, e.pos.1 + e.eye_height, e.pos.2);
        if e.dimension != dim
            || eye.distance_sqr(Vec3::center_of(pos)) > 3.0 * LOOK_RANGE * LOOK_RANGE
        {
            continue;
        }
        // A carved pumpkin on the head hides the gaze.
        if crate::casting::env::Stack::read(srv, &name, 39).id == vanilla::items::CARVED_PUMPKIN {
            continue;
        }
        let end = eye + crate::casting::env::look_vec(srv, &uuid) * (LOOK_RANGE / 1.5);
        if srv
            .raycast_block(dim, eye.tuple(), end.tuple())
            .is_some_and(|h| from_ypos(h.pos) == pos)
        {
            looker = Caster::by_name(srv, &name);
            break;
        }
    }
    let mut imp = impetus(srv, dim, pos);
    let prev = imp.look;
    let new = (prev + if looker.is_some() { 1 } else { -1 }).clamp(0, MAX_LOOK);
    if new == prev {
        return;
    }
    if new == MAX_LOOK {
        imp.look = 0;
        save_impetus(srv, dim, pos, &imp);
        start(srv, dim, pos, looker);
    } else {
        if new % 5 == 1 {
            let t = new as f32 / MAX_LOOK as f32;
            let (pitch, volume) = (0.5 + (1.2 - 0.5) * t, 0.2 + (1.2 - 0.2) * t);
            let c = Vec3::center_of(pos);
            srv.play_sound(dim, c.x, c.y, c.z, SOUND_LOOK_TICK, volume, pitch);
        }
        imp.look = new;
        save_impetus(srv, dim, pos, &imp);
    }
}

pub fn tick(srv: &dyn Server) {
    let now = {
        let mut t = TICKS.lock().unwrap();
        *t += 1;
        *t
    };
    let known: Vec<((String, BlockPos), Known)> = KNOWN
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut changed = false;
    for ((dim, pos), k) in known {
        // Gone some way a break didn't see (an explosion, a piston).
        if now.is_multiple_of(20)
            && srv
                .block_info(&dim, to_ypos(pos))
                .is_some_and(|b| b.block_id() != k.id)
        {
            KNOWN.lock().unwrap().remove(&(dim.clone(), pos));
            forget_impetus(srv, &dim, pos);
            changed = true;
            continue;
        }
        if k.due.is_some_and(|d| now >= d) {
            let due = tick_run(srv, &dim, pos, now);
            if let Some(e) = KNOWN.lock().unwrap().get_mut(&(dim.clone(), pos)) {
                e.due = due;
            }
        }
        match k.id.as_str() {
            Blocks::IMPETUS_LOOK => tick_look(srv, &dim, pos),
            // `BlockRedstoneImpetus.neighborChanged`: power coming on starts it.
            Blocks::IMPETUS_REDSTONE => {
                let powered =
                    srv.block_property(&dim, to_ypos(pos), "powered").as_deref() == Some("true");
                if powered != k.powered {
                    if let Some(e) = KNOWN.lock().unwrap().get_mut(&(dim.clone(), pos)) {
                        e.powered = powered;
                    }
                    if powered {
                        let player = impetus(srv, &dim, pos)
                            .player
                            .and_then(|(_, name)| Caster::by_name(srv, &name));
                        start(srv, &dim, pos, player);
                    }
                }
            }
            _ => {}
        }
    }
    if changed {
        save_known(srv);
    }
}

// ── slates ──────────────────────────────────────────────────────────────────

/// `BlockEntitySlate.pattern`.
pub fn slate_pattern(srv: &dyn Server, dim: &str, pos: BlockPos) -> Option<HexPattern> {
    HexPattern::from_serialized(store(srv, dim, pos).get_str(&key("slate", pos))?)
}

fn set_slate_pattern(srv: &dyn Server, dim: &str, pos: BlockPos, pattern: Option<&HexPattern>) {
    let mut s = store(srv, dim, pos);
    match pattern {
        Some(p) => s.set(key("slate", pos), p.serialized_form()),
        None => {
            s.remove(&key("slate", pos));
        }
    }
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
    registry.on_server_started(|srv| {
        load_known(srv);
        *TICKS.lock().unwrap() = 0;
    });
    registry.on_tick(tick);

    registry.on_player_place_block(|ev, phase, srv| {
        let pos = from_ypos(ev.pos);
        let Some(k) = kind(&ev.block_id) else {
            return true;
        };
        match phase {
            EventPhase::Pre => {
                if k == Kind::Slate {
                    // The item placing it is in a hand now; it may be used up after.
                    let selected = srv
                        .player_info(&ev.player_name)
                        .map_or(0, |p| p.selected_slot);
                    let pattern = [selected, crate::casting::env::SLOT_OFFHAND]
                        .into_iter()
                        .map(|slot| crate::casting::env::Stack::read(srv, &ev.player_name, slot))
                        .find(|s| s.id == Blocks::SLATE)
                        .and_then(|s| holder::written_pattern(&s.id, &s.nbt));
                    PLACING
                        .lock()
                        .unwrap()
                        .insert(ev.player_name.clone(), pattern);
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
                    && let Some(f) = srv
                        .block_property(&ev.dimension, ev.pos, "facing")
                        .as_deref()
                        .and_then(Dir::parse)
                {
                    srv.set_block_properties(
                        &ev.dimension,
                        ev.pos,
                        &format!("facing={}", f.opposite().name()),
                    );
                }
                if k == Kind::Slate {
                    let pattern = PLACING.lock().unwrap().remove(&ev.player_name).flatten();
                    set_slate_pattern(srv, &ev.dimension, pos, pattern.as_ref());
                    crate::render::slate_changed(srv, &ev.dimension, pos, pattern.as_ref());
                }
                if is_impetus(&ev.block_id) {
                    forget_impetus(srv, &ev.dimension, pos);
                    let powered = srv
                        .block_property(&ev.dimension, ev.pos, "powered")
                        .as_deref()
                        == Some("true");
                    KNOWN.lock().unwrap().insert(
                        (ev.dimension.clone(), pos),
                        Known {
                            id: ev.block_id.clone(),
                            due: None,
                            powered,
                        },
                    );
                    save_known(srv);
                }
            }
        }
        true
    });

    // `BlockEntityAbstractImpetus` as a container: hoppers and droppers
    // feed it media (`insertMedia`); a creative unlocker makes it endless.
    registry.on_item_insert(|ev, phase, srv| {
        if !is_impetus(&ev.block_id) {
            return false;
        }
        let pos = from_ypos(ev.pos);
        let mut imp = impetus(srv, &ev.dimension, pos);
        if imp.room() == 0 {
            return false;
        }
        if ev.item_id == crate::content::Items::CREATIVE_UNLOCKER {
            if phase == EventPhase::Post {
                imp.media = -1;
                save_impetus(srv, &ev.dimension, pos, &imp);
            }
            return true;
        }
        let mut stack = crate::casting::env::Stack {
            slot: 0,
            id: ev.item_id.clone(),
            count: ev.count,
            nbt: crate::snbt::Nbt::parse(&ev.nbt),
        };
        let taken = crate::item::media::withdraw(&mut stack, imp.room(), phase == EventPhase::Pre);
        if taken <= 0 {
            return false;
        }
        if phase == EventPhase::Post {
            imp.media = (imp.media + taken).min(MAX_MEDIA);
            save_impetus(srv, &ev.dimension, pos, &imp);
        }
        true
    });

    registry.on_block_break(|ev, phase, srv| {
        if phase != EventPhase::Post {
            return true;
        }
        let pos = from_ypos(ev.pos);
        if ev.block_id == Blocks::SLATE {
            // The slate keeps its pattern (the original's loot table copies it).
            let pattern = slate_pattern(srv, &ev.dimension, pos);
            set_slate_pattern(srv, &ev.dimension, pos, None);
            crate::render::slate_changed(srv, &ev.dimension, pos, None);
            let creative = srv
                .player_info(&ev.player_name)
                .is_some_and(|p| p.gamemode == "creative");
            if !creative {
                let c = Vec3::center_of(pos);
                srv.spawn_item(
                    &ev.dimension,
                    c.tuple(),
                    (0.0, 0.1, 0.0),
                    Blocks::SLATE,
                    1,
                    &slate_item_nbt(pattern.as_ref()),
                );
            }
        }
        if is_impetus(&ev.block_id) {
            let mut imp = impetus(srv, &ev.dimension, pos);
            end(srv, &ev.dimension, &mut imp);
            forget_impetus(srv, &ev.dimension, pos);
            KNOWN.lock().unwrap().remove(&(ev.dimension.clone(), pos));
            save_known(srv);
        }
        true
    });

    registry.on_use_block(|ev, phase, srv| {
        if phase != EventPhase::Pre {
            return true;
        }
        let pos = from_ypos(ev.pos);
        match ev.block_id.as_str() {
            // `BlockRightClickImpetus.use`.
            Blocks::IMPETUS_RIGHTCLICK if !ev.sneaking => {
                start(
                    srv,
                    &ev.dimension,
                    pos,
                    Caster::by_name(srv, &ev.player_name),
                );
                false
            }
            // `BlockRedstoneImpetus.use`: bind the player a held iota names, or
            // (sneaking, empty-handed) unbind.
            Blocks::IMPETUS_REDSTONE => {
                let selected = srv
                    .player_info(&ev.player_name)
                    .map_or(0, |p| p.selected_slot);
                let held = crate::casting::env::Stack::read(srv, &ev.player_name, selected);
                let c = Vec3::center_of(pos);
                if held.is_empty() && ev.sneaking {
                    let mut imp = impetus(srv, &ev.dimension, pos);
                    imp.player = None;
                    save_impetus(srv, &ev.dimension, pos, &imp);
                    srv.play_sound(&ev.dimension, c.x, c.y, c.z, SOUND_REDSTONE_CLEAR, 1.0, 1.0);
                    return false;
                }
                let iota = holder::read(&held);
                if let Some(Iota::Entity(e)) = iota
                    && let Some(info) = srv.entity_info(&e.uuid)
                    && info.has(yog_api::core::entity_flags::PLAYER)
                {
                    let mut imp = impetus(srv, &ev.dimension, pos);
                    imp.player = Some((e.uuid.clone(), info.name));
                    save_impetus(srv, &ev.dimension, pos, &imp);
                    srv.play_sound(&ev.dimension, c.x, c.y, c.z, SOUND_REDSTONE_DING, 1.0, 1.0);
                    return false;
                }
                true
            }
            _ => true,
        }
    });
}
