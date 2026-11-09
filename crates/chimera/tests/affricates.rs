//! A voiceless affricate is a closure, then a release straight into frication that lasts until the
//! voicing starts (experiment 22). Its constriction used to let go 40 ms before the folds closed: a
//! short /ʃ/ and then an [h], heard as a stop and an [h] ("chain" as "king"). A voiced one is the same with
//! a closure that starts voiced (experiment 29).
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE, HumanBody};
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use chimera::voice::human_voice::{HumanVoice, SourcePatch};

const FS: f64 = 48000.0;
const FRAME: usize = 480;

/// Per 10 ms frame: the level (dB), and the share (dB) left by two one-pole low-passes at 1 kHz.
/// /ʃ/, with its peak above 2 kHz, loses most of its energy to them; an [h] and a vowel don't.
fn frames(x: &[f32]) -> Vec<(f64, f64)> {
    let a = 1.0 - (-2.0 * std::f64::consts::PI * 1000.0 / FS).exp();
    let (mut y1, mut y2) = (0.0f64, 0.0f64);
    x.chunks_exact(FRAME)
        .map(|f| {
            let (mut all, mut low) = (1e-20f64, 1e-20f64);
            for s in f {
                let s = f64::from(*s);
                y1 += a * (s - y1);
                y2 += a * (y1 - y2);
                all += s * s;
                low += y2 * y2;
            }
            (10.0 * (all / FRAME as f64).log10(), 10.0 * (low / all).log10())
        })
        .collect()
}

/// A VCV's frames, with where its affricate's span falls, in frames.
fn say(ipa: &str, affricate: &str, body: HumanBody, name: &str, f0: f64) -> (Vec<(f64, f64)>, f64, f64) {
    let plan = plan_utterance(&tokenize_ipa(ipa), &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
    let span = plan.segments.iter().find(|s| s.ipa == affricate).unwrap();
    let (t0, t1) = (span.t0, span.t1);
    let mut v = HumanVoice::new(FS, body, 2);
    v.set_source(&SourcePatch::f0(f0));
    v.schedule(&plan.gestures);
    let mut out = vec![0.0f32; (FS * (plan.end + 0.3)).round() as usize];
    for chunk in out.chunks_mut(128) {
        v.render(chunk);
    }
    (frames(&out), t0, t1)
}

/// How long the frication lasts (ms), and how long after it the voice arrives (ms).
fn frication(fr: &[(f64, f64)], t0: f64, t1: f64, name: &str) -> (usize, usize) {
    let at = |t: f64| (t * FS / FRAME as f64) as usize;
    let vowel = fr[at(t1 + 0.1)..at(t1 + 0.14)].iter().map(|f| f.0).fold(f64::MIN, f64::max);
    let fric: Vec<usize> = (at(t0)..at(t1 + 0.1)).filter(|&i| fr[i].1 < -10.0 && fr[i].0 > vowel - 30.0).collect();
    let (first, last) = (*fric.first().unwrap_or_else(|| panic!("{name}: no frication")), *fric.last().unwrap());
    // the voice: as loud as the vowel, and with its energy below 1 kHz. Level alone would take a
    // loud frication for the vowel, and report no gap whatever follows it.
    let voiced = (last + 1..fr.len()).find(|&i| fr[i].0 > vowel - 8.0 && fr[i].1 >= -10.0).unwrap();
    ((last - first + 1) * 10, (voiced - last - 1) * 10)
}

/// The detector itself: a frication as loud as the vowel, then a quiet [h], then the vowel.
#[test]
fn the_gap_measure_sees_an_h_after_a_loud_frication() {
    let at = |t: f64| (t * FS / FRAME as f64) as usize;
    let (t0, t1) = (0.1, 0.2);
    let mut fr = vec![(-60.0, 0.0); at(0.6)];
    fr[at(t0)..at(t1)].fill((-16.0, -20.0)); // frication, with little below 1 kHz
    fr[at(t1)..at(t1 + 0.12)].fill((-35.0, -3.0)); // 120 ms of a quiet [h]
    fr[at(t1 + 0.12)..].fill((-15.0, -1.0)); // the vowel
    let (length, gap) = frication(&fr, t0, t1, "made up");
    assert_eq!((length, gap), (100, 120), "frication {length} ms, then {gap} ms to the voice");
}

const VOICES: [(HumanBody, &str, f64); 2] = [(BODY_MALE, "male", 112.0), (BODY_FEMALE, "female", 205.0)];

#[test]
fn a_voiceless_affricates_frication_runs_into_the_vowel_with_no_h_after_it() {
    for (body, name, f0) in VOICES {
        let (fr, t0, t1) = say("ˈɑtʃɑ", "tʃ", body, name, f0);
        let (length, gap) = frication(&fr, t0, t1, name);
        println!("{name}: frication {length} ms, then {gap} ms to the voice");
        assert!(length >= 50, "{name}: frication lasts {length} ms (real /tʃ/: 50-80)");
        assert!(gap <= 20, "{name}: {gap} ms between the frication and the voice");
    }
}

/// A voiced affricate has frication too (experiment 29). Its folds stayed together, so voicing ran through
/// a release with no audible frication, MFA called that the vowel, and /dʒ/ read half its length. What sets
/// it apart from /tʃ/ is that the voice carries into its closure and fades there, where /tʃ/'s folds have
/// parted and its closure starts with a breath. The closing's frames: their least share below 1 kHz.
#[test]
fn a_voiced_affricate_closes_on_the_voice_and_releases_into_frication() {
    let at = |t: f64| (t * FS / FRAME as f64) as usize;
    let closing = |f: &[(f64, f64)], t: f64| f[at(t - 0.01)..at(t + 0.03)].iter().map(|x| x.1).fold(f64::MAX, f64::min);
    for (body, name, f0) in VOICES {
        let (fr, t0, t1) = say("ˈɑdʒɑ", "dʒ", body, name, f0);
        let (length, gap) = frication(&fr, t0, t1, name);
        let (tsh, s0, _) = say("ˈɑtʃɑ", "tʃ", body, name, f0);
        let (voiced, breathy) = (closing(&fr, t0), closing(&tsh, s0));
        println!("{name}: frication {length} ms, then {gap} ms to the voice; closing's share below 1 kHz {voiced:.1} dB, /tʃ/ {breathy:.1} dB");
        assert!(length >= 30, "{name}: frication lasts {length} ms (the speakers' /dʒ/: about 45-60)");
        assert!(gap <= 20, "{name}: {gap} ms between the frication and the voice");
        assert!(voiced > breathy + 3.0, "{name}: closing's share below 1 kHz {voiced:.1} dB, /tʃ/ {breathy:.1} dB");
    }
}
