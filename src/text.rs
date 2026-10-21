//! Display text built on the server and worded on the client — Minecraft's
//! `Component.translatable(key, args…)` in miniature. The server never
//! formats a message itself: it sends keys and arguments, and the player's
//! client fills them in its own language ([`Text::render`]).

#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    /// Shown as is.
    Lit(String),
    /// Translation `key`, its `%s` filled with `args`.
    Key(String, Vec<Text>),
    /// The parts, separated by `sep`.
    Join(Vec<Text>, String),
    /// A pattern: drawn as a small rune where the text is drawn
    /// ([`Text::segments`]), worded where it can only be words (chat).
    Pattern(crate::math::HexPattern),
}

/// A piece of text as it's drawn: words, or a pattern's rune.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Words(String),
    Rune(crate::math::HexPattern),
}

/// Stands for argument `i` while a translation with runes in it is split.
fn marker(i: usize) -> String {
    format!("\u{E000}{i}\u{E001}")
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
            Text::Join(parts, sep) => parts.iter().map(Text::render).collect::<Vec<_>>().join(sep),
            Text::Pattern(p) => self::worded(p).render(),
        }
    }

    /// The text as it's drawn: words, and runes where it has patterns.
    pub fn segments(&self) -> Vec<Segment> {
        let mut out = Vec::new();
        self.push_segments(&mut out);
        out
    }

    fn has_pattern(&self) -> bool {
        match self {
            Text::Pattern(_) => true,
            Text::Lit(_) => false,
            Text::Key(_, args) | Text::Join(args, _) => args.iter().any(Text::has_pattern),
        }
    }

    fn push_segments(&self, out: &mut Vec<Segment>) {
        fn words(out: &mut Vec<Segment>, s: &str) {
            if s.is_empty() {
                return;
            }
            match out.last_mut() {
                Some(Segment::Words(w)) => w.push_str(s),
                _ => out.push(Segment::Words(s.to_owned())),
            }
        }
        match self {
            Text::Pattern(p) => out.push(Segment::Rune(p.clone())),
            Text::Join(parts, sep) => {
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        words(out, sep);
                    }
                    part.push_segments(out);
                }
            }
            // A translation around runes: worded with markers, split at them.
            Text::Key(key, args) if self.has_pattern() => {
                let markers: Vec<String> = (0..args.len()).map(marker).collect();
                let refs: Vec<&dyn std::fmt::Display> = markers
                    .iter()
                    .map(|m| m as &dyn std::fmt::Display)
                    .collect();
                let mut rest = yog_api::tr_args(key, &refs);
                let mut placed = vec![false; args.len()];
                while let Some(start) = rest.find('\u{E000}') {
                    words(out, &rest[..start]);
                    let after = &rest[start + '\u{E000}'.len_utf8()..];
                    let Some(end) = after.find('\u{E001}') else {
                        break;
                    };
                    if let Some(i) = after[..end]
                        .parse::<usize>()
                        .ok()
                        .filter(|i| *i < args.len())
                    {
                        args[i].push_segments(out);
                        placed[i] = true;
                    }
                    rest = after[end + '\u{E001}'.len_utf8()..].to_owned();
                }
                words(out, &rest);
                // A translation without room for some (a missing one): after it.
                for (arg, _) in args.iter().zip(&placed).filter(|(_, p)| !**p) {
                    words(out, " ");
                    arg.push_segments(out);
                }
            }
            _ => words(out, &self.render()),
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
            Text::Pattern(p) => worded(p).to_json(),
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

    /// [`to_json`](Self::to_json) in a color (`"gray"`, `"#RRGGBB"`, …).
    /// Into the wire format (for packets and storage).
    pub fn write(&self, w: &mut crate::wire::Writer) {
        match self {
            Text::Lit(v) => {
                w.tag('L').str(v);
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
            Text::Pattern(p) => {
                w.tag('P').str(&p.serialized_form());
            }
        }
    }

    pub fn read(r: &mut crate::wire::Reader) -> Option<Text> {
        match r.tag()? {
            'L' => Some(Text::Lit(r.str()?.to_owned())),
            'K' => {
                let key = r.str()?.to_owned();
                let n = r.int()?;
                let args = (0..n).map(|_| Text::read(r)).collect::<Option<Vec<_>>>()?;
                Some(Text::Key(key, args))
            }
            'J' => {
                let sep = r.str()?.to_owned();
                let n = r.int()?;
                let parts = (0..n).map(|_| Text::read(r)).collect::<Option<Vec<_>>>()?;
                Some(Text::Join(parts, sep))
            }
            'P' => crate::math::HexPattern::from_serialized(r.str()?).map(Text::Pattern),
            _ => None,
        }
    }

    pub fn encode(&self) -> String {
        let mut w = crate::wire::Writer::new();
        self.write(&mut w);
        w.finish()
    }

    pub fn decode(s: &str) -> Option<Text> {
        Text::read(&mut crate::wire::Reader::new(s))
    }

    pub fn to_json_colored(&self, color: &str) -> String {
        format!(
            "{{\"text\":\"\",\"color\":\"{color}\",\"extra\":[{}]}}",
            self.to_json()
        )
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
    fn doubles_like_hex() {
        assert_eq!(fmt_double(1.0), "1");
        assert_eq!(fmt_double(-2.5), "-2.5");
        assert_eq!(fmt_double(std::f64::consts::PI), "3.1416");
        assert_eq!(fmt_double(-0.00001), "0");
    }
}

#[cfg(test)]
mod wire_tests {
    use super::Text;

    #[test]
    fn a_text_survives_the_wire() {
        let t = Text::key_args(
            "a.b",
            vec![
                Text::lit("x:y;"),
                Text::Join(vec![Text::int(3), Text::key("c")], ", ".into()),
            ],
        );
        assert_eq!(Text::decode(&t.encode()), Some(t));
    }
}

/// A pattern in words: `Pattern(NORTH_EAST qaq)`.
fn worded(p: &crate::math::HexPattern) -> Text {
    Text::key_args(
        crate::res::Lang::TOOLTIP_PATTERN_IOTA_KEY,
        vec![Text::Lit(crate::iota::pattern_text(p))],
    )
}
