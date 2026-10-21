//! Media in items (Hex Casting's `ADMediaHolder`s and `MediaHelper`):
//! amethyst that's spent whole, and phials / casting items that hold an
//! amount in their NBT.

use yog_api::vanilla;

use crate::casting::env::{SLOT_OFFHAND, Stack};
use crate::content::{Blocks, Items};
use crate::snbt::Nbt;

/// `MediaConstants`.
pub const DUST: i64 = 10_000;
pub const SHARD: i64 = 5 * DUST;
pub const CRYSTAL: i64 = 10 * DUST;
pub const QUENCHED_SHARD: i64 = 3 * CRYSTAL;
pub const QUENCHED_BLOCK: i64 = 4 * QUENCHED_SHARD;

/// `ItemMediaHolder.TAG_MEDIA` / `TAG_MAX_MEDIA`.
pub const TAG_MEDIA: &str = "hexcasting:media";
pub const TAG_MAX_MEDIA: &str = "hexcasting:start_media";

pub const BATTERY: &str = Items::BATTERY;

/// Items worth a fixed amount each, spent whole: (id, worth, priority).
const STATIC: [(&str, i64, i32); 5] = [
    (Items::AMETHYST_DUST, DUST, 3000),
    (vanilla::items::AMETHYST_SHARD, SHARD, 2000),
    (Items::CHARGED_AMETHYST, CRYSTAL, 1000),
    (Items::QUENCHED_ALLAY_SHARD, QUENCHED_SHARD, 900),
    (Blocks::QUENCHED_ALLAY, QUENCHED_BLOCK, 800),
];
const BATTERY_PRIORITY: i32 = 4000;

/// Cyphers, trinkets and artifacts (`ItemPackagedHex`).
pub fn is_packaged(id: &str) -> bool {
    matches!(
        id,
        Items::CYPHER | Items::ANCIENT_CYPHER | Items::TRINKET | Items::ARTIFACT
    )
}

/// `canDrawMediaFromInventory`: only the artifact.
pub fn draws_from_inventory(id: &str) -> bool {
    id == Items::ARTIFACT
}

/// `ItemPackagedHex.cooldown`: ticks before a cypher, trinket or artifact
/// can be used again.
pub fn cooldown(id: &str) -> u32 {
    use crate::casting::env::{ARTIFACT_COOLDOWN, CYPHER_COOLDOWN, TRINKET_COOLDOWN};
    match id {
        Items::TRINKET => TRINKET_COOLDOWN,
        Items::ARTIFACT => ARTIFACT_COOLDOWN,
        _ => CYPHER_COOLDOWN,
    }
}

/// `breakAfterDepletion`: cyphers.
pub fn breaks_after_depletion(id: &str) -> bool {
    matches!(id, Items::CYPHER | Items::ANCIENT_CYPHER)
}

/// Items holding media in NBT.
pub fn holds_media(id: &str) -> bool {
    id == BATTERY || is_packaged(id)
}

/// `isMediaItem`: something media can be drawn out of (phials and amethyst).
pub fn is_media_item(stack: &Stack) -> bool {
    !stack.is_empty() && (stack.id == BATTERY || static_worth(&stack.id).is_some())
}

/// `canRecharge`: phials, trinkets and artifacts (not cyphers).
pub fn can_recharge(id: &str) -> bool {
    id == BATTERY || (is_packaged(id) && !breaks_after_depletion(id))
}

/// `extractMedia(stack, drainForBatteries = true)`: all the media in raw
/// amethyst (`canConstructBattery`); nothing from anything else.
pub fn extract_for_battery(stack: &mut Stack, simulate: bool) -> i64 {
    if static_worth(&stack.id).is_none() {
        return 0;
    }
    withdraw(stack, -1, simulate)
}

/// What one item of `id` is worth, for items spent whole.
pub fn static_worth(id: &str) -> Option<i64> {
    STATIC.iter().find(|(i, ..)| *i == id).map(|(_, w, _)| *w)
}

