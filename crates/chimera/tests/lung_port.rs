//! The lungs' port (experiment 32). Its resistance, half the trachea's characteristic impedance, is a
//! termination for sound, and it also sat in the steady flow: 8.2 cmH₂O/(L/s) male, 10.1 female, so
//! /ʃ/ lost about half the lungs' pressure before the folds, and every open-glottis sound was
//! underdriven. And what the fix leans on: the folds' effort stays where it was, and folds held apart
//! lose closed phase.
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE, HumanBody};
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use chimera::physics::CM_H2O;
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

const FS: f64 = 48000.0;
const VOICES: [(HumanBody, &str, f64); 2] = [(BODY_MALE, "male", 112.0), (BODY_FEMALE, "female", 205.0)];

/// Render a VCV and call `at` on every block inside the middle half of the span of `ipa`.
fn in_span(body: HumanBody, name: &str, f0: f64, vcv: &str, ipa: &str, mut at: impl FnMut(&HumanVoice)) {
    let plan = plan_utterance(&tokenize_ipa(vcv), &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
    let s = plan.segments.iter().find(|s| s.ipa == ipa).unwrap();
    let (a, b) = (s.t0 + (s.t1 - s.t0) / 4.0, s.t1 - (s.t1 - s.t0) / 4.0);
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.schedule(&plan.gestures);
    let mut block = [0.0f32; 128];
    let mut n = 0usize;
    while (n as f64) < FS * (plan.end + 0.05) {
        v.render(&mut block);
        n += block.len();
        let t = n as f64 / FS;
        if t >= a && t <= b {
            at(&v);
        }
    }
}

#[test]
fn a_fricative_keeps_the_lungs_pressure_at_its_folds() {
    for (body, name, f0) in VOICES {
        let mut folds = vec![];
        in_span(body, name, f0, "ˈɑʃɑ", "ʃ", |v| folds.push(v.airway.net.p[v.airway.trachea.last_node()] / CM_H2O));
        folds.sort_by(f64::total_cmp);
        let median = folds[folds.len() / 2];
        // 0.6 L/s through the lower airways' 0.8-1.1 cmH₂O/(L/s) and the trachea's walls. It read 4.1-4.2
        assert!(median > 5.8, "{name}: {median} cmH₂O under the folds in /ʃ/");
    }
}

#[test]
fn the_port_moves_neither_the_folds_pressure_nor_their_effort() {
    let mut v = HumanVoice::with_defaults(FS);
    v.set_source(&SourcePatch::default());
    assert!((v.glottis.params.effort - 0.5).abs() < 1e-12, "{}", v.glottis.params.effort);
    // what the folds read in a stressed vowel before the fix: 7.1-7.2 cmH₂O
    for (body, name, f0) in VOICES {
        let mut folds = vec![];
        in_span(body, name, f0, "ˈɑtɑ", "ɑ", |v| folds.push(v.airway.net.p[v.airway.trachea.last_node()] / CM_H2O));
        let mean = folds.iter().sum::<f64>() / folds.len() as f64;
        assert!((6.5..7.5).contains(&mean), "{name}: {mean} cmH₂O under the folds in /ɑ/");
    }
}

#[test]
fn folds_held_apart_meet_for_less_of_the_cycle() {
    let closed_share = |abduction: f64| {
        let mut g = HumanVoice::with_defaults(FS).glottis;
        g.params.abduction = abduction;
        g.params.chink = 0.01;
        g.params.jitter = 0.0;
        g.params.drift = 0.0;
        let (mut closed, mut n) = (0, 0);
        for i in 0..(0.5 * FS) as usize {
            let a = g.next(7.0 * CM_H2O);
            if i as f64 > 0.25 * FS {
                closed += i32::from(a <= abduction + 0.01 + 1e-9);
                n += 1;
            }
        }
        f64::from(closed) / f64::from(n)
    };
    // Zhang 2016, table II: closed quotient −0.0247 per degree, where 0-4° open 0-10.1 mm²
    let lost = closed_share(0.0) - closed_share(0.1);
    assert!((lost - 0.098).abs() < 0.02, "closed share lost at 0.1 cm²: {lost}");
}
