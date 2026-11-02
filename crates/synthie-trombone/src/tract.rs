//! Vocal tract: a 44-cell Kelly–Lochbaum waveguide with a 28-cell nasal
//! branch, plus the articulation model that shapes it (tongue, constrictions,
//! velum).
//!
//! The waveguide runs at twice [`INTERNAL_RATE`](crate::INTERNAL_RATE).
//! Everything here is fixed-size: no allocation, ever.

use crate::INTERNAL_RATE;
use crate::math::{cos, lerp, move_towards, powf};
use core::f32::consts::PI;

/// Cells in the oral tract, glottis (0) to lips (`N - 1`).
pub const N: usize = 44;
/// Cells in the nasal tract.
pub const NOSE_LEN: usize = 28;
/// Cell where the nasal tract branches off (the velum).
pub const NOSE_START: usize = N - NOSE_LEN + 1;
/// First cell of the tongue blade.
pub const BLADE_START: usize = 10;
/// First cell of the tongue tip.
pub const TIP_START: usize = 32;
/// First cell of the lips.
pub const LIP_START: usize = 39;
/// Number of simultaneous constrictions.
pub const MAX_CONSTRICTIONS: usize = 4;

/// Valid tongue body positions (cell index, back to front).
pub const TONGUE_INDEX: core::ops::RangeInclusive<f32> = 12.0..=29.0;
/// Valid tongue body heights (diameter under the tongue; small is high).
pub const TONGUE_DIAMETER: core::ops::RangeInclusive<f32> = 2.05..=3.5;

const GLOTTAL_REFLECTION: f32 = 0.75;
const LIP_REFLECTION: f32 = -0.85;
const DAMPING: f32 = 0.999;
/// The original's articulator speed, diameter units per second.
pub(crate) const MOVEMENT_SPEED: f32 = 15.0;
const GRID_OFFSET: f32 = 1.7;
const VELUM_CLOSED: f32 = 0.01;
const VELUM_OPEN: f32 = 0.4;
const FRICATIVE_ATTACK: f32 = 0.1;
const MAX_TRANSIENTS: usize = 8;
const TRACT_RATE: f32 = 2.0 * INTERNAL_RATE;

/// A narrowing of the tract made by the tongue tip, the lips, or the back of
/// the tongue, as a finger on the original's tract diagram.
///
/// `diameter` uses the original's touch scale: `≤ 0.3` closes the tract
/// (stops, nasals), about `0.35..0.7` is narrow enough for turbulence
/// (fricatives), larger values only shade the shape (approximants, lip
/// rounding).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constriction {
    /// Cell index, `2.0..44.0`.
    pub index: f32,
    /// Opening at the constriction.
    pub diameter: f32,
}

