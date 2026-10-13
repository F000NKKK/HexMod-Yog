//! Iotas — the values on a hex's stack (Hex Casting's `Iota` types).

use crate::casting::cont::Cont;
use crate::math::{HexPattern, Vec3};
use crate::text::{fmt_double, Text};
use crate::wire::{Reader, Writer};

/// How close two numbers (or vectors) must be to count as equal.
pub const TOLERANCE: f64 = 0.0001;

/// Hex Casting's `MAX_SERIALIZATION_TOTAL`: no iota (counting nested ones)
/// may be bigger than this.
pub const MAX_SERIALIZATION_TOTAL: usize = 1024;
/// Hex Casting's `MAX_SERIALIZATION_DEPTH`.
pub const MAX_SERIALIZATION_DEPTH: usize = 256;

/// An entity, by uuid; `name` is what it was called when the iota was
/// made (shown on the stack, and in mishap messages).
#[derive(Debug, Clone)]
pub struct EntityRef {
    pub uuid: String,
    pub name: String,
}

impl PartialEq for EntityRef {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Iota {
    Null,
    Garbage,
    Bool(bool),
    Double(f64),
    Vec3(Vec3),
    Entity(EntityRef),
    List(Vec<Iota>),
    Pattern(HexPattern),
    Continuation(Cont),
}

/// The kinds of iota, with their registry ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IotaType {
    Null,
    Garbage,
    Bool,
    Double,
    Vec3,
    Entity,
    List,
    Pattern,
    Continuation,
}

impl IotaType {
    /// Registry id path (`hexcasting:<id>`).
    pub fn id(self) -> &'static str {
        match self {
            IotaType::Null => "null",
            IotaType::Garbage => "garbage",
            IotaType::Bool => "boolean",
            IotaType::Double => "double",
            IotaType::Vec3 => "vec3",
            IotaType::Entity => "entity",
            IotaType::List => "list",
            IotaType::Pattern => "pattern",
            // (sic) — the original's lang key.
            IotaType::Continuation => "continution",
        }
    }

    /// The type's name (`hexcasting.iota.hexcasting:<id>`).
    pub fn name(self) -> Text {
        Text::key(format!("hexcasting.iota.hexcasting:{}", self.id()))
    }

    /// The type described for mishap messages ("a number").
    pub fn desc(self) -> Text {
        Text::key(format!("hexcasting.iota.hexcasting:{}.desc", self.id()))
    }

    /// Hex Casting's display colour (ARGB).
    pub fn color(self) -> u32 {
        match self {
            IotaType::Null => 0xFF_AAAAAA,
            IotaType::Garbage => 0xFF_505050,
            IotaType::Bool => 0xFF_AAFF55,
            IotaType::Double => 0xFF_55FF55,
            IotaType::Vec3 => 0xFF_FF3030,
            IotaType::Entity => 0xFF_55FFFF,
            IotaType::List => 0xFF_AAAAAA,
            IotaType::Pattern => 0xFF_FFAA00,
            IotaType::Continuation => 0xFF_CC0000,
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "null" => IotaType::Null,
            "garbage" => IotaType::Garbage,
            "boolean" => IotaType::Bool,
            "double" => IotaType::Double,
            "vec3" => IotaType::Vec3,
            "entity" => IotaType::Entity,
            "list" => IotaType::List,
            "pattern" => IotaType::Pattern,
            "continution" => IotaType::Continuation,
            _ => return None,
        })
    }
}

impl Iota {
    pub fn vec(x: f64, y: f64, z: f64) -> Self {
        Iota::Vec3(Vec3::new(x, y, z))
    }

    pub fn entity(uuid: impl Into<String>, name: impl Into<String>) -> Self {
        Iota::Entity(EntityRef { uuid: uuid.into(), name: name.into() })
    }

    pub fn kind(&self) -> IotaType {
        match self {
            Iota::Null => IotaType::Null,
            Iota::Garbage => IotaType::Garbage,
            Iota::Bool(_) => IotaType::Bool,
            Iota::Double(_) => IotaType::Double,
            Iota::Vec3(_) => IotaType::Vec3,
            Iota::Entity(_) => IotaType::Entity,
            Iota::List(_) => IotaType::List,
            Iota::Pattern(_) => IotaType::Pattern,
            Iota::Continuation(_) => IotaType::Continuation,
        }
    }

