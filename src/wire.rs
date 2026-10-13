//! A tiny self-delimiting text encoding used for everything this mod stores
//! or sends: iotas (stack, item NBT, slates, akashic records), display texts
//! and the casting reports.
//!
//! Strings are length-prefixed (`5:hello`), numbers end with `;`, and a
//! value starts with a one-character tag — so nested lists, names with
//! commas or newlines, and SNBT quoting never get in each other's way.

use std::fmt::Write as _;

#[derive(Default)]
pub struct Writer {
    buf: String,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tag(&mut self, c: char) -> &mut Self {
        self.buf.push(c);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        let _ = write!(self.buf, "{}:", s.len());
        self.buf.push_str(s);
        self
    }

    pub fn num(&mut self, v: f64) -> &mut Self {
        // `{:?}` round-trips every f64 (incl. inf/NaN spelled `inf`/`NaN`).
        let _ = write!(self.buf, "{v:?};");
        self
    }

    pub fn int(&mut self, v: i64) -> &mut Self {
        let _ = write!(self.buf, "{v};");
        self
    }

    pub fn finish(self) -> String {
        self.buf
    }
}

pub struct Reader<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(src: &'a str) -> Self {
        Reader { src, pos: 0 }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    pub fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    pub fn tag(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn until(&mut self, end: char) -> Option<&'a str> {
        let rest = &self.src[self.pos..];
        let i = rest.find(end)?;
        self.pos += i + end.len_utf8();
        Some(&rest[..i])
    }

    pub fn str(&mut self) -> Option<&'a str> {
        let len: usize = self.until(':')?.parse().ok()?;
        let end = self.pos.checked_add(len)?;
        let s = self.src.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    pub fn num(&mut self) -> Option<f64> {
        self.until(';')?.parse().ok()
    }

    pub fn int(&mut self) -> Option<i64> {
        self.until(';')?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut w = Writer::new();
        w.tag('x').str("a:b;c\n").num(-1.5).int(42).str("");
        let s = w.finish();
        let mut r = Reader::new(&s);
        assert_eq!(r.tag(), Some('x'));
        assert_eq!(r.str(), Some("a:b;c\n"));
        assert_eq!(r.num(), Some(-1.5));
        assert_eq!(r.int(), Some(42));
        assert_eq!(r.str(), Some(""));
        assert!(r.at_end());
    }
}