pub fn media(stack: &Stack) -> i64 {
    if holds_media(&stack.id) {
        stack.nbt.get_i64(TAG_MEDIA).unwrap_or(0)
    } else {
        static_worth(&stack.id).map_or(0, |w| w * stack.count as i64)
    }
}

pub fn max_media(stack: &Stack) -> i64 {
    if holds_media(&stack.id) {
        stack.nbt.get_i64(TAG_MAX_MEDIA).unwrap_or(0)
    } else {
        media(stack)
    }
}

/// `ItemMediaHolder.setMedia`: clamped to `[0, max]`.
pub fn set_media(stack: &mut Stack, media: i64) {
    let max = max_media(stack);
    stack
        .nbt
        .set(TAG_MEDIA, Nbt::long(media.clamp(0, max.max(0))));
}

/// `ItemMediaHolder.withdrawMedia` (a negative `cost` takes everything).
pub fn withdraw_battery(stack: &mut Stack, cost: i64, simulate: bool) -> i64 {
    let here = media(stack);
    let cost = if cost < 0 { here } else { cost };
    if !simulate {
        set_media(stack, here - cost);
    }
    cost.min(here)
}

/// `ItemMediaHolder.insertMedia` (Recharge Item); returns what fit.
pub fn insert_media(stack: &mut Stack, amount: i64, simulate: bool) -> i64 {
    let here = media(stack);
    let room = (max_media(stack) - here).max(0);
    let amount = if amount < 0 { room } else { amount.min(room) };
    if !simulate {
        set_media(stack, here + amount);
    }
    amount
}

fn priority(stack: &Stack) -> Option<i32> {
    if stack.is_empty() {
        return None;
    }
    if stack.id == BATTERY {
        return Some(BATTERY_PRIORITY);
    }
    STATIC
        .iter()
        .find(|(i, ..)| *i == stack.id)
        .map(|(.., p)| *p)
}

/// Takes up to `cost` media from a media-providing stack (whole items for
/// amethyst); returns how much it took.
pub fn withdraw(stack: &mut Stack, cost: i64, simulate: bool) -> i64 {
    if stack.id == BATTERY {
        return withdraw_battery(stack, cost, simulate);
    }
    let Some(worth) = static_worth(&stack.id) else {
        return 0;
    };
    if stack.is_empty() {
        return 0;
    }
    let cost = if cost < 0 {
        worth * stack.count as i64
    } else {
        cost
    };
    let needed = ((cost + worth - 1) / worth).min(stack.count as i64);
    if !simulate {
        stack.count -= needed as u32;
    }
    needed * worth
}

/// `scanPlayerForMediaStuff`: everything that can pay, most important first.
pub fn scan_player(player: &str) -> Vec<Stack> {
    let mut found: Vec<(i32, i64, Stack)> = (0..36u32)
        .chain(36..40)
        .chain([SLOT_OFFHAND])
        .map(|slot| Stack::read(player, slot))
        .filter_map(|s| Some((priority(&s)?, media(&s), s)))
        .collect();
    // compareMediaItem, then reversed.
    found.sort_by_key(|a| (a.0, a.1));
    found.reverse();
    found.into_iter().map(|(.., s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: &str, count: u32) -> Stack {
        Stack {
            slot: 0,
            id: id.into(),
            count,
            nbt: Nbt::default(),
        }
    }

    #[test]
    fn whole_items_are_spent() {
        let mut s = stack(Items::AMETHYST_DUST, 3);
        assert_eq!(withdraw(&mut s, DUST + 1, true), 2 * DUST);
        assert_eq!(s.count, 3);
        assert_eq!(withdraw(&mut s, 10 * DUST, false), 3 * DUST);
        assert_eq!(s.count, 0);
    }

    #[test]
    fn phials_hold_an_amount() {
        let mut s = stack(BATTERY, 1);
        s.nbt.set(TAG_MAX_MEDIA, Nbt::long(100));
        s.nbt.set(TAG_MEDIA, Nbt::long(60));
        assert_eq!(withdraw(&mut s, 80, false), 60);
        assert_eq!(media(&s), 0);
        assert_eq!(insert_media(&mut s, 500, false), 100);
        assert_eq!(media(&s), 100);
    }
}
