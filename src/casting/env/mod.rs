//! Where a hex runs (Hex Casting's `CastingEnvironment` and its staff,
//! packaged-item and circle variants): who casts it, what's in range, what
//! media pays for it, and what a mishap does to the caster.

use yog_api::core::EntityInfo;
use yog_api::vanilla;

use crate::casting::mishap::{BlockPos, ShownItem, item_name};
use crate::casting::vm::Sound;
use crate::content::Advancements;
use crate::item::pigment::{Pigment, dye_pigment};
use crate::math::Vec3;
use crate::protocol::Hand;
use crate::snbt::Nbt;
use crate::text::Text;

mod items;
mod mishaps;
mod payment;
mod player;
mod spray;
mod world;

pub use mishaps::{ARMOR_SLOTS, bound};
pub use player::{
    Sentinel, from_ypos, get_pigment, get_sentinel, look_vec, other_hand, set_pigment,
    set_sentinel, to_ypos,
};
pub use spray::{Spray, spawn_dust};

pub const MOD_ID: &str = "hexcasting";

/// Enlightenment: most of one's health (`EnlightenmentTrigger`'s 0.8) paid
/// for one hex, and alive with at most half a heart's worth left over 1.
pub(super) const ENLIGHTENING_HEALTH_USED: f64 = 0.8;
pub(super) const ENLIGHTENING_HEALTH_LEFT: f64 = 1.0;
/// Media wasted, and spent, in one cast for "Waste Not…" and "…Want Not".
pub(super) const WASTEFUL_MEDIA: i64 = 89_000;
pub(super) const BIG_CAST_MEDIA: i64 = 6_400_000;
/// The attribute a player's health bar is.
pub(super) const MAX_HEALTH: &str = yog_api::yog_ids::attribute::MAX_HEALTH;
/// `HexConfig.server().maxOpCount()`.
pub const MAX_OP_COUNT: i64 = 100_000;
/// `PlayerBasedCastEnv.DEFAULT_AMBIT_RADIUS` / `DEFAULT_SENTINEL_RADIUS`.
pub const AMBIT_RADIUS: f64 = 32.0;
pub const SENTINEL_RADIUS: f64 = 16.0;
/// `HexConfig.common().mediaToHealthRate()`.
pub const MEDIA_TO_HEALTH: f64 = 10_000.0;
/// `HexConfig.common()`'s cooldowns, in ticks, after using a cypher,
/// trinket or artifact.
pub const CYPHER_COOLDOWN: u32 = 8;
pub const TRINKET_COOLDOWN: u32 = 5;
pub const ARTIFACT_COOLDOWN: u32 = 3;
/// `HexConfig.server().canTeleportInThisDimension()`: the dimensions
/// teleports don't work in (`DEFAULT_DIM_TP_DENYLIST`).
pub const DIM_TP_DENYLIST: &[&str] = &["twilightforest:twilight_forest"];
/// Inventory slot of the off hand (0–35 main, 36–39 armour).
pub const SLOT_OFFHAND: u32 = 40;
/// The head's armor slot in a player's inventory.
pub const SLOT_HEAD: u32 = 39;

