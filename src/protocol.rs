//! The staff-casting packets, after Hex Casting's messages:
//!
//! * `hexcasting:pat_cs` (client → server, `MsgNewSpellPatternC2S`): a
//!   [`NewPattern`] — the hand the staff is in, the drawn pattern and every
//!   pattern on the grid so far (the last one is the new one).
//! * `hexcasting:pat_sc` (server → client, `MsgNewSpellPatternS2C`): a
//!   [`PatternResult`] — how the pattern resolved and the stack after it.
//! * `hexcasting:cgui` (server → client, `MsgOpenSpellGuiS2C`): an
//!   [`OpenGui`] — open the grid with the saved patterns and stack.
//!
//! Iotas travel whole ([`crate::wire`]); the client words them in its own
//! language, as the original sends iota tags and lets the client render.

use crate::iota::{Iota, read_stack, write_stack};
use crate::math::{HexCoord, HexPattern};
use crate::wire::{Reader, Writer};

pub const CH_NEW_PATTERN: &str = "hexcasting:pat_cs";
pub const CH_PATTERN_RESULT: &str = "hexcasting:pat_sc";
pub const CH_OPEN_GUI: &str = "hexcasting:cgui";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Main,
    Off,
}

/// Hex Casting's `ResolvedPatternType`: how a drawn pattern went, and the
/// colors the grid draws it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedType {
    Unresolved,
    Evaluated,
    Escaped,
    Undone,
    Errored,
    Invalid,
}

impl ResolvedType {
    const ALL: [ResolvedType; 6] = [
        ResolvedType::Unresolved,
        ResolvedType::Evaluated,
        ResolvedType::Escaped,
        ResolvedType::Undone,
        ResolvedType::Errored,
        ResolvedType::Invalid,
    ];

    /// `(color, fade_color)`, RGB.
    pub fn colors(self) -> (u32, u32) {
        match self {
            ResolvedType::Unresolved => (0x7f7f7f, 0xcccccc),
            ResolvedType::Evaluated => (0x7385de, 0xfecbe6),
            ResolvedType::Escaped => (0xddcc73, 0xfffae5),
            ResolvedType::Undone => (0xb26b6b, 0xcca88e),
            ResolvedType::Errored => (0xde6262, 0xffc7a0),
            ResolvedType::Invalid => (0xb26b6b, 0xcca88e),
        }
    }

    pub fn success(self) -> bool {
        matches!(
            self,
            ResolvedType::Evaluated | ResolvedType::Escaped | ResolvedType::Undone
        )
    }

    pub fn index(self) -> i64 {
        self as i64
    }

    pub fn from_index(i: i64) -> Self {
        // Like the original's `getSafe`: unknown → the first.
        usize::try_from(i)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(ResolvedType::Unresolved)
    }
}

/// A pattern on the grid where it was drawn (`ResolvedPattern`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPattern {
    pub pattern: HexPattern,
    pub origin: HexCoord,
    pub kind: ResolvedType,
}

impl ResolvedPattern {
    pub fn write(&self, w: &mut Writer) {
        w.str(&self.pattern.serialized_form())
            .int(self.origin.0 as i64)
            .int(self.origin.1 as i64)
            .int(self.kind.index());
    }

    pub fn read(r: &mut Reader) -> Option<Self> {
        let pattern = HexPattern::from_serialized(r.str()?)?;
        let origin = HexCoord(r.int()? as i32, r.int()? as i32);
        Some(Self {
            pattern,
            origin,
            kind: ResolvedType::from_index(r.int()?),
        })
    }
}

pub fn write_patterns(w: &mut Writer, pats: &[ResolvedPattern]) {
    w.int(pats.len() as i64);
    for p in pats {
        p.write(w);
    }
}

pub fn read_patterns(r: &mut Reader) -> Option<Vec<ResolvedPattern>> {
    let n = r.int()?;
    (0..n).map(|_| ResolvedPattern::read(r)).collect()
}

fn write_opt(w: &mut Writer, i: &Option<Iota>) {
    match i {
        Some(i) => {
            w.int(1);
            i.write(w);
        }
        None => {
            w.int(0);
        }
    }
}

