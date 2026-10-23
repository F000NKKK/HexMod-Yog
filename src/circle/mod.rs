//! Spell circles: Hex Casting's `ICircleComponent`, `BlockCircleComponent`,
//! `CircleExecutionState`, `BlockEntityAbstractImpetus`, the impetuses,
//! directrices and slates.
//!
//! The blocks are the loader's stateful blocks (`facing`, `energized`, …);
//! what the original keeps in block entities — an impetus' media, its run,
//! its display, a slate's pattern — is kept in the chunk's mod storage,
//! keyed by position.

use yog_api::split_state;

use crate::casting::env::to_ypos;
use crate::casting::image::Image;
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;

mod events;
mod impetus;
mod run;
mod triggers;

pub use events::register;
pub use impetus::{Impetus, impetus};
pub use run::start;
pub use triggers::{slate_pattern, tick};

/// `HexConfig.server().maxSpellCircleLength()` (`DEFAULT_MAX_SPELL_CIRCLE_LENGTH`).
pub(super) const MAX_LENGTH: usize = 1024;
/// `BlockEntityAbstractImpetus.MAX_CAPACITY`.
pub(super) const MAX_MEDIA: i64 = 9_000_000_000_000_000_000;
/// `BlockEntityLookingImpetus.MAX_LOOK_AMOUNT`.
pub(super) const MAX_LOOK: i32 = 30;
pub(super) const LOOK_RANGE: f64 = 20.0;
/// How far up a slate's face sits in its block (15/16).
const SLATE_TOP: f64 = 15.0 / 16.0;

pub(super) const SOUND_FIND_BLOCK: &str = crate::res::SoundEvents::SPELLCIRCLE_FIND_BLOCK;
pub(super) const SOUND_FAIL: &str = crate::res::SoundEvents::SPELLCIRCLE_FAIL;
pub(super) const SOUND_LOOK_TICK: &str = crate::res::SoundEvents::IMPETUS_FLETCHER_TICK;
pub(super) const SOUND_REDSTONE_DING: &str = crate::res::SoundEvents::IMPETUS_REDSTONE_REGISTER;
pub(super) const SOUND_REDSTONE_CLEAR: &str = crate::res::SoundEvents::IMPETUS_REDSTONE_CLEAR;

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

pub(super) fn relative(p: BlockPos, d: Dir) -> BlockPos {
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
pub(super) struct Part {
    pub(super) id: String,
    pub(super) kind: Kind,
    pub(super) props: Vec<(String, String)>,
}

impl Part {
    pub(super) fn at(dim: &str, pos: BlockPos) -> Option<Part> {
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

    pub(super) fn prop(&self, name: &str) -> &str {
        self.props
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v)
    }

    pub(super) fn facing(&self) -> Dir {
        Dir::parse(self.prop("facing")).unwrap_or(Dir::North)
    }

    /// `BlockRedstoneDirectrix.getRealFacing`.
    pub(super) fn real_facing(&self) -> Dir {
        if self.prop("powered") == "true" {
            self.facing()
        } else {
            self.facing().opposite()
        }
    }

    /// `canEnterFromDirection`.
    pub(super) fn can_enter(&self, dim: &str, enter: Dir, pos: BlockPos) -> bool {
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
    pub(super) fn exits(&self, dim: &str, pos: BlockPos) -> Vec<Dir> {
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
    pub(super) fn normal(&self, dim: &str, pos: BlockPos, left: i32) -> Dir {
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
    pub(super) fn particle_height(&self) -> f64 {
        if self.kind == Kind::Slate {
            0.5 - SLATE_TOP
        } else {
            0.5
        }
    }
}

/// `BlockCircleComponent.normalDirOfOther`.
pub(super) fn normal_of_other(dim: &str, other: BlockPos, left: i32) -> Dir {
    if left <= 0 {
        return Dir::Up;
    }
    match Part::at(dim, other) {
        Some(p) => p.normal(dim, other, left - 1),
        None => Dir::Up,
    }
}

/// `ICircleComponent.ControlFlow`.
pub(super) enum Flow {
    Continue(Image, Vec<(BlockPos, Dir)>),
    Stop,
}

/// `startEnergized` / `endEnergized`.
pub(super) fn set_energized(dim: &str, pos: BlockPos, part: &Part, on: bool) {
    let mut props = format!("energized={on}");
    if !on && part.kind == Kind::DirBoolean {
        props.push_str(",state=neither");
    }
    yog_api::event::block::set_properties(dim, to_ypos(pos), &props);
}
