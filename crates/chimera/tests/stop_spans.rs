//! A stop's span starts where it sounds closed (experiment 26). Klatt's durations are acoustic. With
//! the closing gesture aimed at contact, the level fell 10 dB a median 13-21 ms before a /t d/ after a
//! vowel and 25 ms before the /t/ of /st/, which read 110 ms where the speakers' read 70-80, with the
//! /s/ 20-30 ms short. And the two things that land with it: /ð/ after a pause is a silent closure,
//! and the port seals later before a voiced stop.
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE};
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use chimera::phonetics::{Plan, Segment};
use chimera::voice::human_voice::{HumanVoice, SourcePatch, VoiceTelemetry};

const FS: f64 = 48000.0;
const BLOCK: usize = 128;

struct Render {
    plan: Plan,
    /// block end, block power, velum port
    blocks: Vec<(f64, f64, f64)>,
}

fn render(ipa: &str, name: &str) -> Render {
    let (body, f0) = if name == "female" { (BODY_FEMALE, 205.0) } else { (BODY_MALE, 112.0) };
    let input: Vec<Segment> = tokenize_ipa(ipa);
    let plan = plan_utterance(&input, &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.schedule(&plan.gestures);
    let mut block = vec![0.0f32; BLOCK];
    let mut tel = VoiceTelemetry::default();
    let mut blocks = Vec::new();
    let mut n = 0usize;
    while (n as f64) < FS * (plan.end + 0.1) {
        v.render(&mut block);
        v.telemetry(&mut tel);
        n += BLOCK;
        blocks.push((n as f64 / FS, block.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / BLOCK as f64, tel.velum_port));
    }
    Render { plan, blocks }
}

/// dB over about 10 ms around each block, so an f0 period doesn't show.
fn levels(r: &Render) -> Vec<f64> {
    (0..r.blocks.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 2).min(r.blocks.len() - 1));
            10.0 * (r.blocks[a..=b].iter().map(|x| x.1).sum::<f64>() / (b - a + 1) as f64 + 1e-14).log10()
        })
        .collect()
}

/// Median over the stops after `prev_is` of where the level first falls 10 dB under its level 40-60 ms
/// before the span, in ms from the span's start.
fn median_fall(ipa: &str, name: &str, prev_is: impl Fn(&str, &str) -> bool) -> (usize, f64) {
    let r = render(ipa, name);
    let l = levels(&r);
    let mut d = Vec::new();
    for (i, s) in r.plan.segments.iter().enumerate() {
        if s.kind != "stop" || i == 0 || !prev_is(&r.plan.segments[i - 1].kind, &r.plan.segments[i - 1].ipa) {
            continue;
        }
        let pre: Vec<f64> = r.blocks.iter().zip(&l).filter(|(b, _)| b.0 >= s.t0 - 0.06 && b.0 <= s.t0 - 0.04).map(|(_, x)| *x).collect();
        let pre = pre.iter().sum::<f64>() / pre.len() as f64;
        let fall = r.blocks.iter().zip(&l).find(|(b, x)| b.0 >= s.t0 - 0.05 && b.0 <= s.t1 + 0.02 && **x < pre - 10.0);
        d.push(fall.map_or(f64::NAN, |(b, _)| (b.0 - s.t0) * 1000.0));
    }
    d.sort_by(f64::total_cmp);
    (d.len(), d[d.len() / 2])
}

#[test]
fn a_stop_after_a_vowel_sounds_closed_about_when_its_span_starts() {
    // the vowels of running speech, mostly short and high: a fixed delay can't land every vowel's on
    // its span, as the travel moves it (from /ə/ about 10 ms late, from /i/ about 10 early)
    let by_place = [
        ("lips", "ˈlipɪˈsʌbəˈtɪpəˈɹʊbɪˈsipəˈnɪbɪˈkɑpə"),
        ("tip", "ˈsitɪˈnɪdəˈbɪtəˈkidɪˈgʊtəˈɹɪdəˈnitɪˈmʌdɪˈbɝtəˈlɛdəˈkætɪ"),
        ("dorsum", "ˈsikɪˈbɪgəˈtɪkəˈligɪˈbʊkəˈdɪgɪˈpikəˈlʌkɪˈbɑkə"),
    ];
    for name in ["male", "female"] {
        for (place, ipa) in by_place {
            let (n, median) = median_fall(ipa, name, |kind, _| kind == "vowel");
            assert!(n >= 5 && median.abs() < 7.0, "{name} {place}: median {median} ms over {n} stops");
        }
    }
}

