//! The akashic library (Hex Casting's `BlockAkashicRecord`,
//! `BlockEntityAkashicBookshelf` and `AkashicFloodfiller`).
//!
//! A bookshelf's pattern and iota are kept in the chunk's mod storage
//! (the blocks themselves are plain); breaking or placing a bookshelf
//! clears its entry.

use std::collections::{HashSet, VecDeque};

use yog_api::{Server, Storage};

use crate::casting::env::{random, to_ypos, MOD_ID};
use crate::casting::mishap::BlockPos;
use crate::iota::Iota;
use crate::math::HexPattern;

pub const RECORD: &str = "hexcasting:akashic_record";
pub const BOOKSHELF: &str = "hexcasting:akashic_bookshelf";
pub const LIGATURE: &str = "hexcasting:akashic_connector";

/// `AkashicFloodfiller.floodFillFor`'s default range.
const MAX_RANGE: i64 = 128;

fn block_id(srv: &dyn Server, dim: &str, pos: BlockPos) -> String {
    srv.block_info(dim, to_ypos(pos)).map(|b| b.block_id().to_owned()).unwrap_or_default()
}

/// Whether the library floods through this block.
pub fn is_floodfiller(id: &str) -> bool {
    matches!(id, RECORD | BOOKSHELF | LIGATURE)
}

fn store(srv: &dyn Server, dim: &str, pos: BlockPos) -> Storage {
    Storage::open_chunk(&srv.game_dir(), MOD_ID, dim, pos.0 >> 4, pos.2 >> 4)
}

fn key(pos: BlockPos, what: &str) -> String {
    format!("akashic.{}.{}.{}.{what}", pos.0, pos.1, pos.2)
}

/// A bookshelf's mapping: its pattern and iota.
pub fn shelf(srv: &dyn Server, dim: &str, pos: BlockPos) -> Option<(HexPattern, Iota)> {
    let s = store(srv, dim, pos);
    let pattern = HexPattern::from_serialized(s.get_str(&key(pos, "pattern"))?)?;
    let iota = Iota::deserialize(s.get_str(&key(pos, "iota")).unwrap_or(""));
    Some((pattern, iota))
}

/// `BlockEntityAkashicBookshelf.setNewMapping` (`None` clears it).
pub fn set_shelf(srv: &dyn Server, dim: &str, pos: BlockPos, mapping: Option<(&HexPattern, &Iota)>) {
    let mut s = store(srv, dim, pos);
    match mapping {
        Some((p, i)) => {
            s.set(key(pos, "pattern"), p.serialized_form());
            s.set(key(pos, "iota"), i.serialize());
        }
        None => {
            s.remove(&key(pos, "pattern"));
            s.remove(&key(pos, "iota"));
        }
    }
}

/// `AkashicFloodfiller.floodFillFor`: breadth-first through the library
/// from `start`; a target found may be skipped with `skip_chance`, in which
/// case the first skipped one is the answer if nothing else is.
fn flood_fill(
    srv: &dyn Server,
    dim: &str,
    start: BlockPos,
    skip_chance: f64,
    mut is_target: impl FnMut(BlockPos, &str) -> bool,
) -> Option<BlockPos> {
    const DIRS: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
    let mut seen = HashSet::new();
    let mut todo = VecDeque::from([start]);
    let mut skipped = None;
    while let Some(here) = todo.pop_front() {
        for d in DIRS {
            let n = (here.0 + d.0, here.1 + d.1, here.2 + d.2);
            let (dx, dy, dz) = ((n.0 - start.0) as i64, (n.1 - start.1) as i64, (n.2 - start.2) as i64);
            if dx * dx + dy * dy + dz * dz > MAX_RANGE * MAX_RANGE || !seen.insert(n) {
                continue;
            }
            let id = block_id(srv, dim, n);
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

fn holds(srv: &dyn Server, dim: &str, pos: BlockPos, id: &str, key: &HexPattern) -> bool {
    id == BOOKSHELF && shelf(srv, dim, pos).is_some_and(|(p, _)| p.signature() == key.signature())
}

/// `BlockAkashicRecord.lookupPattern`.
pub fn lookup(srv: &dyn Server, dim: &str, record: BlockPos, key: &HexPattern) -> Option<Iota> {
    let at = flood_fill(srv, dim, record, 0.0, |pos, id| holds(srv, dim, pos, id, key))?;
    shelf(srv, dim, at).map(|(_, i)| i)
}

/// `BlockAkashicRecord.addNewDatum`: where it was stored, or `None` if the
/// key is taken or there's no empty shelf. Never overwrites.
pub fn add_new_datum(srv: &dyn Server, dim: &str, record: BlockPos, key: &HexPattern, datum: &Iota) -> Option<BlockPos> {
    if flood_fill(srv, dim, record, 0.0, |pos, id| holds(srv, dim, pos, id, key)).is_some() {
        return None;
    }
    let open = flood_fill(srv, dim, record, 0.9, |pos, id| id == BOOKSHELF && shelf(srv, dim, pos).is_none())?;
    set_shelf(srv, dim, open, Some((key, datum)));
    Some(open)
}
