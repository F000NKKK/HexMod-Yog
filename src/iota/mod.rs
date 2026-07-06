//! Iota types — the basic values in the HexCasting VM.

use crate::math::HexPattern;
use std::fmt;

/// Unique identifier for each iota type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IotaTypeId(u8);

impl IotaTypeId {
    pub const BOOLEAN: Self = IotaTypeId(0);
    pub const DOUBLE: Self = IotaTypeId(1);
    pub const ENTITY: Self = IotaTypeId(2);
    pub const LIST: Self = IotaTypeId(3);
    pub const NULL: Self = IotaTypeId(4);
    pub const PATTERN: Self = IotaTypeId(5);
    pub const VEC3: Self = IotaTypeId(6);
    pub const STRING: Self = IotaTypeId(7);
    pub const INT: Self = IotaTypeId(8);
}

/// Trait for iota type descriptors.
pub trait IotaType: fmt::Debug {
    fn id(&self) -> IotaTypeId;
    fn name(&self) -> &'static str;
    fn color(&self) -> u32;
    fn display(&self, payload: &dyn std::any::Any) -> String;
}

/// The core Iota enum — a dynamically typed value used in the casting VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Iota {
    Boolean(bool),
    Double(f64),
    Int(i64),
    String(String),
    Entity(uuid::Uuid),
    Vec3(f64, f64, f64),
    Pattern(HexPattern),
    List(Vec<Iota>),
    Null,
}

impl Iota {
    pub fn get_type(&self) -> IotaTypeId {
        match self {
            Iota::Boolean(_) => IotaTypeId::BOOLEAN,
            Iota::Double(_) => IotaTypeId::DOUBLE,
            Iota::Int(_) => IotaTypeId::INT,
            Iota::String(_) => IotaTypeId::STRING,
            Iota::Entity(_) => IotaTypeId::ENTITY,
            Iota::Vec3(_, _, _) => IotaTypeId::VEC3,
            Iota::Pattern(_) => IotaTypeId::PATTERN,
            Iota::List(_) => IotaTypeId::LIST,
            Iota::Null => IotaTypeId::NULL,
        }
    }