#[test]
fn a_stop_after_s_sounds_closed_about_when_its_span_starts() {
    for name in ["male", "female"] {
        let (n, median) = median_fall("ˈɑstɪˈsʌstəˈmɛstɪˈkæstəˈlistɪˈnʌstə", name, |_, ipa| ipa == "s");
        assert!(n >= 5 && median.abs() < 7.0, "{name}: median {median} ms over {n} stops");
    }
}

/// As a voiced fricative it read −10 to −14 dB under the stressed vowels through its span, where the
/// speakers' sits under −40 until about 20 ms before its end.
#[test]
fn dh_after_a_pause_is_a_silent_closure() {
    for name in ["male", "female"] {
        let r = render("ðəˈmæn", name);
        let db_in = |a: f64, b: f64| {
            let v: Vec<f64> = r.blocks.iter().filter(|x| x.0 >= a && x.0 <= b).map(|x| x.1).collect();
            10.0 * (v.iter().sum::<f64>() / v.len() as f64 + 1e-14).log10()
        };
        let (dh, vowel) = (&r.plan.segments[0], r.plan.segments.iter().find(|s| s.stress == Some(1)).unwrap());
        assert_eq!(dh.ipa, "ð");
        let closure = db_in(dh.t0, (dh.t0 + dh.t1) / 2.0) - db_in(vowel.t0, vowel.t1);
        assert!(closure < -30.0, "{name}: {closure} dB re the vowel");
    }
}

/// The last block the port is open, in ms after the nasal's span.
fn seal_after_span(ipa: &str, name: &str) -> f64 {
    let r = render(ipa, name);
    let n = r.plan.segments.iter().find(|s| s.ipa == "n").unwrap();
    let last = r.blocks.iter().filter(|b| b.2 > 0.05).last().unwrap();
    (last.0 - n.t1) * 1000.0
}

#[test]
fn the_port_seals_later_before_a_voiced_stop() {
    for name in ["male", "female"] {
        let (voiced, voiceless) = (seal_after_span("ˈɑndɑ", name), seal_after_span("ˈɑntɑ", name));
        assert!(voiced - voiceless > 10.0, "{name}: /nd/ +{voiced} ms, /nt/ +{voiceless} ms");
    }
}

/// The loudest pair of blocks (about 5 ms) around a stop's release, dB re the vowel before it.
fn release_burst(r: &Render) -> f64 {
    let stop = r.plan.segments.iter().find(|s| s.kind == "stop").unwrap();
    let vowel = r.plan.segments.iter().find(|s| s.kind == "vowel").unwrap();
    let mean = |a: f64, b: f64| {
        let v: Vec<f64> = r.blocks.iter().filter(|x| x.0 > a && x.0 <= b).map(|x| x.1).collect();
        v.iter().sum::<f64>() / v.len() as f64
    };
    let at = r.blocks.iter().filter(|x| x.0 > stop.t1 - 0.01 && x.0 <= stop.t1 + 0.02);
    let near: Vec<f64> = at.map(|x| x.1).collect();
    let burst = near.windows(2).map(|w| 0.5 * (w[0] + w[1])).fold(0.0, f64::max);
    10.0 * (burst / mean(vowel.t0 + 0.03, vowel.t1 - 0.02)).log10()
}

/// Lips opened at the full area rate from the first sample stepped up the flow's slope: a thump the
/// speakers' /p/ doesn't have (experiment 27). The male read -4.5 dB here; men's read -11.8 before a vowel.
#[test]
fn a_lip_release_starts_slowly() {
    let b = release_burst(&render("ˈɑpɑ", "male"));
    assert!(b < -5.0, "/p/ release {b:.1} dB re the vowel");
}

/// /ð/ is a closure after any pause, not only the utterance's start: the `_` of IPA's `|` is a pause.
#[test]
fn dh_after_an_explicit_pause_is_a_closure_too() {
    let kind = |ipa: &str| {
        let plan = plan_utterance(&tokenize_ipa(ipa), &PlanOptions { body: "male".into(), t0: 0.05, f0: Some(112.0), ..Default::default() });
        plan.segments.iter().find(|s| s.ipa == "ð").unwrap().kind.clone()
    };
    assert_eq!(kind("ðə"), "stop", "at the utterance's start");
    assert_eq!(kind("ɑ | ðə"), "stop", "after an explicit pause");
    assert_eq!(kind("ɑðə"), "fricative", "between vowels");
}
