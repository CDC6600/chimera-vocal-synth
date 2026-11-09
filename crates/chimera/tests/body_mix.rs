//! The continuous body: presets are exact, blends land between them (anatomy, calibrated
//! targets, formants, pitch), and a blended voice plays.
use chimera::airway::frequency::{find_peaks, transfer_function};
use chimera::anatomy::human::{BODY_CHILD, BODY_FEMALE, BODY_MALE, BodyMix, BodyName, HumanArtic, NEUTRAL};
use chimera::engine::{Engine, default_f0};
use chimera::phonetics::inventory::phoneme;
use chimera::phonetics::planner::{PlanOptions, vowel_artic};
use chimera::voice::human_airway::HumanAirway;

fn between(x: f64, a: f64, b: f64) -> bool {
    x >= a.min(b) - 1e-12 && x <= a.max(b) + 1e-12
}

#[test]
fn presets_are_exact_and_weights_sum_to_one() {
    for (name, mix, body) in [(BodyName::Male, BodyMix::MALE, BODY_MALE), (BodyName::Female, BodyMix::FEMALE, BODY_FEMALE), (BodyName::Child, BodyMix::CHILD, BODY_CHILD)] {
        assert_eq!(mix.preset(), Some(name));
        assert_eq!(mix.body(), body);
        assert_eq!(BodyMix::from(name.as_str()), mix);
        assert_eq!(vowel_artic("i", mix), vowel_artic("i", name.into()));
    }
    // a child has no female axis
    assert_eq!(BodyMix::new(0.7, 1.0).preset(), Some(BodyName::Child));
    for f in [0.0, 0.3, 1.0] {
        for c in [0.0, 0.5, 0.9] {
            let w: f64 = BodyMix::new(f, c).weights().iter().map(|x| x.1).sum();
            assert!((w - 1.0).abs() < 1e-12);
        }
    }
    assert_eq!(BodyMix::new(f64::NAN, 3.0), BodyMix { female: 0.0, child: 1.0 });
}

#[test]
fn deserializes_from_a_name_or_a_mix() {
    let o: PlanOptions = serde_json::from_str(r#"{"body":"female","t0":0}"#).unwrap();
    assert_eq!(o.body, BodyMix::FEMALE);
    let o: PlanOptions = serde_json::from_str(r#"{"body":{"female":0.25,"child":0.5},"t0":0}"#).unwrap();
    assert_eq!(o.body, BodyMix { female: 0.25, child: 0.5 });
}

#[test]
fn blends_land_between_the_presets() {
    let mid = BodyMix::new(0.5, 0.0);
    assert!((mid.body().scale - 0.95).abs() < 1e-12);
    assert!(between(mid.body().fold_length, BODY_MALE.fold_length, BODY_FEMALE.fold_length));
    assert!(between(mid.body().open_phase, BODY_MALE.open_phase, BODY_FEMALE.open_phase));
    assert!(default_f0(mid) > 110.0 && default_f0(mid) < 205.0);
    // calibrated targets: each key between the two presets'
    for v in ["i", "a", "u"] {
        let (m, f, x) = (vowel_artic(v, BodyMix::MALE).unwrap(), vowel_artic(v, BodyMix::FEMALE).unwrap(), vowel_artic(v, mid).unwrap());
        for (k, xv) in x.iter() {
            assert!(between(xv, m.get(k).unwrap(), f.get(k).unwrap()), "{v} {k:?}");
        }
    }
    let (m, c, x) = (phoneme("s", BodyMix::MALE).unwrap(), phoneme("s", BodyMix::CHILD).unwrap(), phoneme("s", BodyMix::new(0.0, 0.5)).unwrap());
    for (k, xv) in x.targets.iter() {
        assert!(between(xv, m.targets.get(k).unwrap(), c.targets.get(k).unwrap()), "s {k:?}");
    }
    // the formants of /a/ for a body half way to a child sit between the adult male's and the child's
    let f = |b: BodyMix| {
        let a: HumanArtic = vowel_artic("a", b).unwrap().apply_to(&NEUTRAL);
        let mut w = HumanAirway::new(96000.0, b.body());
        w.apply(&a, 0.0);
        let el = w.net.linear_elements();
        let freqs: Vec<f64> = (0..360).map(|i| 60.0 * (8000.0f64 / 60.0).powf(i as f64 / 359.0)).collect();
        let h = transfer_function(&el, w.pharynx.first_node(), &freqs, &[w.glottis_port]);
        find_peaks(&freqs, &h.mag, 150.0).iter().map(|p| p.freq).take(2).collect::<Vec<_>>()
    };
    let (fm, fc, fx) = (f(BodyMix::MALE), f(BodyMix::CHILD), f(BodyMix::new(0.0, 0.5)));
    for i in 0..2 {
        assert!(fx[i] > fm[i] && fx[i] < fc[i], "F{}: male {} half {} child {}", i + 1, fm[i], fx[i], fc[i]);
    }
}

#[test]
fn a_blended_voice_plays() {
    let mut e = Engine::new(48000.0);
    let info = e.set_body(BodyMix::new(0.4, 0.3));
    assert_eq!(info.body, BodyMix::new(0.4, 0.3));
    e.set_gate(1.0);
    let mut out = vec![0.0f32; 24000];
    e.render(&mut out);
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak > 1e-3, "peak {peak}");
}
