//! Fast sung runs (scat): every vowel lands on its beat however short the note, and the pitch
//! moves on the onset consonant, so it has arrived when the vowel starts.
use chimera::control::score::ControlKey;
use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::sing::{SongOptions, SungNote, midi_to_hz, plan_song};
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera::phonetics::{Plan, PlannedSegment};

fn g2p() -> EnglishG2P {
    let mut g = EnglishG2P::new();
    g.load_dict("ah\tAA1\nbah\tB AA1\nbee\tB IY1\nbop\tB AA1 P\ncha\tCH AA1\ndee\tD IY1\ndoo\tD UW1\nja\tJH AA1\nscoo\tS K UW1\nskee\tS K IY1\nstrut\tS T R AH1 T\n");
    g
}

/// The melody alternates C4 and E4, `beats` long each.
fn sing(lyrics: &str, beats: f64, bpm: f64) -> (Vec<SungNote>, Vec<f64>, Plan) {
    let syl = lyrics_to_syllables(lyrics, &g2p());
    let notes: Vec<SungNote> = syl.iter().enumerate().map(|(i, s)| SungNote { midi: if i % 2 == 0 { 60.0 } else { 64.0 }, beats, syl: s.clone() }).collect();
    let times = (0..notes.len()).map(|i| 0.5 + i as f64 * beats * 60.0 / bpm).collect();
    let plan = plan_song(&notes, &SongOptions { body: "male".into(), bpm, t0: 0.5, vibrato: 0.0 });
    (notes, times, plan)
}

/// The first vowel segment of each syllable (a diphthong's second part follows its first).
fn vowel_onsets(segs: &[PlannedSegment]) -> Vec<f64> {
    segs.iter().enumerate().filter(|(i, s)| s.kind == "vowel" && (*i == 0 || segs[i - 1].kind != "vowel")).map(|(_, s)| s.t0).collect()
}

#[test]
fn fast_runs_land_on_the_beat() {
    for lyrics in ["scoo bah dee bop scoo bah dee bop", "skee dee bop bah skee dee bop bah", "bah bee doo bop bah bee doo bop", "strut strut strut strut"] {
        for (bpm, beats) in [(184.0, 0.25), (184.0, 1.0 / 3.0), (240.0, 0.25), (100.0, 0.5)] {
            if lyrics.starts_with("strut") && bpm == 240.0 {
                continue; // four consonants in a 62 ms note: see too_many_consonants_degrade_gently
            }
            let (_, times, plan) = sing(lyrics, beats, bpm);
            let got = vowel_onsets(&plan.segments);
            assert_eq!(got.len(), times.len(), "{lyrics:?} at {bpm} bpm");
            for (k, (g, want)) in got.iter().zip(&times).enumerate() {
                assert!((g - want).abs() < 1e-6, "{lyrics:?}, {beats} beats at {bpm} bpm: vowel {k} at {g:.4} s, its beat is {want:.4} s");
            }
            // and every vowel still sounds for a while
            for s in plan.segments.iter().filter(|s| s.kind == "vowel") {
                assert!(s.t1 - s.t0 >= 0.02 - 1e-9, "{lyrics:?}: a {:.3} s vowel", s.t1 - s.t0);
            }
        }
    }
}

#[test]
fn too_many_consonants_degrade_gently() {
    // "t | s t r" between vowels can't shrink below its floor in a 62 ms note: the vowels keep
    // their 20 ms and the run slips a few milliseconds a note instead of a whole note
    let (_, times, plan) = sing("strut strut strut strut", 0.25, 240.0);
    for (k, (g, want)) in vowel_onsets(&plan.segments).iter().zip(&times).enumerate() {
        assert!(g - want < 0.008 * k as f64 + 1e-6, "vowel {k} {:.1} ms late", (g - want) * 1000.0);
    }
}

#[test]
fn notes_with_room_keep_their_consonants() {
    // at a walking pace nothing is squeezed: the onset is the consonant's sung length
    let (_, _, plan) = sing("bah dee bah dee", 1.0, 100.0);
    let d = plan.segments.iter().find(|s| s.ipa == "d").unwrap();
    assert!(d.t1 - d.t0 > 0.04, "d squeezed to {:.3} s", d.t1 - d.t0);
}

#[test]
fn onset_consonants_carry_the_new_pitch() {
    let (notes, _, plan) = sing("bah dee bah dee", 1.0 / 3.0, 184.0);
    let d = plan.segments.iter().find(|s| s.ipa == "d").unwrap();
    let hz = midi_to_hz(notes[1].midi);
    let on_d = plan.gestures.iter().any(|g| (g.t0 - d.t0).abs() < 1e-9 && g.targets.get(ControlKey::F0) == Some(hz));
    assert!(on_d, "no pitch gesture for {hz:.1} Hz starting on the 'd' at {:.4} s", d.t0);
}

