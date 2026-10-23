//! The caster's items: held, in their inventory, used up.

use super::player::other_hand;
use super::{CastEnv, Discovery, SLOT_OFFHAND, Source, Stack};
use crate::protocol::Hand;

impl CastEnv {
    // ── items ──────────────────────────────────────────────────────────────

    pub fn hand_slot(&self, hand: Hand) -> Option<u32> {
        let name = self.caster_name()?;
        Some(match hand {
            Hand::Main => yog_api::event::player::info(name)?.selected_slot,
            Hand::Off => SLOT_OFFHAND,
        })
    }

    pub fn stack_at(&self, slot: u32) -> Stack {
        match self.caster_name() {
            Some(n) => Stack::read(n, slot),
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
            stack.save(n);
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
        let selected = yog_api::event::player::info(name).map_or(0, |p| p.selected_slot);
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
            s.save(name);
        };
        if let Some(h) = hand
            && let Some(s) = self.held(h)
            && ok(&s)
        {
            put(s.slot);
            return true;
        }
        let selected = yog_api::event::player::info(name).map_or(0, |p| p.selected_slot);
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
}
