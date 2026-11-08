//! The website's close-mic chain (a low shelf, a presence peak, a fast compressor, make-up gain),
//! which there is Web Audio nodes, in Rust for the audio thread. Close to the browser's, not identical:
//! its `DynamicsCompressorNode` has its own knee curve and pre-delay. Allocation-free, per sample.

use chimera::math::{cos, exp, ln, pow, sin, sqrt};

const TWO_PI: f64 = core::f64::consts::TAU;

const SHELF_HZ: f64 = 120.0;
const SHELF_DB: f64 = 9.0;
const PEAK_HZ: f64 = 3500.0;
const PEAK_Q: f64 = 0.8;
const PEAK_DB: f64 = 3.0;
const THRESHOLD_DB: f64 = -28.0;
const KNEE_DB: f64 = 8.0;
const RATIO: f64 = 5.0;
const ATTACK: f64 = 0.002;
const RELEASE: f64 = 0.12;
/// The website's make-up gain (2.2) and what `DynamicsCompressorNode` adds by itself for these settings
/// (about 12 dB): how much a full-scale peak is turned down, to the power 0.6.
const MAKEUP: f64 = 2.2 * 4.07;

/// A biquad in transposed direct form II.
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn normalised(b: [f64; 3], a: [f64; 3]) -> Biquad {
        Biquad { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }

    /// As Web Audio's `lowshelf` (shelf slope 1).
    fn low_shelf(fs: f64, hz: f64, db: f64) -> Biquad {
        let a = pow(10.0, db / 40.0);
        let w = TWO_PI * hz / fs;
        let (c, alpha) = (cos(w), sin(w) / 2.0 * sqrt(2.0));
        let k = 2.0 * sqrt(a) * alpha;
        Biquad::normalised(
            [a * ((a + 1.0) - (a - 1.0) * c + k), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - k)],
            [(a + 1.0) + (a - 1.0) * c + k, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - k],
        )
    }

    /// As Web Audio's `peaking`.
    fn peaking(fs: f64, hz: f64, q: f64, db: f64) -> Biquad {
        let a = pow(10.0, db / 40.0);
        let w = TWO_PI * hz / fs;
        let (c, alpha) = (cos(w), sin(w) / (2.0 * q));
        Biquad::normalised([1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a])
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// How far to turn a level down (dB, at most 0), with a soft knee.
#[inline]
fn reduction_db(level_db: f64) -> f64 {
    let over = level_db - THRESHOLD_DB;
    let slope = 1.0 / RATIO - 1.0;
    if 2.0 * over <= -KNEE_DB {
        0.0
    } else if 2.0 * over >= KNEE_DB {
        slope * over
    } else {
        slope * (over + KNEE_DB / 2.0) * (over + KNEE_DB / 2.0) / (2.0 * KNEE_DB)
    }
}

#[derive(Clone, Debug)]
pub struct CloseMic {
    shelf: Biquad,
    peak: Biquad,
    attack: f64,
    release: f64,
    /// the reduction now (dB, at most 0)
    reduction: f64,
}

/// Below this a level counts as silence (dB).
const FLOOR_DB: f64 = -120.0;
const DB_PER_NEPER: f64 = 8.685889638065037;

impl CloseMic {
    pub fn new(fs: f64) -> CloseMic {
        CloseMic {
            shelf: Biquad::low_shelf(fs, SHELF_HZ, SHELF_DB),
            peak: Biquad::peaking(fs, PEAK_HZ, PEAK_Q, PEAK_DB),
            attack: exp(-1.0 / (ATTACK * fs)),
            release: exp(-1.0 / (RELEASE * fs)),
            reduction: 0.0,
        }
    }

    pub fn process(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            let x = self.peak.process(self.shelf.process(*s as f64));
            let level = x.abs();
            let level_db = if level > 1e-6 { DB_PER_NEPER * ln(level) } else { FLOOR_DB };
            let want = reduction_db(level_db);
            // turning down is the attack, coming back up the release
            let k = if want < self.reduction { self.attack } else { self.release };
            self.reduction = want + k * (self.reduction - want);
            *s = (x * pow(10.0, self.reduction / 20.0) * MAKEUP) as f32;
        }
    }
}
