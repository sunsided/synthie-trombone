#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
//! LPC formant estimation (autocorrelation + Levinson–Durbin), for the
//! tuning example and the vowel tests.

use std::f32::consts::PI;

/// LPC coefficients `a[0..=order]` (`a[0] = 1`) of a pre-emphasised,
/// Hamming-windowed signal.
pub fn lpc(x: &[f32], order: usize) -> Vec<f64> {
    let x: Vec<f64> = x
        .windows(2)
        .enumerate()
        .map(|(n, w)| {
            let h =
                0.54 - 0.46 * (2.0 * std::f64::consts::PI * n as f64 / (x.len() - 2) as f64).cos();
            f64::from(w[1] - 0.97 * w[0]) * h
        })
        .collect();
    let r: Vec<f64> = (0..=order)
        .map(|k| x.iter().zip(&x[k..]).map(|(a, b)| a * b).sum())
        .collect();
    let mut a = vec![0.0; order + 1];
    a[0] = 1.0;
    let mut err = r[0] * (1.0 + 1e-9);
    for i in 1..=order {
        let acc: f64 = (1..i).map(|j| a[j] * r[i - j]).sum();
        let k = -(r[i] + acc) / err;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        err *= 1.0 - k * k;
    }
    a
}

/// Peaks of the LPC envelope `1/|A(e^jw)|²` below `max_hz`.
pub fn formants(x: &[f32], sr: f32, max_hz: f32) -> Vec<f32> {
    let a = lpc(x, 2 + (sr / 1000.0) as usize);
    let env: Vec<(f32, f64)> = (5..(max_hz / 5.0) as usize)
        .map(|k| {
            let f = k as f32 * 5.0;
            let w = f64::from(2.0 * PI * f / sr);
            let (mut re, mut im) = (0.0, 0.0);
            for (n, c) in a.iter().enumerate() {
                re += c * (w * n as f64).cos();
                im -= c * (w * n as f64).sin();
            }
            (f, 1.0 / (re * re + im * im))
        })
        .collect();
    (1..env.len() - 1)
        .filter(|&i| env[i].1 > env[i - 1].1 && env[i].1 >= env[i + 1].1 && env[i].0 > 150.0)
        .map(|i| env[i].0)
        .collect()
}
