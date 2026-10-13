//! Core math: hex-grid directions, angles, coordinates and patterns, and the
//! 3-D vectors spells work with.

use std::ops::{Add, Mul, Neg, Sub};

/// The six directions of the hex grid, clockwise from north-east (Hex
/// Casting's `HexDir` order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HexDir {
    NorthEast,
    East,
    SouthEast,
    SouthWest,
    West,
    NorthWest,
}

impl HexDir {
    pub const ALL: [HexDir; 6] = [
        HexDir::NorthEast,
        HexDir::East,
        HexDir::SouthEast,
        HexDir::SouthWest,
        HexDir::West,
        HexDir::NorthWest,
    ];

    /// This direction turned by `angle` (clockwise).
    pub fn rotated_by(self, angle: HexAngle) -> Self {
        Self::from_ordinal(self.ordinal() + angle.ordinal())
    }

    /// The turn that takes `other` to `self`.
    pub fn angle_from(self, other: Self) -> HexAngle {
        HexAngle::from_ordinal((self.ordinal() as i32 - other.ordinal() as i32).rem_euclid(6) as u8)
    }

    /// Axial coordinate step.
    pub fn as_delta(self) -> HexCoord {
        match self {
            HexDir::NorthEast => HexCoord(1, -1),
            HexDir::East => HexCoord(1, 0),
            HexDir::SouthEast => HexCoord(0, 1),
            HexDir::SouthWest => HexCoord(-1, 1),
            HexDir::West => HexCoord(-1, 0),
            HexDir::NorthWest => HexCoord(0, -1),
        }
    }

    pub fn ordinal(self) -> u8 {
        self as u8
    }

    pub fn from_ordinal(ord: u8) -> Self {
        Self::ALL[(ord % 6) as usize]
    }

    /// `NORTH_EAST`, `EAST`, … (what Hex Casting prints and serializes).
    pub fn name(self) -> &'static str {
        match self {
            HexDir::NorthEast => "NORTH_EAST",
            HexDir::East => "EAST",
            HexDir::SouthEast => "SOUTH_EAST",
            HexDir::SouthWest => "SOUTH_WEST",
            HexDir::West => "WEST",
            HexDir::NorthWest => "NORTH_WEST",
        }
    }

    fn letter(self) -> char {
        (b'A' + self.ordinal()) as char
    }

    fn from_letter(c: char) -> Option<Self> {
        match c {
            'A'..='F' => Some(Self::from_ordinal(c as u8 - b'A')),
            _ => None,
        }
    }
}

impl std::fmt::Display for HexDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A turn between two strokes, clockwise in sixths (Hex Casting's
/// `HexAngle`: FORWARD `w`, RIGHT `e`, RIGHT_BACK `d`, BACK `s`, LEFT_BACK
/// `a`, LEFT `q`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HexAngle {
    Zero = 0,
    Sixty = 1,
    OneTwenty = 2,
    OneEighty = 3,
    TwoForty = 4,
    ThreeHundred = 5,
}

impl HexAngle {
    pub fn ordinal(self) -> u8 {
        self as u8
    }

    pub fn from_ordinal(ord: u8) -> Self {
        match ord % 6 {
            0 => HexAngle::Zero,
            1 => HexAngle::Sixty,
            2 => HexAngle::OneTwenty,
            3 => HexAngle::OneEighty,
            4 => HexAngle::TwoForty,
            _ => HexAngle::ThreeHundred,
        }
    }

    pub fn letter(self) -> char {
        b"wedsaq"[self as usize] as char
    }

    pub fn from_letter(c: char) -> Option<Self> {
        "wedsaq".find(c).map(|i| Self::from_ordinal(i as u8))
    }
}

/// A point of the hex grid, in axial coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HexCoord(pub i32, pub i32);

impl Add for HexCoord {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        HexCoord(self.0 + other.0, self.1 + other.1)
    }
}

impl Sub for HexCoord {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        HexCoord(self.0 - other.0, self.1 - other.1)
    }
}

/// A pattern: the direction of its first stroke and of every stroke after it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HexPattern {
    pub start: HexDir,
    pub steps: Vec<HexDir>,
}

impl HexPattern {
    pub fn new(start: HexDir, steps: Vec<HexDir>) -> Self {
        HexPattern { start, steps }
    }

    /// A pattern from its strokes' directions (the first is the start).
    pub fn from_list(dirs: Vec<HexDir>) -> Self {
        match dirs.split_first() {
            Some((start, rest)) => HexPattern { start: *start, steps: rest.to_vec() },
            None => HexPattern { start: HexDir::NorthEast, steps: Vec::new() },
        }
    }

