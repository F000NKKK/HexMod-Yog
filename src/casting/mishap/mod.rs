//! Mishaps — what goes wrong while casting, and what it does to the caster
//! (Hex Casting's `api/casting/mishaps/*`).

use crate::casting::env::{CastEnv, Spray};
use crate::iota::{EntityRef, Iota};
use crate::math::{HexPattern, Vec3};
use crate::protocol::ResolvedType;
use crate::res::Lang;
use crate::text::Text;

mod effects;
mod message;

/// A block position.
pub type BlockPos = (i32, i32, i32);

/// Where a mishap happened: the pattern and the action's name.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub pattern: Option<HexPattern>,
    pub name: Option<Text>,
}

/// An item stack as a mishap message shows it: count and name.
#[derive(Debug, Clone)]
pub struct ShownItem {
    pub count: u32,
    pub name: Text,
}

#[derive(Debug, Clone)]
pub enum Mishap {
    AlreadyBrainswept {
        mob: EntityRef,
        eye: Vec3,
    },
    BadBlock {
        pos: BlockPos,
        expected: Text,
    },
    BadBrainsweep {
        mob: EntityRef,
        pos: BlockPos,
    },
    BadCaster,
    BadEntity {
        entity: EntityRef,
        pos: Vec3,
        wanted: Text,
    },
    /// A dropped item entity that isn't the right item.
    BadItem {
        entity: EntityRef,
        item: Option<ShownItem>,
        wanted: Text,
    },
    BadLocation {
        pos: Vec3,
        kind: &'static str,
    },
    BadOffhandItem {
        item: Option<ShownItem>,
        wanted: Text,
    },
    DivideByZero {
        a: Text,
        b: Text,
        suffix: &'static str,
    },
    EntityTooFar {
        entity: EntityRef,
        pos: Vec3,
    },
    EvalTooMuch,
    ImmuneEntity {
        entity: EntityRef,
        pos: Vec3,
    },
    InvalidIota {
        perp: Iota,
        rev_idx: usize,
        expected: Text,
    },
    InvalidOperatorArgs(Vec<Iota>),
    InvalidPattern(Option<HexPattern>),
    LackingHotbarItem(Text),
    WrongDimension(String),
    NeedsParens,
    NoAkashicRecord(BlockPos),
    NoSpellCircle,
    /// `MishapBoolDirectrixEmptyStack`: the stack was empty at a boolean directrix.
    BoolDirectrixEmptyStack(BlockPos),
    /// `MishapBoolDirectrixNotBool`: what a boolean directrix found wasn't a bool.
    BoolDirectrixNotBool(Iota, BlockPos),
    NotEnoughArgs {
        expected: usize,
        got: usize,
    },
    NotEnoughMedia(i64),
    OthersName {
        name: String,
        is_self: bool,
    },
    StackSize,
    Unenlightened,
    Unescaped(Iota),
}

pub type MResult<T> = Result<T, Mishap>;

/// Minecraft's dye colors (`DyeColor` text colors), for mishap sprays.
pub mod dye;

/// A mishap's spray, this far above where it's sprayed from.
const SPRAY: Spray = Spray::look((0.0, 2.0, 0.0), 0.2, std::f64::consts::PI / 4.0, 40);
const SPRAY_ABOVE: f64 = 0.2;

fn error(stub: &str, args: Vec<Text>) -> Text {
    Text::key_args(format!("hexcasting.mishap.{stub}"), args)
}

fn pos_text(p: BlockPos) -> Text {
    // `BlockPos.toShortString()`
    Text::lit(format!("{}, {}, {}", p.0, p.1, p.2))
}

/// `Vec3Iota.display`.
fn vec_text(v: Vec3) -> Text {
    Iota::Vec3(v).display()
}

impl Mishap {
    // ── constructors, after the originals' companions ──────────────────────

    /// `MishapInvalidIota.ofType`.
    pub fn of_type(perp: &Iota, rev_idx: usize, name: &str) -> Self {
        Self::invalid(perp, rev_idx, &format!("class.{name}"), vec![])
    }

