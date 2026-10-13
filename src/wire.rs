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

/// Quote `s` as an SNBT string (`"…"` with `\` escapes).
pub fn snbt_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The string value of top-level key `key` in an SNBT compound, if it is a
/// quoted string.
pub fn snbt_string(snbt: &str, key: &str) -> Option<String> {
    let bytes = snbt.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'"' | b'\'' => {
                // Skip a quoted string that is not a key we want.
                let (_, end) = read_quoted(snbt, i)?;
                i = end;
                continue;
            }
            _ if depth == 1 && snbt.is_char_boundary(i) && snbt[i..].starts_with(key) => {
                let before = snbt[..i].trim_end();
                let after = snbt[i + key.len()..].trim_start();
                if (before.ends_with('{') || before.ends_with(',')) && after.starts_with(':') {
                    let value = after[1..].trim_start();
                    let start = snbt.len() - value.len();
                    if matches!(value.as_bytes().first(), Some(b'"' | b'\'')) {
                        return read_quoted(snbt, start).map(|(s, _)| s);
                    }
                    return None;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The unescaped content of the quoted string starting at byte `start`, and
/// the index just past its closing quote.
fn read_quoted(s: &str, start: usize) -> Option<(String, usize)> {
    let quote = s[start..].chars().next()?;
    let mut out = String::new();
    let mut escaped = false;
    for (off, c) in s[start + 1..].char_indices() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((out, start + 1 + off + 1));
        } else {
            out.push(c);
        }
    }
    None
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

    #[test]
    fn snbt_strings() {
        let tag = format!("{{a:1b,hex_iota:{},n:{{hex_iota:\"no\"}}}}", snbt_quote("q\"uo\\te"));
        assert_eq!(snbt_string(&tag, "hex_iota").as_deref(), Some("q\"uo\\te"));
        assert_eq!(snbt_string("{a:1b}", "hex_iota"), None);
        assert_eq!(snbt_string("{other_hex_iota:\"x\"}", "hex_iota"), None);
    }
}
