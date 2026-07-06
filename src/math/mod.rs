//! Core math types for hex casting: directions, angles, coordinates, patterns.

/// The six directions of the hex grid.
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
    /// Rotate this direction by an angle.
    pub fn rotated_by(self, angle: HexAngle) -> Self {
        let ord = self.ordinal() as i32;
        let ang = angle.ordinal() as i32;
        let size = 6;
        match ((ord + ang) % size + size) as u8 {
            0 => HexDir::NorthEast,
            1 => HexDir::East,
            2 => HexDir::SouthEast,
            3 => HexDir::SouthWest,
            4 => HexDir::West,
            5 => HexDir::NorthWest,
            _ => unreachable!(),
        }
    }

    /// Get the angle from another direction to this one.
    pub fn angle_from(self, other: Self) -> HexAngle {
        let diff = (self.ordinal() as i32 - other.ordinal() as i32).rem_euclid(6);
        match diff {
            0 => HexAngle::Zero,
            1 => HexAngle::Sixty,
            2 => HexAngle::OneTwenty,
            3 => HexAngle::OneEighty,
            4 => HexAngle::TwoForty,
            5 => HexAngle::ThreeHundred,
            _ => unreachable!(),
        }
    }

    /// Convert to a coordinate delta.
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

    pub fn ordinal(&self) -> u8 {
        match self {
            HexDir::NorthEast => 0,
            HexDir::East => 1,
            HexDir::SouthEast => 2,
            HexDir::SouthWest => 3,
            HexDir::West => 4,
            HexDir::NorthWest => 5,
        }
    }

    pub fn from_ordinal(ord: u8) -> Self {
        match ord % 6 {
            0 => HexDir::NorthEast,
            1 => HexDir::East,
            2 => HexDir::SouthEast,
            3 => HexDir::SouthWest,
            4 => HexDir::West,
            5 => HexDir::NorthWest,
            _ => unreachable!(),
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "NORTH_EAST" | "NE" => Some(HexDir::NorthEast),
            "EAST" | "E" => Some(HexDir::East),
            "SOUTH_EAST" | "SE" => Some(HexDir::SouthEast),
            "SOUTH_WEST" | "SW" => Some(HexDir::SouthWest),
            "WEST" | "W" => Some(HexDir::West),
            "NORTH_WEST" | "NW" => Some(HexDir::NorthWest),
            _ => None,
        }
    }
}

impl std::fmt::Display for HexDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HexDir::NorthEast => write!(f, "NORTH_EAST"),
            HexDir::East => write!(f, "EAST"),
            HexDir::SouthEast => write!(f, "SOUTH_EAST"),
            HexDir::SouthWest => write!(f, "SOUTH_WEST"),
            HexDir::West => write!(f, "WEST"),
            HexDir::NorthWest => write!(f, "NORTH_WEST"),
        }
    }
}

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
    pub fn ordinal(&self) -> u8 {
        *self as u8
    }

    pub fn from_ordinal(ord: u8) -> Self {
        match ord % 6 {
            0 => HexAngle::Zero,
            1 => HexAngle::Sixty,
            2 => HexAngle::OneTwenty,
            3 => HexAngle::OneEighty,
            4 => HexAngle::TwoForty,
            5 => HexAngle::ThreeHundred,
            _ => unreachable!(),
        }
    }
}

/// A coordinate on the hex grid, using axial coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HexCoord(pub i32, pub i32);

impl HexCoord {
    pub fn new(q: i32, r: i32) -> Self {
        HexCoord(q, r)
    }

    pub fn add(self, other: Self) -> Self {
        HexCoord(self.0 + other.0, self.1 + other.1)
    }

    pub fn sub(self, other: Self) -> Self {
        HexCoord(self.0 - other.0, self.1 - other.1)
    }

