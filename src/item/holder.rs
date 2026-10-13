//! Items that store an iota (Hex Casting's `IotaHolderItem`s): foci,
//! spellbooks, thought-knots, scrolls, slates, the abacus — and the
//! entities that delegate to the item they hold (dropped items, frames).
//!
//! Iotas are kept in the item's NBT as [`crate::wire`] strings.

use crate::casting::env::Stack;
use crate::iota::Iota;
use crate::math::HexPattern;
use crate::snbt::Nbt;

pub const FOCUS: &str = "hexcasting:focus";
pub const SPELLBOOK: &str = "hexcasting:spellbook";
pub const THOUGHT_KNOT: &str = "hexcasting:thought_knot";
pub const ABACUS: &str = "hexcasting:abacus";
pub const SLATE: &str = "hexcasting:slate";

/// `ItemSpellbook.MAX_PAGES`.
pub const MAX_PAGES: i64 = 64;

pub fn is_scroll(id: &str) -> bool {
    matches!(id, "hexcasting:scroll" | "hexcasting:scroll_small" | "hexcasting:scroll_medium")
}

/// `findDataHolder(stack) != null`.
pub fn is_holder(stack: &Stack) -> bool {
    !stack.is_empty()
        && (matches!(stack.id.as_str(), FOCUS | SPELLBOOK | THOUGHT_KNOT | ABACUS | SLATE)
            || is_scroll(&stack.id)
            || stack.id == "minecraft:pumpkin_pie")
}

fn get_iota(nbt: &Nbt, key: &str) -> Option<Iota> {
    nbt.get_str(key).map(Iota::deserialize)
}

fn pattern_of(nbt: &Nbt, key: &str) -> Option<Iota> {
    let s = nbt.get_str(key)?;
    HexPattern::from_serialized(s).map(Iota::Pattern)
}

/// `readIota`: what the item holds, if anything.
pub fn read(stack: &Stack) -> Option<Iota> {
    let nbt = &stack.nbt;
    match stack.id.as_str() {
        FOCUS | THOUGHT_KNOT => get_iota(nbt, "data"),
        SPELLBOOK => {
            let idx = page(nbt, 1);
            get_iota(nbt.get("pages")?, &idx.to_string())
        }
        ABACUS => Some(Iota::Double(nbt.get_f64("value").unwrap_or(0.0))),
        SLATE => pattern_of(nbt.get("BlockEntityTag")?, "pattern"),
        // Hex Casting's pie: as many π as there are pies.
        "minecraft:pumpkin_pie" => Some(Iota::Double(std::f64::consts::PI * stack.count as f64)),
        id if is_scroll(id) => pattern_of(nbt, "pattern"),
        _ => None,
    }
}

/// `writeable`.
pub fn writeable(stack: &Stack) -> bool {
    match stack.id.as_str() {
        FOCUS => !stack.nbt.get_bool("sealed"),
        SPELLBOOK => !page_sealed(&stack.nbt),
        THOUGHT_KNOT => !stack.nbt.contains("data"),
        ABACUS | "minecraft:pumpkin_pie" => false,
        SLATE => true,
        id => is_scroll(id),
    }
}

/// `canWrite` (`None` erases).
pub fn can_write(stack: &Stack, iota: Option<&Iota>) -> bool {
    match stack.id.as_str() {
        FOCUS => iota.is_none() || !stack.nbt.get_bool("sealed"),
        SPELLBOOK => iota.is_none() || !page_sealed(&stack.nbt),
        THOUGHT_KNOT => iota.is_some() && writeable(stack),
        SLATE => matches!(iota, None | Some(Iota::Pattern(_))),
        id if is_scroll(id) => matches!(iota, None | Some(Iota::Pattern(_))),
        _ => false,
    }
}

/// `writeIota(iota, simulate)`: whether it can be written, writing it
/// unless simulating.
pub fn write(stack: &mut Stack, iota: Option<&Iota>, simulate: bool) -> bool {
    if !is_holder(stack) || !can_write(stack, iota) {
        return false;
    }
    if !simulate {
        write_datum(stack, iota);
    }
    true
}

fn write_datum(stack: &mut Stack, iota: Option<&Iota>) {
    let nbt = &mut stack.nbt;
    match stack.id.as_str() {
        FOCUS => match iota {
            None => {
                nbt.remove("data");
                nbt.remove("sealed");
            }
            Some(i) if !nbt.get_bool("sealed") => nbt.set("data", Nbt::str(i.serialize())),
            Some(_) => {}
        },
        THOUGHT_KNOT => {
            if let Some(i) = iota {
                nbt.set("data", Nbt::str(i.serialize()));
            }
        }
        SPELLBOOK => {
            if iota.is_some() && page_sealed(nbt) {
                return;
            }
            let key = page(nbt, 1).to_string();
            if nbt.contains("pages") {
                match iota {
                    None => {
                        nbt.compound_mut("pages").remove(&key);
                        if let Some(s) = nbt.get_mut("sealed_pages") {
                            s.remove(&key);
                        }
                    }
                    Some(i) => nbt.compound_mut("pages").set(&key, Nbt::str(i.serialize())),
                }
                if nbt.get("pages").is_some_and(Nbt::is_empty) {
                    nbt.remove("pages");
                }
            } else if let Some(i) = iota {
                nbt.compound_mut("pages").set(&key, Nbt::str(i.serialize()));
            }
        }
        SLATE => {
            match iota {
                Some(Iota::Pattern(p)) => {
                    nbt.compound_mut("BlockEntityTag").set("pattern", Nbt::str(p.serialized_form()))
                }
                None => {
                    let bet = nbt.compound_mut("BlockEntityTag");
                    bet.remove("pattern");
                    if bet.is_empty() {
                        nbt.remove("BlockEntityTag");
                    }
                }
                Some(_) => {}
            }
        }
        id if is_scroll(id) => match iota {
            Some(Iota::Pattern(p)) => nbt.set("pattern", Nbt::str(p.serialized_form())),
            None => {
                nbt.remove("pattern");
            }
            Some(_) => {}
        },
        _ => {}
    }
}

