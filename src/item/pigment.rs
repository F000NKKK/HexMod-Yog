//! Pigments — the colours a caster's particles take (Hex Casting's
//! `FrozenPigment` and the `ColorProvider`s of its pigment items).

use crate::casting::mishap::dye;
use crate::math::Vec3;

/// A pigment item and its owner, as the original freezes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pigment {
    pub item: String,
    pub owner: String,
}

/// `FrozenPigment.DEFAULT`.
pub const DEFAULT_ITEM: &str = "hexcasting:default_colorizer";
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

impl Default for Pigment {
    fn default() -> Self {
        Pigment { item: DEFAULT_ITEM.into(), owner: NIL_UUID.into() }
    }
}

/// A pigment made from one dye colour (the mishap sprays).
pub fn dye_pigment(color: u32) -> Pigment {
    let name = DYES.iter().find(|(_, c)| *c == color).map_or("red", |(n, _)| n);
    Pigment { item: format!("hexcasting:dye_colorizer_{name}"), owner: NIL_UUID.into() }
}

const DYES: [(&str, u32); 16] = [
    ("white", dye::WHITE),
    ("orange", dye::ORANGE),
    ("magenta", dye::MAGENTA),
    ("light_blue", dye::LIGHT_BLUE),
    ("yellow", dye::YELLOW),
    ("lime", dye::LIME),
    ("pink", dye::PINK),
    ("gray", dye::GRAY),
    ("light_gray", dye::LIGHT_GRAY),
    ("cyan", dye::CYAN),
    ("purple", dye::PURPLE),
    ("blue", dye::BLUE),
    ("brown", dye::BROWN),
    ("green", dye::GREEN),
    ("red", dye::RED),
    ("black", dye::BLACK),
];

const PRIDE: [(&str, &[u32]); 16] = [
    ("agender", &[0x16a10c, 0xffffff, 0x7a8081, 0x302f30]),
    ("aroace", &[0x7210bc, 0xebf367, 0xffffff, 0x82dceb, 0x2f4dd8]),
    ("aromantic", &[0x16a10c, 0x82eb8b, 0xffffff, 0x7a8081, 0x302f30]),
    ("asexual", &[0x333233, 0x9a9fa1, 0xffffff, 0x7210bc]),
    ("bisexual", &[0xdb45ff, 0x9c2bd0, 0x6894d4]),
    ("demiboy", &[0x9a9fa1, 0xa9ffff, 0xffffff]),
    ("demigirl", &[0x9a9fa1, 0xfcb1ff, 0xffffff]),
    ("gay", &[0xd82f3a, 0xe0883f, 0xebf367, 0x2db418, 0x2f4dd8]),
    ("genderfluid", &[0xfbacf9, 0xffffff, 0x9c2bd0, 0x333233, 0x2f4dd8]),
    ("genderqueer", &[0xca78ef, 0xffffff, 0x2db418]),
    ("intersex", &[0xebf367, 0x7210bc]),
    ("lesbian", &[0xd82f3a, 0xefb87d, 0xffffff, 0xfbacf9, 0xa30262]),
    ("nonbinary", &[0xebf367, 0xffffff, 0x7210bc, 0x333233]),
    ("pansexual", &[0xe278ef, 0xebf367, 0x6ac2e4]),
    ("plural", &[0x30c69f, 0x347ddf, 0x6b3fbe, 0x000000]),
    ("transgender", &[0xeb92ea, 0xffffff, 0x6ac2e4]),
];

const ANCIENT: [u32; 5] = [0x54398a, 0xcfa0f3, 0xfecbe6, 0xcfa0f3, 0xe77c56];

/// Whether `item` is a pigment (`PigmentItem`).
pub fn is_pigment(item: &str) -> bool {
    provider(item).is_some() || item == "hexcasting:uuid_colorizer"
}

enum Provider {
    Solid(u32),
    /// `morphBetweenColors(colors, (0.1, 0.1, 0.1), time / period, pos)`.
    Morph(Vec<u32>, f32),
}

fn provider(item: &str) -> Option<Provider> {
    let path = item.strip_prefix("hexcasting:")?;
    Some(match path {
        "default_colorizer" => Provider::Solid(0xab65eb),
        "ancient_colorizer" => Provider::Morph(ANCIENT.to_vec(), 600.0),
        "uuid_colorizer" => return None, // needs the owner; see `color`
        _ => {
            if let Some(c) = path.strip_prefix("dye_colorizer_") {
                Provider::Solid(DYES.iter().find(|(n, _)| *n == c)?.1)
            } else {
                let t = path.strip_prefix("pride_colorizer_")?;
                Provider::Morph(PRIDE.iter().find(|(n, _)| *n == t)?.1.to_vec(), 400.0)
            }
        }
    })
}

