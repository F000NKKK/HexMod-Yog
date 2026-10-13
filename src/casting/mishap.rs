//! Mishaps — what goes wrong while casting, and what it does to the caster
//! (Hex Casting's `api/casting/mishaps/*`).

use crate::casting::env::{CastEnv, Spray};
use crate::iota::{EntityRef, Iota, IotaType};
use crate::math::{HexPattern, Vec3};
use crate::protocol::ResolvedType;
use crate::text::Text;

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
    AlreadyBrainswept { mob: EntityRef, eye: Vec3 },
    BadBlock { pos: BlockPos, expected: Text },
    BadBrainsweep { mob: EntityRef, pos: BlockPos },
    BadCaster,
    BadEntity { entity: EntityRef, pos: Vec3, wanted: Text },
    /// A dropped item entity that isn't the right item.
    BadItem { entity: EntityRef, item: Option<ShownItem>, wanted: Text },
    BadLocation { pos: Vec3, kind: &'static str },
    BadOffhandItem { item: Option<ShownItem>, wanted: Text },
    DivideByZero { a: Text, b: Text, suffix: &'static str },
    EntityTooFar { entity: EntityRef, pos: Vec3 },
    EvalTooMuch,
    ImmuneEntity { entity: EntityRef, pos: Vec3 },
    Internal(String),
    InvalidIota { perp: Iota, rev_idx: usize, expected: Text },
    InvalidOperatorArgs(Vec<Iota>),
    InvalidPattern(Option<HexPattern>),
    LackingHotbarItem(Text),
    WrongDimension(String),
    NeedsParens,
    NoAkashicRecord(BlockPos),
    NotEnoughArgs { expected: usize, got: usize },
    NotEnoughMedia(i64),
    OthersName { name: String, is_self: bool },
    StackSize,
    Unenlightened,
    Unescaped(Iota),
}

pub type MResult<T> = Result<T, Mishap>;

/// Minecraft's dye colours (`DyeColor` text colours), for mishap sprays.
pub mod dye {
    pub const WHITE: u32 = 0xF9FFFE;
    pub const ORANGE: u32 = 0xF9801D;
    pub const MAGENTA: u32 = 0xC74EBD;
    pub const LIGHT_BLUE: u32 = 0x3AB3DA;
    pub const YELLOW: u32 = 0xFED83D;
    pub const LIME: u32 = 0x80C71F;
    pub const PINK: u32 = 0xF38BAA;
    pub const GRAY: u32 = 0x474F52;
    pub const LIGHT_GRAY: u32 = 0x9D9D97;
    pub const CYAN: u32 = 0x169C9C;
    pub const PURPLE: u32 = 0x8932B8;
    pub const BLUE: u32 = 0x3C44AA;
    pub const BROWN: u32 = 0x835432;
    pub const GREEN: u32 = 0x5E7C16;
    pub const RED: u32 = 0xB02E26;
    pub const BLACK: u32 = 0x1D1D21;
}

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
        Mishap::BadBlock { pos, expected: Text::key(format!("hexcasting.mishap.bad_block.{stub}")) }
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
        if let Some((id, count, _)) = env.srv.item_entity_stack(&entity.uuid) {
            let item = (count > 0).then(|| ShownItem { count, name: item_name(&id) });
            return Mishap::BadItem { entity: entity.clone(), item, wanted };
        }
        let pos = env.entity_pos(entity).unwrap_or_default();
        Mishap::BadEntity { entity: entity.clone(), pos, wanted }
    }

    /// `MishapDivideByZero.of(Iota, Iota, suffix)`.
    pub fn divide_by_zero(a: &Iota, b: &Iota, suffix: &'static str) -> Self {
        let b = if suffix == "exponent" { power_of(b) } else { translate_operand(b) };
        Mishap::DivideByZero { a: translate_operand(a), b, suffix }
    }

    /// `MishapDivideByZero.tan`.
    pub fn tan(angle: f64) -> Self {
        let t = translate_operand(&Iota::Double(angle));
        Mishap::DivideByZero {
            a: Text::key_args("hexcasting.mishap.divide_by_zero.sin", vec![t.clone()]),
            b: Text::key_args("hexcasting.mishap.divide_by_zero.cos", vec![t]),
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
        use dye::*;
        match self {
            Mishap::AlreadyBrainswept { .. } | Mishap::BadBrainsweep { .. } => GREEN,
            Mishap::BadBlock { .. } => LIME,
            Mishap::BadCaster | Mishap::DivideByZero { .. } | Mishap::NotEnoughMedia(_) => RED,
            Mishap::Unenlightened => RED,
            Mishap::BadEntity { .. }
            | Mishap::BadItem { .. }
            | Mishap::BadOffhandItem { .. }
            | Mishap::LackingHotbarItem(_) => BROWN,
            Mishap::BadLocation { .. } | Mishap::WrongDimension(_) => MAGENTA,
            Mishap::EntityTooFar { .. } => PINK,
            Mishap::EvalTooMuch | Mishap::ImmuneEntity { .. } => BLUE,
            Mishap::Internal(_) | Mishap::OthersName { .. } | Mishap::StackSize => BLACK,
            Mishap::InvalidIota { .. } | Mishap::InvalidOperatorArgs(_) | Mishap::Unescaped(_) => GRAY,
            Mishap::InvalidPattern(_) => YELLOW,
            Mishap::NeedsParens => ORANGE,
            Mishap::NoAkashicRecord(_) => PURPLE,
            Mishap::NotEnoughArgs { .. } => LIGHT_GRAY,
        }
    }

    pub fn particle_spray(&self, env: &CastEnv) -> Spray {
        match self {
            Mishap::AlreadyBrainswept { eye, .. } => Spray::burst(*eye, 1.0),
            Mishap::BadBlock { pos, .. } | Mishap::BadBrainsweep { pos, .. } => {
                Spray::burst(Vec3::center_of(*pos), 1.0)
            }
            _ => Spray {
                pos: env.mishap_spray_pos() + Vec3::new(0.0, 0.2, 0.0),
                vel: Vec3::new(0.0, 2.0, 0.0),
                fuzziness: 0.2,
                spread: std::f64::consts::PI / 4.0,
                count: 40,
            },
        }
    }

    /// The effect on the world and the stack (no sound or particles).
    pub fn execute(&self, env: &mut CastEnv, ctx: &Context, stack: &mut Vec<Iota>) {
        match self {
            Mishap::AlreadyBrainswept { mob, .. } => {
                let health = env.srv.entity_health(&mob.uuid).unwrap_or(0.0);
                let caster = env.caster_uuid().unwrap_or_default();
                env.srv.entity_damage(&mob.uuid, health, "hexcasting:overcast", &caster);
            }
            Mishap::BadBlock { pos, .. } => {
                let p = Vec3::center_of(*pos);
                env.srv.explode(&env.dim, p.x, p.y, p.z, 0.25, false, "");
            }
            Mishap::BadBrainsweep { mob, .. } => env.truly_hurt(&mob.uuid, 1.0),
            Mishap::BadCaster | Mishap::Internal(_) | Mishap::Unescaped(_) => {}
            Mishap::BadEntity { pos, .. }
            | Mishap::EntityTooFar { pos, .. }
            | Mishap::ImmuneEntity { pos, .. } => env.yeet_held_items_towards(*pos),
            Mishap::BadLocation { pos, .. } => env.yeet_held_items_towards(*pos),
            Mishap::BadItem { entity, .. } => {
                let r = crate::casting::env::random();
                env.srv.entity_add_velocity(&entity.uuid, (r - 0.5) * 0.05, 0.75, (r * 7.0 % 1.0 - 0.5) * 0.05);
            }
            Mishap::BadOffhandItem { .. } | Mishap::LackingHotbarItem(_) => env.drop_held_items(),
            Mishap::DivideByZero { .. } => {
                stack.push(Iota::Garbage);
                env.mishap_damage(0.5);
            }
            Mishap::EvalTooMuch => env.drown(),
            Mishap::InvalidIota { rev_idx, .. } => {
                if let Some(i) = stack.len().checked_sub(1 + rev_idx) {
                    stack[i] = Iota::Garbage;
                }
            }
            Mishap::InvalidOperatorArgs(perps) => {
                for i in 0..perps.len() {
                    if let Some(j) = stack.len().checked_sub(1 + i) {
                        stack[j] = Iota::Garbage;
                    }
                }
            }
            Mishap::InvalidPattern(_) | Mishap::WrongDimension(_) => stack.push(Iota::Garbage),
            Mishap::NeedsParens => {
                if let Some(p) = &ctx.pattern {
                    stack.push(Iota::Pattern(p.clone()));
                }
            }
            Mishap::NoAkashicRecord(_) => env.remove_xp(100),
            Mishap::NotEnoughArgs { expected, got } => {
                for _ in *got..*expected {
                    stack.push(Iota::Garbage);
                }
            }
            Mishap::NotEnoughMedia(cost) => {
                env.extract_media(*cost, false);
            }
            Mishap::OthersName { is_self, .. } => {
                let seconds = if *is_self { 5 } else { 60 };
                env.blind(seconds * 20);
            }
            Mishap::StackSize => {
                stack.clear();
                stack.push(Iota::Garbage);
            }
            Mishap::Unenlightened => {
                env.drop_held_items();
                env.print_message(&Text::key("hexcasting.message.cant_great_spell"));
                let p = env.mishap_spray_pos();
                env.srv.play_sound(&env.dim, p.x, p.y, p.z, "minecraft:block.glass.break", 0.5, 0.7);
                // HexAdvancementTriggers.FAIL_GREAT_SPELL_TRIGGER: unlocks
                // overcasting ("Blind Diversion").
                env.grant_flag(crate::casting::env::FLAG_CAN_OVERCAST);
            }
        }
    }

    /// The message, without the action's name.
    fn error_message(&self, env: &CastEnv) -> Option<Text> {
        Some(match self {
            Mishap::AlreadyBrainswept { .. } => error("already_brainswept", vec![]),
            Mishap::BadBlock { pos, expected } => {
                error("bad_block", vec![expected.clone(), pos_text(*pos), env.block_name(*pos)])
            }
            Mishap::BadBrainsweep { pos, .. } => error("bad_brainsweep", vec![env.block_name(*pos)]),
            Mishap::BadCaster => error("bad_caster", vec![]),
            Mishap::BadEntity { entity, wanted, .. } => {
                error("bad_entity", vec![wanted.clone(), entity_name(entity)])
            }
            Mishap::BadItem { item: None, wanted, .. } => error("no_item", vec![wanted.clone()]),
            Mishap::BadItem { item: Some(i), wanted, .. } => {
                error("bad_item", vec![wanted.clone(), Text::int(i.count as i64), i.name.clone()])
            }
            Mishap::BadLocation { pos, kind } => error(&format!("location_{kind}"), vec![vec_text(*pos)]),
            Mishap::BadOffhandItem { item: Some(i), wanted } => error(
                "bad_item.offhand",
                vec![wanted.clone(), Text::int(i.count as i64), i.name.clone()],
            ),
            Mishap::BadOffhandItem { item: None, wanted } => {
                error("no_item.offhand", vec![wanted.clone()])
            }
            Mishap::DivideByZero { a, b, suffix } => {
                error(&format!("divide_by_zero.{suffix}"), vec![a.clone(), b.clone()])
            }
            Mishap::EntityTooFar { entity, .. } => error("entity_too_far", vec![entity_name(entity)]),
            Mishap::EvalTooMuch => error("eval_too_much", vec![]),
            Mishap::ImmuneEntity { entity, .. } => error("immune_entity", vec![entity_name(entity)]),
            Mishap::Internal(e) => error("unknown", vec![Text::lit(e.clone())]),
            Mishap::InvalidIota { perp, rev_idx, expected } => error(
                "invalid_value",
                vec![expected.clone(), Text::int(*rev_idx as i64), perp.kind().desc(), perp.display()],
            ),
            Mishap::InvalidOperatorArgs(perps) if perps.len() == 1 => {
                error("invalid_operator_args.one", vec![Text::int(0), perps[0].display()])
            }
            Mishap::InvalidOperatorArgs(perps) => error(
                "invalid_operator_args.many",
                vec![
                    Text::int(perps.len() as i64),
                    Text::int(0),
                    Text::int(perps.len() as i64 - 1),
                    Text::Join(perps.iter().map(Iota::display).collect(), ", ".into()),
                ],
            ),
            Mishap::InvalidPattern(None) => error("invalid_pattern_generic", vec![]),
            Mishap::InvalidPattern(Some(p)) => {
                error("invalid_pattern", vec![Iota::Pattern(p.clone()).display()])
            }
            Mishap::LackingHotbarItem(wanted) => error("bad_item.hotbar", vec![wanted.clone()]),
            Mishap::WrongDimension(proper) => {
                error("wrong_dimension", vec![Text::lit(proper.clone()), Text::lit(env.dim.clone())])
            }
            Mishap::NeedsParens => error("needs_parens", vec![]),
            Mishap::NoAkashicRecord(pos) => error("no_akashic_record", vec![pos_text(*pos)]),
            Mishap::NotEnoughArgs { expected, got: 0 } => {
                error("no_args", vec![Text::int(*expected as i64)])
            }
            Mishap::NotEnoughArgs { expected, got } => {
                error("not_enough_args", vec![Text::int(*expected as i64), Text::int(*got as i64)])
            }
            Mishap::NotEnoughMedia(_) => Text::key("hexcasting.message.cant_overcast"),
            Mishap::OthersName { is_self: true, .. } => error("others_name.self", vec![]),
            Mishap::OthersName { name, .. } => error("others_name", vec![Text::lit(name.clone())]),
            Mishap::StackSize => error("stack_size", vec![]),
            Mishap::Unenlightened => return None,
            Mishap::Unescaped(perp) => error("unescaped", vec![perp.display()]),
        })
    }

    /// `errorMessageWithName`: "<action>: <message>".
    pub fn message_with_name(&self, env: &CastEnv, ctx: &Context) -> Option<Text> {
        let msg = self.error_message(env)?;
        Some(match &ctx.name {
            Some(name) => Text::key_args("hexcasting.mishap", vec![name.clone(), msg]),
            None => msg,
        })
    }
}

