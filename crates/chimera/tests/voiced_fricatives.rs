//! A voiced fricative divides the lung pressure between the folds and the constriction, so its voicing
//! is far weaker than a vowel's (experiment 23). With the folds nearly as closed as in a vowel, the oral
//! pressure stayed near 1 cmH2O and the voice under /v z/ was 10-18 dB above the real speakers' (their
//! band below 1.2 kHz: /v/ -23 dB re their stressed vowels, /z/ -33 to -37).
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE};
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

const FS: f64 = 48000.0;

/// Power below about 1 kHz (two one-pole low-passes), per sample, for the samples from `a` to `b`.
fn low_band(x: &[f32], a: f64, b: f64) -> f64 {
    let k = 1.0 - (-2.0 * std::f64::consts::PI * 1000.0 / FS).exp();
    let (mut y1, mut y2, mut sum, mut n) = (0.0f64, 0.0f64, 1e-20f64, 0.0f64);
    for (i, s) in x.iter().enumerate() {
        y1 += k * (f64::from(*s) - y1);
        y2 += k * (y1 - y2);
        let t = i as f64 / FS;
        if t >= a && t < b {
            sum += y2 * y2;
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn the_voice_weakens_behind_a_voiced_fricatives_constriction() {
    for (body, name, f0) in [(BODY_MALE, "male", 112.0), (BODY_FEMALE, "female", 205.0)] {
        for (c, limit) in [("z", -13.0), ("v", -6.5)] {
            let plan = plan_utterance(&tokenize_ipa(&format!("ˈɑ{c}ɑ")), &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
            let span = plan.segments.iter().find(|s| s.ipa == c).unwrap();
            let vowel = &plan.segments[0];
            let mut v = HumanVoice::new(FS, body, 2);
            v.set_source(&SourcePatch::f0(f0));
            v.schedule(&plan.gestures);
            let mut out = vec![0.0f32; (FS * (plan.end + 0.3)).round() as usize];
            for chunk in out.chunks_mut(128) {
                v.render(chunk);
            }
            let quarter = 0.25 * (span.t1 - span.t0);
            let voice = low_band(&out, span.t0 + quarter, span.t1 - quarter);
            let reference = low_band(&out, vowel.t0 + 0.03, vowel.t1 - 0.03);
            let level = 10.0 * (voice / reference).log10();
            println!("{name} /{c}/: voice {level:.1} dB re the vowel");
            assert!(level < limit, "{name} /{c}/: voice {level:.1} dB re the vowel, limit {limit}");
        }
    }
}
