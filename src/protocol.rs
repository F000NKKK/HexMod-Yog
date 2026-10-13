//! The casting packets between the client's grid and the server's VM.
//!
//! * `hexcasting:cast` (client → server): a drawn pattern, in
//!   [`HexPattern::serialized_form`].
//! * `hexcasting:open_grid` (server → client): open the casting screen;
//!   payload `reset` when the caster's stack was just cleared.
//! * `hexcasting:stack` (server → client): a [`Report`] — what the last
//!   pattern did and the stack after it.
//!
//! Nothing here is display text: the server sends keys ([`Action`],
//! [`Mishap`]) and serialized iotas; the client words them in its own
//! language.

use crate::iota::Iota;

pub const CH_CAST: &str = "hexcasting:cast";
pub const CH_STACK: &str = "hexcasting:stack";
pub const CH_OPEN: &str = "hexcasting:open_grid";

/// A pattern that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    MindsReflection,
    CompassPurification,
    AlidadePurification,
    Reveal,
    JestersGambit,
    ClearStack,
}

/// Why a pattern failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mishap {
    /// The server doesn't know the caster's uuid yet.
    NoCaster,
    /// The entity on the stack isn't loaded.
    EntityGone,
    ExpectedEntity,
    StackEmpty,
    NothingToReveal,
    NeedTwo,
    /// No pattern has this angle signature.
    UnknownPattern(String),
    /// The strokes don't form a pattern.
    BadStrokes,
}

/// What the last cast did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(Action),
    Mishap(Mishap),
    /// The stack was wiped (sneak + staff).
    Reset,
}

/// A `hexcasting:stack` packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    /// Top first.
    pub stack: Vec<Iota>,
}

impl Action {
    const ALL: [(Action, &'static str); 6] = [
        (Action::MindsReflection, "minds_reflection"),
        (Action::CompassPurification, "compass_purification"),
        (Action::AlidadePurification, "alidade_purification"),
        (Action::Reveal, "reveal"),
        (Action::JestersGambit, "jesters_gambit"),
        (Action::ClearStack, "clear_stack"),
    ];

    pub fn key(self) -> &'static str {
        Self::ALL.iter().find(|(a, _)| *a == self).map(|(_, k)| *k).unwrap_or_default()
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, k)| *k == key).map(|(a, _)| *a)
    }
}

impl Mishap {
    fn encode(&self) -> String {
        match self {
            Mishap::NoCaster => "no_caster".into(),
            Mishap::EntityGone => "entity_gone".into(),
            Mishap::ExpectedEntity => "expected_entity".into(),
            Mishap::StackEmpty => "stack_empty".into(),
            Mishap::NothingToReveal => "nothing_to_reveal".into(),
            Mishap::NeedTwo => "need_two".into(),
            Mishap::UnknownPattern(sig) => format!("unknown_pattern\t{sig}"),
            Mishap::BadStrokes => "bad_strokes".into(),
        }
    }

    fn decode(s: &str) -> Option<Self> {
        let (head, arg) = s.split_once('\t').unwrap_or((s, ""));
        Some(match head {
            "no_caster" => Mishap::NoCaster,
            "entity_gone" => Mishap::EntityGone,
            "expected_entity" => Mishap::ExpectedEntity,
            "stack_empty" => Mishap::StackEmpty,
            "nothing_to_reveal" => Mishap::NothingToReveal,
            "need_two" => Mishap::NeedTwo,
            "unknown_pattern" => Mishap::UnknownPattern(arg.to_string()),
            "bad_strokes" => Mishap::BadStrokes,
            _ => return None,
        })
    }
}

impl Report {
    /// First line: `ok\t<action>`, `mishap\t<mishap>[\t<arg>]` or `reset`;
    /// then one serialized iota per line, top first.
    pub fn encode(&self) -> Vec<u8> {
        let mut lines = vec![match &self.outcome {
            Outcome::Ok(a) => format!("ok\t{}", a.key()),
            Outcome::Mishap(m) => format!("mishap\t{}", m.encode()),
            Outcome::Reset => "reset".into(),
        }];
        lines.extend(self.stack.iter().map(|i| i.serialize().replace('\n', " ")));
        lines.join("\n").into_bytes()
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        let text = String::from_utf8_lossy(payload);
        let mut lines = text.lines();
        let head = lines.next()?;
        let outcome = if head == "reset" {
            Outcome::Reset
        } else if let Some(a) = head.strip_prefix("ok\t") {
            Outcome::Ok(Action::from_key(a)?)
        } else {
            Outcome::Mishap(Mishap::decode(head.strip_prefix("mishap\t")?)?)
        };
        let stack = lines.filter_map(Iota::deserialize).collect();
        Some(Report { outcome, stack })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_round_trip() {
        for outcome in [
            Outcome::Ok(Action::Reveal),
            Outcome::Mishap(Mishap::UnknownPattern("qqq".into())),
            Outcome::Mishap(Mishap::StackEmpty),
            Outcome::Reset,
        ] {
            let r = Report {
                outcome,
                stack: vec![Iota::Vec3(1.0, 2.5, -3.0), Iota::Boolean(true), Iota::Null],
            };
            assert_eq!(Report::decode(&r.encode()), Some(r));
        }
        assert_eq!(Report::decode(b"what"), None);
    }
}
