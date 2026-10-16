//! Loot (Hex Casting's `HexLootHandler`): what chests and amethyst clusters
//! give besides the game's own. Which tables get scrolls, lore and cyphers
//! is data (`content/loot/*.ydef`); here are the loot functions those name —
//! a scroll of one of this world's great spells, an ancient cypher's old
//! hex — and the amethyst cluster's dust and charged crystals, which depend
//! on the tool.

use yog_api::Registry;
use yog_api::loot::{Loot, LootStack, random};

use crate::casting::actions::{all, per_world_pattern};
use crate::casting::env::{MOD_ID, Stack};
use crate::content::{Items, LootHex};
use crate::iota::Iota;
use crate::item::media::SHARD;
use crate::item::packaged;
use crate::item::pigment::Pigment;
use crate::math::{HexDir, HexPattern};
use crate::snbt::Nbt;

/// The loot table amethyst clusters drop from.
const AMETHYST_CLUSTER: &str = "minecraft:blocks/amethyst_cluster";
/// Tools that harvest a grown cluster whole (the game's tag).
const CLUSTER_HARVESTERS: &str = yog_api::vanilla::item_tags::CLUSTER_MAX_HARVESTABLES;
const SILK_TOUCH: &str = yog_api::vanilla::enchantments::SILK_TOUCH;
const FORTUNE: &str = yog_api::vanilla::enchantments::FORTUNE;
/// `FrozenPigment.ANCIENT`'s owner: nobody.
const NO_OWNER: &str = "00000000-0000-0000-0000-000000000000";
/// The media an ancient cypher comes with: 32 shards.
const CYPHER_MEDIA: i64 = 32 * SHARD;
/// The textures an ancient cypher can wear (`VariantItem`).
const CYPHER_VARIANTS: u32 = 8;
/// A charged crystal's chance from a grown cluster, by fortune level
/// (`table_bonus`); and from one that isn't harvested whole.
const CRYSTAL_CHANCE: [f64; 5] = [0.25, 0.35, 0.5, 0.75, 1.0];
const CRYSTAL_CHANCE_UNHARVESTED: f64 = 0.125;

pub fn register(registry: &mut Registry) {
    registry.loot_function("hexcasting:per_world_pattern", |srv, stack, _| per_world_scroll(srv, stack));
    registry.loot_function("hexcasting:ancient_hex", |_, stack, _| ancient_hex(stack));
    registry.on_loot(|srv, loot| {
        if loot.table == AMETHYST_CLUSTER {
            amethyst_cluster(srv, loot);
        }
    });
    // An ancient cypher is named after its hex.
    registry.on_item_text(Items::ANCIENT_CYPHER, |_, snbt| {
        let key = Nbt::parse(snbt).get_str(TAG_HEX_NAME)?.to_owned();
        Some((Some(crate::res::Items::ancient_cypher_preset(yog_api::tr(&key))), Vec::new()))
    });
}

/// `ItemAncientCypher.TAG_HEX_NAME`.
const TAG_HEX_NAME: &str = "hex_name";

/// A whole number in `0..n`.
fn below(n: u32) -> u32 {
    ((random() * n as f64) as u32).min(n.saturating_sub(1))
}

/// `AddPerWorldPatternToScrollFunc`: one of this world's great spells, its
/// pattern as this world draws it.
fn per_world_scroll(srv: &dyn yog_api::Server, stack: &mut LootStack) {
    let great: Vec<_> = all().iter().filter(|a| a.great()).collect();
    if great.is_empty() {
        return;
    }
    let action = great[below(great.len() as u32) as usize];
    let Some(pattern) = per_world_pattern(srv, action.id) else { return };
    let mut nbt = Nbt::parse(&stack.nbt);
    nbt.set("op_id", Nbt::str(format!("{MOD_ID}:{}", action.id)));
    nbt.set("pattern", Nbt::str(pattern.serialized_form()));
    stack.nbt = nbt.to_snbt();
}

/// `AddHexToAncientCypherFunc`: one of the old hexes, the ancient pigment,
/// 32 shards of media, one of its looks.
fn ancient_hex(stack: &mut LootStack) {
    let hex = LootHex::ALL[below(LootHex::ALL.len() as u32) as usize];
    let program: Vec<Iota> = hex.patterns().iter().filter_map(|p| parse_pattern(p)).map(Iota::Pattern).collect();
    let mut s = Stack { slot: 0, id: stack.id.clone(), count: stack.count, nbt: Nbt::parse(&stack.nbt) };
    let ancient = Pigment { item: Items::ANCIENT_COLORIZER.to_owned(), owner: NO_OWNER.to_owned() };
    packaged::write_hex(&mut s, &program, Some(&ancient), CYPHER_MEDIA);
    s.nbt.set(TAG_HEX_NAME, Nbt::str(hex.name()));
    s.nbt.set("variant", Nbt::int(below(CYPHER_VARIANTS) as i32));
    stack.nbt = s.nbt.to_snbt();
}

/// `NORTH_EAST qaq`: a start direction and the angles.
fn parse_pattern(text: &str) -> Option<HexPattern> {
    let (dir, angles) = text.split_once(' ')?;
    let start = HexDir::ALL.into_iter().find(|d| d.name() == dir)?;
    HexPattern::from_angles(angles, start)
}

/// `inject/amethyst_cluster`: amethyst dust and now and then a charged
/// crystal — more of both for a cluster harvested whole, with fortune.
/// Nothing besides with silk touch: the cluster drops itself.
fn amethyst_cluster(srv: &dyn yog_api::Server, loot: &mut Loot) {
    if loot.enchantment(SILK_TOUCH) > 0 {
        return;
    }
    let fortune = loot.enchantment(FORTUNE);
    let harvested = loot.tool.as_ref().is_some_and(|t| srv.has_item_tag(&t.id, CLUSTER_HARVESTERS));
    let (dust, crystal_chance) = if harvested {
        // 1–4, then the game's `ore_drops` fortune bonus.
        let base = 1 + below(4);
        let bonus = if fortune > 0 { below(fortune + 2).saturating_sub(1) + 1 } else { 1 };
        (base * bonus, CRYSTAL_CHANCE[(fortune as usize).min(CRYSTAL_CHANCE.len() - 1)])
    } else {
        (below(3), CRYSTAL_CHANCE_UNHARVESTED)
    };
    if dust > 0 {
        loot.items.push(LootStack::new(Items::AMETHYST_DUST, dust));
    }
    if random() < crystal_chance {
        loot.items.push(LootStack::new(Items::CHARGED_AMETHYST, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_hexes_are_patterns() {
        for hex in LootHex::ALL {
            for p in hex.patterns() {
                assert!(parse_pattern(p).is_some(), "{}: {p}", hex.name());
            }
        }
    }

    #[test]
    fn an_ancient_cypher_gets_a_hex_media_and_a_look() {
        let mut stack = LootStack::new(Items::ANCIENT_CYPHER, 1);
        ancient_hex(&mut stack);
        let nbt = Nbt::parse(&stack.nbt);
        assert!(nbt.get_str(TAG_HEX_NAME).is_some_and(|k| k.starts_with("hexcasting.loot_hex.")));
        let s = Stack { slot: 0, id: stack.id.clone(), count: 1, nbt };
        assert!(packaged::hex(&s).is_some_and(|h| !h.is_empty()));
        assert!(s.nbt.get_i64("variant").is_some_and(|v| (0..CYPHER_VARIANTS as i64).contains(&v)));
    }
}