fn read_opt(r: &mut Reader) -> Option<Option<Iota>> {
    Some(if r.int()? != 0 {
        Some(Iota::read(r)?)
    } else {
        None
    })
}

/// `MsgNewSpellPatternC2S`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPattern {
    pub hand: Hand,
    pub pattern: HexPattern,
    pub resolved: Vec<ResolvedPattern>,
}

impl NewPattern {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.int(matches!(self.hand, Hand::Off) as i64)
            .str(&self.pattern.serialized_form());
        write_patterns(&mut w, &self.resolved);
        w.finish().into_bytes()
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(b).ok()?;
        let mut r = Reader::new(s);
        let hand = if r.int()? != 0 { Hand::Off } else { Hand::Main };
        let pattern = HexPattern::from_serialized(r.str()?)?;
        Some(Self {
            hand,
            pattern,
            resolved: read_patterns(&mut r)?,
        })
    }
}

/// `ExecutionClientView` + the index of the pattern it answers
/// (`MsgNewSpellPatternS2C`).
#[derive(Debug, Clone, PartialEq)]
pub struct PatternResult {
    pub stack_clear: bool,
    pub kind: ResolvedType,
    /// Bottom first, as the VM keeps it.
    pub stack: Vec<Iota>,
    pub ravenmind: Option<Iota>,
    pub index: i64,
}

impl PatternResult {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.int(self.stack_clear as i64)
            .int(self.kind.index())
            .int(self.index);
        write_stack(&mut w, &self.stack);
        write_opt(&mut w, &self.ravenmind);
        w.finish().into_bytes()
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(b).ok()?;
        let mut r = Reader::new(s);
        let stack_clear = r.int()? != 0;
        let kind = ResolvedType::from_index(r.int()?);
        let index = r.int()?;
        let stack = read_stack(&mut r)?;
        let ravenmind = read_opt(&mut r)?;
        Some(Self {
            stack_clear,
            kind,
            stack,
            ravenmind,
            index,
        })
    }
}

/// `MsgOpenSpellGuiS2C`.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenGui {
    pub hand: Hand,
    pub patterns: Vec<ResolvedPattern>,
    /// Bottom first.
    pub stack: Vec<Iota>,
    pub ravenmind: Option<Iota>,
    pub paren_count: i64,
}

impl OpenGui {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.int(matches!(self.hand, Hand::Off) as i64);
        write_patterns(&mut w, &self.patterns);
        write_stack(&mut w, &self.stack);
        write_opt(&mut w, &self.ravenmind);
        w.int(self.paren_count);
        w.finish().into_bytes()
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(b).ok()?;
        let mut r = Reader::new(s);
        let hand = if r.int()? != 0 { Hand::Off } else { Hand::Main };
        let patterns = read_patterns(&mut r)?;
        let stack = read_stack(&mut r)?;
        let ravenmind = read_opt(&mut r)?;
        Some(Self {
            hand,
            patterns,
            stack,
            ravenmind,
            paren_count: r.int()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn packets_round_trip() {
        let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        let rp = ResolvedPattern {
            pattern: p.clone(),
            origin: HexCoord(-2, 3),
            kind: ResolvedType::Escaped,
        };
        let n = NewPattern {
            hand: Hand::Off,
            pattern: p,
            resolved: vec![rp.clone()],
        };
        assert_eq!(NewPattern::decode(&n.encode()), Some(n));
        let r = PatternResult {
            stack_clear: false,
            kind: ResolvedType::Errored,
            stack: vec![Iota::Double(2.0), Iota::Null],
            ravenmind: Some(Iota::Bool(true)),
            index: 4,
        };
        assert_eq!(PatternResult::decode(&r.encode()), Some(r));
        let o = OpenGui {
            hand: Hand::Main,
            patterns: vec![rp],
            stack: vec![],
            ravenmind: None,
            paren_count: 1,
        };
        assert_eq!(OpenGui::decode(&o.encode()), Some(o));
        assert_eq!(ResolvedType::from_index(99), ResolvedType::Unresolved);
    }
}
