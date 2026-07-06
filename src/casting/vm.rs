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

/// Per-player casting stacks, keyed by player name.
static STACKS: Mutex<Option<HashMap<String, Vec<Iota>>>> = Mutex::new(None);

/// player name → uuid, filled from join events (packets carry only names).
static NAME2UUID: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub fn remember_player(name: &str, uuid: &str) {
    NAME2UUID.lock().unwrap()
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

fn display_iota(i: &Iota) -> String {
    match i {
        Iota::Boolean(b) => b.to_string(),
        Iota::Double(d) => format!("{d:.2}"),
        Iota::Int(n) => n.to_string(),
        Iota::String(s) => format!("\"{s}\""),
        Iota::Entity(u) => format!("entity {}", &u.to_string()[..8]),
        Iota::Vec3(x, y, z) => format!("({x:.1}, {y:.1}, {z:.1})"),
        Iota::Pattern(p) => format!("pattern {}", angle_signature(p)),
        Iota::List(l) => format!("[{} iotas]", l.len()),
        Iota::Null => "null".into(),
    }
}

enum Outcome {
    Ok(&'static str),
    Mishap(String),
}

/// Execute one drawn pattern for `player`. Returns a status line.
fn execute(sig: &str, player: &str, srv: &dyn Server, stack: &mut Vec<Iota>) -> Outcome {
    match sig {
        // Mind's Reflection — push a reference to the caster.
        "qaq" => match uuid_of(player).and_then(|u| uuid::Uuid::parse_str(&u).ok()) {
            Some(u) => {
                stack.push(Iota::Entity(u));
                Outcome::Ok("Mind's Reflection")
            }
            None => Outcome::Mishap("the caster's mind is out of reach".into()),
        },

        // Compass' Purification — entity → its position.
        "aa" => match stack.pop() {
            Some(Iota::Entity(u)) => {
                match Entity::new(srv, u.to_string()).position() {
                    Some((x, y, z)) => {
                        stack.push(Iota::Vec3(x, y, z));
                        Outcome::Ok("Compass' Purification")
                    }
                    None => Outcome::Mishap("that entity is nowhere to be found".into()),
                }
            }
            Some(other) => {
                stack.push(other);
                Outcome::Mishap("expected an entity on the stack".into())
            }
            None => Outcome::Mishap("the stack is empty".into()),
        },

        // Alidade's Purification — entity → its look vector.
        "wa" => match stack.pop() {
            Some(Iota::Entity(u)) => {
                match Entity::new(srv, u.to_string()).look_vector() {
                    Some((x, y, z)) => {
                        stack.push(Iota::Vec3(x, y, z));
                        Outcome::Ok("Alidade's Purification")
                    }
                    None => Outcome::Mishap("that entity is nowhere to be found".into()),
                }
            }
            Some(other) => {
                stack.push(other);
                Outcome::Mishap("expected an entity on the stack".into())
            }
            None => Outcome::Mishap("the stack is empty".into()),
        },

        // Reveal — show the top iota without consuming it.
        "de" => match stack.last() {
            Some(top) => {
                srv.send_title(player, "", &display_iota(top), 2, 30, 8);
                Outcome::Ok("Reveal")
            }
            None => Outcome::Mishap("nothing to reveal".into()),
        },

        // Jester's Gambit — swap the top two iotas.
        "aawdd" => {
            let n = stack.len();
            if n >= 2 {
                stack.swap(n - 1, n - 2);
                Outcome::Ok("Jester's Gambit")
            } else {
                Outcome::Mishap("need two iotas to swap".into())
            }
        }

        // Charon's Gambit-ish: clear the stack (practical while developing).
        "dsw" => {
            stack.clear();
            Outcome::Ok("stack cleared")
        }

        _ => Outcome::Mishap(format!("the pattern `{sig}` fizzles")),
    }
}

/// Clear a player's stack (sneak + use staff). Pushes the empty view.
pub fn reset(player: &str, srv: &dyn Server) {
    if let Some(map) = STACKS.lock().unwrap().as_mut() {
        map.remove(player);
    }
    srv.send_to_player(player, crate::grid::CH_STACK, b"#ok\treset");
}

/// Entry point for the `hexcasting:cast` packet.
pub fn handle_cast(player: &str, payload: &[u8], srv: &dyn Server) {
    let serialized = String::from_utf8_lossy(payload);
    let Some(pattern) = HexPattern::from_serialized(&serialized) else {
        srv.send_actionbar(player, "the strokes make no sense");
        return;
    };
    let sig = angle_signature(&pattern);

    let mut guard = STACKS.lock().unwrap();
    let stack = guard
        .get_or_insert_with(HashMap::new)
        .entry(player.to_string())
        .or_default();

    // First line is a result marker (`#ok\tname` / `#mishap\tmsg`) so the
    // grid can colour the just-drawn stroke; the rest is the stack, top first.
    let marker = match execute(&sig, player, srv, stack) {
        Outcome::Ok(name) => {
            srv.send_actionbar(player, name);
            format!("#ok\t{name}")
        }
        Outcome::Mishap(msg) => {
            srv.send_actionbar(player, &format!("mishap: {msg}"));
            format!("#mishap\t{msg}")
        }
    };

    let mut lines = vec![marker];
    lines.extend(stack.iter().rev().map(display_iota));
    srv.send_to_player(player, crate::grid::CH_STACK, lines.join("\n").as_bytes());
}
