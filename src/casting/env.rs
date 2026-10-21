//! Where a hex runs (Hex Casting's `CastingEnvironment` and its staff,
//! packaged-item and circle variants): who casts it, what's in range, what
//! media pays for it, and what a mishap does to the caster.

use std::sync::atomic::{AtomicU8, Ordering};

use yog_api::core::{EntityInfo, entity_flags};
use yog_api::vanilla;
use yog_api::{Server, Storage};

use crate::casting::mishap::{BlockPos, MResult, Mishap, ShownItem, item_name};
use crate::casting::vm::Sound;
use crate::content::Advancements;
use crate::iota::EntityRef;
use crate::item::media;
use crate::item::pigment::{Pigment, dye_pigment};
use crate::math::Vec3;
use crate::protocol::Hand;
use crate::snbt::Nbt;
use crate::text::Text;

pub const MOD_ID: &str = "hexcasting";

/// Enlightenment: most of one's health (`EnlightenmentTrigger`'s 0.8) paid
/// for one hex, and alive with at most half a heart's worth left over 1.
const ENLIGHTENING_HEALTH_USED: f64 = 0.8;
const ENLIGHTENING_HEALTH_LEFT: f64 = 1.0;
/// Media wasted, and spent, in one cast for "Waste Not…" and "…Want Not".
const WASTEFUL_MEDIA: i64 = 89_000;
const BIG_CAST_MEDIA: i64 = 6_400_000;
/// The attribute a player's health bar is.
const MAX_HEALTH: &str = "minecraft:generic.max_health";
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

/// A player's persistent state (flags, pigment, sentinel, staff grid).
pub fn player_storage(srv: &dyn Server, uuid: &str) -> Storage {
    Storage::open_player(&srv.world_dir(), MOD_ID, uuid)
}

/// The caster: a player, by name (for inventory calls) and uuid.
#[derive(Debug, Clone, PartialEq)]
pub struct Caster {
    pub name: String,
    pub uuid: String,
}

