//! The glottal pulse follows the fold's length (experiment 27). It went with the body's scale, so a
//! woman at twice a man's f0 and 0.81 of his area had 1.45 times his maximum flow declination rate at
//! the same lung pressure, where real women's is lower than men's, and her vowels stood too far above
//! her consonants. Her folds also stay apart for more of the cycle (experiment 31): at the same pulse
//! length as his, her flow still fell 1.10 times as fast and her H1-H2 sat below his, where real
//! women's fall is slower and their H1-H2 higher.
use chimera::anatomy::human::{BODY_CHILD, BODY_FEMALE, BODY_MALE, HumanBody};
use chimera::phonetics::planner::vowel_artic;
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

const FS: f64 = 48000.0;

/// A harmonic's power in the output: the largest over ±3 % of its frequency, for the drift.
fn harmonic(x: &[f64], f: f64) -> f64 {
    let at = |f: f64| {
        let n = x.len() as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
            let a = std::f64::consts::TAU * f * i as f64 / FS;
            re += w * s * a.cos();
            im += w * s * a.sin();
        }
        re * re + im * im
    };
    (0..21).map(|i| at(f * (0.97 + 0.003 * i as f64))).fold(0.0, f64::max)
}

/// Peak-to-peak flow (L/s) and the steepest fall of flow (L/s²) of a held /a/, medians over cycles,
/// and its H1-H2 (dB).
fn held(body: HumanBody, name: &str, f0: f64) -> (f64, f64, f64) {
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.set_articulation(&vowel_artic("a", name.into()).unwrap(), None);
    v.set_gate(1.0);
    let port = v.airway.glottis_port;
    let mut one = [0f32; 1];
    let (mut flow, mut out) = (Vec::new(), Vec::new());
    for i in 0..(0.8 * FS) as usize {
        v.render(&mut one);
        if i as f64 > 0.4 * FS {
            flow.push(v.airway.net.u[port] / 1000.0);
            out.push(one[0] as f64);
        }
    }
    let period = (FS / f0).round() as usize;
    let median = |mut x: Vec<f64>| {
        x.sort_by(f64::total_cmp);
        x[x.len() / 2]
    };
    let ac = median(flow.chunks_exact(period).map(|c| c.iter().cloned().fold(f64::MIN, f64::max) - c.iter().cloned().fold(f64::MAX, f64::min)).collect());
    let fall = median(flow.chunks_exact(period).map(|c| c.windows(2).map(|w| (w[0] - w[1]) * FS).fold(0.0, f64::max)).collect());
    (ac, fall, 10.0 * (harmonic(&out, f0) / harmonic(&out, 2.0 * f0)).log10())
}

#[test]
fn her_folds_pass_less_air_than_his() {
    let (male_ac, male_fall, _) = held(BODY_MALE, "male", 112.0);
    let (female_ac, female_fall, _) = held(BODY_FEMALE, "female", 205.0);
    let (child_ac, ..) = held(BODY_CHILD, "child", 260.0);
    let (flow, fall) = (female_ac / male_ac, female_fall / male_fall);
    assert!((0.55..0.72).contains(&flow), "her peak flow is {flow:.2} of his");
    assert!(fall < 1.25, "her flow falls {fall:.2} times as fast as his");
    assert!(child_ac < female_ac, "a child {child_ac:.3} L/s, a woman {female_ac:.3}");
}

#[test]
fn her_folds_stay_apart_for_more_of_the_cycle() {
    let (_, male_fall, male_h) = held(BODY_MALE, "male", 112.0);
    let (_, female_fall, female_h) = held(BODY_FEMALE, "female", 205.0);
    assert!(female_fall < male_fall, "her flow falls at {female_fall:.0} L/s², his at {male_fall:.0}");
    assert!(female_h > male_h, "her H1-H2 {female_h:.2} dB, his {male_h:.2}");
}
