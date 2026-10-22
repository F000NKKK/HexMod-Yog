//! Patterns for other mods (`interop/…`, Hex Casting's `HexInterop`):
//! Pehkui's size of entities (`interop/pehkui`, `OpGetScale`, `OpSetScale`),
//! there only when Pehkui is loaded — its API reached by nanites.

use yog_api::nanite;
use yog_api::nanite::Nanite;

use crate::casting::actions::{Action, actions};
use crate::casting::args::{Ent, double_between};
use crate::casting::env::Spray;
use crate::casting::vm::Spell;
use crate::iota::Iota;

/// The mod id Hex Casting checks (`HexInterop.PEHKUI_ID`).
pub const PEHKUI: &str = "pehkui";

/// The game's entity, as Pehkui's API takes it.
#[nanite("net.minecraft.world.entity.Entity")]
pub struct Entity;

/// Pehkui's scale types (`ScaleTypes.BASE`: an entity's base size).
#[nanite("virtuoel.pehkui.api.ScaleTypes")]
pub struct ScaleTypes;

#[nanite("virtuoel.pehkui.api.ScaleType")]
pub struct ScaleType;

/// An entity's scale of one type.
#[nanite("virtuoel.pehkui.api.ScaleData")]
pub struct ScaleData;

#[nanite]
impl ScaleTypes {
    #[field]
    #[name("BASE")]
    pub fn base() -> Option<ScaleType>;
}

#[nanite]
impl ScaleType {
    #[name("getScaleData")]
    pub fn scale_data(&self, entity: &Entity) -> Option<ScaleData>;
}

#[nanite]
impl ScaleData {
    #[name("getScale")]
    pub fn scale(&self) -> f32;
    #[name("setScale")]
    pub fn set_scale(&self, scale: f32);
}

/// Entity `uuid`'s base scale data.
fn base_scale(uuid: &str) -> Option<ScaleData> {
    let entity = Entity::of(&yog_api::Entity::new(uuid));
    ScaleTypes::base()?.scale_data(&entity)
}

pub fn actions() -> Vec<Action> {
    if yog_api::mod_loaded(PEHKUI) {
        pehkui()
    } else {
        Vec::new()
    }
}

/// Pehkui's patterns (`HexActions`: `interop/pehkui/get|set`).
pub fn pehkui() -> Vec<Action> {
    actions! {
        "interop/pehkui/get" NW "aawawwawwa" => konst [env, e: Ent] {
            e.in_range(env)?;
            base_scale(e.uuid()).map_or(1.0, |s| f64::from(s.scale()))
        };
        "interop/pehkui/set" NE "ddwdwwdwwd" => spell [env, img; e: Ent, scale: Iota] {
            let scale = double_between(&scale, 0, 1.0 / 32.0, 8.0)?;
            e.in_range(env)?;
            let uuid = e.uuid().to_owned();
            Ok(Spell::new(50_000, vec![Spray::burst_n(e.pos(), scale, 40)], move |_, _| {
                if let Some(s) = base_scale(&uuid) {
                    s.set_scale(scale as f32);
                }
            }))
        };
    }
}
