//! Artifacts a listener reported in running speech (2026-11-07): a tinny ring as sounds fade, and
//! a pop on the /p/ of "sharp" in "A cold paper walks under the sharp tree.", and a click before
//! every /f/ and /v/ (found in experiment 18).
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE, HumanBody};
use chimera::phonetics::Plan;
use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PhraseInput, PlanOptions, plan_phrases, plan_utterance};
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

const FS: f64 = 48000.0;
/// Output samples per control update (`HumanVoice` updates its controls every 64 output samples).
const BLOCK: usize = 64;
const VOICES: [(HumanBody, &str, f64); 2] = [(BODY_MALE, "male", 112.0), (BODY_FEMALE, "female", 205.0)];

fn render(plan: Plan, body: HumanBody, f0: f64) -> (Plan, Vec<f32>) {
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.schedule(&plan.gestures);
    let mut out = vec![0.0f32; (FS * (plan.end + 0.3)).round() as usize];
    for chunk in out.chunks_mut(128) {
        v.render(chunk);
    }
    (plan, out)
}

fn options(body: &str, f0: f64) -> PlanOptions {
    PlanOptions { body: body.into(), t0: 0.05, f0: Some(f0), ..Default::default() }
}

fn idx(t: f64) -> usize {
    (t * FS).round() as usize
}

/// Share of the energy in `x[a..b]` that repeats exactly every control block: K·Σavg² / Σx² over
/// K block-aligned frames, frame means removed. Noise gives about 1/K; a pure buzz gives 1.
fn block_periodic_share(x: &[f32], a: usize, b: usize) -> f64 {
    let a = a.div_ceil(BLOCK) * BLOCK;
    let k = (b.min(x.len()) - a) / BLOCK;
    let mut avg = [0.0f64; BLOCK];
    let mut total = 0.0;
    for f in 0..k {
        let fr = &x[a + f * BLOCK..a + (f + 1) * BLOCK];
        let m = fr.iter().map(|v| *v as f64).sum::<f64>() / BLOCK as f64;
        for (i, v) in fr.iter().enumerate() {
            let d = *v as f64 - m;
            avg[i] += d / k as f64;
            total += d * d;
        }
    }
    let mean = avg.iter().sum::<f64>() / BLOCK as f64;
    k as f64 * avg.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / total
}

fn peak(x: &[f32], a: usize, b: usize) -> f64 {
    x[a.min(x.len())..b.min(x.len())].iter().fold(0.0f64, |m, v| m.max(v.abs() as f64))
}

#[test]
fn fading_sounds_do_not_buzz_at_the_control_rate() {
    // The lung pressure used to step once per control block, so as a word's breath ran down after
    // its last vowel, the tail was about a fifth block-periodic: a 750 Hz buzz with overtones.
    for (body, name, f0) in VOICES {
        for w in ["tɹˈʌk", "ʃˈit", "pˈʊʃəz", "ʃˈɑɹptɹˈi"] {
            let (plan, out) = render(plan_utterance(&tokenize_ipa(w), &options(name, f0)), body, f0);
            let last_vowel = plan.segments.iter().rev().find(|s| s.kind == "vowel").unwrap();
            let (a, b) = (idx(last_vowel.t1), idx(plan.end + 0.25));
            let share = block_periodic_share(&out, a, b);
            println!("{w} ({name}): {:.1} % of the tail is block-periodic", 100.0 * share);
            assert!(share < 0.05, "{w} ({name}): {:.0} % of the tail repeats every control block", 100.0 * share);
        }
    }
}

