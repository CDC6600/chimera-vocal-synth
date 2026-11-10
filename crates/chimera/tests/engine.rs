//! The instrument as a whole: the beatbox bar of the TypeScript worklet's scheduling reproduced
//! through `Engine`, and output independent of the host's block size with the sequencer playing,
//! a body switch, schedules and releases at fixed absolute sample positions.
mod common;
use chimera::anatomy::human::{ArticKey, ArticPatch, BodyName};
use chimera::control::score::{Gesture, Targets};
use chimera::engine::Engine;
use chimera::phonetics::beatbox::{BeatStep, beat_ready, parse_beat};
use chimera::voice::human_voice::SourcePatch;
use common::*;

#[test]
fn engine_plays_the_beatbox_bar_like_the_typescript_worklet() {
    let g = load("planning");
    let a = &g["audio"];
    let reference = load_f32("beatbox-bar");
    let mut e = Engine::new(num(&a["fs"]));
    e.set_source(&SourcePatch::f0(num(&a["f0"])));
    e.set_artic(&beat_ready("male".into()), None);
    e.set_beat_steps(&parse_beat(a["text"].as_str().unwrap()));
    e.set_beat_tempo(Some(num(&a["bpm"])), Some(num(&a["swing"])));
    e.set_beat_playing(true);
    let n = a["samples"].as_u64().unwrap() as usize;
    let tempo_at = a["tempoAt"].as_u64().unwrap() as usize * 128;
    let tempo = nums(&a["tempo"]);
    let mut out = vec![0.0f32; n];
    e.render(&mut out[..tempo_at]);
    e.set_beat_tempo(Some(tempo[0]), Some(tempo[1]));
    e.render(&mut out[tempo_at..]);
    let d = compare_audio(&reference, &out, 1e-6, 480);
    println!("beatbox bar through Engine: max err {:.2e} of peak, first >1e-6 at {:?}", d.max_err, d.first_diverge);
    assert!(d.first_diverge.is_none(), "{d:?}");
}

/// Commands at absolute sample positions, then the samples, rendered in blocks of `block`.
fn session(block: usize) -> Vec<f32> {
    let fs = 48000.0;
    let mut e = Engine::new(fs);
    let n = 2 * 48000;
    let mut out = vec![0.0f32; n];
    let mut note = Targets::default();
    note.set(ArticKey::TbX, 3.6).set(chimera::control::score::ControlKey::F0, 180.0).set(chimera::control::score::ControlKey::BREATH, 1.0);
    let held = [Gesture::new(0.0, 1e9, 0.03, 0.08, 4.0, note).tag(3.0).label("ɑ")];
    // (sample, command)
    let events: Vec<(usize, Box<dyn Fn(&mut Engine)>)> = vec![
        (0, Box::new(|e: &mut Engine| e.set_beat_steps(&parse_beat("B t Pf t | B B Pf ^K")))),
        (0, Box::new(|e: &mut Engine| e.set_beat_tempo(Some(120.0), Some(0.15)))),
        (0, Box::new(|e: &mut Engine| e.set_beat_playing(true))),
        (20000, Box::new(|e: &mut Engine| e.beat_hit(&BeatStep::new("K")))),
        (30000, Box::new(|e: &mut Engine| e.set_beat_tempo(Some(90.0), None))),
        (41000, Box::new(|e: &mut Engine| {
            e.set_body(BodyName::Female);
        })),
        (55000, Box::new(|e: &mut Engine| e.set_beat_playing(false))),
        (56000, Box::new(move |e: &mut Engine| {
            e.schedule(&held, 0.01, false);
        })),
        (60000, Box::new(|e: &mut Engine| e.set_artic(&ArticPatch::default().with(ArticKey::Jaw, 1.2), None))),
        (75000, Box::new(|e: &mut Engine| e.release(3.0))),
        (80000, Box::new(|e: &mut Engine| e.set_gate(1.0))),
    ];
    let mut i = 0;
    let mut ei = 0;
    while i < n {
        while ei < events.len() && events[ei].0 <= i {
            (events[ei].1)(&mut e);
            ei += 1;
        }
        let next = if ei < events.len() { events[ei].0 } else { n };
        let end = n.min(i + block).min(next.max(i + 1));
        e.render(&mut out[i..end]);
        i = end;
    }
    out
}

#[test]
fn output_does_not_depend_on_the_host_block_size() {
    let reference = session(128);
    let peak = reference.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.05, "the session should make sound (peak {peak})");
    for block in [1usize, 77, 4096, 48000] {
        let got = session(block);
        let first = reference.iter().zip(&got).position(|(a, b)| a.to_bits() != b.to_bits());
        assert_eq!(first, None, "block size {block}: differs from 128-sample blocks at sample {first:?}");
    }
}

/// A held "ah" from the start button: two seconds at this output rate, its RMS over the second.
fn held_voice_rms(fs: f64) -> f64 {
    let mut e = Engine::new(fs);
    e.set_gate(1.0);
    let mut out = vec![0.0f32; 2 * fs as usize];
    for chunk in out.chunks_mut(128) {
        e.render(chunk);
    }
    let tail = &out[fs as usize..];
    assert!(tail.iter().all(|x| x.is_finite()), "{fs} Hz: the voice went NaN");
    (tail.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / tail.len() as f64).sqrt()
}

#[test]
fn the_voice_holds_at_a_bluetooth_headsets_sample_rate() {
    // Issue #33: the browser's context runs at the output device's rate. 2x oversampling put a
    // 16 kHz device's airway at 32 kHz, below its stable rate: NaN within a second, then silence.
    // 24 kHz ran at the limiter ceiling. Each rate now sounds like 48 kHz does.
    let reference = held_voice_rms(48000.0);
    for fs in [8000.0, 16000.0, 22050.0, 24000.0, 32000.0] {
        let rms = held_voice_rms(fs);
        assert!(rms > 0.5 * reference && rms < 2.0 * reference, "{fs} Hz: RMS {rms:.3}, 48 kHz {reference:.3}");
    }
    // the usual rates simulate as before (the golden fixtures stand)
    use chimera::voice::human_voice::oversample_for;
    assert_eq!([44100.0, 48000.0, 60000.0, 88200.0, 96000.0].map(oversample_for), [2, 2, 2, 1, 1]);
}
