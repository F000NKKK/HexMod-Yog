//! An impetus' state (`BlockEntityAbstractImpetus`), and the impetuses in the world.

use super::{Dir, MAX_MEDIA};
use crate::casting::env::to_ypos;
use crate::casting::image::Image;
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::item::pigment::Pigment;
use crate::math::HexPattern;
use crate::res::Lang;
use crate::text::Text;
use std::collections::{HashMap, HashSet};
use yog_api::storage::Store;
use yog_api::sync::{LazyLock, Mutex};
use yog_api::vanilla;

/// The chunk's store `pos` is in, with a world open.
pub(super) fn store(dim: &str, pos: BlockPos) -> Option<Store> {
    yog_api::storage::chunk(dim, pos.0 >> 4, pos.2 >> 4)
}

/// Where a chunk's impetuses and slates are kept in its store.
pub(super) const IMPETUS_KEY: &str = "impetus";
pub(super) const SLATES_KEY: &str = "slates";

/// What an impetus keeps, as it's kept with its chunk (its message and
/// run in the casting's own text forms).
#[derive(Default, yog_api::Schema)]
pub(super) struct KeptImpetus {
    pub(super) pos: BlockPos,
    pub(super) media: i64,
    /// The item beside its message, and the message.
    pub(super) display: Option<(String, String)>,
    pub(super) pigment: Option<Pigment>,
    pub(super) run: Option<String>,
    pub(super) look: i32,
    /// The cleric impetus' player: uuid and name.
    pub(super) player: Option<(String, String)>,
}

/// A slate's pattern, kept with its chunk.
#[derive(yog_api::Schema)]
pub(super) struct KeptSlate {
    pub(super) pos: BlockPos,
    pub(super) pattern: String,
}

/// `CircleExecutionState`.
#[derive(Debug, Clone)]
pub(super) struct Run {
    pub(super) impetus: BlockPos,
    pub(super) impetus_dir: Dir,
    pub(super) reached: HashSet<BlockPos>,
    pub(super) current: BlockPos,
    pub(super) entered_from: Dir,
    pub(super) image: Image,
    pub(super) caster: Option<String>,
    pub(super) caster_pigment: Option<Pigment>,
    pub(super) reached_slate: i64,
    /// The circle's bounds, both inclusive.
    pub(super) min: BlockPos,
    pub(super) max: BlockPos,
}

pub(super) fn pos_str(p: BlockPos) -> String {
    format!("{},{},{}", p.0, p.1, p.2)
}

pub(super) fn parse_pos(s: &str) -> Option<BlockPos> {
    let mut it = s.split(',').map(|n| n.parse::<i32>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

impl Run {
    /// One `key\tvalue` a line.
    pub(super) fn encode(&self) -> String {
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

    pub(super) fn decode(s: &str) -> Option<Run> {
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
    pub(super) fn tick_speed(&self) -> u64 {
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
    pub(super) run: Option<Run>,
    /// The fletcher impetus' gaze count.
    pub(super) look: i32,
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

pub(super) fn save_impetus(dim: &str, pos: BlockPos, i: &Impetus) {
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

pub(super) fn forget_impetus(dim: &str, pos: BlockPos) {
    if let Some(s) = store(dim, pos) {
        s.update(IMPETUS_KEY, |all: &mut Vec<KeptImpetus>| {
            all.retain(|k| k.pos != pos)
        });
    }
}

impl Impetus {
    /// `getPigment`.
    pub(super) fn pigment(&self) -> Pigment {
        self.pigment
            .clone()
            .or_else(|| self.run.as_ref().and_then(|r| r.caster_pigment.clone()))
            .unwrap_or_default()
    }

    /// `postDisplay`.
    pub(super) fn post(&mut self, msg: &Text, item: &str) {
        self.display = Some((msg.clone(), item.to_owned()));
    }

    /// `postNoExits`.
    pub(super) fn post_no_exits(&mut self, pos: BlockPos) {
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
pub(super) struct Known {
    pub(super) id: String,
    pub(super) due: Option<u64>,
    pub(super) powered: bool,
}

pub(super) static KNOWN: LazyLock<Mutex<HashMap<(String, BlockPos), Known>>> =
    LazyLock::new(Default::default);
pub(super) static TICKS: Mutex<u64> = Mutex::new(0);
/// The slate a player is about to place: its pattern (from the item).
pub(super) static PLACING: LazyLock<Mutex<HashMap<String, Option<HexPattern>>>> =
    LazyLock::new(Default::default);

/// Where the impetuses there are, are kept in the world store.
pub(super) const KNOWN_KEY: &str = "circles";

/// An impetus there is, as it's kept with the world.
#[derive(yog_api::Schema)]
pub(super) struct KeptKnown {
    pub(super) dim: String,
    pub(super) pos: BlockPos,
    pub(super) id: String,
}

pub(super) fn save_known() {
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

pub(super) fn load_known() {
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
pub(super) const IMPETUSES: [&str; 3] = [
    Blocks::IMPETUS_RIGHTCLICK,
    Blocks::IMPETUS_LOOK,
    Blocks::IMPETUS_REDSTONE,
];

pub(super) fn is_impetus(id: &str) -> bool {
    IMPETUSES.contains(&id)
}