#[test]
fn a_delayed_cluster_closure_stays_inside_a_short_sung_stop() {
    // In "harped" the /t/ closes behind the /p/ with its tip already raised by the /ɹ/, so its
    // closure is aimed later (the pop on "sharp tree"). In a quarter-beat note at 180 bpm the /t/
    // lasts only a few milliseconds: a fixed 25 ms delay pushed its closure 23 ms into the next
    // vowel and reversed its secondary gesture.
    let mut g = EnglishG2P::new();
    g.load_dict("harped\tHH AA1 R P T\nah\tAA1\n");
    let syl = lyrics_to_syllables("harped ah harped ah", &g);
    let notes: Vec<SungNote> = syl.iter().map(|s| SungNote { midi: 60.0, beats: 0.25, syl: s.clone() }).collect();
    let plan = plan_song(&notes, &SongOptions { body: "male".into(), bpm: 180.0, t0: 0.5, vibrato: 0.0 });
    let ts: Vec<_> = plan.segments.iter().enumerate().filter(|(i, s)| s.ipa == "t" && *i > 0 && plan.segments[i - 1].ipa == "p").map(|(_, s)| s.clone()).collect();
    assert!(!ts.is_empty());
    for t in ts {
        for gst in plan.gestures.iter().filter(|g| g.label.as_ref().is_some_and(|l| l.as_str() == "t") && g.t0 >= t.t0 - 0.05 && g.t0 <= t.t1) {
            assert!(gst.t0 <= gst.t1, "a /t/ gesture runs backwards: {:.4} → {:.4}", gst.t0, gst.t1);
            assert!(gst.t1 <= t.t1 + 1e-9, "a /t/ gesture ends at {:.4} s, after its /t/ ({:.4}–{:.4} s)", gst.t1, t.t0, t.t1);
        }
    }
}

/// A fast sung note squeezes an affricate to a few ms (12.5 ms for a 0.125-beat vowel before /tʃɑ/ at
/// 120 bpm). Its folds still close inside its span, or the breath spills into the vowel (it ran 19 ms
/// into it), and its release doesn't start ahead of the span.
#[test]
fn a_squeezed_affricate_keeps_its_folds_and_its_release_inside_its_span() {
    for lyrics in ["ah cha ah cha", "ah ja ah ja"] {
        for (bpm, beats) in [(120.0, 0.125), (184.0, 0.25), (240.0, 0.25), (120.0, 0.25), (90.0, 0.5)] {
            let (_, _, plan) = sing(lyrics, beats, bpm);
            let spans: Vec<&PlannedSegment> = plan.segments.iter().filter(|s| s.ipa == "tʃ" || s.ipa == "dʒ").collect();
            assert!(!spans.is_empty(), "{lyrics:?}");
            for span in spans {
                let folds: Vec<_> = plan
                    .gestures
                    .iter()
                    .filter(|g| g.t0 >= span.t0 - 1e-9 && g.t0 < span.t1 && g.label.as_ref().is_some_and(|l| matches!(l.as_str(), "aspiration" | "voiced affricate")))
                    .collect();
                assert!(!folds.is_empty(), "{lyrics:?} at {bpm} bpm: no folds in a {:.1} ms span", (span.t1 - span.t0) * 1000.0);
                for g in folds {
                    let end = g.t1 + g.ramp_out;
                    assert!(end <= span.t1 + 1e-9, "{lyrics:?}, {beats} beats at {bpm} bpm: the folds close {:.1} ms into the vowel", (end - span.t1) * 1000.0);
                }
                // the release is the fricative's own gesture (its speed picks it out from the
                // closure's), with its 8 ms lead
                let releases: Vec<_> = plan.gestures.iter().filter(|g| g.label.as_ref().is_some_and(|l| l.as_str() == span.ipa) && g.speed == Some(1.4)).filter(|g| (g.t1 - span.t1).abs() < 0.03).collect();
                assert!(!releases.is_empty(), "{lyrics:?} at {bpm} bpm: no release");
                for g in releases {
                    assert!(g.t0 >= span.t0 - 0.008 - 1e-9, "{lyrics:?}, {beats} beats at {bpm} bpm: the release starts {:.1} ms before the span", (span.t0 - g.t0) * 1000.0);
                }
            }
        }
    }
}
