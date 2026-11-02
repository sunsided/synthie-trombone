//! Noise sources: white noise, a band-pass biquad, and 1-D simplex noise
//! for the slow, organic pitch and tenseness wobble.

use crate::math::{cos, floor_i64, sin};
use core::f32::consts::PI;

/// Xorshift32 white noise. Small, fast and seedable, no allocation.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u32);

impl Rng {
    pub(crate) fn new(seed: u32) -> Self {
        // Zero is a fixed point of xorshift.
        Self(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform white noise in `[-1, 1)`.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn bipolar(&mut self) -> f32 {
        // 24 high bits map exactly onto the f32 mantissa.
        (self.next_u32() >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
}

/// Constant-peak-gain band-pass biquad (RBJ cookbook).
#[derive(Clone, Debug)]
pub(crate) struct BandPass {
    b0: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BandPass {
    pub(crate) fn new(f0: f32, q: f32, sample_rate: f32) -> Self {
        let w0 = 2.0 * PI * f0 / sample_rate;
        let alpha = sin(w0) / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            a1: -2.0 * cos(w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub(crate) fn process(&mut self, x: f32) -> f32 {
        // b1 = 0, b2 = -b0
        let y = self.b0 * (x - self.x2) - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub(crate) fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// 1-D simplex noise (after Stefan Gustavson), output roughly in `[-1, 1]`.
#[derive(Clone, Debug)]
pub(crate) struct Simplex {
    perm: [u8; 256],
}

impl Simplex {
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn new(rng: &mut Rng) -> Self {
        let mut perm = [0u8; 256];
        for (i, p) in perm.iter_mut().enumerate() {
            *p = i as u8;
        }
        for i in (1..256).rev() {
            let j = (rng.next_u32() % (i as u32 + 1)) as usize;
            perm.swap(i, j);
        }
        Self { perm }
    }

    fn grad(hash: u8, x: f32) -> f32 {
        let h = hash & 15;
        let g = 1.0 + f32::from(h & 7);
        if h & 8 == 0 { g * x } else { -g * x }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub(crate) fn sample(&self, x: f64) -> f32 {
        let i0 = floor_i64(x);
        let x0 = (x - i0 as f64) as f32;
        let x1 = x0 - 1.0;
        let h0 = self.perm[(i0 & 255) as usize];
        let h1 = self.perm[((i0 + 1) & 255) as usize];
        let t0 = 1.0 - x0 * x0;
        let t1 = 1.0 - x1 * x1;
        let n0 = t0 * t0 * t0 * t0 * Self::grad(h0, x0);
        let n1 = t1 * t1 * t1 * t1 * Self::grad(h1, x1);
        0.395 * (n0 + n1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_noise_is_bipolar_and_centred() {
        let mut rng = Rng::new(1);
        let mut sum = 0.0f64;
        for _ in 0..100_000 {
            let x = rng.bipolar();
            assert!((-1.0..1.0).contains(&x));
            sum += f64::from(x);
        }
        assert!((sum / 100_000.0).abs() < 0.01);
    }

    #[test]
    fn simplex_is_bounded_and_continuous() {
        let s = Simplex::new(&mut Rng::new(7));
        let mut prev = s.sample(0.0);
        for i in 1..20_000 {
            let v = s.sample(f64::from(i) * 0.001);
            assert!(v.abs() <= 1.0, "{v}");
            assert!((v - prev).abs() < 0.05, "jump at {i}");
            prev = v;
        }
    }
}