    /// `MishapInvalidIota.of`.
    pub fn invalid(perp: &Iota, rev_idx: usize, name: &str, args: Vec<Text>) -> Self {
        Mishap::InvalidIota {
            perp: perp.clone(),
            rev_idx,
            expected: Text::key_args(format!("hexcasting.mishap.invalid_value.{name}"), args),
        }
    }

    /// `MishapBadBlock.of`.
    pub fn bad_block(pos: BlockPos, stub: &str) -> Self {
        Mishap::BadBlock {
            pos,
            expected: Text::key(format!("hexcasting.mishap.bad_block.{stub}")),
        }
    }

    /// `MishapBadOffhandItem.of`.
    pub fn bad_offhand(item: Option<ShownItem>, stub: &str, args: Vec<Text>) -> Self {
        Mishap::BadOffhandItem {
            item,
            wanted: Text::key_args(format!("hexcasting.mishap.bad_item.{stub}"), args),
        }
    }

    /// `MishapLackingHotbarItem.of`.
    pub fn lacking_hotbar(stub: &str) -> Self {
        Mishap::LackingHotbarItem(Text::key(format!("hexcasting.mishap.bad_item.{stub}")))
    }

    /// `MishapBadEntity.of` (an item entity gets `MishapBadItem`).
    pub fn bad_entity(env: &CastEnv, entity: &EntityRef, stub: &str) -> Self {
        let wanted = Text::key(format!("hexcasting.mishap.bad_item.{stub}"));
        if let Some((id, count, _)) = yog_api::event::item::entity_stack(&entity.uuid)
            .map(|s| (s.item.to_string(), s.count, s.snbt))
        {
            let item = (count > 0).then(|| ShownItem {
                count,
                name: item_name(&id),
            });
            return Mishap::BadItem {
                entity: entity.clone(),
                item,
                wanted,
            };
        }
        let pos = env.entity_pos(entity).unwrap_or_default();
        Mishap::BadEntity {
            entity: entity.clone(),
            pos,
            wanted,
        }
    }

    /// `MishapDivideByZero.of(Iota, Iota, suffix)`.
    pub fn divide_by_zero(a: &Iota, b: &Iota, suffix: &'static str) -> Self {
        let b = if suffix == "exponent" {
            power_of(b)
        } else {
            translate_operand(b)
        };
        Mishap::DivideByZero {
            a: translate_operand(a),
            b,
            suffix,
        }
    }

    /// `MishapDivideByZero.tan`.
    pub fn tan(angle: f64) -> Self {
        let t = translate_operand(&Iota::Double(angle));
        Mishap::DivideByZero {
            a: Text::key_args(Lang::MISHAP_DIVIDE_BY_ZERO_SIN_KEY, vec![t.clone()]),
            b: Text::key_args(Lang::MISHAP_DIVIDE_BY_ZERO_COS_KEY, vec![t]),
            suffix: "divide",
        }
    }

    // ── behaviour ───────────────────────────────────────────────────────────

    pub fn resolution_type(&self) -> ResolvedType {
        match self {
            Mishap::InvalidPattern(_) | Mishap::Unenlightened => ResolvedType::Invalid,
            _ => ResolvedType::Errored,
        }
    }

