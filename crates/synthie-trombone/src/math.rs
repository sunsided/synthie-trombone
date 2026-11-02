//! Float backend: `std` intrinsics or `libm`.

#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("synthie-trombone needs either the `std` or the `libm` feature");

#[cfg(feature = "std")]
mod imp {
    #[inline]
    pub fn sin(x: f32) -> f32 {
        x.sin()
    }
    #[inline]
    pub fn cos(x: f32) -> f32 {
        x.cos()
    }
    #[inline]
    pub fn exp(x: f32) -> f32 {
        x.exp()
    }
    #[inline]
    pub fn ln(x: f32) -> f32 {
        x.ln()
    }
    #[inline]
    pub fn powf(x: f32, y: f32) -> f32 {
        x.powf(y)
    }
    #[inline]
    pub fn sqrt(x: f32) -> f32 {
        x.sqrt()
    }
}

#[cfg(all(not(feature = "std"), feature = "libm"))]
mod imp {
    #[inline]
    pub fn sin(x: f32) -> f32 {
        libm::sinf(x)
    }
    #[inline]
    pub fn cos(x: f32) -> f32 {
        libm::cosf(x)
    }
    #[inline]
    pub fn exp(x: f32) -> f32 {
        libm::expf(x)
    }
    #[inline]
    pub fn ln(x: f32) -> f32 {
        libm::logf(x)
    }
    #[inline]
    pub fn powf(x: f32, y: f32) -> f32 {
        libm::powf(x, y)
    }
    #[inline]
    pub fn sqrt(x: f32) -> f32 {
        libm::sqrtf(x)
    }
}

pub(crate) use imp::*;

/// Linear interpolation between `a` and `b`.
#[inline]
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Move `current` towards `target`, rising by at most `up` and falling by at
/// most `down`.
#[inline]
pub(crate) fn move_towards(current: f32, target: f32, up: f32, down: f32) -> f32 {
    if current < target {
        (current + up).min(target)
    } else {
        (current - down).max(target)
    }
}

/// `floor` for `f64` without `std`.
#[inline]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(crate) fn floor_i64(x: f64) -> i64 {
    let i = x as i64;
    if x < i as f64 { i - 1 } else { i }
}

/// Frequency in Hz of a (fractional) MIDI note number.
#[must_use]
#[inline]
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * powf(2.0, (note - 69.0) / 12.0)
}
