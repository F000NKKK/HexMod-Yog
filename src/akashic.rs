//! The akashic library (Hex Casting's `BlockAkashicRecord`,
//! `BlockEntityAkashicBookshelf` and `AkashicFloodfiller`).
//!
//! A bookshelf's pattern and iota are kept in the chunk's mod storage
//! (the blocks themselves are plain); breaking or placing a bookshelf
//! clears its entry, and writing to a chunk forgets the shelves that
//! vanished from it some other way.

use std::collections::{HashSet, VecDeque};

use yog_api::Storage;

use crate::casting::env::{MOD_ID, random, to_ypos};
use crate::casting::mishap::BlockPos;
use crate::content::Blocks;
use crate::iota::Iota;
use crate::math::HexPattern;

pub const RECORD: &str = Blocks::AKASHIC_RECORD;
pub const BOOKSHELF: &str = Blocks::AKASHIC_BOOKSHELF;
pub const LIGATURE: &str = Blocks::AKASHIC_CONNECTOR;

/// `AkashicFloodfiller.floodFillFor`'s default range.
const MAX_RANGE: i64 = 128;

fn block_id(dim: &str, pos: BlockPos) -> String {
    yog_api::event::block::info(dim, to_ypos(pos))
        .map(|b| b.block_id().to_owned())
        .unwrap_or_default()
}

/// Whether the library floods through this block.
pub fn is_floodfiller(id: &str) -> bool {
    matches!(id, RECORD | BOOKSHELF | LIGATURE)
}

fn store(dim: &str, pos: BlockPos) -> Storage {
    Storage::open_chunk(
        &yog_api::event::game::world_dir().unwrap_or_default(),
        MOD_ID,
        dim,
        pos.0 >> 4,
        pos.2 >> 4,
    )
}

const KEY_PREFIX: &str = "akashic.";
const PATTERN: &str = "pattern";
const IOTA: &str = "iota";

fn key(pos: BlockPos, what: &str) -> String {
    format!("{KEY_PREFIX}{}.{}.{}.{what}", pos.0, pos.1, pos.2)
}

/// The shelf a `pattern` key belongs to.
fn pos_of_key(key: &str) -> Option<BlockPos> {
    let rest = key
        .strip_prefix(KEY_PREFIX)?
        .strip_suffix(PATTERN)?
        .strip_suffix('.')?;
    let mut it = rest.split('.').map(str::parse);
    let pos = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    it.next().is_none().then_some(pos)
}

/// A bookshelf's mapping: its pattern and iota.
pub fn shelf(dim: &str, pos: BlockPos) -> Option<(HexPattern, Iota)> {
    let s = store(dim, pos);
    let pattern = HexPattern::from_serialized(s.get_str(&key(pos, PATTERN))?)?;
    let iota = Iota::deserialize(s.get_str(&key(pos, IOTA)).unwrap_or(""));
    Some((pattern, iota))
}

/// `BlockEntityAkashicBookshelf.setNewMapping` (`None` clears it).
pub fn set_shelf(dim: &str, pos: BlockPos, mapping: Option<(&HexPattern, &Iota)>) {
    let mut s = store(dim, pos);
    match mapping {
        Some((p, i)) => {
            forget_lost_shelves(dim, &mut s);
            s.set(key(pos, PATTERN), p.serialized_form());
            s.set(key(pos, IOTA), i.serialize());
        }
        None => {
            s.remove(&key(pos, PATTERN));
            s.remove(&key(pos, IOTA));
        }
    }
    // `HAS_BOOKS`: the shelf shows books while it holds something.
    if block_id(dim, pos) == BOOKSHELF {
        let has = if mapping.is_some() { "true" } else { "false" };
        yog_api::event::block::set_properties(dim, to_ypos(pos), &format!("has_books={has}"));
    }
    crate::render::shelf_changed(dim, pos, mapping.map(|(p, _)| p));
}

/// Drops what shelves no longer there held: blown up, pushed away or
/// replaced without a player (only player breaks and Break Block clear
/// their entry as it happens).
fn forget_lost_shelves(dim: &str, s: &mut Storage) {
    let lost: Vec<BlockPos> = s
        .iter()
        .filter_map(|(k, _)| pos_of_key(k))
        .filter(|&pos| {
            yog_api::event::block::info(dim, to_ypos(pos))
                .is_some_and(|b| b.block_id() != BOOKSHELF)
        })
        .collect();
    for pos in lost {
        s.remove(&key(pos, PATTERN));
        s.remove(&key(pos, IOTA));
    }
}

/// `AkashicFloodfiller.floodFillFor`: breadth-first through the library
/// from `start`; a target found may be skipped with `skip_chance`, in which
/// case the first skipped one is the answer if nothing else is.
fn flood_fill(
    dim: &str,
    start: BlockPos,
    skip_chance: f64,
    mut is_target: impl FnMut(BlockPos, &str) -> bool,
) -> Option<BlockPos> {
    const DIRS: [(i32, i32, i32); 6] = [
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
        (-1, 0, 0),
        (1, 0, 0),
    ];
    let mut seen = HashSet::new();
    let mut todo = VecDeque::from([start]);
    let mut skipped = None;
    while let Some(here) = todo.pop_front() {
        for d in DIRS {
            let n = (here.0 + d.0, here.1 + d.1, here.2 + d.2);
            let (dx, dy, dz) = (
                (n.0 - start.0) as i64,
                (n.1 - start.1) as i64,
                (n.2 - start.2) as i64,
            );
            if dx * dx + dy * dy + dz * dz > MAX_RANGE * MAX_RANGE || !seen.insert(n) {
                continue;
            }
            let id = block_id(dim, n);
            if is_target(n, &id) {
                if random() > skip_chance {
                    return Some(n);
                }
                skipped.get_or_insert(n);
            }
            if is_floodfiller(&id) {
                todo.push_back(n);
            }
        }
    }
    skipped
}

fn holds(dim: &str, pos: BlockPos, id: &str, key: &HexPattern) -> bool {
    id == BOOKSHELF && shelf(dim, pos).is_some_and(|(p, _)| p.signature() == key.signature())
}

/// `BlockAkashicRecord.lookupPattern`.
pub fn lookup(dim: &str, record: BlockPos, key: &HexPattern) -> Option<Iota> {
    let at = flood_fill(dim, record, 0.0, |pos, id| holds(dim, pos, id, key))?;
    shelf(dim, at).map(|(_, i)| i)
}

/// `BlockAkashicRecord.addNewDatum`: where it was stored, or `None` if the
/// key is taken or there's no empty shelf. Never overwrites.
pub fn add_new_datum(
    dim: &str,
    record: BlockPos,
    key: &HexPattern,
    datum: &Iota,
) -> Option<BlockPos> {
    if flood_fill(dim, record, 0.0, |pos, id| holds(dim, pos, id, key)).is_some() {
        return None;
    }
    let open = flood_fill(dim, record, 0.9, |pos, id| {
        id == BOOKSHELF && shelf(dim, pos).is_none()
    })?;
    set_shelf(dim, open, Some((key, datum)));
    Some(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_their_shelf() {
        let pos = (-3, 64, 17);
        assert_eq!(pos_of_key(&key(pos, PATTERN)), Some(pos));
        assert_eq!(pos_of_key(&key(pos, IOTA)), None);
        assert_eq!(pos_of_key("akashic.1.2.pattern"), None);
    }
}
