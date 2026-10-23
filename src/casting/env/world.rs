//! Entities and blocks a hex reaches, and its ambit.

use super::player::{get_sentinel, to_ypos};
use super::{AMBIT_RADIUS, CastEnv, SENTINEL_RADIUS, Source};
use crate::casting::mishap::{BlockPos, MResult, Mishap};
use crate::client::text::Text;
use crate::iota::EntityRef;
use crate::math::Vec3;
use yog_api::core::{EntityInfo, entity_flags};
use yog_api::vanilla;

impl CastEnv {
    // ── entities and blocks ────────────────────────────────────────────────

    /// The entity, if it's loaded in this dimension.
    pub fn entity(&self, e: &EntityRef) -> Option<EntityInfo> {
        yog_api::event::entity::info(&e.uuid).filter(|i| i.dimension == self.dim)
    }

    pub fn entity_pos(&self, e: &EntityRef) -> Option<Vec3> {
        self.entity(e).map(|i| Vec3::from_tuple(i.pos))
    }

    pub fn is_player(&self, uuid: &str) -> bool {
        yog_api::event::entity::info(uuid).is_some_and(|i| i.has(entity_flags::PLAYER))
    }

    pub fn block_id(&self, pos: BlockPos) -> String {
        yog_api::event::block::info(&self.dim, to_ypos(pos))
            .map(|b| b.block_id().to_owned())
            .unwrap_or_else(|| vanilla::items::AIR.into())
    }

    /// A block's name (`block.<ns>.<path>`).
    pub fn block_name(&self, pos: BlockPos) -> Text {
        let id = self.block_id(pos);
        Text::key(yog_api::yog_ids::lang_key(
            yog_api::yog_ids::kind::BLOCK,
            &id,
        ))
    }

    // ── ambit ──────────────────────────────────────────────────────────────

    pub fn mishap_spray_pos(&self) -> Vec3 {
        match &self.source {
            Source::Circle(c) => Vec3::center_of(c.current),
            _ => self.caster_pos().unwrap_or_default(),
        }
    }

    /// The caster's sentinel, if it lets them reach around it.
    pub(super) fn sentinel_in_range(&self, v: Vec3) -> bool {
        let Some(c) = &self.caster else { return false };
        match get_sentinel(&c.uuid) {
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
        yog_api::event::block::info(&self.dim, to_ypos(v.block())).is_some_and(|b| b.in_world)
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
        match self.caster_name().and_then(yog_api::event::player::info) {
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
}
