//! Patterns for other mods (`interop/…`, Hex Casting's `HexInterop`):
//! Pehkui's size of entities (`interop/pehkui`, `OpGetScale`, `OpSetScale`),
//! there only when Pehkui is loaded.

use yog_api::yog_data::{Ref, Value};

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Ent, double_between};
use crate::casting::env::Spray;
use crate::casting::vm::Spell;
use crate::iota::Iota;

/// The mod id Hex Casting checks (`HexInterop.PEHKUI_ID`).
pub const PEHKUI: &str = "pehkui";

/// Pehkui's base scale of an entity, by its API (`ScaleTypes.BASE`).
const SCALE: &str = "virtuoel.pehkui.api.ScaleTypes.BASE.getScaleData($0)";

pub fn actions() -> Vec<Action> {
    if yog_api::mod_loaded(PEHKUI) {
        pehkui()
    } else {
        Vec::new()
    }
}

fn entity(uuid: &str) -> Value {
    Value::Ref(Ref::new("entity", uuid))
}

/// Pehkui's patterns (`HexActions`: `interop/pehkui/get|set`).
pub fn pehkui() -> Vec<Action> {
    actions! {
        "interop/pehkui/get" NW "aawawwawwa" => konst [env, e: Ent] {
            e.in_range(env)?;
            yog_api::java::call(&format!("{SCALE}.getScale()"), &[entity(e.uuid())]).and_then(|v| v.as_f64()).unwrap_or(1.0)
        };
        "interop/pehkui/set" NE "ddwdwwdwwd" => spell [env, img; e: Ent, scale: Iota] {
            let scale = double_between(&scale, 0, 1.0 / 32.0, 8.0)?;
            e.in_range(env)?;
            let uuid = e.uuid().to_owned();
            Ok(Spell::new(50_000, vec![Spray::burst_n(e.pos(), scale, 40)], move |_, _| {
                yog_api::java::call(&format!("{SCALE}.setScale($1)"), &[entity(&uuid), Value::Float(scale)]);
            }))
        };
    }
}
