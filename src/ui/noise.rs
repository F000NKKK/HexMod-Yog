//! The noise the zappy pattern lines wobble by: Minecraft's `SimplexNoise`
//! seeded with `SingleThreadedRandomSource(9001)` (Hex Casting's
//! `RenderLib.NOISE`), bit-for-bit, so the lines shake like the original's.

use std::sync::OnceLock;

/// `java.util.Random`'s LCG as Minecraft's `SingleThreadedRandomSource`
/// (`BitRandomSource` rules for `nextInt`/`nextDouble`).
struct JavaRandom(i64);

impl JavaRandom {
    const MULT: i64 = 0x5DEECE66D;
    const MASK: i64 = (1 << 48) - 1;

    fn new(seed: i64) -> Self {
        JavaRandom((seed ^ Self::MULT) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let i = self.next(31);
            let j = i % bound;
            if i.wrapping_sub(j).wrapping_add(bound - 1) >= 0 {
                return j;
            }
        }
    }

    fn next_double(&mut self) -> f64 {
        let i = self.next(26) as i64;
        let j = self.next(27) as i64;
        // `DOUBLE_MULTIPLIER` is the float literal `1.110223E-16F`.
        ((i << 27) + j) as f64 * (1.110223e-16_f32 as f64)
    }
}

const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

struct Simplex {
    p: [i32; 256],
}

impl Simplex {
    fn new(rand: &mut JavaRandom) -> Self {
        // The offsets (`xo`, `yo`, `zo`) only matter to the 2-D noise, but
        // drawing them advances the generator.
        for _ in 0..3 {
            rand.next_double();
        }
        let mut p = [0i32; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as i32;
        }
        for i in 0..256 {
            let j = rand.next_int(256 - i as i32) as usize;
            p.swap(i, i + j);
        }
        Simplex { p }
    }

    fn p(&self, i: i32) -> i32 {
        self.p[(i & 255) as usize]
    }

    fn corner(gi: i32, x: f64, y: f64, z: f64, off: f64) -> f64 {
        let mut d = off - x * x - y * y - z * z;
        if d < 0.0 {
            return 0.0;
        }
        d *= d;
        let g = GRADIENT[gi as usize];
        d * d * (g[0] as f64 * x + g[1] as f64 * y + g[2] as f64 * z)
    }

    fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        let s = (x + y + z) * (1.0 / 3.0);
        let i = (x + s).floor() as i32;
        let j = (y + s).floor() as i32;
        let k = (z + s).floor() as i32;
        let t = (i + j + k) as f64 * (1.0 / 6.0);
        let x0 = x - (i as f64 - t);
        let y0 = y - (j as f64 - t);
        let z0 = z - (k as f64 - t);
        let (i1, j1, k1, i2, j2, k2) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let g = 1.0 / 6.0;
        let (x1, y1, z1) = (x0 - i1 as f64 + g, y0 - j1 as f64 + g, z0 - k1 as f64 + g);
        let (x2, y2, z2) = (x0 - i2 as f64 + 2.0 * g, y0 - j2 as f64 + 2.0 * g, z0 - k2 as f64 + 2.0 * g);
        let (x3, y3, z3) = (x0 - 0.5, y0 - 0.5, z0 - 0.5);
        let (ii, jj, kk) = (i & 255, j & 255, k & 255);
        let g0 = self.p(ii + self.p(jj + self.p(kk))) % 12;
        let g1 = self.p(ii + i1 + self.p(jj + j1 + self.p(kk + k1))) % 12;
        let g2 = self.p(ii + i2 + self.p(jj + j2 + self.p(kk + k2))) % 12;
        let g3 = self.p(ii + 1 + self.p(jj + 1 + self.p(kk + 1))) % 12;
        32.0 * (Self::corner(g0, x0, y0, z0, 0.6)
            + Self::corner(g1, x1, y1, z1, 0.6)
            + Self::corner(g2, x2, y2, z2, 0.6)
            + Self::corner(g3, x3, y3, z3, 0.6))
    }
}

fn simplex() -> &'static Simplex {
    static NOISE: OnceLock<Simplex> = OnceLock::new();
    NOISE.get_or_init(|| Simplex::new(&mut JavaRandom::new(9001)))
}

/// `RenderLib.getNoise`: mostly within -0.5..0.5.
pub fn get_noise(x: f64, y: f64, z: f64) -> f64 {
    simplex().value(x * 0.6, y * 0.6, z * 0.6) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_java() {
        // new java.util.Random(42).nextInt() == -1170105035
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next(32), -1170105035);
        // new Random(42).nextInt(10) == 0
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(10), 0);
    }

    #[test]
    fn noise_is_smooth_and_small() {
        assert_eq!(get_noise(0.0, 0.0, 0.0), 0.0);
        let mut max: f64 = 0.0;
        for i in 0..200 {
            let v = get_noise(i as f64 * 0.37, 1337.0, i as f64 * 0.11);
            max = max.max(v.abs());
        }
        assert!(max > 0.05 && max <= 0.6, "{max}");
        let (a, b) = (get_noise(3.0, 4.0, 5.0), get_noise(3.001, 4.0, 5.0));
        assert!((a - b).abs() < 0.01);
    }
}
