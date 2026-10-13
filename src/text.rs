//! Display text built on the server and worded on the client — Minecraft's
//! `Component.translatable(key, args…)` in miniature. The server never
//! formats a message itself: it sends keys and arguments, and the player's
//! client fills them in its own language ([`Text::render`]).

use crate::wire::{Reader, Writer};

#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    /// Shown as is.
    Lit(String),
    /// Translation `key`, its `%s` filled with `args`.
    Key(String, Vec<Text>),
    /// The parts, separated by `sep`.
    Join(Vec<Text>, String),
}

impl Text {
    pub fn lit(s: impl Into<String>) -> Self {
        Text::Lit(s.into())
    }

    pub fn key(key: impl Into<String>) -> Self {
        Text::Key(key.into(), Vec::new())
    }

    pub fn key_args(key: impl Into<String>, args: Vec<Text>) -> Self {
        Text::Key(key.into(), args)
    }

    /// A number argument, formatted like Hex Casting's `####.####`.
    pub fn num(v: f64) -> Self {
        Text::Lit(fmt_double(v))
    }

    pub fn int(v: i64) -> Self {
        Text::Lit(v.to_string())
    }

    /// The text in the current language (translations come from the game's
    /// lang files; a missing key shows as itself).
    pub fn render(&self) -> String {
        match self {
            Text::Lit(s) => s.clone(),
            Text::Key(key, args) => {
                let args: Vec<String> = args.iter().map(Text::render).collect();
                let refs: Vec<&dyn std::fmt::Display> =
                    args.iter().map(|a| a as &dyn std::fmt::Display).collect();
                yog_api::tr_args(key, &refs)
            }
            Text::Join(parts, sep) => {
                parts.iter().map(Text::render).collect::<Vec<_>>().join(sep)
            }
        }
    }

    /// As a Minecraft JSON text component (for `tellraw`), so the player's
    /// client words it in its own language.
    pub fn to_json(&self) -> String {
        fn s(v: &str) -> String {
            let mut out = String::from("\"");
            for c in v.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        match self {
            Text::Lit(v) => format!("{{\"text\":{}}}", s(v)),
            Text::Key(key, args) if args.is_empty() => format!("{{\"translate\":{}}}", s(key)),
            Text::Key(key, args) => format!(
                "{{\"translate\":{},\"with\":[{}]}}",
                s(key),
                args.iter().map(Text::to_json).collect::<Vec<_>>().join(",")
            ),
            Text::Join(parts, sep) => {
                let mut extra = Vec::new();
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 && !sep.is_empty() {
                        extra.push(Text::Lit(sep.clone()).to_json());
                    }
                    extra.push(p.to_json());
                }
                if extra.is_empty() {
                    "{\"text\":\"\"}".to_owned()
                } else {
                    format!("{{\"text\":\"\",\"extra\":[{}]}}", extra.join(","))
                }
            }
        }
    }

    pub fn write(&self, w: &mut Writer) {
        match self {
            Text::Lit(s) => {
                w.tag('L').str(s);
            }
            Text::Key(key, args) => {
                w.tag('K').str(key).int(args.len() as i64);
                for a in args {
                    a.write(w);
                }
            }
            Text::Join(parts, sep) => {
                w.tag('J').str(sep).int(parts.len() as i64);
                for p in parts {
                    p.write(w);
                }
            }
        }
    }

    pub fn read(r: &mut Reader) -> Option<Self> {
        Some(match r.tag()? {
            'L' => Text::Lit(r.str()?.to_owned()),
            'K' => {
                let key = r.str()?.to_owned();
                let n = r.int()?;
                let mut args = Vec::new();
                for _ in 0..n {
                    args.push(Text::read(r)?);
                }
                Text::Key(key, args)
            }
            'J' => {
                let sep = r.str()?.to_owned();
                let n = r.int()?;
                let mut parts = Vec::new();
                for _ in 0..n {
                    parts.push(Text::read(r)?);
                }
                Text::Join(parts, sep)
            }
            _ => return None,
        })
    }

    pub fn encode(&self) -> String {
        let mut w = Writer::new();
        self.write(&mut w);
        w.finish()
    }

    pub fn decode(s: &str) -> Option<Self> {
        Text::read(&mut Reader::new(s))
    }
}

/// Hex Casting's `DecimalFormat("####.####")`: at most four decimals,
/// without trailing zeros.
pub fn fmt_double(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "-0" | "" => "0".into(),
        s => s.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_round_trip() {
        let t = Text::key_args(
            "hexcasting.mishap",
            vec![Text::lit("Jester's: Gambit"), Text::Join(vec![Text::num(1.5), Text::key("x")], ", ".into())],
        );
        assert_eq!(Text::decode(&t.encode()), Some(t));
    }

    #[test]
    fn doubles_like_hex() {
        assert_eq!(fmt_double(1.0), "1");
        assert_eq!(fmt_double(-2.5), "-2.5");
        assert_eq!(fmt_double(std::f64::consts::PI), "3.1416");
        assert_eq!(fmt_double(-0.00001), "0");
    }
}