// ── the spellbook's pages ─────────────────────────────────────────────────

fn pages_empty(nbt: &Nbt) -> bool {
    nbt.get("pages").is_none_or(Nbt::is_empty)
}

/// `ItemSpellbook.getPage`.
pub fn page(nbt: &Nbt, if_empty: i64) -> i64 {
    if pages_empty(nbt) {
        if_empty
    } else {
        match nbt.get_i64("page_idx") {
            Some(0) | None => 1,
            Some(i) => i,
        }
    }
}

/// `ItemSpellbook.isSealed`: whether the current page is sealed.
pub fn page_sealed(nbt: &Nbt) -> bool {
    let key = page(nbt, 1).to_string();
    nbt.get("sealed_pages").is_some_and(|s| s.get_bool(&key))
}

/// `ItemSpellbook.highestPage`.
pub fn highest_page(nbt: &Nbt) -> i64 {
    match nbt.get("pages") {
        Some(Nbt::Compound(e)) => e.iter().filter_map(|(k, _)| k.parse().ok()).max().unwrap_or(0),
        _ => 0,
    }
}

/// `ItemSpellbook.rotatePageIdx` (the page's name, if any, becomes the
/// item's name).
pub fn rotate_page(nbt: &mut Nbt, increase: bool) -> i64 {
    let mut idx = page(nbt, 0);
    if idx != 0 {
        idx += if increase { 1 } else { -1 };
        idx = idx.max(1);
    }
    idx = idx.clamp(0, MAX_PAGES);
    nbt.set("page_idx", Nbt::int(idx as i32));
    let key = idx.max(1).to_string();
    let name = nbt.get("page_names").and_then(|n| n.get_str(&key)).map(str::to_owned);
    match name {
        Some(json) => nbt.compound_mut("display").set("Name", Nbt::str(json)),
        None => {
            if let Some(d) = nbt.get_mut("display") {
                d.remove("Name");
            }
            if nbt.get("display").is_some_and(Nbt::is_empty) {
                nbt.remove("display");
            }
        }
    }
    idx
}

/// `ItemSpellbook.setSealed` for the current page.
pub fn set_page_sealed(nbt: &mut Nbt, sealed: bool) {
    let key = page(nbt, 1).to_string();
    let names = nbt.compound_mut("sealed_pages");
    if sealed {
        names.set(&key, Nbt::byte(true));
    } else {
        names.remove(&key);
    }
    if nbt.get("sealed_pages").is_some_and(Nbt::is_empty) {
        nbt.remove("sealed_pages");
    }
}

/// The abacus' value (scrolled with sneak).
pub fn abacus_scroll(nbt: &mut Nbt, delta: f64, sneaking: bool) {
    let v = nbt.get_f64("value").unwrap_or(0.0);
    // ItemAbacus: ctrl (here: sneak) scrolls by 10, otherwise by 1.
    let step = if sneaking { 10.0 } else { 1.0 };
    nbt.set("value", Nbt::double(v + delta.signum() * step));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    fn stack(id: &str) -> Stack {
        Stack { slot: 0, id: id.into(), count: 1, nbt: Nbt::default() }
    }

    #[test]
    fn foci_and_knots() {
        let mut f = stack(FOCUS);
        assert_eq!(read(&f), None);
        assert!(write(&mut f, Some(&Iota::Double(2.0)), false));
        assert_eq!(read(&f), Some(Iota::Double(2.0)));
        f.nbt.set("sealed", Nbt::byte(true));
        assert!(!write(&mut f, Some(&Iota::Null), true));
        assert!(write(&mut f, None, false));
        assert_eq!(read(&f), None);

        let mut k = stack(THOUGHT_KNOT);
        assert!(!write(&mut k, None, true));
        assert!(write(&mut k, Some(&Iota::Bool(true)), false));
        assert!(!writeable(&k));
    }

    #[test]
    fn scrolls_take_patterns_only() {
        let mut s = stack("hexcasting:scroll");
        let p = Iota::Pattern(HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap());
        assert!(!write(&mut s, Some(&Iota::Double(1.0)), true));
        assert!(write(&mut s, Some(&p), false));
        assert!(read(&s).unwrap().tolerates(&p));
    }

    #[test]
    fn spellbook_pages() {
        let mut b = stack(SPELLBOOK);
        assert!(write(&mut b, Some(&Iota::Double(1.0)), false));
        assert_eq!(rotate_page(&mut b.nbt, true), 2);
        assert_eq!(read(&b), None);
        assert!(write(&mut b, Some(&Iota::Double(2.0)), false));
        assert_eq!(highest_page(&b.nbt), 2);
        set_page_sealed(&mut b.nbt, true);
        assert!(!writeable(&b));
        assert_eq!(rotate_page(&mut b.nbt, false), 1);
        assert_eq!(read(&b), Some(Iota::Double(1.0)));
    }
}