    /// Hex Casting's `isTruthy`.
    pub fn is_truthy(&self) -> bool {
        match self {
            Iota::Null | Iota::Garbage => false,
            Iota::Bool(b) => *b,
            Iota::Double(d) => *d != 0.0,
            Iota::Vec3(v) => *v != Vec3::ZERO,
            Iota::List(l) => !l.is_empty(),
            Iota::Entity(_) | Iota::Pattern(_) | Iota::Continuation(_) => true,
        }
    }

    /// Hex Casting's `Iota.tolerates`: equality as Equality Distillation
    /// sees it (numbers within [`TOLERANCE`], patterns by shape only).
    pub fn tolerates(&self, other: &Self) -> bool {
        match (self, other) {
            (Iota::Null, Iota::Null) | (Iota::Garbage, Iota::Garbage) => true,
            (Iota::Bool(a), Iota::Bool(b)) => a == b,
            (Iota::Double(a), Iota::Double(b)) => (a - b).abs() < TOLERANCE,
            (Iota::Vec3(a), Iota::Vec3(b)) => a.distance_sqr(*b) < TOLERANCE * TOLERANCE,
            (Iota::Entity(a), Iota::Entity(b)) => a.uuid == b.uuid,
            (Iota::List(a), Iota::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.tolerates(y))
            }
            (Iota::Pattern(a), Iota::Pattern(b)) => a.angles() == b.angles(),
            (Iota::Continuation(a), Iota::Continuation(b)) => a == b,
            _ => false,
        }
    }

    /// Whether drawing this iota does something (patterns do; everything
    /// else mishaps as "unescaped" when executed).
    pub fn executable(&self) -> bool {
        matches!(self, Iota::Pattern(_) | Iota::Continuation(_))
    }

    /// Iotas inside this one (for size checks and true-name scans).
    pub fn sub_iotas(&self) -> Option<Box<dyn Iterator<Item = &Iota> + '_>> {
        match self {
            Iota::List(l) => Some(Box::new(l.iter())),
            _ => None,
        }
    }

    /// How many iotas this is, nested ones included, and how deep it nests.
    pub fn size_and_depth(&self) -> (usize, usize) {
        match self {
            Iota::List(l) => {
                let mut total = 1;
                let mut depth = 0;
                for i in l {
                    let (t, d) = i.size_and_depth();
                    total += t;
                    depth = depth.max(d);
                }
                (total, depth + 1)
            }
            Iota::Continuation(c) => (1 + c.size(), 1),
            _ => (1, 0),
        }
    }

    /// Hex Casting's `isTooLargeToSerialize` for a whole stack.
    pub fn too_large(iotas: &[Iota]) -> bool {
        let mut total = 0;
        for i in iotas {
            let (t, d) = i.size_and_depth();
            total += t;
            if total > MAX_SERIALIZATION_TOTAL || d > MAX_SERIALIZATION_DEPTH {
                return true;
            }
        }
        false
    }

    /// How the iota is shown on the stack and in messages.
    pub fn display(&self) -> Text {
        match self {
            Iota::Null => Text::key("hexcasting.tooltip.null_iota"),
            Iota::Garbage => Text::key("hexcasting.spelldata.unknown"),
            Iota::Bool(true) => Text::key("hexcasting.tooltip.boolean_true"),
            Iota::Bool(false) => Text::key("hexcasting.tooltip.boolean_false"),
            Iota::Double(d) => Text::Lit(fmt_double(*d)),
            Iota::Vec3(v) => Text::Lit(fmt_vec(*v)),
            Iota::Entity(e) if e.name.is_empty() => Text::key("hexcasting.spelldata.entity.whoknows"),
            Iota::Entity(e) => Text::Lit(e.name.clone()),
            Iota::List(l) => Text::key_args(
                "hexcasting.tooltip.list_contents",
                vec![Text::Join(l.iter().map(Iota::display).collect(), ", ".into())],
            ),
            Iota::Pattern(p) => Text::key_args(
                "hexcasting.tooltip.pattern_iota",
                vec![Text::Lit(pattern_text(p))],
            ),
            Iota::Continuation(_) => Text::key("hexcasting.tooltip.jump_iota"),
        }
    }

    pub fn write(&self, w: &mut Writer) {
        match self {
            Iota::Null => {
                w.tag('n');
            }
            Iota::Garbage => {
                w.tag('g');
            }
            Iota::Bool(b) => {
                w.tag(if *b { 't' } else { 'f' });
            }
            Iota::Double(d) => {
                w.tag('d').num(*d);
            }
            Iota::Vec3(v) => {
                w.tag('v').num(v.x).num(v.y).num(v.z);
            }
            Iota::Entity(e) => {
                w.tag('e').str(&e.uuid).str(&e.name);
            }
            Iota::List(l) => {
                w.tag('l').int(l.len() as i64);
                for i in l {
                    i.write(w);
                }
            }
            Iota::Pattern(p) => {
                w.tag('p').str(&p.serialized_form());
            }
            Iota::Continuation(c) => {
                w.tag('c');
                c.write(w);
            }
        }
    }

    pub fn read(r: &mut Reader) -> Option<Self> {
        Some(match r.tag()? {
            'n' => Iota::Null,
            'g' => Iota::Garbage,
            't' => Iota::Bool(true),
            'f' => Iota::Bool(false),
            'd' => Iota::Double(r.num()?),
            'v' => Iota::Vec3(Vec3::new(r.num()?, r.num()?, r.num()?)),
            'e' => Iota::entity(r.str()?, r.str()?),
            'l' => {
                let n = r.int()?;
                let mut items = Vec::new();
                for _ in 0..n {
                    items.push(Iota::read(r)?);
                }
                Iota::List(items)
            }
            'p' => Iota::Pattern(HexPattern::from_serialized(r.str()?)?),
            'c' => Iota::Continuation(Cont::read(r)?),
            _ => return None,
        })
    }

    /// The iota as a self-contained string ([`crate::wire`]).
    pub fn serialize(&self) -> String {
        let mut w = Writer::new();
        self.write(&mut w);
        w.finish()
    }

    /// Unreadable data comes back as garbage, like Hex Casting's.
    pub fn deserialize(s: &str) -> Self {
        let mut r = Reader::new(s);
        Iota::read(&mut r).filter(|_| r.is_done()).unwrap_or(Iota::Garbage)
    }
}