/// A uniform random number in `[0, 1)`.
pub fn random() -> f64 {
    use std::cell::Cell;
    use std::hash::{BuildHasher, Hasher};
    thread_local! {
        static STATE: Cell<u64> = Cell::new({
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(0x9E37_79B9_7F4A_7C15);
            h.finish() | 1
        });
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// The caster: a player, by name (for inventory calls) and uuid.
#[derive(Debug, Clone, PartialEq)]
pub struct Caster {
    pub name: String,
    pub uuid: String,
}

impl Caster {
    pub fn by_name(name: &str) -> Option<Self> {
        let info = yog_api::event::player::info(name)?;
        Some(Caster {
            name: name.to_owned(),
            uuid: info.uuid,
        })
    }
}

/// An item stack in a player's inventory.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stack {
    pub slot: u32,
    pub id: String,
    pub count: u32,
    pub nbt: Nbt,
}

impl Stack {
    pub fn read(player: &str, slot: u32) -> Stack {
        match yog_api::event::player::slot_item(player, slot)
            .map(|s| (s.item.to_string(), s.count, s.snbt))
        {
            Some((id, count, snbt)) => Stack {
                slot,
                id,
                count,
                nbt: Nbt::parse(&snbt),
            },
            None => Stack {
                slot,
                ..Stack::default()
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.id.is_empty() || self.id == vanilla::items::AIR
    }

    pub fn is(&self, id: &str) -> bool {
        !self.is_empty() && self.id == id
    }

    pub fn shown(&self) -> Option<ShownItem> {
        (!self.is_empty()).then(|| ShownItem {
            count: self.count,
            name: item_name(&self.id),
        })
    }

    /// Writes the stack back to its slot.
    pub fn save(&self, player: &str) {
        let count = if self.is_empty() { 0 } else { self.count };
        yog_api::event::player::set_slot_item(
            player,
            self.slot,
            &self.id,
            count,
            self.nbt.to_snbt(),
        );
    }
}

/// A spell circle's run (`CircleExecutionState`), for the circle env.
#[derive(Debug, Clone, PartialEq)]
pub struct CircleRun {
    pub impetus: BlockPos,
    /// The way the impetus faces (a unit block offset).
    pub facing: (i32, i32, i32),
    pub min: BlockPos,
    pub max: BlockPos,
    /// The component running now (sounds and mishaps come from it).
    pub current: BlockPos,
}

/// What a hex is cast from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A staff, drawing on the grid.
    Staff,
    /// A cypher, trinket or artifact in the casting hand.
    Packaged,
    /// A spell circle (the caster, if any, is the impetus' owner).
    Circle(CircleRun),
}

/// `StackDiscoveryMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discovery {
    Query,
    Extraction,
}

pub struct CastEnv {
    pub dim: String,
    pub caster: Option<Caster>,
    pub hand: Hand,
    pub source: Source,
    /// Staff casts play at most 100 sounds per cast.
    sounds_played: u32,
    /// The loudest sound of the cast so far (packaged items play it at the end).
    pub loudest: Sound,
    /// Media a circle's impetus holds (circle casts).
    pub circle_media: i64,
    /// The impetus' pigment (circle casts; a colorize changes it).
    pub circle_pigment: Option<Pigment>,
    /// What the impetus shows after this (circle casts): a message and the
    /// item beside it (`postPrint`, `postMishap`).
    pub circle_posts: Vec<(Text, &'static str)>,
}

impl CastEnv {
    pub fn new(caster: Option<Caster>, hand: Hand, source: Source, dim: String) -> Self {
        CastEnv {
            dim,
            caster,
            hand,
            source,
            sounds_played: 0,
            loudest: Sound::Nothing,
            circle_media: 0,
            circle_pigment: None,
            circle_posts: Vec::new(),
        }
    }

    /// A player's env, in the dimension they're in.
    pub fn for_player(caster: Caster, hand: Hand, source: Source) -> Option<Self> {
        let dim = yog_api::event::entity::info(&caster.uuid)?
            .dimension
            .to_string();
        Some(Self::new(Some(caster), hand, source, dim))
    }

    // ── the caster ─────────────────────────────────────────────────────────

    pub fn caster_uuid(&self) -> Option<String> {
        self.caster.as_ref().map(|c| c.uuid.clone())
    }

    pub fn caster_name(&self) -> Option<&str> {
        self.caster.as_ref().map(|c| c.name.as_str())
    }

    pub fn caster_info(&self) -> Option<EntityInfo> {
        yog_api::event::entity::info(&self.caster.as_ref()?.uuid)
    }

    pub fn caster_pos(&self) -> Option<Vec3> {
        self.caster_info().map(|i| Vec3::from_tuple(i.pos))
    }

    pub fn is_creative(&self) -> bool {
        self.caster_name()
            .and_then(yog_api::event::player::info)
            .is_some_and(|p| p.gamemode == "creative")
    }

    /// Whether the caster has advancement `id`.
    pub fn has_advancement(&self, id: &str) -> bool {
        self.caster
            .as_ref()
            .is_some_and(|c| yog_api::advancements::done(&c.uuid, id))
    }

    /// Awards the caster advancement `id`.
    pub fn grant_advancement(&self, id: &str) {
        if let Some(c) = &self.caster {
            yog_api::advancements::grant(&c.uuid, id);
        }
    }

    /// `isEnlightened` (a circle without a caster is).
    pub fn is_enlightened(&self) -> bool {
        if matches!(self.source, Source::Circle(_)) && self.caster.is_none() {
            return true;
        }
        self.has_advancement(Advancements::ENLIGHTENMENT)
    }

    /// `printMessage`: a circle's impetus shows it instead (`postPrint`).
    pub fn print_message(&mut self, msg: &Text) {
        if matches!(self.source, Source::Circle(_)) {
            self.circle_posts.push((msg.clone(), vanilla::items::BOOK));
        } else if let Some(name) = self.caster_name() {
            yog_api::event::chat::tellraw(name, msg.to_json());
        }
    }

    /// A mishap's message: to the caster, or on a circle's impetus
    /// (`postMishap`).
    pub fn report_mishap(&mut self, msg: &Text) {
        if matches!(self.source, Source::Circle(_)) {
            self.circle_posts
                .push((msg.clone(), vanilla::items::MUSIC_DISC_11));
        } else {
            self.print_message(msg);
        }
    }

    /// The caster's pigment; a packaged hex uses the one it was made with,
    /// a circle its impetus'.
    pub fn pigment(&self) -> Pigment {
        if matches!(self.source, Source::Circle(_)) {
            return self.circle_pigment.clone().unwrap_or_default();
        }
        if self.source == Source::Packaged
            && let Some(p) = self
                .held(self.hand)
                .and_then(|s| crate::item::packaged::pigment(&s))
        {
            return p;
        }
        match &self.caster {
            Some(c) => get_pigment(&c.uuid),
            None => Pigment::default(),
        }
    }

    pub fn spray(&self, spray: &Spray) {
        spray.spawn(&self.dim, &self.pigment());
    }

    /// The mishap's two sprays: its accent dye, then red.
    pub fn mishap_spray(&self, spray: &Spray, accent: u32) {
        spray.spawn(&self.dim, &dye_pigment(accent));
        spray.spawn(&self.dim, &dye_pigment(crate::casting::mishap::dye::RED));
    }

    /// `setPigment`: the caster's, or a circle's impetus'.
    pub fn set_pigment(&mut self, p: Pigment) {
        if matches!(self.source, Source::Circle(_)) {
            self.circle_pigment = Some(p);
        } else if let Some(uuid) = self.caster_uuid() {
            set_pigment(&uuid, &p);
        }
    }

    /// `postExecution`: a staff plays each pattern's sound, a circle where
    /// it runs; packaged items remember the loudest one.
    pub fn post_execution(&mut self, sound: Sound) {
        if let Source::Circle(c) = &self.source {
            if let Some(id) = sound.id() {
                let p = Vec3::center_of(c.current);
                yog_api::event::sound::play(&self.dim, (p.x, p.y, p.z), id, 1.0, 1.0);
            }
            return;
        }
        match self.source {
            Source::Staff => {
                if let (Some(id), Some(pos)) = (sound.id(), self.caster_pos())
                    && self.sounds_played < 100
                {
                    yog_api::event::sound::play(&self.dim, (pos.x, pos.y, pos.z), id, 1.0, 1.0);
                    self.sounds_played += 1;
                }
            }
            _ => self.loudest = self.loudest.max(sound),
        }
    }
}
