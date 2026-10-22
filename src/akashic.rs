//! The akashic library (Hex Casting's `BlockAkashicRecord`,
//! `BlockEntityAkashicBookshelf` and `AkashicFloodfiller`).
//!
//! A bookshelf's pattern and iota are kept in the chunk's mod storage
//! (the blocks themselves are plain); breaking or placing a bookshelf
//! clears its entry, and writing to a chunk forgets the shelves that
//! vanished from it some other way.

use std::collections::{HashSet, VecDeque};

use yog_api::storage::Store;

use crate::casting::env::{random, to_ypos};
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

/// The chunk's store `pos` is in, with a world open.
fn store(dim: &str, pos: BlockPos) -> Option<Store> {
    yog_api::storage::chunk(dim, pos.0 >> 4, pos.2 >> 4)
}

/// Where a chunk's shelves are kept in its store.
const SHELVES_KEY: &str = "akashic";

/// A bookshelf's mapping, kept with its chunk: its pattern and iota, in
/// the casting's own text forms.
#[derive(Clone, yog_api::Schema)]
struct Shelf {
    pos: BlockPos,
    pattern: String,
    iota: String,
}

/// A bookshelf's mapping: its pattern and iota.
pub fn shelf(dim: &str, pos: BlockPos) -> Option<(HexPattern, Iota)> {
    let shelves: Vec<Shelf> = store(dim, pos)?.get_or_default(SHELVES_KEY);
    let found = shelves.into_iter().find(|s| s.pos == pos)?;
    let pattern = HexPattern::from_serialized(&found.pattern)?;
    Some((pattern, Iota::deserialize(&found.iota)))
}

/// `BlockEntityAkashicBookshelf.setNewMapping` (`None` clears it).
pub fn set_shelf(dim: &str, pos: BlockPos, mapping: Option<(&HexPattern, &Iota)>) {
    if let Some(s) = store(dim, pos) {
        s.update(SHELVES_KEY, |shelves: &mut Vec<Shelf>| {
            shelves.retain(|s| s.pos != pos);
            if let Some((p, i)) = mapping {
                forget_lost_shelves(dim, shelves);
                shelves.push(Shelf {
                    pos,
                    pattern: p.serialized_form(),
                    iota: i.serialize(),
                });
            }
        });
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
fn forget_lost_shelves(dim: &str, shelves: &mut Vec<Shelf>) {
    shelves.retain(|s| {
        yog_api::event::block::info(dim, to_ypos(s.pos)).is_none_or(|b| b.block_id() == BOOKSHELF)
    });
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
