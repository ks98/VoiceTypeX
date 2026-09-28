// SPDX-License-Identifier: GPL-3.0-or-later
//! Input level for the overlay waveform.
//!
//! The cpal callback records the loudest block RMS since the last read into
//! a [`LevelTap`] (one atomic, no lock, no allocation — the callback runs on
//! the real-time audio thread). The pipeline's level emitter drains it every
//! tick and smooths it with a [`LevelMeter`] into a 0..1 display value.

use std::sync::atomic::{AtomicU32, Ordering};

/// Quietest level that still moves the bars; room noise sits below it.
const FLOOR_DB: f32 = -60.0;
/// Loud speech close to the microphone; everything above is full scale.
const CEIL_DB: f32 = -10.0;
/// Per-tick decay: fast rise, calm fall (≈ 0.3 s to a tenth at 25 Hz).
const RELEASE: f32 = 0.7;

/// Root mean square of a block of samples in -1..1.
pub fn rms(samples: impl ExactSizeIterator<Item = f32>) -> f32 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let sum: f32 = samples.map(|s| s * s).sum();
    (sum / n as f32).sqrt()
}

/// Peak-hold of block RMS values between two reads.
#[derive(Default)]
pub struct LevelTap(AtomicU32);

impl LevelTap {
    /// Audio thread. For non-negative finite floats the IEEE-754 bit
    /// patterns order like the values, so `fetch_max` on the bits keeps the
    /// loudest block.
    pub fn record(&self, rms: f32) {
        if rms.is_finite() && rms > 0.0 {
            self.0.fetch_max(rms.to_bits(), Ordering::Relaxed);
        }
    }

    /// Loudest RMS since the last call; resets the hold.
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

/// dBFS mapping plus release smoothing for the display value.
#[derive(Default)]
pub struct LevelMeter {
    level: f32,
}

impl LevelMeter {
    pub fn step(&mut self, rms: f32) -> f32 {
        let target = if rms > 0.0 {
            let db = 20.0 * rms.log10();
            ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.level = target.max(self.level * RELEASE);
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_known_signals() {
        assert_eq!(rms(std::iter::empty()), 0.0);
        assert_eq!(rms(vec![0.0; 64].into_iter()), 0.0);
        let square = (0..64).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 });
        assert!((rms(square.collect::<Vec<_>>().into_iter()) - 1.0).abs() < 1e-6);
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * std::f32::consts::TAU / 48.0).sin())
            .collect();
        assert!((rms(sine.into_iter()) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    }

    #[test]
    fn tap_keeps_the_loudest_block_and_resets() {
        let tap = LevelTap::default();
        assert_eq!(tap.take(), 0.0);
        for v in [0.1, 0.5, 0.25] {
            tap.record(v);
        }
        tap.record(f32::NAN);
        tap.record(-1.0);
        assert_eq!(tap.take(), 0.5);
        assert_eq!(tap.take(), 0.0);
    }

    #[test]
    fn float_bits_order_like_values() {
        let values = [0.0f32, 1e-9, 0.001, 0.3, 0.30001, 1.0, 2.0];
        for w in values.windows(2) {
            assert!(w[0].to_bits() < w[1].to_bits(), "{} vs {}", w[0], w[1]);
        }
    }

    #[test]
    fn meter_maps_dbfs_to_unit_range() {
        let mut m = LevelMeter::default();
        assert_eq!(m.step(0.0), 0.0);
        assert_eq!(LevelMeter::default().step(10f32.powf(-70.0 / 20.0)), 0.0);
        assert_eq!(LevelMeter::default().step(1.0), 1.0);
        let mid = LevelMeter::default().step(10f32.powf(-35.0 / 20.0));
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
    }

    #[test]
    fn meter_rises_instantly_and_falls_smoothly() {
        let mut m = LevelMeter::default();
        assert_eq!(m.step(1.0), 1.0);
        let mut prev = 1.0;
        for _ in 0..20 {
            let v = m.step(0.0);
            assert!(v < prev && v >= 0.0 && v.is_finite());
            prev = v;
        }
        assert!(prev < 0.01);
    }
}