impl Pigment {
    /// `ColorProvider.getColor`: the colour at `time` (ticks) for a particle
    /// heading along `pos`, with the original's minimum luminance.
    pub fn color(&self, time: f32, pos: Vec3) -> u32 {
        let raw = match provider(&self.item) {
            Some(Provider::Solid(c)) => c,
            Some(Provider::Morph(c, period)) => morph(&c, time / period, pos),
            None if self.item == "hexcasting:uuid_colorizer" => {
                morph(&uuid_colors(&self.owner), time / 400.0, pos)
            }
            // ColorProvider.MISSING
            None => 0xff00dc,
        };
        let (mut r, mut g, mut b) = ((raw >> 16) & 0xff, (raw >> 8) & 0xff, raw & 0xff);
        let lum = (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0;
        if lum < 0.05 {
            const WHEEL: [u32; 6] = [0x200000, 0x202000, 0x002000, 0x002020, 0x000020, 0x200020];
            let m = morph(&WHEEL, time / 20.0 / 20.0, pos);
            r += (m >> 16) & 0xff;
            g += (m >> 8) & 0xff;
            b += m & 0xff;
        }
        (r << 16) | (g << 8) | b
    }
}

/// `ADPigment.morphBetweenColors` (alpha dropped).
fn morph(colors: &[u32], time: f32, pos: Vec3) -> u32 {
    if colors.is_empty() {
        return 0;
    }
    let shift = (pos.x * 0.1 + pos.y * 0.1 + pos.z * 0.1) as f32;
    let f_idx = (time + shift).rem_euclid(1.0) * colors.len() as f32;
    let base = f_idx.floor() as usize;
    let t_raw = f_idx - base as f32;
    let t = if t_raw < 0.5 {
        4.0 * t_raw * t_raw * t_raw
    } else {
        1.0 - (-2.0 * t_raw + 2.0).powi(3) / 2.0
    };
    let start = colors[base % colors.len()];
    let end = colors[(base + 1) % colors.len()];
    let lerp = |shift: u32| {
        let a = ((start >> shift) & 0xff) as f32;
        let b = ((end >> shift) & 0xff) as f32;
        ((a + t * (b - a)) as u32).min(255)
    };
    (lerp(16) << 16) | (lerp(8) << 8) | lerp(0)
}

/// `ItemUUIDPigment`'s two colours "scrungled" from the owner's uuid
/// (`java.util.Random` seeded with `least ^ most`).
fn uuid_colors(owner: &str) -> Vec<u32> {
    let hex: String = owner.chars().filter(char::is_ascii_hexdigit).collect();
    let most = u64::from_str_radix(hex.get(..16).unwrap_or("0"), 16).unwrap_or(0);
    let least = u64::from_str_radix(hex.get(16..32).unwrap_or("0"), 16).unwrap_or(0);
    let mut rand = JavaRandom::new((least ^ most) as i64);
    let hue1 = rand.next_float();
    let sat1 = rand.next_float_in(0.4, 0.8);
    let bri1 = rand.next_float_in(0.7, 1.0);
    let hue2 = rand.next_float();
    let sat2 = rand.next_float_in(0.7, 1.0);
    let bri2 = rand.next_float_in(0.2, 0.7);
    vec![hsv_to_rgb(hue1, sat1, bri1), hsv_to_rgb(hue2, sat2, bri2)]
}

/// `java.util.Random`, for the colours the original derives from seeds.
pub struct JavaRandom(u64);

impl JavaRandom {
    const MULT: u64 = 0x5DEECE66D;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> Self {
        JavaRandom((seed as u64 ^ Self::MULT) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - bits)) as i64 as i32
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    /// `RandomGenerator.nextFloat(origin, bound)`.
    pub fn next_float_in(&mut self, origin: f32, bound: f32) -> f32 {
        let r = self.next_float() * (bound - origin) + origin;
        if r >= bound { f32::from_bits(bound.to_bits() - 1) } else { r }
    }

    /// `Random.nextInt(bound)`.
    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits - val + (bound - 1) >= 0 {
                return val;
            }
        }
    }
}

/// `Mth.hsvToRgb`.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let i = ((h * 6.0) as i32).rem_euclid(6);
    let f = h * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let c = |x: f32| ((x * 255.0) as i32).clamp(0, 255) as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches() {
        // new Random(0).nextInt() == -1155484576
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next(32), -1155484576);
    }

    #[test]
    fn colours() {
        let p = Pigment::default();
        assert_eq!(p.color(0.0, Vec3::ZERO), 0xab65eb);
        assert!(is_pigment("hexcasting:pride_colorizer_gay"));
        assert!(!is_pigment("minecraft:stick"));
        assert!(is_pigment("hexcasting:uuid_colorizer"));
        assert_eq!(dye_pigment(dye::RED).color(5.0, Vec3::ZERO), dye::RED);
    }
}