impl Caster {
    pub fn by_name(srv: &dyn Server, name: &str) -> Option<Self> {
        let info = srv.player_info(name)?;
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
    pub fn read(srv: &dyn Server, player: &str, slot: u32) -> Stack {
        match srv.get_slot_item(player, slot) {
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
    pub fn save(&self, srv: &dyn Server, player: &str) {
        let count = if self.is_empty() { 0 } else { self.count };
        srv.set_slot_item(player, self.slot, &self.id, count, &self.nbt.to_snbt());
    }
}

/// Hex Casting's `ParticleSpray`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spray {
    pub pos: Vec3,
    pub vel: Vec3,
    /// The radius of the sphere a particle may start in.
    pub fuzziness: f64,
    /// The largest angle (radians) a particle may stray from `vel`.
    pub spread: f64,
    pub count: u32,
}

impl Spray {
    pub fn burst(pos: Vec3, size: f64) -> Self {
        Self::burst_n(pos, size, 20)
    }

    pub fn burst_n(pos: Vec3, size: f64, count: u32) -> Self {
        Spray {
            pos,
            vel: Vec3::new(size, 0.0, 0.0),
            fuzziness: 0.0,
            spread: std::f64::consts::PI,
            count,
        }
    }

    pub fn cloud(pos: Vec3, size: f64) -> Self {
        Self::cloud_n(pos, size, 20)
    }

    pub fn cloud_n(pos: Vec3, size: f64, count: u32) -> Self {
        Spray {
            pos,
            vel: Vec3::new(0.0, 0.001, 0.0),
            fuzziness: size,
            spread: 0.0,
            count,
        }
    }

    /// `MsgCastParticleS2C.handle`, done on the server with colored dust
    /// (Hex Casting's conjure particles are client-side).
    pub fn spawn(&self, srv: &dyn Server, dim: &str, pigment: &Pigment) {
        let time = srv.world_time(dim).unwrap_or(0) as f32;
        let speed = self.vel.length() / 20.0;
        let v = self.vel.normalize();
        let k = if v.x == 0.0 && v.y == 0.0 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            v.cross(Vec3::new(0.0, 0.0, 1.0))
        };
        for _ in 0..self.count.min(64) {
            let offset = random_on_sphere() * (random() * self.fuzziness / 2.0);
            let pos = self.pos + offset;
            let phi = (1.0 - random() * (1.0 - self.spread.cos())).acos();
            let theta = std::f64::consts::TAU * random();
            let unlen = v * phi.cos()
                + k * (phi.sin() * theta.cos())
                + v.cross(k) * (phi.sin() * theta.sin());
            let vel = unlen * speed;
            let color = pigment.color(time, unlen);
            spawn_dust(srv, dim, pos, vel, color);
        }
    }
}

fn random_on_sphere() -> Vec3 {
    let th = random() * std::f64::consts::TAU;
    let y = random() * 2.0 - 1.0;
    let r = (1.0 - y * y).sqrt();
    Vec3::new(r * th.cos(), y, r * th.sin())
}

/// 0: not tried yet, 1: `dust{color:[…]}` (1.20.5+), 2: `dust r g b s`.
static DUST_SYNTAX: AtomicU8 = AtomicU8::new(0);

/// One colored dust particle moving along `vel`.
pub fn spawn_dust(srv: &dyn Server, dim: &str, pos: Vec3, vel: Vec3, color: u32) {
    let (r, g, b) = (
        ((color >> 16) & 0xff) as f64 / 255.0,
        ((color >> 8) & 0xff) as f64 / 255.0,
        (color & 0xff) as f64 / 255.0,
    );
    let new = || format!("minecraft:dust{{color:[{r:.3}f,{g:.3}f,{b:.3}f],scale:1.0f}}");
    let old = || format!("minecraft:dust {r:.3} {g:.3} {b:.3} 1.0");
    let send =
        |ty: &str| srv.spawn_particles(dim, pos.x, pos.y, pos.z, ty, 0, vel.x, vel.y, vel.z, 1.0);
    match DUST_SYNTAX.load(Ordering::Relaxed) {
        1 => {
            send(&new());
        }
        2 => {
            send(&old());
        }
        _ => {
            if send(&new()) {
                DUST_SYNTAX.store(1, Ordering::Relaxed);
            } else if send(&old()) {
                DUST_SYNTAX.store(2, Ordering::Relaxed);
            }
        }
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

pub struct CastEnv<'a> {
    pub srv: &'a dyn Server,
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

impl<'a> CastEnv<'a> {
    pub fn new(
        srv: &'a dyn Server,
        caster: Option<Caster>,
        hand: Hand,
        source: Source,
        dim: String,
    ) -> Self {
        CastEnv {
            srv,
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
    pub fn for_player(
        srv: &'a dyn Server,
        caster: Caster,
        hand: Hand,
        source: Source,
    ) -> Option<Self> {
        let dim = srv.entity_info(&caster.uuid)?.dimension;
        Some(Self::new(srv, Some(caster), hand, source, dim))
    }

    // ── the caster ─────────────────────────────────────────────────────────

    pub fn caster_uuid(&self) -> Option<String> {
        self.caster.as_ref().map(|c| c.uuid.clone())
    }

    pub fn caster_name(&self) -> Option<&str> {
        self.caster.as_ref().map(|c| c.name.as_str())
    }

    pub fn caster_info(&self) -> Option<EntityInfo> {
        self.srv.entity_info(&self.caster.as_ref()?.uuid)
    }

    pub fn caster_pos(&self) -> Option<Vec3> {
        self.caster_info().map(|i| Vec3::from_tuple(i.pos))
    }

    pub fn is_creative(&self) -> bool {
        self.caster_name()
            .and_then(|n| self.srv.player_info(n))
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
            self.srv.tellraw(name, &msg.to_json());
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
            Some(c) => get_pigment(self.srv, &c.uuid),
            None => Pigment::default(),
        }
    }

    pub fn spray(&self, spray: &Spray) {
        spray.spawn(self.srv, &self.dim, &self.pigment());
    }

    /// The mishap's two sprays: its accent dye, then red.
    pub fn mishap_spray(&self, spray: &Spray, accent: u32) {
        spray.spawn(self.srv, &self.dim, &dye_pigment(accent));
        spray.spawn(
            self.srv,
            &self.dim,
            &dye_pigment(crate::casting::mishap::dye::RED),
        );
    }

    /// `setPigment`: the caster's, or a circle's impetus'.
    pub fn set_pigment(&mut self, p: Pigment) {
        if matches!(self.source, Source::Circle(_)) {
            self.circle_pigment = Some(p);
        } else if let Some(uuid) = self.caster_uuid() {
            set_pigment(self.srv, &uuid, &p);
        }
    }

    /// `postExecution`: a staff plays each pattern's sound, a circle where
    /// it runs; packaged items remember the loudest one.
    pub fn post_execution(&mut self, sound: Sound) {
        if let Source::Circle(c) = &self.source {
            if let Some(id) = sound.id() {
                let p = Vec3::center_of(c.current);
                self.srv.play_sound(&self.dim, p.x, p.y, p.z, id, 1.0, 1.0);
            }
            return;
        }
        match self.source {
            Source::Staff => {
                if let (Some(id), Some(pos)) = (sound.id(), self.caster_pos())
                    && self.sounds_played < 100
                {
                    self.srv
                        .play_sound(&self.dim, pos.x, pos.y, pos.z, id, 1.0, 1.0);
                    self.sounds_played += 1;
                }
            }
            _ => self.loudest = self.loudest.max(sound),
        }
    }

    // ── entities and blocks ────────────────────────────────────────────────

    /// The entity, if it's loaded in this dimension.
    pub fn entity(&self, e: &EntityRef) -> Option<EntityInfo> {
        self.srv
            .entity_info(&e.uuid)
            .filter(|i| i.dimension == self.dim)
    }

    pub fn entity_pos(&self, e: &EntityRef) -> Option<Vec3> {
        self.entity(e).map(|i| Vec3::from_tuple(i.pos))
    }

    pub fn is_player(&self, uuid: &str) -> bool {
        self.srv
            .entity_info(uuid)
            .is_some_and(|i| i.has(entity_flags::PLAYER))
    }

    pub fn block_id(&self, pos: BlockPos) -> String {
        self.srv
            .block_info(&self.dim, to_ypos(pos))
            .map(|b| b.block_id().to_owned())
            .unwrap_or_else(|| vanilla::items::AIR.into())
    }

    /// A block's name (`block.<ns>.<path>`).
    pub fn block_name(&self, pos: BlockPos) -> Text {
        let id = self.block_id(pos);
        let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
        Text::key(format!("block.{ns}.{}", path.replace('/', ".")))
    }

    // ── ambit ──────────────────────────────────────────────────────────────

    pub fn mishap_spray_pos(&self) -> Vec3 {
        match &self.source {
            Source::Circle(c) => Vec3::center_of(c.current),
            _ => self.caster_pos().unwrap_or_default(),
        }
    }

    /// The caster's sentinel, if it lets them reach around it.
    fn sentinel_in_range(&self, v: Vec3) -> bool {
        let Some(c) = &self.caster else { return false };
        match get_sentinel(self.srv, &c.uuid) {
            Some(s) => {
                s.great
                    && s.dim == self.dim
                    && v.distance_sqr(s.pos) <= SENTINEL_RADIUS * SENTINEL_RADIUS + 1e-11
            }
            None => false,
        }
    }

    /// `isVecInRange`.
    pub fn is_vec_in_range(&self, v: Vec3) -> bool {
        if self.sentinel_in_range(v) {
            return true;
        }
        match &self.source {
            Source::Circle(c) => {
                let (lo, hi) = (c.min, c.max);
                v.x >= lo.0 as f64
                    && v.y >= lo.1 as f64
                    && v.z >= lo.2 as f64
                    && v.x <= hi.0 as f64 + 1.0
                    && v.y <= hi.1 as f64 + 1.0
                    && v.z <= hi.2 as f64 + 1.0
            }
            _ => match self.caster_pos() {
                Some(p) => v.distance_sqr(p) <= AMBIT_RADIUS * AMBIT_RADIUS + 1e-11,
                None => false,
            },
        }
    }

    /// `isVecInWorld`: inside the build height (and the loaded world).
    pub fn is_vec_in_world(&self, v: Vec3) -> bool {
        self.srv
            .block_info(&self.dim, to_ypos(v.block()))
            .is_some_and(|b| b.in_world)
    }

    /// `isEntityInRange` (players are always in range: `trueNameHasAmbit`).
    pub fn is_entity_in_range(&self, info: &EntityInfo) -> bool {
        let p = Vec3::from_tuple(info.pos);
        info.has(entity_flags::PLAYER)
            || (info.dimension == self.dim && self.is_vec_in_world(p) && self.is_vec_in_range(p))
    }

    pub fn assert_vec_in_world(&self, v: Vec3) -> MResult<()> {
        if self.is_vec_in_world(v) {
            Ok(())
        } else {
            Err(Mishap::BadLocation {
                pos: v,
                kind: "out_of_world",
            })
        }
    }

    pub fn assert_vec_in_range(&self, v: Vec3) -> MResult<()> {
        self.assert_vec_in_world(v)?;
        if self.is_vec_in_range(v) {
            Ok(())
        } else {
            Err(Mishap::BadLocation {
                pos: v,
                kind: "too_far",
            })
        }
    }

    pub fn assert_pos_in_range(&self, pos: BlockPos) -> MResult<()> {
        self.assert_vec_in_range(Vec3::center_of(pos))
    }

    /// `hasEditPermissionsAt`: adventure-mode casters can't edit; circles can.
    pub fn has_edit_permissions(&self, _pos: BlockPos) -> bool {
        if matches!(self.source, Source::Circle(_)) {
            return true;
        }
        match self.caster_name().and_then(|n| self.srv.player_info(n)) {
            Some(p) => p.gamemode != "adventure",
            None => matches!(self.source, Source::Circle(_)),
        }
    }

    pub fn can_edit_block_at(&self, pos: BlockPos) -> bool {
        self.is_vec_in_range(Vec3::center_of(pos)) && self.has_edit_permissions(pos)
    }

    pub fn assert_pos_in_range_for_editing(&self, pos: BlockPos) -> MResult<()> {
        let c = Vec3::center_of(pos);
        self.assert_vec_in_range(c)?;
        if self.can_edit_block_at(pos) {
            Ok(())
        } else {
            Err(Mishap::BadLocation {
                pos: c,
                kind: "forbidden",
            })
        }
    }

    pub fn assert_entity_in_range(&self, e: &EntityRef, info: &EntityInfo) -> MResult<()> {
        if info.has(entity_flags::PLAYER) {
            return Ok(());
        }
        let p = Vec3::from_tuple(info.pos);
        if info.dimension == self.dim && self.is_vec_in_world(p) && self.is_vec_in_range(p) {
            Ok(())
        } else {
            Err(Mishap::EntityTooFar {
                entity: e.clone(),
                pos: p,
            })
        }
    }

    // ── items ──────────────────────────────────────────────────────────────

    pub fn hand_slot(&self, hand: Hand) -> Option<u32> {
        let name = self.caster_name()?;
        Some(match hand {
            Hand::Main => self.srv.player_info(name)?.selected_slot,
            Hand::Off => SLOT_OFFHAND,
        })
    }

    pub fn stack_at(&self, slot: u32) -> Stack {
        match self.caster_name() {
            Some(n) => Stack::read(self.srv, n, slot),
            None => Stack {
                slot,
                ..Stack::default()
            },
        }
    }

    pub fn held(&self, hand: Hand) -> Option<Stack> {
        Some(self.stack_at(self.hand_slot(hand)?))
    }

    pub fn save(&self, stack: &Stack) {
        if let Some(n) = self.caster_name() {
            stack.save(self.srv, n);
        }
    }

    /// `getPrimaryStacks`: the other hand, then the casting hand.
    pub fn primary_stacks(&self) -> Vec<(Stack, Hand)> {
        if self.caster.is_none() {
            return Vec::new();
        }
        let other = other_hand(self.hand);
        [other, self.hand]
            .into_iter()
            .filter_map(|h| Some((self.held(h)?, h)))
            .collect()
    }

    /// `getHeldItemToOperateOn`.
    pub fn held_item_to_operate_on(&self, ok: impl Fn(&Stack) -> bool) -> Option<(Stack, Hand)> {
        self.primary_stacks().into_iter().find(|(s, _)| ok(s))
    }

    /// `getUsableStacks`.
    pub fn usable_stacks(&self, mode: Discovery) -> Vec<Stack> {
        let Some(name) = self.caster_name() else {
            return Vec::new();
        };
        let circle = matches!(self.source, Source::Circle(_));
        let selected = self.srv.player_info(name).map_or(0, |p| p.selected_slot);
        let mut slots = Vec::new();
        match mode {
            // A circle casts with no hand: both hands, then the hotbar
            // from right of the selected slot.
            Discovery::Query if circle => {
                slots.push(selected);
                slots.push(SLOT_OFFHAND);
                slots.extend((0..9).map(|d| (selected + 1 + d) % 9));
            }
            Discovery::Query => {
                slots.push(match other_hand(self.hand) {
                    Hand::Main => selected,
                    Hand::Off => SLOT_OFFHAND,
                });
                let anchor = if self.hand != Hand::Off {
                    (selected + 1) % 9
                } else {
                    0
                };
                slots.extend((0..9).map(|d| (anchor + d) % 9));
            }
            Discovery::Extraction => {
                slots.extend((0..36).rev().filter(|&i| i != selected));
                slots.push(SLOT_OFFHAND);
                slots.push(selected);
            }
        }
        slots
            .into_iter()
            .map(|s| self.stack_at(s))
            .filter(|s| mode == Discovery::Extraction || !s.is_empty())
            .collect()
    }

    pub fn query_for_matching_stack(&self, ok: impl Fn(&Stack) -> bool) -> Option<Stack> {
        self.usable_stacks(Discovery::Query)
            .into_iter()
            .find(|s| ok(s))
    }

    /// `withdrawItem`: takes `count` matching items (creative casters don't
    /// pay); `false` if there aren't enough.
    pub fn withdraw_item(&self, ok: impl Fn(&Stack) -> bool, count: u32, remove: bool) -> bool {
        if self.is_creative() {
            return true;
        }
        let mut present = 0;
        let mut matches = Vec::new();
        for s in self.usable_stacks(Discovery::Extraction) {
            if !s.is_empty() && ok(&s) {
                present += s.count;
                matches.push(s);
                if present >= count {
                    break;
                }
            }
        }
        if present < count {
            return false;
        }
        if remove {
            let mut left = count;
            for mut m in matches {
                let take = m.count.min(left);
                m.count -= take;
                left -= take;
                self.save(&m);
                if left == 0 {
                    break;
                }
            }
        }
        true
    }

    /// `replaceItem`: puts `with` where the first matching stack is.
    pub fn replace_item(
        &self,
        ok: impl Fn(&Stack) -> bool,
        with: &Stack,
        hand: Option<Hand>,
    ) -> bool {
        let Some(name) = self.caster_name() else {
            return false;
        };
        let put = |slot: u32| {
            let s = Stack {
                slot,
                ..with.clone()
            };
            s.save(self.srv, name);
        };
        if let Some(h) = hand
            && let Some(s) = self.held(h)
            && ok(&s)
        {
            put(s.slot);
            return true;
        }
        let selected = self.srv.player_info(name).map_or(0, |p| p.selected_slot);
        for i in (0..36).rev().filter(|&i| i != selected) {
            if ok(&self.stack_at(i)) {
                put(i);
                return true;
            }
        }
        for h in [other_hand(self.hand), self.hand] {
            if let Some(s) = self.held(h)
                && ok(&s)
            {
                put(s.slot);
                return true;
            }
        }
        false
    }

    // ── media ──────────────────────────────────────────────────────────────

    /// `extractMedia`: pays `cost`, returning what's left unpaid (≤ 0 when
    /// fully paid). With `simulate`, only says what would be left.
    pub fn extract_media(&mut self, cost: i64, simulate: bool) -> i64 {
        let left = self.extract_media_from_sources(cost, simulate);
        // `SpendMediaTrigger`: what one cast spent, and wasted.
        if !simulate {
            if cost - left.max(0) >= BIG_CAST_MEDIA {
                self.grant_advancement(Advancements::AAB_BIG_CAST);
            }
            if -left >= WASTEFUL_MEDIA {
                self.grant_advancement(Advancements::AAA_WASTEFUL_CAST);
            }
        }
        left
    }

    /// `OvercastTrigger`: `used` health paid for a hex, `left` left — eyes
    /// opened, and enlightenment for nearly all of it.
    fn overcast(&self, uuid: &str, used: f64, left: f64) {
        self.grant_advancement(Advancements::OPENED_EYES);
        let max = self
            .srv
            .entity_attribute_value(uuid, MAX_HEALTH)
            .unwrap_or(20.0)
            .max(1.0);
        if used / max >= ENLIGHTENING_HEALTH_USED && left > 0.0 && left <= ENLIGHTENING_HEALTH_LEFT
        {
            self.grant_advancement(Advancements::ENLIGHTENMENT);
        }
    }

    fn extract_media_from_sources(&mut self, cost: i64, simulate: bool) -> i64 {
        if cost == 0 {
            return 0;
        }
        match self.source.clone() {
            Source::Staff => {
                if self.is_creative() {
                    return 0;
                }
                let over = self.has_advancement(Advancements::Y_U_NO_CAST_ANGY);
                self.extract_from_inventory(cost, over, simulate)
            }
            Source::Packaged => {
                if self.is_creative() {
                    return 0;
                }
                let Some(mut stack) = self.held(self.hand) else {
                    return cost;
                };
                if !media::is_packaged(&stack.id) {
                    return cost;
                }
                let mut left = cost;
                let taken = media::withdraw_battery(&mut stack, left, simulate);
                if !simulate {
                    self.save(&stack);
                }
                left -= taken;
                if media::draws_from_inventory(&stack.id) && left > 0 {
                    let over = self.has_advancement(Advancements::Y_U_NO_CAST_ANGY);
                    left = self.extract_from_inventory(left, over, simulate);
                }
                left
            }
            Source::Circle(_) => {
                let taken = self.circle_media.min(cost);
                if !simulate {
                    self.circle_media -= taken;
                }
                cost - taken
            }
        }
    }

    /// `extractMediaFromInventory`.
    fn extract_from_inventory(&mut self, mut left: i64, overcast: bool, simulate: bool) -> i64 {
        let Some(name) = self.caster_name().map(str::to_owned) else {
            return left;
        };
        for mut source in media::scan_player(self.srv, &name) {
            let found = media::withdraw(&mut source, left, simulate);
            if !simulate {
                source.save(self.srv, &name);
            }
            left -= found;
            if left <= 0 {
                break;
            }
        }
        if left > 0 && overcast {
            let Some(uuid) = self.caster_uuid() else {
                return left;
            };
            let health = self.srv.entity_health(&uuid).unwrap_or(0.0) as f64;
            let to_remove = (left as f64 / MEDIA_TO_HEALTH).max(0.5);
            if simulate {
                left -= (health.min(to_remove) * MEDIA_TO_HEALTH).ceil() as i64;
            } else {
                let able = health * MEDIA_TO_HEALTH;
                self.truly_hurt(&uuid, to_remove as f32);
                let after = self.srv.entity_health(&uuid).unwrap_or(0.0) as f64;
                left -= (able - after * MEDIA_TO_HEALTH).ceil() as i64;
                self.overcast(&uuid, health - after, after);
            }
        }
        left
    }

    // ── what mishaps do to the caster (`MishapEnvironment`) ────────────────

    /// `Mishap.trulyHurt`: damage that lands even through invulnerability.
    pub fn truly_hurt(&self, uuid: &str, amount: f32) {
        let before = self.srv.entity_health(uuid).unwrap_or(0.0);
        let attacker = self.caster_uuid().unwrap_or_default();
        self.srv
            .entity_damage(uuid, amount, crate::ids::damage::OVERCAST, &attacker);
        let after = self.srv.entity_health(uuid).unwrap_or(0.0);
        if before > 0.0 && after >= before {
            let target = before - amount;
            if target <= 0.0 {
                self.srv.entity_kill(uuid);
            } else {
                self.srv.entity_set_health(uuid, target);
            }
        }
    }

    pub fn yeet_held_items_towards(&self, target: Vec3) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let Some(pos) = self.caster_pos() else { return };
        let delta = (target - pos).normalize() * 0.5;
        for h in [Hand::Main, Hand::Off] {
            let Some(stack) = self.held(h) else { continue };
            if stack.is_empty() {
                continue;
            }
            self.save(&Stack {
                slot: stack.slot,
                ..Stack::default()
            });
            self.yeet_item(&stack, pos, delta);
        }
    }

    fn yeet_item(&self, stack: &Stack, src: Vec3, delta: Vec3) {
        let vel = Vec3::new(
            delta.x + (random() - 0.5) * 0.1,
            delta.y + (random() - 0.5) * 0.1,
            delta.z + (random() - 0.5) * 0.1,
        );
        self.srv.spawn_item(
            &self.dim,
            (src.x, src.y + 1.0, src.z),
            vel.tuple(),
            &stack.id,
            stack.count,
            &stack.nbt.to_snbt(),
        );
    }

    pub fn drop_held_items(&self) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let Some(c) = self.caster_info() else { return };
        let look = self
            .caster_uuid()
            .map(|u| look_vec(self.srv, &u))
            .unwrap_or_default();
        self.yeet_held_items_towards(Vec3::from_tuple(c.pos) + look);
    }

    /// `MishapNoSpellCircle`: the whole inventory (but armour with Curse of
    /// Binding) is thrown about.
    pub fn drop_everything(&self) {
        let (Some(name), Some(pos)) = (self.caster_name(), self.caster_pos()) else {
            return;
        };
        for slot in 0..=SLOT_OFFHAND {
            let stack = self.stack_at(slot);
            if stack.is_empty()
                || (36..40).contains(&slot) && stack.nbt.to_snbt().contains("binding_curse")
            {
                continue;
            }
            Stack {
                slot,
                ..Stack::default()
            }
            .save(self.srv, name);
            // `Player.drop(stack, dropAround = true)`.
            let (f, a) = (random() * 0.5, random() * std::f64::consts::TAU);
            let delta = Vec3::new(-a.sin() * f, 0.2, a.cos() * f);
            self.srv.spawn_item(
                &self.dim,
                (pos.x, pos.y + 1.3, pos.z),
                delta.tuple(),
                &stack.id,
                stack.count,
                &stack.nbt.to_snbt(),
            );
        }
    }

    /// `damage(healthProportion)`.
    pub fn mishap_damage(&self, proportion: f32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(uuid) = self.caster_uuid() {
            let health = self.srv.entity_health(&uuid).unwrap_or(0.0);
            self.truly_hurt(&uuid, health * proportion);
        }
    }

    pub fn drown(&self) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        let (Some(uuid), Some(name)) = (self.caster_uuid(), self.caster_name()) else {
            return;
        };
        let air = self.srv.player_info(name).map_or(300, |p| p.air);
        if air < 200 {
            self.srv
                .entity_damage(&uuid, 2.0, vanilla::damage_types::DROWN, "");
        }
        self.srv.entity_set_nbt(&uuid, "{Air:0s}");
    }

    pub fn remove_xp(&self, amount: i32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(name) = self.caster_name() {
            self.srv.player_add_xp(name, -amount);
        }
    }

    pub fn blind(&self, ticks: i32) {
        if matches!(self.source, Source::Circle(_)) {
            return; // `CircleMishapEnv`: the caster isn't touched
        }
        if let Some(uuid) = self.caster_uuid() {
            self.srv
                .entity_add_effect(&uuid, vanilla::effects::BLINDNESS, ticks, 0, true);
        }
    }
}

pub fn other_hand(h: Hand) -> Hand {
    match h {
        Hand::Main => Hand::Off,
        Hand::Off => Hand::Main,
    }
}

pub fn to_ypos(p: BlockPos) -> yog_api::BlockPos {
    yog_api::BlockPos {
        x: p.0,
        y: p.1,
        z: p.2,
    }
}

pub fn from_ypos(p: yog_api::BlockPos) -> BlockPos {
    (p.x, p.y, p.z)
}

/// `Entity.getLookAngle`.
pub fn look_vec(srv: &dyn Server, uuid: &str) -> Vec3 {
    let Some((yaw, pitch)) = srv.entity_rotation(uuid) else {
        return Vec3::ZERO;
    };
    let (yaw, pitch) = ((yaw as f64).to_radians(), (pitch as f64).to_radians());
    Vec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

// ── per-player state ──────────────────────────────────────────────────────

pub fn get_pigment(srv: &dyn Server, uuid: &str) -> Pigment {
    let s = player_storage(srv, uuid);
    match s.get_str("pigment.item") {
        Some(item) => Pigment {
            item: item.to_owned(),
            owner: s.get_str("pigment.owner").unwrap_or(uuid).to_owned(),
        },
        None => Pigment::default(),
    }
}

pub fn set_pigment(srv: &dyn Server, uuid: &str, p: &Pigment) {
    let mut s = player_storage(srv, uuid);
    s.set("pigment.item", p.item.as_str());
    s.set("pigment.owner", p.owner.as_str());
}

/// A player's sentinel (`Sentinel`).
#[derive(Debug, Clone, PartialEq)]
pub struct Sentinel {
    /// Greater Sentinels extend the caster's ambit.
    pub great: bool,
    pub pos: Vec3,
    pub dim: String,
}

pub fn get_sentinel(srv: &dyn Server, uuid: &str) -> Option<Sentinel> {
    let s = player_storage(srv, uuid);
    Some(Sentinel {
        great: s.get_bool("sentinel.great").unwrap_or(false),
        pos: Vec3::new(
            s.get_float("sentinel.x")?,
            s.get_float("sentinel.y")?,
            s.get_float("sentinel.z")?,
        ),
        dim: s.get_str("sentinel.dim")?.to_owned(),
    })
}

pub fn set_sentinel(srv: &dyn Server, uuid: &str, sentinel: Option<&Sentinel>) {
    let mut s = player_storage(srv, uuid);
    match sentinel {
        Some(t) => {
            s.set("sentinel.great", t.great);
            s.set("sentinel.x", t.pos.x);
            s.set("sentinel.y", t.pos.y);
            s.set("sentinel.z", t.pos.z);
            s.set("sentinel.dim", t.dim.as_str());
        }
        None => {
            for k in [
                "sentinel.great",
                "sentinel.x",
                "sentinel.y",
                "sentinel.z",
                "sentinel.dim",
            ] {
                s.remove(k);
            }
        }
    }
}