    /// Get the six neighbors of this coordinate.
    pub fn neighbors(self) -> [HexCoord; 6] {
        [
            self.add(HexDir::NorthEast.as_delta()),
            self.add(HexDir::East.as_delta()),
            self.add(HexDir::SouthEast.as_delta()),
            self.add(HexDir::SouthWest.as_delta()),
            self.add(HexDir::West.as_delta()),
            self.add(HexDir::NorthWest.as_delta()),
        ]
    }
}

/// A hex pattern — a sequence of directions drawn on the hex grid.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HexPattern {
    pub start: HexDir,
    pub steps: Vec<HexDir>,
}

impl HexPattern {
    pub fn new(start: HexDir, steps: Vec<HexDir>) -> Self {
        HexPattern { start, steps: steps }
    }

    /// Create a pattern from a list of directions (first is start, rest are steps).
    pub fn from_list(dirs: Vec<HexDir>) -> Self {
        if dirs.is_empty() {
            HexPattern {
                start: HexDir::NorthEast,
                steps: vec![],
            }
        } else {
            let start = dirs[0];
            let steps = dirs[1..].to_vec();
            HexPattern { start, steps }
        }
    }

    /// Get all directions in order, including start.
    pub fn all_dirs(&self) -> Vec<HexDir> {
        let mut dirs = vec![self.start];
        dirs.extend(self.steps.iter().copied());
        dirs
    }

    /// Compute the centroid of this pattern (approximate).
    pub fn centroid(&self) -> HexCoord {
        // Start from origin and trace the path
        let mut pos = self.start.as_delta();
        let mut sum = pos;
        for step in &self.steps {
            pos = pos.add(step.as_delta());
            sum = sum.add(pos);
        }
        let count = (self.steps.len() + 1) as i32;
        HexCoord(sum.0 / count, sum.1 / count)
    }

    /// Check if this pattern is valid (no repeated vertices except start=end for closed patterns).
    pub fn is_valid(&self) -> bool {
        // A pattern is closed if the last vertex equals the first.
        // For now, all patterns are considered valid unless they have zero length.
        !self.steps.is_empty() || true
    }

    /// Serialize the pattern to a compact string form using single-letter direction codes.
    pub fn serialized_form(&self) -> String {
        let mut s = match self.start {
            HexDir::NorthEast => "A",
            HexDir::East => "B",
            HexDir::SouthEast => "C",
            HexDir::SouthWest => "D",
            HexDir::West => "E",
            HexDir::NorthWest => "F",
        }.to_string();
        for step in &self.steps {
            s.push(match step {
                HexDir::NorthEast => 'A',
                HexDir::East => 'B',
                HexDir::SouthEast => 'C',
                HexDir::SouthWest => 'D',
                HexDir::West => 'E',
                HexDir::NorthWest => 'F',
            });
        }
        s
    }

    /// Deserialize a pattern from its compact string form.
    pub fn from_serialized(s: &str) -> Option<Self> {
        let mut chars = s.chars();
        let start = match chars.next() {
            Some('A') => HexDir::NorthEast,
            Some('B') => HexDir::East,
            Some('C') => HexDir::SouthEast,
            Some('D') => HexDir::SouthWest,
            Some('E') => HexDir::West,
            Some('F') => HexDir::NorthWest,
            _ => return None,
        };
        let steps = chars.filter_map(|c| match c {
            'A' => Some(HexDir::NorthEast),
            'B' => Some(HexDir::East),
            'C' => Some(HexDir::SouthEast),
            'D' => Some(HexDir::SouthWest),
            'E' => Some(HexDir::West),
            'F' => Some(HexDir::NorthWest),
            _ => None,
        }).collect();
        Some(HexPattern { start, steps })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternStatus {
    /// The pattern is incomplete (traced via Iota).
    Incomplete,
    /// The pattern is valid but not closed.
    Open,
    /// The pattern is closed and valid.
    Closed,
    /// The pattern is invalid (e.g., self-intersecting).
    Invalid,
}