/// `(x, y, z)` with Hex Casting's number format.
pub fn fmt_vec(v: Vec3) -> String {
    format!("({}, {}, {})", fmt_double(v.x), fmt_double(v.y), fmt_double(v.z))
}

/// `EAST qaq` — a pattern as Hex Casting's `displayNonInline` writes it.
pub fn pattern_text(p: &HexPattern) -> String {
    let sig = p.signature();
    if sig.is_empty() {
        p.start.name().to_owned()
    } else {
        format!("{} {}", p.start.name(), sig)
    }
}

/// A stack written as one string (top last, like the VM keeps it).
pub fn write_stack(w: &mut Writer, stack: &[Iota]) {
    w.int(stack.len() as i64);
    for i in stack {
        i.write(w);
    }
}

pub fn read_stack(r: &mut Reader) -> Option<Vec<Iota>> {
    let n = r.int()?;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(Iota::read(r)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::HexDir;

    #[test]
    fn iotas_round_trip() {
        let i = Iota::List(vec![
            Iota::Double(1.25),
            Iota::vec(1.0, -2.0, 3.5),
            Iota::entity("069a79f4-44e9-4726-a5be-fca90e38aaf5", "Notch, the \"first\""),
            Iota::List(vec![Iota::Null, Iota::Garbage, Iota::Bool(true)]),
            Iota::Pattern(HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap()),
        ]);
        assert_eq!(Iota::deserialize(&i.serialize()), i);
        assert_eq!(Iota::deserialize("nonsense"), Iota::Garbage);
    }

    #[test]
    fn tolerance_and_truth() {
        assert!(Iota::Double(1.0).tolerates(&Iota::Double(1.00001)));
        assert!(!Iota::Double(1.0).tolerates(&Iota::Bool(true)));
        let a = Iota::Pattern(HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap());
        let b = Iota::Pattern(HexPattern::from_angles("qaq", HexDir::West).unwrap());
        assert!(a.tolerates(&b));
        assert!(!Iota::List(vec![]).is_truthy());
        assert!(!Iota::Vec3(Vec3::ZERO).is_truthy());
        assert!(Iota::Double(-1.0).is_truthy());
    }
}
