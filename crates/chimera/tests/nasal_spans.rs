//! A nasal's span is where it sounds closed, and a vowel and the /ɹ/ after it share one span
//! (experiment 25). Klatt's durations are acoustic. With the closing gesture aimed at contact, the
//! oral area fell under 0.05 cm² 7-20 ms before the span, the nasal read 90 ms where its plan said 56
//! and the speakers' read 70, and each sentence's vowels before nasals lost that time.
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE};
use chimera::phonetics::durations::{SegInfo, klatt_durations};
use chimera::phonetics::inventory::Manner;
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use chimera::phonetics::Segment;
use chimera::voice::human_voice::{HumanVoice, SourcePatch, VoiceTelemetry};

const FS: f64 = 48000.0;

/// For each nasal after a vowel in a phrase, the time from its span's start to where the oral area
/// first falls under 0.05 cm², in ms.
fn closures_vs_spans(ipa: &str, name: &str) -> Vec<f64> {
    let (body, f0) = if name == "female" { (BODY_FEMALE, 205.0) } else { (BODY_MALE, 112.0) };
    let plan = plan_utterance(&tokenize_ipa(ipa), &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.schedule(&plan.gestures);
    let mut block = vec![0.0f32; 128];
    let mut tel = VoiceTelemetry::default();
    let mut closed = Vec::new();
    let mut n = 0usize;
    while (n as f64) < FS * plan.end {
        v.render(&mut block);
        v.telemetry(&mut tel);
        n += 128;
        closed.push((n as f64 / FS, tel.or_areas.iter().any(|a| *a < 0.05)));
    }
    let mut out = Vec::new();
    for (i, s) in plan.segments.iter().enumerate() {
        if matches!(s.ipa.as_str(), "m" | "n" | "ŋ") && i > 0 && plan.segments[i - 1].kind == "vowel" {
            let at = closed.iter().find(|(t, c)| *t > s.t0 - 0.06 && *c).unwrap_or_else(|| panic!("/{}/ never closes", s.ipa)).0;
            out.push((at - s.t0) * 1000.0);
        }
    }
    out
}

/// The median nasal: a fixed delay can't land every one on its span. Its travel moves it, as a stop's
/// (experiment 17): from a stressed /ɑ/ it lands about 20 ms late, from /i/ about 10 early.
#[test]
fn a_nasal_sounds_closed_about_when_its_span_starts() {
    for name in ["male", "female"] {
        let mut d = closures_vs_spans("ðəˈmɛnɪˈsʌməˈlaɪnzɑnəˈfɔɹmɪŋˈlɔŋəˈsɪŋɪŋˈsʌmənˈɡɹinˈfɪŋɡɚ", name);
        d.sort_by(f64::total_cmp);
        let median = d[d.len() / 2];
        assert!(d.len() >= 10 && median.abs() < 7.0, "{name}: {d:?}");
    }
}

fn seg(ipa: &str) -> Segment {
    Segment { ipa: ipa.to_string(), ..Default::default() }
}

fn info(s: &Segment) -> SegInfo {
    match s.ipa.as_str() {
        "ɑ" => SegInfo { vowel: true, ..Default::default() },
        "ɹ" | "l" => SegInfo { vowel: false, manner: Some(Manner::Approximant), voiced: Some(true), dur: None },
        _ => SegInfo { vowel: false, manner: Some(Manner::Stop), voiced: Some(false), dur: None },
    }
}

#[test]
fn a_vowel_and_its_coda_r_share_the_vowels_span() {
    // /l/ takes the same inherent duration and the same context rules as /ɹ/, and no nucleus
    let with_l = klatt_durations(&[seg("p"), seg("ɑ"), seg("l"), seg("k")], info);
    let with_r = klatt_durations(&[seg("p"), seg("ɑ"), seg("ɹ"), seg("k")], info);
    assert!((with_r[1] + with_r[2] - with_l[1]).abs() < 1e-9, "ɑ {} + ɹ {} against ɑ {}", with_r[1], with_r[2], with_l[1]);
    // a word's last /ɹ/ is its coda even before a vowel: "car is" keeps "car"'s nucleus
    let end = Segment { word_end: Some(true), ..seg("ɹ") };
    let linked = klatt_durations(&[seg("p"), seg("ɑ"), end.clone(), seg("ɑ")], info);
    let closed = klatt_durations(&[seg("p"), seg("ɑ"), end, seg("k"), seg("ɑ")], info);
    assert!(linked[1] + linked[2] < 0.75 * (klatt_durations(&[seg("p"), seg("ɑ"), seg("ɹ"), seg("ɑ")], info)[1..3].iter().sum::<f64>()), "\"car is\": ɑ {} + ɹ {}", linked[1], linked[2]);
    assert!((linked[1] - closed[1]).abs() < 0.03, "ɑ {} before a vowel in the next word, {} before a consonant", linked[1], closed[1]);
    // an /ɹ/ that opens the next syllable is not a coda
    let open = klatt_durations(&[seg("p"), seg("ɑ"), seg("ɹ"), seg("ɑ")], info);
    let open_l = klatt_durations(&[seg("p"), seg("ɑ"), seg("l"), seg("ɑ")], info);
    assert!((open[1] - open_l[1]).abs() < 1e-9);
}
