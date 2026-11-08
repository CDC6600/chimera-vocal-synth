//! A voiceless affricate is a closure, then a release straight into frication that lasts until the
//! voicing starts (experiment 22). Its constriction used to let go 40 ms before the folds closed: a
//! short /ʃ/ and then an [h], heard as a stop and an [h] ("chain" as "king").
use chimera::anatomy::human::{BODY_FEMALE, BODY_MALE};
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

#[test]
fn a_voiceless_affricates_frication_runs_into_the_vowel_with_no_h_after_it() {
    for (body, name, f0) in [(BODY_MALE, "male", 112.0), (BODY_FEMALE, "female", 205.0)] {
        let plan = plan_utterance(&tokenize_ipa("ˈɑtʃɑ"), &PlanOptions { body: name.into(), t0: 0.05, f0: Some(f0), ..Default::default() });
        let span = plan.segments.iter().find(|s| s.ipa == "tʃ").unwrap();
        let (t0, t1) = (span.t0, span.t1);
        let mut v = HumanVoice::new(FS, body, 2);
        v.set_source(&SourcePatch::f0(f0));
        v.schedule(&plan.gestures);
        let mut out = vec![0.0f32; (FS * (plan.end + 0.3)).round() as usize];
        for chunk in out.chunks_mut(128) {
            v.render(chunk);
        }
        let fr = frames(&out);
        let at = |t: f64| (t * FS / FRAME as f64) as usize;
        let vowel = fr[at(t1 + 0.1)..at(t1 + 0.14)].iter().map(|f| f.0).fold(f64::MIN, f64::max);
        let fric: Vec<usize> = (at(t0)..at(t1 + 0.1)).filter(|&i| fr[i].1 < -10.0 && fr[i].0 > vowel - 30.0).collect();
        let (first, last) = (*fric.first().unwrap(), *fric.last().unwrap());
        let voiced = (last..fr.len()).find(|&i| fr[i].0 > vowel - 8.0).unwrap();
        let (length, gap) = ((last - first + 1) * 10, (voiced - last - 1) * 10);
        println!("{name}: frication {length} ms, then {gap} ms to the voice");
        assert!(length >= 50, "{name}: frication lasts {length} ms (real /tʃ/: 50-80)");
        assert!(gap <= 20, "{name}: {gap} ms between the frication and the voice");
    }
}
