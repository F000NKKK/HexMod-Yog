//! Cyphers, trinkets and artifacts (Hex Casting's `ItemPackagedHex`): a hex
//! kept in the item, cast on right-click and paid for with the media the
//! item holds.

use crate::casting::env::Stack;
use crate::iota::Iota;
use crate::item::media;
use crate::item::pigment::Pigment;
use crate::snbt::Nbt;

/// `ItemPackagedHex.TAG_PROGRAM` / `TAG_PIGMENT`.
pub const TAG_PROGRAM: &str = "patterns";
pub const TAG_PIGMENT: &str = "pigment";

/// `hasHex`.
pub fn has_hex(stack: &Stack) -> bool {
    stack.nbt.contains(TAG_PROGRAM)
}

/// `getHex`: the iotas the item casts.
pub fn hex(stack: &Stack) -> Option<Vec<Iota>> {
    match Iota::deserialize(stack.nbt.get_str(TAG_PROGRAM)?) {
        Iota::List(l) => Some(l),
        _ => None,
    }
}

/// `writeHex`: stores the hex, the caster's pigment and `media` (also the
/// item's capacity).
pub fn write_hex(
    stack: &mut Stack,
    program: &[Iota],
    pigment: Option<&Pigment>,
    media_amount: i64,
) {
    stack.nbt.set(
        TAG_PROGRAM,
        Nbt::str(Iota::List(program.to_vec()).serialize()),
    );
    match pigment {
        Some(p) => {
            let mut c = Nbt::default();
            c.set("item", Nbt::str(p.item.clone()));
            c.set("owner", Nbt::str(p.owner.clone()));
            stack.nbt.set(TAG_PIGMENT, c);
        }
        None => {
            stack.nbt.remove(TAG_PIGMENT);
        }
    }
    stack.nbt.set(media::TAG_MAX_MEDIA, Nbt::long(media_amount));
    stack.nbt.set(media::TAG_MEDIA, Nbt::long(media_amount));
}

/// `clearHex`.
pub fn clear_hex(stack: &mut Stack) {
    for k in [
        TAG_PROGRAM,
        TAG_PIGMENT,
        media::TAG_MEDIA,
        media::TAG_MAX_MEDIA,
    ] {
        stack.nbt.remove(k);
    }
}

/// `getPigment`: the pigment it was made with.
pub fn pigment(stack: &Stack) -> Option<Pigment> {
    let c = stack.nbt.get(TAG_PIGMENT)?;
    Some(Pigment {
        item: c.get_str("item")?.to_owned(),
        owner: c.get_str("owner").unwrap_or_default().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Stack {
            slot: 0,
            id: crate::content::Items::TRINKET.into(),
            count: 1,
            nbt: Nbt::default(),
        };
        assert!(!has_hex(&s));
        let p = Pigment::default();
        write_hex(&mut s, &[Iota::Double(1.0), Iota::Null], Some(&p), 500);
        assert_eq!(hex(&s), Some(vec![Iota::Double(1.0), Iota::Null]));
        assert_eq!(pigment(&s), Some(p));
        assert_eq!(media::media(&s), 500);
        clear_hex(&mut s);
        assert!(!has_hex(&s) && hex(&s).is_none());
    }
}