    /// Whether this iota is considered "truthy" in the VM.
    pub fn is_truthy(&self) -> bool {
        match self {
            Iota::Boolean(b) => *b,
            Iota::Null => false,
            Iota::Double(d) => *d != 0.0,
            Iota::Int(i) => *i != 0,
            Iota::List(l) => !l.is_empty(),
            Iota::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// Check if this iota tolerates (is equal to) another, within type constraints.
    pub fn tolerates(&self, other: &Self) -> bool {
        if self.get_type() != other.get_type() {
            return false;
        }
        match (self, other) {
            (Iota::Boolean(a), Iota::Boolean(b)) => a == b,
            (Iota::Double(a), Iota::Double(b)) => (a - b).abs() < 1e-9,
            (Iota::Int(a), Iota::Int(b)) => a == b,
            (Iota::String(a), Iota::String(b)) => a == b,
            (Iota::Entity(a), Iota::Entity(b)) => a == b,
            (Iota::Vec3(a1, a2, a3), Iota::Vec3(b1, b2, b3)) => {
                (a1 - b1).abs() < 1e-9 && (a2 - b2).abs() < 1e-9 && (a3 - b3).abs() < 1e-9
            }
            (Iota::Pattern(a), Iota::Pattern(b)) => a == b,
            (Iota::List(a), Iota::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.tolerates(y)),
            (Iota::Null, Iota::Null) => true,
            _ => false,
        }
    }

    /// Serialize this iota to a simple string representation (for Yog storage / NBT-like).
    pub fn serialize(&self) -> String {
        match self {
            Iota::Boolean(b) => format!("bool:{}", b),
            Iota::Double(d) => format!("double:{}", d),
            Iota::Int(i) => format!("int:{}", i),
            Iota::String(s) => format!("str:{}", s),
            Iota::Entity(u) => format!("entity:{}", u),
            Iota::Vec3(x, y, z) => format!("vec3:{} {} {}", x, y, z),
            Iota::Pattern(p) => format!("pattern:{}", p.serialized_form()),
            Iota::List(items) => {
                let inner = items.iter().map(|i| i.serialize()).collect::<Vec<_>>().join(",");
                format!("list:[{}]", inner)
            }
            Iota::Null => "null".to_string(),
        }
    }

    pub fn deserialize(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("bool:") {
            match rest {
                "true" => Some(Iota::Boolean(true)),
                "false" => Some(Iota::Boolean(false)),
                _ => None,
            }
        } else if let Some(rest) = s.strip_prefix("double:") {
            rest.parse::<f64>().ok().map(Iota::Double)
        } else if let Some(rest) = s.strip_prefix("int:") {
            rest.parse::<i64>().ok().map(Iota::Int)
        } else if let Some(rest) = s.strip_prefix("str:") {
            Some(Iota::String(rest.to_string()))
        } else if let Some(rest) = s.strip_prefix("entity:") {
            uuid::Uuid::parse_str(rest).ok().map(Iota::Entity)
        } else if let Some(rest) = s.strip_prefix("vec3:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 3 {
                let x = parts[0].parse::<f64>().ok()?;
                let y = parts[1].parse::<f64>().ok()?;
                let z = parts[2].parse::<f64>().ok()?;
                Some(Iota::Vec3(x, y, z))
            } else {
                None
            }
        } else if let Some(rest) = s.strip_prefix("pattern:") {
            HexPattern::from_serialized(rest).map(Iota::Pattern)
        } else if let Some(rest) = s.strip_prefix("list:[") {
            if let Some(inner) = rest.strip_suffix(']') {
                let items = inner.split(',').filter_map(Iota::deserialize).collect();
                Some(Iota::List(items))
            } else {
                None
            }
        } else if s == "null" {
            Some(Iota::Null)
        } else {
            None
        }
    }

    /// Human-readable display name for tooltips.
    pub fn display_name(&self) -> String {
        match self {
            Iota::Boolean(b) => format!("{}", b),
            Iota::Double(d) => format!("{}", d),
            Iota::Int(i) => format!("{}", i),
            Iota::String(s) => format!("\"{}\"", s),
            Iota::Entity(_) => "Entity".to_string(),
            Iota::Vec3(x, y, z) => format!("({}, {}, {})", x, y, z),
            Iota::Pattern(p) => format!("Pattern({})", p.serialized_form()),
            Iota::List(_) => "List".to_string(),
            Iota::Null => "Null".to_string(),
        }
    }
}

impl fmt::Display for Iota {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Registry of iota types — used for deserialization and display colors.
#[derive(Debug, Default)]
pub struct IotaTypeRegistry {
    types: Vec<&'static dyn IotaType>,
}

impl IotaTypeRegistry {
    pub fn register(&mut self, t: &'static dyn IotaType) {
        self.types.push(t);
    }

    pub fn get(&self, id: IotaTypeId) -> Option<&dyn IotaType> {
        self.types.iter().find(|t| t.id() == id).copied()
    }
}

#[derive(Debug)]
pub struct BoolIotaType;

impl IotaType for BoolIotaType {
    fn id(&self) -> IotaTypeId { IotaTypeId::BOOLEAN }
    fn name(&self) -> &'static str { "boolean" }
    fn color(&self) -> u32 { 0xff_ffff55 }
    fn display(&self, _payload: &dyn std::any::Any) -> String {
        let b = _payload.downcast_ref::<bool>().copied().unwrap_or(false);
        format!("{}", b)
    }
}

#[derive(Debug)]
pub struct DoubleIotaType;

impl IotaType for DoubleIotaType {
    fn id(&self) -> IotaTypeId { IotaTypeId::DOUBLE }
    fn name(&self) -> &'static str { "double" }
    fn color(&self) -> u32 { 0xff_55_55_ff }
    fn display(&self, _payload: &dyn std::any::Any) -> String {
        let d = _payload.downcast_ref::<f64>().copied().unwrap_or(0.0);
        format!("{}", d)
    }
}

#[derive(Debug)]
pub struct PatternIotaType;

impl IotaType for PatternIotaType {
    fn id(&self) -> IotaTypeId { IotaTypeId::PATTERN }
    fn name(&self) -> &'static str { "pattern" }
    fn color(&self) -> u32 { 0xff_ff_55_ff }
    fn display(&self, _payload: &dyn std::any::Any) -> String {
        "Pattern".to_string()
    }
}

/// Register all built-in iota types into a registry.
pub fn register_builtin_types(reg: &mut IotaTypeRegistry) {
    reg.register(&BoolIotaType);
    reg.register(&DoubleIotaType);
    reg.register(&PatternIotaType);
    // Additional types can be added here by other modules.
}