impl Constriction {
    /// Construct a constriction.
    #[must_use]
    pub const fn new(index: f32, diameter: f32) -> Self {
        Self { index, diameter }
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    c: Constriction,
    alive: bool,
    /// Turbulence envelope, `0..1`, ramps on and off over 100 ms.
    fricative: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Transient {
    position: usize,
    amplitude: f32,
    steps_left: u32,
}

/// Oral and nasal waveguides plus their articulation state.
#[derive(Clone, Debug)]
pub(crate) struct Tract {
    right: [f32; N],
    left: [f32; N],
    junction_right: [f32; N],
    junction_left: [f32; N + 1],
    reflection: [f32; N],
    new_reflection: [f32; N],
    max_amplitude: [f32; N],
    pub(crate) diameter: [f32; N],
    pub(crate) target: [f32; N],

    nose_right: [f32; NOSE_LEN],
    nose_left: [f32; NOSE_LEN],
    nose_junction_right: [f32; NOSE_LEN],
    nose_junction_left: [f32; NOSE_LEN + 1],
    nose_reflection: [f32; NOSE_LEN],
    pub(crate) nose_diameter: [f32; NOSE_LEN],
    nose_max_amplitude: [f32; NOSE_LEN],

    reflection_left: f32,
    reflection_right: f32,
    reflection_nose: f32,
    new_reflection_left: f32,
    new_reflection_right: f32,
    new_reflection_nose: f32,

    pub(crate) tongue_index: f32,
    pub(crate) tongue_diameter: f32,
    pub(crate) velum_target: f32,
    pub(crate) movement_speed: f32,
    slots: [Option<Slot>; MAX_CONSTRICTIONS],
    transients: [Transient; MAX_TRANSIENTS],
    transient_decay: f32,
    transient_life: u32,
    damping: f32,
    last_obstruction: Option<usize>,
}

impl Tract {
    pub(crate) fn new() -> Self {
        let mut t = Self {
            right: [0.0; N],
            left: [0.0; N],
            junction_right: [0.0; N],
            junction_left: [0.0; N + 1],
            reflection: [0.0; N],
            new_reflection: [0.0; N],
            max_amplitude: [0.0; N],
            diameter: [0.0; N],
            target: [0.0; N],
            nose_right: [0.0; NOSE_LEN],
            nose_left: [0.0; NOSE_LEN],
            nose_junction_right: [0.0; NOSE_LEN],
            nose_junction_left: [0.0; NOSE_LEN + 1],
            nose_reflection: [0.0; NOSE_LEN],
            nose_diameter: [0.0; NOSE_LEN],
            nose_max_amplitude: [0.0; NOSE_LEN],
            reflection_left: 0.0,
            reflection_right: 0.0,
            reflection_nose: 0.0,
            new_reflection_left: 0.0,
            new_reflection_right: 0.0,
            new_reflection_nose: 0.0,
            tongue_index: 12.9,
            tongue_diameter: 2.43,
            velum_target: VELUM_CLOSED,
            movement_speed: MOVEMENT_SPEED,
            slots: [None; MAX_CONSTRICTIONS],
            transients: [Transient::default(); MAX_TRANSIENTS],
            // Original: strength · 2^(−200·t) over 0.2 s.
            transient_decay: powf(2.0, -200.0 / TRACT_RATE),
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            transient_life: (0.2 * TRACT_RATE) as u32,
            damping: DAMPING,
            last_obstruction: None,
        };

        // Nasal cavity shape. Its reflections are computed once, with the
        // velum open, as in the original.
        #[allow(clippy::cast_precision_loss)]
        for i in 0..NOSE_LEN {
            let d = 2.0 * i as f32 / NOSE_LEN as f32;
            let dia = if i == 0 {
                VELUM_OPEN
            } else if d < 1.0 {
                0.4 + 1.6 * d
            } else {
                0.5 + 1.5 * (2.0 - d)
            };
            t.nose_diameter[i] = dia.min(1.9);
        }
        t.calculate_nose_reflections();
        t.nose_diameter[0] = VELUM_CLOSED;

        t.compute_targets();
        t.diameter = t.target;
        t.calculate_reflections();
        t.calculate_reflections();
        t
    }

    /// Change the glottal rate the waveguide runs at (twice this). A higher
    /// rate makes sound cross the 44 cells faster: a shorter tract. Loss and
    /// burst timing are kept constant in seconds.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn set_rate(&mut self, internal_rate: f32) {
        let tract_rate = 2.0 * internal_rate;
        self.transient_decay = powf(2.0, -200.0 / tract_rate);
        self.transient_life = (0.2 * tract_rate) as u32;
        self.damping = powf(DAMPING, TRACT_RATE / tract_rate);
    }

    /// Silence the waveguides without touching the articulation.
    pub(crate) fn reset_waves(&mut self) {
        self.right = [0.0; N];
        self.left = [0.0; N];
        self.junction_right = [0.0; N];
        self.junction_left = [0.0; N + 1];
        self.nose_right = [0.0; NOSE_LEN];
        self.nose_left = [0.0; NOSE_LEN];
        self.nose_junction_right = [0.0; NOSE_LEN];
        self.nose_junction_left = [0.0; NOSE_LEN + 1];
        self.max_amplitude = [0.0; N];
        self.nose_max_amplitude = [0.0; NOSE_LEN];
        self.transients = [Transient::default(); MAX_TRANSIENTS];
    }

    pub(crate) fn set_tongue(&mut self, index: f32, diameter: f32) {
        self.tongue_index = index.clamp(*TONGUE_INDEX.start(), *TONGUE_INDEX.end());
        self.tongue_diameter = diameter.clamp(*TONGUE_DIAMETER.start(), *TONGUE_DIAMETER.end());
    }

    pub(crate) fn set_velum(&mut self, openness: f32) {
        self.velum_target = lerp(VELUM_CLOSED, VELUM_OPEN, openness.clamp(0.0, 1.0));
    }

    pub(crate) fn velum_openness(&self) -> f32 {
        ((self.nose_diameter[0] - VELUM_CLOSED) / (VELUM_OPEN - VELUM_CLOSED)).clamp(0.0, 1.0)
    }

    /// Place, move or release (`None`) the constriction in `slot`. A released
    /// constriction stops shaping the tract at once but its turbulence fades
    /// out over 100 ms.
    pub(crate) fn set_constriction(&mut self, slot: usize, c: Option<Constriction>) {
        let Some(s) = self.slots.get_mut(slot) else {
            return;
        };
        match (c, s.as_mut()) {
            (Some(c), Some(s)) => {
                s.c = sanitize(c);
                s.alive = true;
            }
            (Some(c), None) => {
                *s = Some(Slot {
                    c: sanitize(c),
                    alive: true,
                    fricative: 0.0,
                });
            }
            (None, Some(s)) => s.alive = false,
            (None, None) => {}
        }
    }

    pub(crate) fn constriction(&self, slot: usize) -> Option<Constriction> {
        self.slots
            .get(slot)
            .copied()
            .flatten()
            .filter(|s| s.alive)
            .map(|s| s.c)
    }

    pub(crate) fn amplitudes(&self) -> &[f32; N] {
        &self.max_amplitude
    }

    pub(crate) fn nose_amplitudes(&self) -> &[f32; NOSE_LEN] {
        &self.nose_max_amplitude
    }

    /// Rest shape of cell `i` for the current tongue (after the original's
    /// `TractUI.setRestDiameter`).
    #[allow(clippy::cast_precision_loss)]
    fn rest_diameter(&self, i: usize) -> f32 {
        if i < 7 {
            return 0.6;
        }
        if i < BLADE_START {
            return 1.1;
        }
        if i >= LIP_START {
            return 1.5;
        }
        let t = 1.1 * PI * (self.tongue_index - i as f32) / (TIP_START - BLADE_START) as f32;
        let fixed = 2.0 + (self.tongue_diameter - 2.0) / 1.5;
        let mut curve = (1.5 - fixed + GRID_OFFSET) * cos(t);
        if i == BLADE_START - 2 || i == LIP_START - 1 {
            curve *= 0.8;
        }
        if i == BLADE_START || i == LIP_START - 2 {
            curve *= 0.94;
        }
        1.5 - curve
    }

    /// Target shape: tongue rest shape, then every live constriction carved
    /// in with a raised-cosine shoulder.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn compute_targets(&mut self) {
        for i in 0..N {
            self.target[i] = self.rest_diameter(i);
        }
        for s in self.slots.iter().flatten().filter(|s| s.alive) {
            let index = s.c.index;
            let d = (s.c.diameter - 0.3).max(0.0);
            if !(2.0..N as f32).contains(&index) || d >= 3.0 {
                continue;
            }
            let width = if index < 25.0 {
                10.0
            } else if index >= TIP_START as f32 {
                5.0
            } else {
                10.0 - 5.0 * (index - 25.0) / (TIP_START as f32 - 25.0)
            };
            let centre = (index + 0.5) as i32;
            let reach = width as i32 + 2;
            for k in -reach..=reach {
                let j = centre + k;
                if !(0..N as i32).contains(&j) {
                    continue;
                }
                let j = j as usize;
                let rel = (j as f32 - index).abs() - 0.5;
                let shrink = if rel <= 0.0 {
                    0.0
                } else if rel > width {
                    1.0
                } else {
                    0.5 * (1.0 - cos(PI * rel / width))
                };
                if d < self.target[j] {
                    self.target[j] = d + (self.target[j] - d) * shrink;
                }
            }
        }
    }

