//! The VM's state between patterns (Hex Casting's `CastingImage`).

use crate::iota::{Iota, read_stack, write_stack};
use crate::wire::{Reader, Writer};

/// An iota collected inside Introspection–Retrospection, and whether it
/// was escaped (escaped parens don't count towards the nesting).
#[derive(Debug, Clone, PartialEq)]
pub struct ParenIota {
    pub iota: Iota,
    pub escaped: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    /// Bottom first.
    pub stack: Vec<Iota>,
    pub paren_count: i64,
    pub parenthesized: Vec<ParenIota>,
    pub escape_next: bool,
    pub ops_consumed: i64,
    /// The Ravenmind (`userData["hexcasting:ravenmind"]`).
    pub ravenmind: Option<Iota>,
    /// Entities already pushed by Impulse this cast
    /// (`userData["hexcasting:impulsed"]`).
    pub marked_moved: Vec<String>,
}

impl Image {
    pub fn with_used_ops(mut self, n: i64) -> Self {
        self.ops_consumed += n;
        self
    }

    pub fn with_used_op(self) -> Self {
        self.with_used_ops(1)
    }

    pub fn with_reset_escape(mut self) -> Self {
        self.paren_count = 0;
        self.parenthesized.clear();
        self.escape_next = false;
        self
    }

    pub fn with_new_parenthesized(mut self, iota: Iota, escaped: bool) -> Self {
        self.parenthesized.push(ParenIota { iota, escaped });
        self
    }

    /// `CastingImage.checkAndMarkGivenMotion`: whether `uuid` was already
    /// pushed this cast (marking it if not).
    pub fn check_and_mark_moved(&mut self, uuid: &str) -> bool {
        if self.marked_moved.iter().any(|u| u == uuid) {
            true
        } else {
            self.marked_moved.push(uuid.to_owned());
            false
        }
    }

    pub fn write(&self, w: &mut Writer) {
        write_stack(w, &self.stack);
        w.int(self.paren_count);
        // Like `serializeToNBT`: a too-large parenthesized list is dropped.
        let parens: Vec<Iota> = self.parenthesized.iter().map(|p| p.iota.clone()).collect();
        if Iota::too_large(&parens) {
            w.int(0);
        } else {
            w.int(self.parenthesized.len() as i64);
            for p in &self.parenthesized {
                w.int(p.escaped as i64);
                p.iota.write(w);
            }
        }
        w.int(self.escape_next as i64).int(self.ops_consumed);
        match &self.ravenmind {
            Some(i) => {
                w.int(1);
                i.write(w);
            }
            None => {
                w.int(0);
            }
        }
        w.int(self.marked_moved.len() as i64);
        for u in &self.marked_moved {
            w.str(u);
        }
    }

    pub fn read(r: &mut Reader) -> Option<Self> {
        let stack = read_stack(r)?;
        let paren_count = r.int()?;
        let n = r.int()?;
        let mut parenthesized = Vec::new();
        for _ in 0..n {
            let escaped = r.int()? != 0;
            parenthesized.push(ParenIota { iota: Iota::read(r)?, escaped });
        }
        let escape_next = r.int()? != 0;
        let ops_consumed = r.int()?;
        let ravenmind = if r.int()? != 0 { Some(Iota::read(r)?) } else { None };
        let m = r.int()?;
        let mut marked_moved = Vec::new();
        for _ in 0..m {
            marked_moved.push(r.str()?.to_owned());
        }
        Some(Image {
            stack,
            paren_count,
            parenthesized,
            escape_next,
            ops_consumed,
            ravenmind,
            marked_moved,
        })
    }

    pub fn encode(&self) -> String {
        let mut w = Writer::new();
        self.write(&mut w);
        w.finish()
    }

    /// Unreadable data gives a fresh image, like `loadFromNbt`.
    pub fn decode(s: &str) -> Self {
        Image::read(&mut Reader::new(s)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_round_trip() {
        let mut img = Image {
            stack: vec![Iota::Double(3.0), Iota::Null],
            paren_count: 2,
            escape_next: true,
            ops_consumed: 7,
            ravenmind: Some(Iota::Bool(false)),
            ..Image::default()
        }
        .with_new_parenthesized(Iota::Garbage, true);
        assert!(!img.check_and_mark_moved("u-1"));
        assert!(img.check_and_mark_moved("u-1"));
        assert_eq!(Image::decode(&img.encode()), img);
        assert_eq!(Image::decode("garbage"), Image::default());
    }
}