    /// Hex Casting's `HexPattern.fromAngles(signature, startDir)`.
    /// `None` if `sig` has a letter other than `wedsaq`.
    pub fn from_angles(sig: &str, start: HexDir) -> Option<Self> {
        let mut dir = start;
        let mut steps = Vec::with_capacity(sig.len());
        for c in sig.chars() {
            dir = dir.rotated_by(HexAngle::from_letter(c)?);
            steps.push(dir);
        }
        Some(HexPattern { start, steps })
    }

    /// Every stroke's direction, the start first.
    pub fn directions(&self) -> Vec<HexDir> {
        let mut dirs = Vec::with_capacity(self.steps.len() + 1);
        dirs.push(self.start);
        dirs.extend(self.steps.iter().copied());
        dirs
    }

    /// The turns between consecutive strokes.
    pub fn angles(&self) -> Vec<HexAngle> {
        let mut prev = self.start;
        self.steps
            .iter()
            .map(|&d| {
                let a = d.angle_from(prev);
                prev = d;
                a
            })
            .collect()
    }

    /// The angle signature, e.g. `"qaq"` for Mind's Reflection — what a
    /// pattern means, whatever way it was drawn.
    pub fn signature(&self) -> String {
        self.angles().into_iter().map(HexAngle::letter).collect()
    }

    /// The grid points the pattern visits, starting at the origin.
    pub fn positions(&self) -> Vec<HexCoord> {
        let mut at = HexCoord(0, 0);
        let mut out = vec![at];
        for d in self.directions() {
            at = at + d.as_delta();
            out.push(at);
        }
        out
    }

    /// Compact form, one letter (`A`–`F`) per stroke.
    pub fn serialized_form(&self) -> String {
        self.directions().into_iter().map(HexDir::letter).collect()
    }

    pub fn from_serialized(s: &str) -> Option<Self> {
        let dirs: Option<Vec<HexDir>> = s.chars().map(HexDir::from_letter).collect();
        let dirs = dirs?;
        if dirs.is_empty() {
            return None;
        }
        Some(Self::from_list(dirs))
    }
}

/// A 3-D vector (Minecraft's `Vec3`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    pub fn splat(v: f64) -> Self {
        Vec3::new(v, v, v)
    }

    pub fn tuple(self) -> (f64, f64, f64) {
        (self.x, self.y, self.z)
    }

    pub fn from_tuple((x, y, z): (f64, f64, f64)) -> Self {
        Vec3::new(x, y, z)
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length_sqr(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_sqr().sqrt()
    }

    /// Unit vector; the zero vector stays zero (like Minecraft's).
    pub fn normalize(self) -> Vec3 {
        let len = self.length();
        if len < 1.0e-4 { Vec3::ZERO } else { self * (1.0 / len) }
    }

    pub fn distance_sqr(self, o: Vec3) -> f64 {
        (self - o).length_sqr()
    }

    pub fn map(self, f: impl Fn(f64) -> f64) -> Vec3 {
        Vec3::new(f(self.x), f(self.y), f(self.z))
    }

    /// The block containing this point.
    pub fn block(self) -> (i32, i32, i32) {
        (self.x.floor() as i32, self.y.floor() as i32, self.z.floor() as i32)
    }

    /// The centre of block `(x, y, z)`.
    pub fn center_of((x, y, z): (i32, i32, i32)) -> Vec3 {
        Vec3::new(x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5)
    }

    /// Minecraft's `Direction.getNearest` as a unit vector.
    pub fn nearest_axis(self) -> Vec3 {
        const AXES: [Vec3; 6] = [
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        ];
        let mut best = AXES[2];
        let mut best_dot = f64::MIN;
        for a in AXES {
            let d = self.dot(a);
            if d > best_dot {
                best_dot = d;
                best = a;
            }
        }
        best
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_round_trip() {
        let p = HexPattern::from_angles("qaq", HexDir::NorthEast).unwrap();
        assert_eq!(p.signature(), "qaq");
        assert_eq!(p.start, HexDir::NorthEast);
        assert_eq!(p.steps[0], HexDir::NorthWest);
        let back = HexPattern::from_serialized(&p.serialized_form()).unwrap();
        assert_eq!(back, p);
        assert!(HexPattern::from_angles("qxq", HexDir::East).is_none());
    }

    #[test]
    fn vectors() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(a.cross(b), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::new(0.2, -3.0, 1.0).nearest_axis(), Vec3::new(0.0, -1.0, 0.0));
    }
}