    /// Per-block articulation update: move the tract towards its target,
    /// fire release bursts, update reflection coefficients.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn update_block(&mut self, dt: f32) {
        // Fricative envelopes; free slots whose noise has faded out.
        let ramp = dt / FRICATIVE_ATTACK;
        for slot in &mut self.slots {
            if let Some(s) = slot {
                if s.alive {
                    s.fricative = (s.fricative + ramp).min(1.0);
                } else {
                    s.fricative -= ramp;
                    if s.fricative <= 0.0 {
                        *slot = None;
                    }
                }
            }
        }

        self.compute_targets();

        let amount = dt * self.movement_speed;
        let mut obstruction = None;
        for i in 0..N {
            if self.diameter[i] <= 0.0 {
                obstruction = Some(i);
            }
            // The back of the tract moves slower than the tip and lips.
            let slow_return = if i < NOSE_START {
                0.6
            } else if i >= TIP_START {
                1.0
            } else {
                0.6 + 0.4 * (i - NOSE_START) as f32 / (TIP_START - NOSE_START) as f32
            };
            self.diameter[i] = move_towards(
                self.diameter[i],
                self.target[i],
                slow_return * amount,
                2.0 * amount,
            );
        }
        // Releasing a closure with the velum shut makes a plosive burst.
        if let (Some(pos), None) = (self.last_obstruction, obstruction)
            && self.nose_diameter[0] < 0.223
        {
            self.add_transient(pos);
        }
        self.last_obstruction = obstruction;