    pub fn accent_color(&self) -> u32 {
        match self {
            Mishap::AlreadyBrainswept { .. } | Mishap::BadBrainsweep { .. } => dye::GREEN,
            Mishap::BadBlock { .. } => dye::LIME,
            Mishap::BadCaster | Mishap::DivideByZero { .. } | Mishap::NotEnoughMedia(_) => dye::RED,
            Mishap::Unenlightened => dye::RED,
            Mishap::BadEntity { .. }
            | Mishap::BadItem { .. }
            | Mishap::BadOffhandItem { .. }
            | Mishap::LackingHotbarItem(_) => dye::BROWN,
            Mishap::BadLocation { .. } | Mishap::WrongDimension(_) => dye::MAGENTA,
            Mishap::EntityTooFar { .. } => dye::PINK,
            Mishap::EvalTooMuch | Mishap::ImmuneEntity { .. } => dye::BLUE,
            Mishap::OthersName { .. } | Mishap::StackSize => dye::BLACK,
            Mishap::InvalidIota { .. } | Mishap::InvalidOperatorArgs(_) | Mishap::Unescaped(_) => {
                dye::GRAY
            }
            Mishap::InvalidPattern(_) => dye::YELLOW,
            Mishap::NeedsParens => dye::ORANGE,
            Mishap::NoAkashicRecord(_) => dye::PURPLE,
            Mishap::BoolDirectrixEmptyStack(_) | Mishap::BoolDirectrixNotBool(..) => dye::GRAY,
            Mishap::NoSpellCircle => dye::LIGHT_BLUE,
            Mishap::NotEnoughArgs { .. } => dye::LIGHT_GRAY,
        }
    }

    pub fn particle_spray(&self, env: &CastEnv) -> Spray {
        match self {
            Mishap::AlreadyBrainswept { eye, .. } => Spray::burst(*eye, 1.0),
            Mishap::BadBlock { pos, .. } | Mishap::BadBrainsweep { pos, .. } => {
                Spray::burst(Vec3::center_of(*pos), 1.0)
            }
            _ => Spray {
                pos: env.mishap_spray_pos() + Vec3::new(0.0, SPRAY_ABOVE, 0.0),
                ..SPRAY
            },
        }
    }

    /// `errorMessageWithName`: "<action>: <message>".
    pub fn message_with_name(&self, env: &CastEnv, ctx: &Context) -> Option<Text> {
        let msg = self.error_message(env)?;
        Some(match &ctx.name {
            Some(name) => Text::key_args(Lang::MISHAP_KEY, vec![name.clone(), msg]),
            None => msg,
        })
    }
}

fn entity_name(e: &EntityRef) -> Text {
    if e.name.is_empty() {
        Text::key(Lang::SPELLDATA_ENTITY_WHOKNOWS_KEY)
    } else {
        Text::lit(e.name.clone())
    }
}

/// An item's name from its id (`item.<ns>.<path>`).
pub fn item_name(id: &str) -> Text {
    Text::key(yog_api::yog_ids::lang_key(yog_api::yog_ids::kind::ITEM, id))
}

/// `MishapDivideByZero.translate`.
fn translate_operand(i: &Iota) -> Text {
    match i {
        Iota::Double(d) if *d == 0.0 => Text::key(Lang::MISHAP_DIVIDE_BY_ZERO_ZERO_KEY),
        Iota::Vec3(v) if *v == Vec3::ZERO => Text::key(Lang::MISHAP_DIVIDE_BY_ZERO_ZERO_VEC_KEY),
        other => other.display(),
    }
}

/// `MishapDivideByZero.powerOf`.
fn power_of(i: &Iota) -> Text {
    match i {
        Iota::Double(d) if *d == 0.0 => Text::key(Lang::MISHAP_DIVIDE_BY_ZERO_ZERO_POWER_KEY),
        other => Text::key_args(Lang::MISHAP_DIVIDE_BY_ZERO_POWER_KEY, vec![other.display()]),
    }
}

/// `MishapOthersName.getTrueNameFromDatum`: the first player (other than
/// `caster`) named in `datum`, breadth-first.
pub fn true_name_in(env: &CastEnv, datum: &Iota, caster: Option<&str>) -> Option<EntityRef> {
    let mut pool = std::collections::VecDeque::from([datum]);
    while let Some(d) = pool.pop_front() {
        if let Iota::Entity(e) = d
            && Some(e.uuid.as_str()) != caster
            && env.is_player(&e.uuid)
        {
            return Some(e.clone());
        }
        if let Iota::List(l) = d {
            pool.extend(l.iter());
        }
    }
    None
}
