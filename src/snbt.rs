//! A small SNBT (stringified NBT) reader/writer, for item and block data
//! the loader hands over as SNBT text.
//!
//! Numbers keep their literal text (with the type suffix), so a compound
//! read and written back is unchanged.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Compound(Vec<(String, Nbt)>),
    /// `prefix` is `B`/`I`/`L` for typed arrays (`[I;1,2]`), else empty.
    List { prefix: String, items: Vec<Nbt> },
    Str(String),
    /// A number literal as written, e.g. `1b`, `3L`, `0.5d`, `7`.
    Num(String),
}

impl Default for Nbt {
    fn default() -> Self {
        Nbt::Compound(Vec::new())
    }
}

impl Nbt {
    /// Parses SNBT; anything unreadable gives an empty compound.
    pub fn parse(s: &str) -> Nbt {
        let mut p = Parser { s: s.as_bytes(), i: 0 };
        p.value().filter(|_| {
            p.ws();
            p.i == p.s.len()
        })
        .unwrap_or_default()
    }

    pub fn long(v: i64) -> Nbt {
        Nbt::Num(format!("{v}L"))
    }

    pub fn int(v: i32) -> Nbt {
        Nbt::Num(v.to_string())
    }

    pub fn double(v: f64) -> Nbt {
        Nbt::Num(format!("{v:?}d"))
    }

    pub fn byte(v: bool) -> Nbt {
        Nbt::Num(if v { "1b" } else { "0b" }.into())
    }

    pub fn str(s: impl Into<String>) -> Nbt {
        Nbt::Str(s.into())
    }

    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(e) => e.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Nbt> {
        match self {
            Nbt::Compound(e) => e.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets `key` (keeping its place if present). No-op on non-compounds.
    pub fn set(&mut self, key: &str, value: Nbt) {
        if let Nbt::Compound(e) = self {
            match e.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => e.push((key.to_owned(), value)),
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Nbt> {
        match self {
            Nbt::Compound(e) => {
                let i = e.iter().position(|(k, _)| k == key)?;
                Some(e.remove(i).1)
            }
            _ => None,
        }
    }

    /// The compound under `key`, created if missing.
    pub fn compound_mut(&mut self, key: &str) -> &mut Nbt {
        if !matches!(self.get(key), Some(Nbt::Compound(_))) {
            self.set(key, Nbt::default());
        }
        self.get_mut(key).expect("just set")
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Nbt::Compound(e) => e.is_empty(),
            Nbt::List { items, .. } => items.is_empty(),
            Nbt::Str(s) => s.is_empty(),
            Nbt::Num(_) => false,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Any number, as `f64` (`1b`/`true` count as 1).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Nbt::Num(n) => {
                let t = n.trim_end_matches(['b', 'B', 's', 'S', 'l', 'L', 'f', 'F', 'd', 'D']);
                match n.as_str() {
                    "true" => Some(1.0),
                    "false" => Some(0.0),
                    _ => t.parse().ok(),
                }
            }
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|v| v as i64)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Nbt::as_str)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Nbt::as_i64)
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Nbt::as_f64)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).and_then(Nbt::as_f64).is_some_and(|v| v != 0.0)
    }

    pub fn to_snbt(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Nbt::Compound(e) => {
                out.push('{');
                for (i, (k, v)) in e.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !k.is_empty() && k.bytes().all(is_bare) {
                        out.push_str(k);
                    } else {
                        out.push_str(&quote(k));
                    }
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
            Nbt::List { prefix, items } => {
                out.push('[');
                if !prefix.is_empty() {
                    let _ = write!(out, "{prefix};");
                }
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Nbt::Str(s) => out.push_str(&quote(s)),
            Nbt::Num(n) => out.push_str(n),
        }
    }
}

fn is_bare(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+')
}

/// Quote `s` as an SNBT string (`"…"` with `\` escapes).
pub fn quote(s: &str) -> String {
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

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Nbt> {
        self.ws();
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut e = Vec::new();
                if self.eat(b'}') {
                    return Some(Nbt::Compound(e));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    e.push((k, self.value()?));
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b'}') {
                        return Some(Nbt::Compound(e));
                    }
                    return None;
                }
            }
            b'[' => {
                self.i += 1;
                self.ws();
                let mut prefix = String::new();
                if self.s.get(self.i + 1) == Some(&b';') {
                    prefix = (self.s[self.i] as char).to_string();
                    self.i += 2;
                }
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Some(Nbt::List { prefix, items });
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b']') {
                        return Some(Nbt::List { prefix, items });
                    }
                    return None;
                }
            }
            b'"' | b'\'' => self.string().map(Nbt::Str),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && is_bare(self.s[self.i]) {
                    self.i += 1;
                }
                if start == self.i {
                    return None;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).ok()?.to_owned();
                let numeric = t.as_bytes()[0].is_ascii_digit()
                    || (t.len() > 1 && matches!(t.as_bytes()[0], b'-' | b'+' | b'.'))
                    || t == "true"
                    || t == "false";
                Some(if numeric { Nbt::Num(t) } else { Nbt::Str(t) })
            }
        }
    }

    /// A quoted or bare string.
    fn string(&mut self) -> Option<String> {
        let q = *self.s.get(self.i)?;
        if q != b'"' && q != b'\'' {
            let start = self.i;
            while self.i < self.s.len() && is_bare(self.s[self.i]) {
                self.i += 1;
            }
            return (start < self.i).then(|| String::from_utf8_lossy(&self.s[start..self.i]).into_owned());
        }
        self.i += 1;
        let mut out = Vec::new();
        while self.i < self.s.len() {
            let c = self.s[self.i];
            self.i += 1;
            if c == b'\\' {
                out.push(*self.s.get(self.i)?);
                self.i += 1;
            } else if c == q {
                return String::from_utf8(out).ok();
            } else {
                out.push(c);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes_back() {
        let src = r#"{a:1b,"b c":"q\"x",n:{m:[I;1,2],l:[1.5d,-2L]},s:'it'}"#;
        let t = Nbt::parse(src);
        assert_eq!(t.get_str("b c"), Some("q\"x"));
        assert_eq!(t.get("n").and_then(|n| n.get_i64("x")), None);
        assert!(t.get_bool("a"));
        assert_eq!(Nbt::parse(&t.to_snbt()), t);
        let mut t = Nbt::default();
        t.set("hexcasting:media", Nbt::long(5));
        t.compound_mut("pages").set("1", Nbt::str("x"));
        assert_eq!(t.to_snbt(), r#"{"hexcasting:media":5L,pages:{1:"x"}}"#);
        assert_eq!(Nbt::parse(&t.to_snbt()).get_i64("hexcasting:media"), Some(5));
        assert_eq!(Nbt::parse("garbage{"), Nbt::default());
    }
}