#[test]
fn the_p_of_sharp_does_not_pop() {
    // "sharp tree": the lips close for /p/, then the tongue seals for /t/ while still moving and
    // squeezes the air shut in between to about 11x the lungs' pressure (88 cmH2O, male voice).
    // The /p/ releases 7 dB above the loudest vowel of the sentence; the p's of "paper" sit at
    // -6 and -7 dB. With the moving-wall volume source off, the pressure never passes the lungs'.
    // Absorbing a sealed pocket's wall motion halves the pop but breaks
    // `airway_adversarial::sealed_cavity_compression_follows_the_linearised_adiabatic_law`.
    let mut g = EnglishG2P::new();
    g.load_dict("a\tAH0\ncold\tK OW1 L D\npaper\tP EY1 P ER0\nwalks\tW AO1 K S\nunder\tAH1 N D ER0\nthe\tDH AH0\nsharp\tSH AA1 R P\ntree\tT R IY1\n");
    let inputs: Vec<PhraseInput> = g
        .to_phrases("A cold paper walks under the sharp tree.")
        .into_iter()
        .map(|p| PhraseInput { segments: p.segments, intonation: Some(p.intonation), pause: Some(p.pause) })
        .collect();
    for (body, name, f0) in VOICES {
        let plan = plan_phrases(&inputs, &options(name, f0));
        let mut v = HumanVoice::new(FS, body, 2);
        v.set_source(&SourcePatch::f0(f0));
        v.schedule(&plan.gestures);
        let (o0, on) = (v.airway.oral.first_node(), v.airway.oral.n);
        let mut out = vec![0.0f32; (FS * (plan.end + 0.3)).round() as usize];
        let mut most = 0.0f64;
        for c in out.chunks_mut(BLOCK) {
            v.render(c);
            most = (o0..o0 + on).fold(most, |m, k| m.max(v.airway.net.p[k]));
        }
        let lungs = v.source.pressure * 980.665;
        println!("{name}: oral pressure peaks at {:.1}x the lungs'", most / lungs);
        let vowel_peak = plan.segments.iter().filter(|s| s.kind == "vowel").map(|s| peak(&out, idx(s.t0), idx(s.t1))).fold(0.0, f64::max);
        let p = plan.segments.iter().filter(|s| s.ipa == "p").last().unwrap();
        let db = 20.0 * (peak(&out, idx(p.t1 - 0.005), idx(p.t1 + 0.03)) / vowel_peak).log10();
        println!("{name}: the /p/ of sharp releases at {db:+.1} dB re the loudest vowel");
        assert!(most < 2.0 * lungs, "{name}: oral pressure reached {:.1}x the lungs'", most / lungs);
        assert!(db < -3.0, "{name}: the /p/ of sharp releases at {db:+.1} dB re the loudest vowel");
    }
}

/// dB of the 2 ms peaks in [t0, t1), relative to `re`.
fn peaks_2ms(x: &[f32], t0: f64, t1: f64, re: f64) -> Vec<f64> {
    let mut v = vec![];
    let mut t = t0;
    while t + 0.002 <= t1 + 1e-9 {
        v.push(20.0 * (peak(x, idx(t), idx(t + 0.002)) / re + 1e-12).log10());
        t += 0.002;
    }
    v
}

#[test]
fn an_f_does_not_click() {
    // The lips nearly closed, then sprang open as the lower lip tucked under the incisors (two
    // steps in the labiodental geometry): a 2-4 ms [b]-like click about 16 ms before every /f/ and
    // /v/, 15-20 dB above the fricative around it. Measured as the loudest 2 ms peak from 20 ms
    // before the fricative to 10 ms into it, over the median 2 ms peak of its first 30 ms. Only
    // where nothing else is in that window: between vowels, and the /f/ that opens a sentence (in
    // "work faced" the /k/ release would count).
    let mut g = EnglishG2P::new();
    g.load_dict("four\tF AO1 R\nhours\tAW1 ER0 Z\nof\tAH1 V\nsteady\tS T EH1 D IY0\nwork\tW ER1 K\nfaced\tF EY1 S T\nus\tAH1 S\n");
    let inputs: Vec<PhraseInput> = g
        .to_phrases("Four hours of steady work faced us.")
        .into_iter()
        .map(|p| PhraseInput { segments: p.segments, intonation: Some(p.intonation), pause: Some(p.pause) })
        .collect();
    let mut loud = vec![];
    for (body, name, f0) in VOICES {
        for (what, plan) in [("ɑfɑ", plan_utterance(&tokenize_ipa("ˈɑfɑ"), &options(name, f0))), ("ɑvɑ", plan_utterance(&tokenize_ipa("ˈɑvɑ"), &options(name, f0))), ("four hours", plan_phrases(&inputs, &options(name, f0)))] {
            let (plan, out) = render(plan, body, f0);
            let first_only = what == "four hours";
            for s in plan.segments.iter().filter(|s| s.ipa == "f" || s.ipa == "v").take(if first_only { 1 } else { usize::MAX }) {
                let onset = peaks_2ms(&out, s.t0 - 0.02, s.t0 + 0.01, 1.0).into_iter().fold(f64::MIN, f64::max);
                let mut body_db = peaks_2ms(&out, s.t0, s.t0 + 0.03, 1.0);
                body_db.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let excess = onset - body_db[body_db.len() / 2];
                println!("{what} ({name}): /{}/ at {:.2} s: onset peak {excess:+.1} dB over the fricative", s.ipa, s.t0);
                if excess > 6.0 {
                    loud.push(format!("{what} ({name}) /{}/ {excess:+.1} dB", s.ipa));
                }
            }
        }
    }
    assert!(loud.is_empty(), "f/v onsets that click: {loud:?}");
}
