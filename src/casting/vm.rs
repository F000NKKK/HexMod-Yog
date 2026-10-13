//! Server-side casting VM: per-player stacks, pattern resolution, execution.
//!
//! Patterns are matched by their *angle signature* — the sequence of turns
//! between consecutive segments — so a pattern means the same thing whatever
//! direction it was drawn in, exactly like Hexcasting:
//! `w` straight, `e` 60° right, `d` 120° right, `s` 180°, `a` 120° left,
//! `q` 60° left.

use std::collections::HashMap;
use std::sync::Mutex;

use yog_api::{Entity, Server};

use crate::iota::Iota;
use crate::math::{HexAngle, HexPattern};
use crate::protocol::{Action, Mishap, Outcome, Report, CH_STACK};

/// Per-player casting stacks, keyed by player name.
static STACKS: Mutex<Option<HashMap<String, Vec<Iota>>>> = Mutex::new(None);

/// player name → uuid, filled from join events (packets carry only names).
static NAME2UUID: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub fn remember_player(name: &str, uuid: &str) {
    NAME2UUID
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(name.to_string(), uuid.to_string());
}

fn uuid_of(player: &str) -> Option<String> {
    NAME2UUID.lock().unwrap().as_ref()?.get(player).cloned()
}

/// The angle signature of a pattern, e.g. `"qaq"` for Mind's Reflection.
pub fn angle_signature(p: &HexPattern) -> String {
    let mut sig = String::new();
    let mut prev = p.start;
    for &step in &p.steps {
        sig.push(match step.angle_from(prev) {
            HexAngle::Zero => 'w',
            HexAngle::Sixty => 'e',
            HexAngle::OneTwenty => 'd',
            HexAngle::OneEighty => 's',
            HexAngle::TwoForty => 'a',
            HexAngle::ThreeHundred => 'q',
        });
        prev = step;
    }
    sig
}

/// Execute one drawn pattern for `player`.
fn execute(sig: &str, player: &str, srv: &dyn Server, stack: &mut Vec<Iota>) -> Outcome {
    match sig {
        // Mind's Reflection — push a reference to the caster.
        "qaq" => match uuid_of(player).and_then(|u| uuid::Uuid::parse_str(&u).ok()) {
            Some(u) => {
                stack.push(Iota::Entity(u));
                Outcome::Ok(Action::MindsReflection)
            }
            None => Outcome::Mishap(Mishap::NoCaster),
        },

        // Compass' Purification — entity → its position.
        "aa" => match stack.pop() {
            Some(Iota::Entity(u)) => match Entity::new(srv, u.to_string()).position() {
                Some((x, y, z)) => {
                    stack.push(Iota::Vec3(x, y, z));
                    Outcome::Ok(Action::CompassPurification)
                }
                None => Outcome::Mishap(Mishap::EntityGone),
            },
            Some(other) => {
                stack.push(other);
                Outcome::Mishap(Mishap::ExpectedEntity)
            }
            None => Outcome::Mishap(Mishap::StackEmpty),
        },

        // Alidade's Purification — entity → its look vector.
        "wa" => match stack.pop() {
            Some(Iota::Entity(u)) => match Entity::new(srv, u.to_string()).look_vector() {
                Some((x, y, z)) => {
                    stack.push(Iota::Vec3(x, y, z));
                    Outcome::Ok(Action::AlidadePurification)
                }
                None => Outcome::Mishap(Mishap::EntityGone),
            },
            Some(other) => {
                stack.push(other);
                Outcome::Mishap(Mishap::ExpectedEntity)
            }
            None => Outcome::Mishap(Mishap::StackEmpty),
        },

        // Reveal — show the top iota (the caster's grid displays it).
        "de" => match stack.last() {
            Some(_) => Outcome::Ok(Action::Reveal),
            None => Outcome::Mishap(Mishap::NothingToReveal),
        },

        // Jester's Gambit — swap the top two iotas.
        "aawdd" => {
            let n = stack.len();
            if n >= 2 {
                stack.swap(n - 1, n - 2);
                Outcome::Ok(Action::JestersGambit)
            } else {
                Outcome::Mishap(Mishap::NeedTwo)
            }
        }

        // Charon's Gambit-ish: clear the stack (practical while developing).
        "dsw" => {
            stack.clear();
            Outcome::Ok(Action::ClearStack)
        }

        _ => Outcome::Mishap(Mishap::UnknownPattern(sig.to_string())),
    }
}

/// Clear a player's stack (sneak + use staff) and tell their grid.
pub fn reset(player: &str, srv: &dyn Server) {
    if let Some(map) = STACKS.lock().unwrap().as_mut() {
        map.remove(player);
    }
    let report = Report { outcome: Outcome::Reset, stack: Vec::new() };
    srv.send_to_player(player, CH_STACK, &report.encode());
}

/// Entry point for the `hexcasting:cast` packet: runs the pattern and sends
/// the caster a [`Report`].
pub fn handle_cast(player: &str, payload: &[u8], srv: &dyn Server) {
    let mut guard = STACKS.lock().unwrap();
    let stack = guard
        .get_or_insert_with(HashMap::new)
        .entry(player.to_string())
        .or_default();
    let outcome = match HexPattern::from_serialized(&String::from_utf8_lossy(payload)) {
        Some(pattern) => execute(&angle_signature(&pattern), player, srv, stack),
        None => Outcome::Mishap(Mishap::BadStrokes),
    };
    let report = Report { outcome, stack: stack.iter().rev().cloned().collect() };
    srv.send_to_player(player, CH_STACK, &report.encode());
}