        self.nose_diameter[0] = move_towards(
            self.nose_diameter[0],
            self.velum_target,
            amount * 0.25,
            amount * 0.1,
        );

        self.calculate_reflections();
    }

    fn add_transient(&mut self, position: usize) {
        let life = self.transient_life;
        let slot = self
            .transients
            .iter_mut()
            .min_by_key(|t| t.steps_left)
            .expect("MAX_TRANSIENTS > 0");
        *slot = Transient {
            position,
            amplitude: 0.3,
            steps_left: life,
        };
    }

    fn calculate_nose_reflections(&mut self) {
        let mut a = [0.0; NOSE_LEN];
        for (a, d) in a.iter_mut().zip(&self.nose_diameter) {
            *a = (d * d).max(1e-6);
        }
        for i in 1..NOSE_LEN {
            self.nose_reflection[i] = (a[i - 1] - a[i]) / (a[i - 1] + a[i]);
        }
    }

    fn calculate_reflections(&mut self) {
        let mut a = [0.0; N];
        for (a, d) in a.iter_mut().zip(&self.diameter) {
            *a = d * d;
        }
        for i in 1..N {
            self.reflection[i] = self.new_reflection[i];
            let sum = a[i - 1] + a[i];
            self.new_reflection[i] = if sum.abs() > 1e-6 {
                (a[i - 1] - a[i]) / sum
            } else {
                1.0
            };
        }

        self.reflection_left = self.new_reflection_left;
        self.reflection_right = self.new_reflection_right;
        self.reflection_nose = self.new_reflection_nose;
        let velum = self.nose_diameter[0] * self.nose_diameter[0];
        let a0 = a[NOSE_START];
        let a1 = a[NOSE_START + 1];
        let sum = a0 + a1 + velum;
        if sum.abs() > 1e-6 {
            self.new_reflection_left = (2.0 * a0 - sum) / sum;
            self.new_reflection_right = (2.0 * a1 - sum) / sum;
            self.new_reflection_nose = (2.0 * velum - sum) / sum;
        } else {
            self.new_reflection_left = 1.0;
            self.new_reflection_right = 1.0;
            self.new_reflection_nose = 1.0;
        }
    }

    /// One waveguide step. `frication` is band-passed noise already scaled by
    /// the glottal noise modulator. Returns lip plus nostril output.
    pub(crate) fn step(&mut self, glottal: f32, frication: f32, lambda: f32) -> f32 {
        // Plosive bursts.
        for t in &mut self.transients {
            if t.steps_left > 0 {
                self.right[t.position] += t.amplitude * 0.5;
                self.left[t.position] += t.amplitude * 0.5;
                t.amplitude *= self.transient_decay;
                t.steps_left -= 1;
            }
        }

        // Turbulence at each constriction.
        for k in 0..MAX_CONSTRICTIONS {
            if let Some(s) = self.slots[k]
                && s.fricative > 0.0
            {
                self.add_turbulence(0.66 * frication * s.fricative, s.c.index, s.c.diameter);
            }
        }

        self.junction_right[0] = self.left[0] * GLOTTAL_REFLECTION + glottal;
        self.junction_left[N] = self.right[N - 1] * LIP_REFLECTION;

        for i in 1..N {
            let r = lerp(self.reflection[i], self.new_reflection[i], lambda);
            let w = r * (self.right[i - 1] + self.left[i]);
            self.junction_right[i] = self.right[i - 1] - w;
            self.junction_left[i] = self.left[i] + w;
        }

        // Three-way junction at the velum.
        let i = NOSE_START;
        let r = lerp(self.reflection_left, self.new_reflection_left, lambda);
        self.junction_left[i] =
            r * self.right[i - 1] + (1.0 + r) * (self.nose_left[0] + self.left[i]);
        let r = lerp(self.reflection_right, self.new_reflection_right, lambda);
        self.junction_right[i] =
            r * self.left[i] + (1.0 + r) * (self.right[i - 1] + self.nose_left[0]);
        let r = lerp(self.reflection_nose, self.new_reflection_nose, lambda);
        self.nose_junction_right[0] =
            r * self.nose_left[0] + (1.0 + r) * (self.left[i] + self.right[i - 1]);

        for i in 0..N {
            self.right[i] = self.junction_right[i] * self.damping;
            self.left[i] = self.junction_left[i + 1] * self.damping;
            let amp = (self.right[i] + self.left[i]).abs();
            self.max_amplitude[i] = (self.max_amplitude[i] * 0.9999).max(amp);
        }
        let lips = self.right[N - 1];

        self.nose_junction_left[NOSE_LEN] = self.nose_right[NOSE_LEN - 1] * LIP_REFLECTION;
        for i in 1..NOSE_LEN {
            let w = self.nose_reflection[i] * (self.nose_right[i - 1] + self.nose_left[i]);
            self.nose_junction_right[i] = self.nose_right[i - 1] - w;
            self.nose_junction_left[i] = self.nose_left[i] + w;
        }
        for i in 0..NOSE_LEN {
            self.nose_right[i] = self.nose_junction_right[i];
            self.nose_left[i] = self.nose_junction_left[i + 1];
            let amp = (self.nose_right[i] + self.nose_left[i]).abs();
            self.nose_max_amplitude[i] = (self.nose_max_amplitude[i] * 0.9999).max(amp);
        }
        let nose = self.nose_right[NOSE_LEN - 1];

        lips + nose
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn add_turbulence(&mut self, noise: f32, index: f32, diameter: f32) {
        if !(2.0..=N as f32).contains(&index) || diameter <= 0.0 {
            return;
        }
        let i = index as usize;
        let delta = index - i as f32;
        let thinness = (8.0 * (0.7 - diameter)).clamp(0.0, 1.0);
        let openness = (30.0 * (diameter - 0.3)).clamp(0.0, 1.0);
        let n0 = noise * (1.0 - delta) * thinness * openness;
        let n1 = noise * delta * thinness * openness;
        if i + 1 < N {
            self.right[i + 1] += n0 * 0.5;
            self.left[i + 1] += n0 * 0.5;
        }
        if i + 2 < N {
            self.right[i + 2] += n1 * 0.5;
            self.left[i + 2] += n1 * 0.5;
        }
    }
}

fn sanitize(c: Constriction) -> Constriction {
    #[allow(clippy::cast_precision_loss)]
    let index = if c.index.is_finite() {
        c.index.clamp(0.0, N as f32)
    } else {
        0.0
    };
    let diameter = if c.diameter.is_finite() {
        c.diameter.clamp(-1.0, 5.0)
    } else {
        5.0
    };
    Constriction { index, diameter }
}
