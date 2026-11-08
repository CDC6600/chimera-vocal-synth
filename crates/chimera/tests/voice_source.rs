//! How the human voice's source settings shape the glottis.
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

#[test]
fn a_pressed_voice_still_closes_abruptly() {
    // a sung or spoken voice closes softly (tension 0.5-0.55), but the "Pressed" control's top end
    // keeps the abrupt closure of a pressed voice, and the closure sharpens with tension throughout
    let mut v = HumanVoice::with_defaults(48000.0);
    let mut sharpness = |tension: f64| {
        v.set_source(&SourcePatch { tension: Some(tension), ..Default::default() });
        v.glottis.params.closure_sharpness
    };
    assert!(sharpness(0.55) < 0.25);
    assert!(sharpness(1.0) > 0.9);
    let mut last = -1.0;
    for i in 0..=20 {
        let t = i as f64 / 20.0;
        let s = sharpness(t);
        assert!(s > last, "closure sharpness falls at tension {t}");
        last = s;
    }
}

#[test]
fn brightness_tilts_the_voice_and_its_default_changes_nothing() {
    // the Bright control: at its default the closure is what tension alone gives; turned up, the
    // folds close more abruptly and the voice carries more high-frequency energy
    let mut v = HumanVoice::with_defaults(48000.0);
    v.set_source(&SourcePatch { tension: Some(0.55), ..Default::default() });
    let default = v.glottis.params.closure_sharpness;
    assert!((default - 0.22).abs() < 1e-12, "{default}");
    let mut tilt = |b: f64| {
        let mut v = HumanVoice::with_defaults(48000.0);
        v.set_source(&SourcePatch { tension: Some(0.55), brightness: Some(b), ..Default::default() });
        v.set_gate(1.0);
        let mut out = vec![0.0f32; 48000];
        for c in out.chunks_mut(128) {
            v.render(c);
        }
        // mean squared first difference over mean square: rises with the share of high frequencies
        let x = &out[24000..];
        let d: f64 = x.windows(2).map(|w| ((w[1] - w[0]) as f64).powi(2)).sum();
        let e: f64 = x.iter().map(|s| (*s as f64).powi(2)).sum();
        (v.glottis.params.closure_sharpness, 10.0 * (d / e).log10())
    };
    let (dark, mid, bright) = (tilt(0.0), tilt(0.25), tilt(1.0));
    assert_eq!(mid.0, default);
    assert!(dark.0 < mid.0 && mid.0 < bright.0);
    println!("high-frequency share (dB): dark {:.1}, default {:.1}, bright {:.1}", dark.1, mid.1, bright.1);
    // little room to darken (the default closure is already soft), a lot to brighten
    assert!(dark.1 < mid.1 && mid.1 < bright.1 - 3.0, "{dark:?} {mid:?} {bright:?}");
}