fn entity_name(e: &EntityRef) -> Text {
    if e.name.is_empty() {
        Text::key("hexcasting.spelldata.entity.whoknows")
    } else {
        Text::lit(e.name.clone())
    }
}

/// An item's name from its id (`item.<ns>.<path>`).
pub fn item_name(id: &str) -> Text {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    Text::key(format!("item.{ns}.{}", path.replace('/', ".")))
}

/// `MishapDivideByZero.translate`.
fn translate_operand(i: &Iota) -> Text {
    match i {
        Iota::Double(d) if *d == 0.0 => Text::key("hexcasting.mishap.divide_by_zero.zero"),
        Iota::Vec3(v) if *v == Vec3::ZERO => Text::key("hexcasting.mishap.divide_by_zero.zero.vec"),
        other => other.display(),
    }
}

/// `MishapDivideByZero.powerOf`.
fn power_of(i: &Iota) -> Text {
    match i {
        Iota::Double(d) if *d == 0.0 => Text::key("hexcasting.mishap.divide_by_zero.zero.power"),
        other => Text::key_args("hexcasting.mishap.divide_by_zero.power", vec![other.display()]),
    }
}

/// `MishapOthersName.getTrueNameFromDatum`: the first player (other than
/// `caster`) named in `datum`, breadth-first.
pub fn true_name_in(env: &CastEnv, datum: &Iota, caster: Option<&str>) -> Option<EntityRef> {
    let mut pool = std::collections::VecDeque::from([datum]);
    while let Some(d) = pool.pop_front() {
        if let Iota::Entity(e) = d {
            if Some(e.uuid.as_str()) != caster && env.is_player(&e.uuid) {
                return Some(e.clone());
            }
        }
        if let Iota::List(l) = d {
            pool.extend(l.iter());
        }
    }
    None
}

/// The type name `ofType` would use for an iota type.
pub fn class_name(t: IotaType) -> &'static str {
    match t {
        IotaType::Double => "double",
        IotaType::Bool => "boolean",
        IotaType::Vec3 => "vector",
        IotaType::List => "list",
        IotaType::Pattern => "pattern",
        IotaType::Continuation => "continuation",
        IotaType::Garbage => "garbage",
        IotaType::Null => "null",
        IotaType::Entity => "entity",
    }
}
