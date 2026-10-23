//! The media that pays for a hex.

use super::{
    BIG_CAST_MEDIA, CastEnv, ENLIGHTENING_HEALTH_LEFT, ENLIGHTENING_HEALTH_USED, MAX_HEALTH,
    MEDIA_TO_HEALTH, Source, WASTEFUL_MEDIA,
};
use crate::content::Advancements;
use crate::item::media;

/// A player's max health when the game doesn't say.
const DEFAULT_MAX_HEALTH: f64 = 20.0;

impl CastEnv {
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
    pub(super) fn overcast(&self, uuid: &str, used: f64, left: f64) {
        self.grant_advancement(Advancements::OPENED_EYES);
        let max = yog_api::event::entity::attribute_value(uuid, MAX_HEALTH)
            .unwrap_or(DEFAULT_MAX_HEALTH)
            .max(1.0);
        if used / max >= ENLIGHTENING_HEALTH_USED && left > 0.0 && left <= ENLIGHTENING_HEALTH_LEFT
        {
            self.grant_advancement(Advancements::ENLIGHTENMENT);
        }
    }

    pub(super) fn extract_media_from_sources(&mut self, cost: i64, simulate: bool) -> i64 {
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
    pub(super) fn extract_from_inventory(
        &mut self,
        mut left: i64,
        overcast: bool,
        simulate: bool,
    ) -> i64 {
        let Some(name) = self.caster_name().map(str::to_owned) else {
            return left;
        };
        for mut source in media::scan_player(&name) {
            let found = media::withdraw(&mut source, left, simulate);
            if !simulate {
                source.save(&name);
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
            let health = yog_api::event::entity::health(&uuid)
                .map(|h| h as f32)
                .unwrap_or(0.0) as f64;
            let to_remove = (left as f64 / MEDIA_TO_HEALTH).max(0.5);
            if simulate {
                left -= (health.min(to_remove) * MEDIA_TO_HEALTH).ceil() as i64;
            } else {
                let able = health * MEDIA_TO_HEALTH;
                self.truly_hurt(&uuid, to_remove as f32);
                let after = yog_api::event::entity::health(&uuid)
                    .map(|h| h as f32)
                    .unwrap_or(0.0) as f64;
                left -= (able - after * MEDIA_TO_HEALTH).ceil() as i64;
                self.overcast(&uuid, health - after, after);
            }
        }
        left
    }
}
