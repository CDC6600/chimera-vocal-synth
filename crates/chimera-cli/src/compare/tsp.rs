//! `chimera compare tsp`: say the Harvard sentences that the TSP speech database's speakers read
//! (P. Kabal, McGill University, 2018; CC BY 4.0; fetch with `tools/tsp/fetch.sh`), and measure
//! the synth against them, phone by phone. See docs/speech-comparison.md.

use super::{N_CEP, N_MEL, fx, hz_to_mel, mcd, mean, median, read_wav};
use chimera::analysis::fft::{Fft, hann};
use chimera::analysis::lpc::{FormantOpts, estimate_formants};
use chimera::analysis::pitch::{YinOpts, yin_pitch};
use crate::render::{g2p, say};
use crate::{Ctx, repo_path};
use chimera::phonetics::Segment;
use chimera::phonetics::english::{EnglishG2P, arpabet_to_segments};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One phone of a recording (MFA's alignment) or of a render (the plan), in the engine's IPA.
#[derive(Clone, Debug)]
pub struct Phone {
    pub ipa: String,
    /// lexical stress of a vowel (0, 1, 2); 0 for consonants
    pub stress: u8,
    pub t0: f64,
    pub t1: f64,
}

/// One recording: the sentence a speaker read, and MFA's phones for it.
pub struct Utt {
    /// e.g. FA01_03: speaker FA, Harvard list 1, sentence 3
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub phones: Vec<Phone>,
}

impl Utt {
    pub fn female(&self) -> bool {
        self.speaker.starts_with('F')
    }
    /// the sentence's id without the speaker: "01_03"
    pub fn sentence(&self) -> &str {
        &self.id[2..]
    }
}

/// The adult recordings that have an alignment, from tools/tsp/sentences.tsv and alignment.tsv.gz.
pub fn load_utts() -> Result<Vec<Utt>, String> {
    let sent_path = repo_path("tools/tsp/sentences.tsv");
    let sentences = std::fs::read_to_string(&sent_path).map_err(|e| format!("{sent_path}: {e}"))?;
    let al_path = repo_path("tools/tsp/alignment.tsv.gz");
    let gz = std::fs::read(&al_path).map_err(|e| format!("{al_path}: {e}"))?;
    let mut text = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut text)
        .map_err(|e| format!("{al_path}: {e}"))?;
    let mut phones: BTreeMap<&str, Vec<Phone>> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 5 {
            return Err(format!("{al_path}: bad line {line:?}"));
        }
        if f[1] != "phone" {
            continue;
        }
        let (t0, t1) = (
            f[2].parse::<f64>().map_err(|e| format!("{al_path}: {e}"))?,
            f[3].parse::<f64>().map_err(|e| format!("{al_path}: {e}"))?,
        );
        // "spn" (a word MFA's dictionary lacks) stays in as a phone no engine phone matches
        let (ipa, stress) = match arpabet_to_segments(f[4]).pop() {
            Some(s) => (s.ipa, s.stress.unwrap_or(0)),
            None => (f[4].to_string(), 0),
        };
        phones.entry(f[0]).or_default().push(Phone { ipa, stress, t0, t1 });
    }
    let mut out = Vec::new();
    for line in sentences.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [id, speaker, text] = f[..] else {
            return Err(format!("{sent_path}: bad line {line:?}"));
        };
        if let Some(p) = phones.remove(id) {
            out.push(Utt { id: id.into(), speaker: speaker.into(), text: text.into(), phones: p });
        }
    }
    Ok(out)
}

/// The synth saying one sentence in one body: its samples and its phones (the plan's segments).
pub struct Said {
    pub x: Vec<f64>,
    pub phones: Vec<Phone>,
}

/// Say every distinct (sentence, body) the recordings need, on `jobs` threads.
fn say_all(utts: &[&Utt], jobs: usize) -> Result<BTreeMap<(String, bool), Said>, String> {
    let g = g2p()?;
    let mut want: Vec<(String, bool, String)> = utts
        .iter()
        .map(|u| (u.sentence().to_string(), u.female(), u.text.clone()))
        .collect();
    want.sort();
    want.dedup();
    let next = AtomicUsize::new(0);
    let done: Mutex<BTreeMap<(String, bool), Said>> = Mutex::new(BTreeMap::new());
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((sid, female, text)) = want.get(i) else { break };
                    match say(&g, text, if *female { "female" } else { "male" }) {
                        Ok(sp) => {
                            let phones = sp
                                .plan
                                .segments
                                .iter()
                                .map(|p| Phone { ipa: p.ipa.clone(), stress: p.stress.unwrap_or(0), t0: p.t0, t1: p.t1 })
                                .collect();
                            let x = sp.out.iter().map(|&v| v as f64).collect();
                            done.lock().unwrap().insert((sid.clone(), *female), Said { x, phones });
                        }
                        Err(e) => errors.lock().unwrap().push(format!("{sid}: {e}")),
                    }
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(done.into_inner().unwrap())
}

/// The ARPAbet (with a stress digit on vowels) of a G2P segment, before allophones: the inverse of
/// the G2P's table (`english.rs`), so MFA aligns exactly what the engine says.
fn to_arpa(s: &Segment) -> Result<String, String> {
    let stress = s.stress.unwrap_or(0);
    let (base, vowel) = match s.ipa.as_str() {
        "ɑ" => ("AA", true),
        "æ" => ("AE", true),
        "ʌ" => ("AH", true),
        "ə" => return Ok("AH0".into()),
        "ɔ" => ("AO", true),
        "aʊ" => ("AW", true),
        "aɪ" => ("AY", true),
        "ɛ" => ("EH", true),
        "ɝ" => ("ER", true),
        "ɚ" => return Ok("ER0".into()),
        "eɪ" => ("EY", true),
        "ɪ" => ("IH", true),
        "i" => ("IY", true),
        "oʊ" => ("OW", true),
        "ɔɪ" => ("OY", true),
        "ʊ" => ("UH", true),
        "u" => ("UW", true),
        "b" => ("B", false),
        "tʃ" => ("CH", false),
        "d" => ("D", false),
        "ð" => ("DH", false),
        "f" => ("F", false),
        "g" => ("G", false),
        "h" => ("HH", false),
        "dʒ" => ("JH", false),
        "k" => ("K", false),
        "l" => ("L", false),
        "m" => ("M", false),
        "n" => ("N", false),
        "ŋ" => ("NG", false),
        "p" => ("P", false),
        "ɹ" => ("R", false),
        "s" => ("S", false),
        "ʃ" => ("SH", false),
        "t" => ("T", false),
        "θ" => ("TH", false),
        "v" => ("V", false),
        "w" => ("W", false),
        "j" => ("Y", false),
        "z" => ("Z", false),
        "ʒ" => ("ZH", false),
        other => return Err(format!("no ARPAbet for {other:?}")),
    };
    Ok(if vowel { format!("{base}{stress}") } else { base.into() })
}

/// The words of a sentence as the G2P says them, hyphenated compounds split (the G2P says their
/// parts as words).
fn said_words(g: &EnglishG2P, text: &str) -> Vec<String> {
    g.to_phrases(text)
        .into_iter()
        .flat_map(|p| p.words)
        .flat_map(|w| w.split('-').filter(|p| !p.is_empty()).map(String::from).collect::<Vec<_>>())
        .collect()
}

/// A render over a noise floor like the recordings': white noise (a fixed sequence, so a render
/// always aligns the same) `ALIGN_FLOOR_DB` below its loud level. A render's closures are near
/// digital silence where a recording's sit on room and preamp noise, and without a floor MFA gives
/// them to silence and reads the stop as its release alone.
fn over_noise_floor(x: &[f64]) -> Vec<f32> {
    let n20 = (0.02 * crate::FS) as usize;
    let frames: Vec<f64> = x.chunks(n20).filter(|c| c.len() == n20).map(mean_square).collect();
    let rms = (percentile(&frames, 0.95) * 10f64.powf(ALIGN_FLOOR_DB / 10.0) * 3.0).sqrt();
    let mut state: u32 = 0x2545_f491;
    x.iter()
        .map(|&v| {
            // xorshift32, uniform in ±1 (variance 1/3, hence the 3 above)
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (v + rms * (state as f64 / 2_147_483_648.0 - 1.0)) as f32
        })
        .collect()
}

/// The Montreal Forced Aligner that `tools/tsp/setup.sh` installs, with its working files in
/// `root` (one per run: MFA keeps them under the corpus directory's name, and two runs at once
/// would share them) and the path of its english_us_arpa acoustic model.
fn mfa_command(root: &str) -> Result<(std::process::Command, String), String> {
    let home = std::env::var("HOME").map_err(|e| format!("HOME: {e}"))?;
    let cache = format!("{home}/.cache/chimera-mfa");
    let bin = format!("{cache}/env/bin");
    let model = format!("{cache}/mfa/pretrained_models/acoustic/english_us_arpa.zip");
    if !std::path::Path::new(&format!("{bin}/mfa")).exists() || !std::path::Path::new(&model).exists() {
        return Err(format!("no aligner in {cache}: run tools/tsp/setup.sh once"));
    }
    let mut c = std::process::Command::new(format!("{bin}/mfa"));
    c.env("MAMBA_ROOT_PREFIX", format!("{cache}/root"))
        .env("MFA_ROOT_DIR", root)
        .env("PATH", format!("{bin}:{}", std::env::var("PATH").unwrap_or_default()));
    Ok((c, model))
}

/// MFA's version and a fingerprint of its settings (tools/tsp/mfa.yaml): the renders' boundaries
/// move with either, so the verdict compares only runs that share both.
fn aligner_id(dir: &str) -> Result<String, String> {
    let dir = std::fs::canonicalize(dir).map_err(|e| format!("{dir}: {e}"))?.to_string_lossy().into_owned();
    let (mut mfa, _) = mfa_command(&format!("{dir}/root"))?;
    let out = mfa.arg("version").output().map_err(|e| format!("mfa version: {e}"))?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(format!("mfa {version}, settings {}", fingerprint(&repo_path("tools/tsp/mfa.yaml"))))
}

/// Align the renders with MFA as the recordings were (tools/tsp/mfa.yaml: one utterance per
/// "speaker", no dither, no speaker adaptation, so each file's alignment depends on that file
/// alone and is the same on every run), with a dictionary of exactly what the G2P said.
fn align_synth(
    g: &EnglishG2P,
    said: &BTreeMap<(String, bool), Said>,
    texts: &BTreeMap<String, String>,
    dir: &str,
    jobs: usize,
) -> Result<BTreeMap<(String, bool), Vec<Phone>>, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    // absolute paths: MFA's database step fails on a relative working directory
    let dir = std::fs::canonicalize(dir).map_err(|e| format!("{dir}: {e}"))?.to_string_lossy().into_owned();
    let corpus = format!("{dir}/corpus");
    let aligned = format!("{dir}/aligned");
    let mut dict: BTreeMap<String, String> = BTreeMap::new();
    let name = |sid: &str, female: bool| format!("{}-{sid}", if female { "female" } else { "male" });
    for ((sid, female), s) in said {
        let words = said_words(g, &texts[sid]);
        for w in &words {
            if !dict.contains_key(w) {
                let arpa: Result<Vec<String>, String> = g.word(w, true).iter().map(to_arpa).collect();
                dict.insert(w.clone(), arpa.map_err(|e| format!("{w}: {e}"))?.join(" "));
            }
        }
        let stem = name(sid, *female);
        let d = format!("{corpus}/{stem}");
        std::fs::create_dir_all(&d).map_err(|e| format!("{d}: {e}"))?;
        // 16-bit PCM, as the recordings are, over a noise floor like theirs (the measures use
        // the clean render)
        let x = over_noise_floor(&s.x);
        crate::wav::write(&format!("{d}/{stem}.wav"), &x, crate::FS as u32, false).map_err(|e| format!("{d}: {e}"))?;
        std::fs::write(format!("{d}/{stem}.lab"), words.join(" ") + "\n").map_err(|e| format!("{d}: {e}"))?;
    }
    let dict_path = format!("{dir}/synth.dict");
    let lines: String = dict.iter().map(|(w, p)| format!("{w}\t{p}\n")).collect();
    std::fs::write(&dict_path, lines).map_err(|e| format!("{dict_path}: {e}"))?;
    // a working directory of its own, so two runs at once don't share MFA's files
    let (mut mfa, model) = mfa_command(&format!("{dir}/root"))?;
    let status = mfa
        .args(["align", "-q", "--clean", "-c", &repo_path("tools/tsp/mfa.yaml"), "-j", &jobs.to_string()])
        .args(["--output_format", "json"])
        .args([corpus.as_str(), dict_path.as_str(), model.as_str(), aligned.as_str()])
        .status()
        .map_err(|e| format!("mfa: {e}"))?;
    if !status.success() {
        return Err(format!("mfa align failed ({status}); its files are in {dir}"));
    }
    let mut out = BTreeMap::new();
    for (sid, female) in said.keys() {
        let stem = name(sid, *female);
        let path = format!("{aligned}/{stem}/{stem}.json");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let entries = v["tiers"]["phones"]["entries"].as_array().ok_or(format!("{path}: no phones"))?;
        let mut phones = Vec::new();
        for e in entries {
            let (t0, t1, label) = (e[0].as_f64().unwrap_or(f64::NAN), e[1].as_f64().unwrap_or(f64::NAN), e[2].as_str().unwrap_or(""));
            if matches!(label, "" | "sil" | "sp" | "<eps>") {
                continue;
            }
            let (ipa, stress) = match arpabet_to_segments(label).pop() {
                Some(s) => (s.ipa, s.stress.unwrap_or(0)),
                None => (label.to_string(), 0),
            };
            phones.push(Phone { ipa, stress, t0, t1 });
        }
        out.insert((sid.clone(), *female), phones);
    }
    Ok(out)
}

// ---- measuring one voice on one utterance

/// Spectrum frames: 1024 samples (21 ms at 48 kHz), Hann, every 5 ms.
const FRAME: usize = 1024;
const HOP: f64 = 0.005;
/// Pitch and formants run on a copy decimated 4:1 (48 → 12 kHz).
const DEC: usize = 4;
/// Mel bands for the spectrum measure: 60 Hz–10 kHz (fricatives reach well past the singer
/// judge's 8 kHz).
const MEL_HI: f64 = 10000.0;
/// A pitch frame quieter than this far below the utterance's loud level is unvoiced.
const GATE_DB: f64 = 40.0;
/// YIN's aperiodicity threshold for a voiced frame (on the band below 1.2 kHz)
const YIN_THRESHOLD: f64 = 0.3;
/// The noise floor renders are aligned over (dB re their loud level): the recordings' median,
/// measured as `Sig::new` does (the 5th against the 95th percentile of 20 ms frames).
const ALIGN_FLOOR_DB: f64 = -47.0;
/// Below this many tokens a speaker's category value is left out.
const MIN_TOKENS: usize = 3;

const VOWELS: [&str; 17] = ["ɑ", "æ", "ʌ", "ə", "ɔ", "aʊ", "aɪ", "ɛ", "ɝ", "ɚ", "eɪ", "ɪ", "i", "oʊ", "ɔɪ", "ʊ", "u"];
/// vowels whose F1 and F2 are measured: the stressable monophthongs (diphthongs move, and
/// /ə ɚ/ only occur reduced)
const MONOPHTHONGS: [&str; 10] = ["i", "ɪ", "ɛ", "æ", "ɑ", "ɔ", "ʌ", "ʊ", "u", "ɝ"];
const STOPS: [&str; 6] = ["p", "t", "k", "b", "d", "g"];
const SIBILANTS: [&str; 5] = ["s", "z", "ʃ", "tʃ", "dʒ"];
const AFFRICATES: [&str; 2] = ["tʃ", "dʒ"];
/// consonants measured (ʒ is too rare: about one per speaker)
const CONSONANTS: [&str; 23] = [
    "p", "t", "k", "b", "d", "g", "tʃ", "dʒ", "f", "v", "θ", "ð", "s", "z", "ʃ", "h", "m", "n", "ŋ", "l", "ɹ", "w", "j",
];

fn is_vowel(ipa: &str) -> bool {
    VOWELS.contains(&ipa)
}

fn db(ms: f64) -> f64 {
    10.0 * (ms + 1e-30).log10()
}

fn mean_square(x: &[f64]) -> f64 {
    if x.is_empty() { f64::NAN } else { x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64 }
}

fn percentile(xs: &[f64], q: f64) -> f64 {
    let mut s: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return f64::NAN;
    }
    s.sort_by(f64::total_cmp);
    let pos = q * (s.len() - 1) as f64;
    let (i, f) = (pos.floor() as usize, pos.fract());
    if i + 1 < s.len() { s[i] * (1.0 - f) + s[i + 1] * f } else { s[i] }
}

/// What one voice does on one phone.
#[derive(Clone)]
pub struct Tok {
    pub ipa: String,
    pub stressed: bool,
    /// where it starts (s) and how long it lasts (s)
    pub t0: f64,
    pub secs: f64,
    /// what follows the phone in its voice's phones
    pub next: Next,
    /// duration over the utterance's mean phone duration
    pub dur: f64,
    /// middle-half level (dB) re the utterance's stressed vowels, noise floor subtracted; NaN
    /// when within 3 dB of the floor
    pub level: f64,
    /// stops: loudest 5 ms (dB) re the stressed vowels
    pub burst: f64,
    /// sibilants: power-weighted mean frequency 1–10 kHz (Hz)
    pub centroid: f64,
    /// ln of the mel band powers averaged over the middle half
    pub mel: [f64; N_MEL],
    /// share of 10 ms pitch frames in the middle half that are voiced
    pub voiced: f64,
    pub f1: f64,
    pub f2: f64,
    /// stressed vowels: band levels (dB re 0–8 kHz) over the middle half, as the singer judge's
    pub bands: [f64; 5],
    /// stressed vowels: first harmonic minus second (dB) at the middle, at that frame's f0
    pub h1h2: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    /// a voiceless obstruent
    Voiceless,
    /// a voiced obstruent
    Voiced,
    /// a sonorant or a vowel
    Sonorant,
    /// nothing: the utterance's last phone
    End,
}

const VOICELESS_OBSTRUENTS: [&str; 9] = ["p", "t", "k", "f", "θ", "s", "ʃ", "tʃ", "h"];
const VOICED_OBSTRUENTS: [&str; 8] = ["b", "d", "g", "v", "ð", "z", "ʒ", "dʒ"];

/// One voice's signal, prepared once per utterance.
struct Sig<'a> {
    x: &'a [f64],
    fs: f64,
    /// 4:1 decimated copy (12 kHz), for formants
    lo: Vec<f64>,
    /// mean-square noise floor: the 5th percentile of 20 ms frames (0 for a render's silence)
    noise: f64,
    /// f0 every 10 ms (frame i centred at i·10 ms), 0 when unvoiced: YIN on the band below
    /// 1.2 kHz (3 kHz copy), where voicing is strong and frication weak, so a voiced fricative
    /// or a voice bar reads as voiced
    f0: Vec<f64>,
}

/// Windowed-sinc low-pass (95 taps, Blackman; cutoff as a fraction of the input rate) and keep
/// every DEC-th sample.
fn decimate(x: &[f64], fc: f64) -> Vec<f64> {
    const TAPS: usize = 95;
    let h: Vec<f64> = (0..TAPS)
        .map(|i| {
            let m = i as f64 - (TAPS - 1) as f64 / 2.0;
            let sinc = if m == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * m).sin() / (std::f64::consts::PI * m) };
            let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (TAPS - 1) as f64).cos()
                + 0.08 * (4.0 * std::f64::consts::PI * i as f64 / (TAPS - 1) as f64).cos();
            sinc * w
        })
        .collect();
    let half = (TAPS - 1) / 2;
    (0..x.len() / DEC)
        .map(|k| {
            let c = k * DEC;
            let mut s = 0.0;
            for (j, hj) in h.iter().enumerate() {
                let i = c as isize + j as isize - half as isize;
                if i >= 0 && (i as usize) < x.len() {
                    s += hj * x[i as usize];
                }
            }
            s
        })
        .collect()
}

impl<'a> Sig<'a> {
    fn new(x: &'a [f64], fs: f64, female: bool) -> Self {
        let lo = decimate(x, 5400.0 / fs);
        let band = decimate(&lo, 1200.0 / (fs / DEC as f64));
        let fs_band = fs / (DEC * DEC) as f64;
        let n20 = (0.02 * fs) as usize;
        let frames: Vec<f64> = x.chunks(n20).filter(|c| c.len() == n20).map(mean_square).collect();
        let noise = percentile(&frames, 0.05);
        let loud = percentile(&frames, 0.95);
        let gate = loud * 10f64.powf(-GATE_DB / 10.0);
        // speaking pitch ranges wide enough for every TSP speaker and the synth's intonation
        let (min_f0, max_f0) = if female { (110.0, 420.0) } else { (55.0, 260.0) };
        let len = (2.0 * fs_band / min_f0).round() as usize;
        let opts = YinOpts { min_f0: Some(min_f0), max_f0: Some(max_f0), threshold: Some(YIN_THRESHOLD) };
        let n_frames = (x.len() as f64 / fs / 0.01) as usize;
        let k = DEC * DEC;
        let f0 = (0..n_frames)
            .map(|i| {
                let c = (i as f64 * 0.01 * fs_band) as isize - (len / 2) as isize;
                if c < 0 || c as usize + len > band.len() {
                    return 0.0;
                }
                // the gate is on the full-band signal's level over the same frame
                let full = &x[(c as usize * k).min(x.len())..((c as usize + len) * k).min(x.len())];
                if mean_square(full) < gate { 0.0 } else { yin_pitch(&band[c as usize..c as usize + len], fs_band, opts).f0 }
            })
            .collect();
        Sig { x, fs, lo, noise, f0 }
    }

    fn slice(&self, a: f64, b: f64) -> &[f64] {
        let (i, j) = ((a * self.fs).max(0.0) as usize, ((b * self.fs) as usize).min(self.x.len()));
        if i < j { &self.x[i..j] } else { &[] }
    }

    /// The middle half of a phone, at least its centre 10 ms.
    fn middle(p: &Phone) -> (f64, f64) {
        let (c, d) = (0.5 * (p.t0 + p.t1), p.t1 - p.t0);
        let half = (0.25 * d).max(0.005);
        (c - half, c + half)
    }

    /// Power spectrum averaged over frames centred every HOP in [a, b].
    fn spectrum(&self, fft: &Fft, win: &[f64], a: f64, b: f64) -> Option<Vec<f64>> {
        let mut acc = vec![0.0; FRAME / 2 + 1];
        let mut count = 0;
        let mut t = a;
        while t <= b + 1e-9 {
            let c = (t * self.fs) as isize - (FRAME / 2) as isize;
            if c >= 0 && c as usize + FRAME <= self.x.len() {
                let mut re: Vec<f64> = (0..FRAME).map(|i| self.x[c as usize + i] * win[i]).collect();
                let mut im = vec![0.0; FRAME];
                fft.transform(&mut re, &mut im);
                for k in 0..=FRAME / 2 {
                    acc[k] += re[k] * re[k] + im[k] * im[k];
                }
                count += 1;
            }
            t += HOP;
        }
        (count > 0).then(|| acc.into_iter().map(|v| v / count as f64).collect())
    }

    /// F1 and F2 (Hz) at 35, 50 and 65 % of a vowel: LPC on the 12 kHz copy, 25 ms frames;
    /// the median of the frames that find them.
    fn formants(&self, p: &Phone, female: bool) -> (f64, f64) {
        let fs_lo = self.fs / DEC as f64;
        let len = (0.025 * fs_lo) as usize;
        let opts = FormantOpts { order: Some(if female { 12 } else { 14 }), max_bw: Some(600.0), min_freq: Some(150.0), ..Default::default() };
        let (mut f1s, mut f2s) = (Vec::new(), Vec::new());
        for q in [0.35, 0.5, 0.65] {
            let t = p.t0 + q * (p.t1 - p.t0);
            let c = (t * fs_lo) as isize - (len / 2) as isize;
            if c < 0 || c as usize + len > self.lo.len() {
                continue;
            }
            let fm = estimate_formants(&self.lo[c as usize..c as usize + len], fs_lo, opts);
            let Some(f1) = fm.iter().find(|f| f.freq < 1300.0 && f.bw < 500.0) else { continue };
            let Some(f2) = fm.iter().find(|f| f.freq > f1.freq + 200.0 && f.freq < 3500.0) else { continue };
            f1s.push(f1.freq);
            f2s.push(f2.freq);
        }
        (median(&f1s), median(&f2s))
    }

    /// H1−H2 (dB) at time t: a 43 ms frame (resolving harmonics down to 70 Hz) and the f0 of the
    /// pitch frame there; NaN when unvoiced.
    fn h1h2(&self, tools: &Tools, t: f64) -> f64 {
        let f0 = self.f0.get((t / 0.01).round() as usize).copied().unwrap_or(0.0);
        let n = tools.long.n;
        let c = (t * self.fs) as isize - (n / 2) as isize;
        if f0 <= 0.0 || c < 0 || c as usize + n > self.x.len() {
            return f64::NAN;
        }
        let mut re: Vec<f64> = (0..n).map(|i| self.x[c as usize + i] * tools.long_win[i]).collect();
        let mut im = vec![0.0; n];
        tools.long.transform(&mut re, &mut im);
        let p: Vec<f64> = (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
        harmonic_db(&p, self.fs / n as f64, f0) - harmonic_db(&p, self.fs / n as f64, 2.0 * f0)
    }

    /// Share of voiced pitch frames between a and b (NaN if none falls there).
    fn voiced(&self, a: f64, b: f64) -> f64 {
        let (i, j) = ((a / 0.01).ceil().max(0.0) as usize, ((b / 0.01).floor() as usize).min(self.f0.len().saturating_sub(1)));
        if i > j {
            // a window narrower than the 10 ms grid: the nearest frame
            let k = ((0.5 * (a + b)) / 0.01).round() as usize;
            return self.f0.get(k).map_or(f64::NAN, |&f| if f > 0.0 { 1.0 } else { 0.0 });
        }
        (i..=j).filter(|&k| self.f0[k] > 0.0).count() as f64 / (j - i + 1) as f64
    }
}

/// Level (dB) of the harmonic near `hz` in a power spectrum with bins `bin` Hz apart: the
/// strongest bin within ±3 % (at least ±2 bins), refined by a parabola (as the singer judge's).
fn harmonic_db(p: &[f64], bin: f64, hz: f64) -> f64 {
    let half = (0.03 * hz / bin).max(2.0);
    let (lo, hi) = (((hz / bin - half).floor().max(1.0)) as usize, ((hz / bin + half).ceil()) as usize);
    if hi + 1 >= p.len() {
        return f64::NAN;
    }
    let k = (lo..=hi).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
    let (a, b, c) = (db(p[k - 1]), db(p[k]), db(p[k + 1]));
    let den = a - 2.0 * b + c;
    if den < 0.0 { b - 0.125 * (a - c) * (a - c) / den } else { b }
}

/// Mel filterbank over FRAME-point FFT bins at `fs`: N_MEL triangles, 60 Hz–MEL_HI.
fn mel_bank(fs: f64) -> Vec<Vec<(usize, f64)>> {
    let bin = fs / FRAME as f64;
    let (m0, m1) = (hz_to_mel(60.0), hz_to_mel(MEL_HI));
    let edge = |k: usize| {
        let m = m0 + (m1 - m0) * k as f64 / (N_MEL + 1) as f64;
        700.0 * (10f64.powf(m / 2595.0) - 1.0)
    };
    (0..N_MEL)
        .map(|k| {
            let (lo, mid, hi) = (edge(k), edge(k + 1), edge(k + 2));
            (0..=FRAME / 2)
                .filter_map(|i| {
                    let f = i as f64 * bin;
                    let w = if f > lo && f <= mid {
                        (f - lo) / (mid - lo)
                    } else if f > mid && f < hi {
                        (hi - f) / (hi - mid)
                    } else {
                        0.0
                    };
                    (w > 0.0).then_some((i, w))
                })
                .collect()
        })
        .collect()
}

/// Shared analysis tables.
struct Tools {
    fft: Fft,
    win: Vec<f64>,
    /// 2048-point frames for H1−H2
    long: Fft,
    long_win: Vec<f64>,
    mel: Vec<Vec<(usize, f64)>>,
    fs: f64,
}

impl Tools {
    fn new(fs: f64) -> Self {
        Tools { fft: Fft::new(FRAME), win: hann(FRAME), long: Fft::new(2 * FRAME), long_win: hann(2 * FRAME), mel: mel_bank(fs), fs }
    }
}

/// The singer judge's bands (Hz), each re the 0–8 kHz total.
const BANDS: [(f64, f64); 5] = [(0.0, 500.0), (500.0, 1000.0), (1000.0, 2000.0), (2000.0, 4000.0), (4000.0, 8000.0)];
const BAND_NAMES: [&str; 5] = ["0-.5k", ".5-1k", "1-2k", "2-4k", "4-8k"];

/// Utterance-level measures of one voice.
#[derive(Clone, Copy)]
pub struct UttMeas {
    /// phones per second of phone time
    pub rate: f64,
    /// 10th–90th percentile of voiced f0 over the phones (semitones)
    pub f0_range: f64,
}

/// Measure one voice: the utterance measures and a token per wanted phone index.
fn measure(tools: &Tools, sig: &Sig, phones: &[Phone], want: &[usize], female: bool) -> (UttMeas, Vec<Tok>) {
    let total: f64 = phones.iter().map(|p| p.t1 - p.t0).sum();
    let mean_dur = total / phones.len().max(1) as f64;
    let mid_db = |p: &Phone| {
        let (a, b) = Sig::middle(p);
        db(mean_square(sig.slice(a, b)))
    };
    let vref = median(&phones.iter().filter(|p| is_vowel(&p.ipa) && p.stress == 1).map(mid_db).collect::<Vec<_>>());
    let (t_first, t_last) = (phones.first().map_or(0.0, |p| p.t0), phones.last().map_or(0.0, |p| p.t1));
    let voiced_f0: Vec<f64> = sig
        .f0
        .iter()
        .enumerate()
        .filter(|(i, f)| **f > 0.0 && (*i as f64 * 0.01) >= t_first && (*i as f64 * 0.01) <= t_last)
        .map(|(_, f)| *f)
        .collect();
    let um = UttMeas {
        rate: phones.len() as f64 / total,
        f0_range: 12.0 * (percentile(&voiced_f0, 0.9) / percentile(&voiced_f0, 0.1)).log2(),
    };
    let bin = tools.fs / FRAME as f64;
    let toks = want
        .iter()
        .map(|&i| {
            let p = &phones[i];
            let (a, b) = Sig::middle(p);
            let d = p.t1 - p.t0;
            // an affricate's level is its frication's (its second half; the first is the closure)
            let (la, lb) = if AFFRICATES.contains(&p.ipa.as_str()) { (p.t0 + 0.5 * d, p.t1 - 0.1 * d) } else { (a, b) };
            let ms = mean_square(sig.slice(la, lb));
            let level = if ms > 2.0 * sig.noise { db(ms - sig.noise) - vref } else { f64::NAN };
            // a stop's release: MFA gives the burst and any aspiration to the stop, so the window
            // ends with it (past its end lies the next phone, often a louder vowel)
            let burst = if STOPS.contains(&p.ipa.as_str()) {
                let n5 = (0.005 * sig.fs) as usize;
                let seg = sig.slice(p.t0 + d / 3.0, p.t1);
                let step = (0.001 * sig.fs) as usize;
                let mut best = 0.0f64;
                let mut k = 0;
                while k + n5 <= seg.len() {
                    best = best.max(mean_square(&seg[k..k + n5]));
                    k += step;
                }
                if best > 0.0 { db(best) - vref } else { f64::NAN }
            } else {
                f64::NAN
            };
            let mut mel = [f64::NAN; N_MEL];
            let mut centroid = f64::NAN;
            let mut bands = [f64::NAN; 5];
            let stressed_vowel = is_vowel(&p.ipa) && p.stress == 1;
            if let Some(pw) = sig.spectrum(&tools.fft, &tools.win, a, b) {
                for (k, f) in tools.mel.iter().enumerate() {
                    mel[k] = (f.iter().map(|(i, w)| pw[*i] * w).sum::<f64>() + 1e-20).ln();
                }
                if stressed_vowel {
                    let band = |lo: f64, hi: f64| pw.iter().enumerate().filter(|(k, _)| (lo..hi).contains(&(*k as f64 * bin))).map(|(_, v)| v).sum::<f64>();
                    let total = band(0.0, 8000.0) + 1e-30;
                    for (k, (lo, hi)) in BANDS.iter().enumerate() {
                        bands[k] = db(band(*lo, *hi) / total);
                    }
                }
            }
            if SIBILANTS.contains(&p.ipa.as_str()) {
                // an affricate's frication is its second half
                let (ca, cb) = if AFFRICATES.contains(&p.ipa.as_str()) { (p.t0 + 0.5 * d, p.t1 - 0.1 * d) } else { (a, b) };
                if let Some(pw) = sig.spectrum(&tools.fft, &tools.win, ca, cb) {
                    let (mut num, mut den) = (0.0, 0.0);
                    for (k, v) in pw.iter().enumerate() {
                        let f = k as f64 * bin;
                        if (1000.0..=10000.0).contains(&f) {
                            num += f * v;
                            den += v;
                        }
                    }
                    if den > 0.0 {
                        centroid = num / den;
                    }
                }
            }
            let (f1, f2) = if MONOPHTHONGS.contains(&p.ipa.as_str()) { sig.formants(p, female) } else { (f64::NAN, f64::NAN) };
            let next = match phones.get(i + 1).map(|n| n.ipa.as_str()) {
                Some(n) if VOICELESS_OBSTRUENTS.contains(&n) => Next::Voiceless,
                Some(n) if VOICED_OBSTRUENTS.contains(&n) => Next::Voiced,
                Some(_) => Next::Sonorant,
                None => Next::End,
            };
            Tok {
                ipa: p.ipa.clone(),
                stressed: p.stress == 1,
                t0: p.t0,
                secs: d,
                next,
                dur: d / mean_dur,
                level,
                burst,
                centroid,
                mel,
                voiced: sig.voiced(a, b),
                f1,
                f2,
                bands,
                h1h2: if stressed_vowel { sig.h1h2(tools, 0.5 * (p.t0 + p.t1)) } else { f64::NAN },
            }
        })
        .collect();
    (um, toks)
}

/// Pairs (i, j) of the same phone in a and b, from a Levenshtein alignment of the two sequences.
fn match_phones(a: &[Phone], b: &[Phone]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    let sub = |i: usize, j: usize| usize::from(a[i].ipa != b[j].ipa);
    for i in 1..=n {
        for j in 1..=m {
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + sub(i - 1, j - 1));
        }
    }
    let (mut i, mut j, mut out) = (n, m, Vec::new());
    while i > 0 && j > 0 {
        if d[i][j] == d[i - 1][j - 1] + sub(i - 1, j - 1) {
            if sub(i - 1, j - 1) == 0 {
                out.push((i - 1, j - 1));
            }
            i -= 1;
            j -= 1;
        } else if d[i][j] == d[i - 1][j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out.reverse();
    out
}

/// A recording and the synth's render of its sentence, measured on their matched phones.
pub struct Pair {
    pub id: String,
    pub speaker: String,
    pub female: bool,
    pub human: (UttMeas, Vec<Tok>),
    pub synth: (UttMeas, Vec<Tok>),
}

fn measure_pair(tsp: &str, u: &Utt, said: &Said, synth_phones: &[Phone]) -> Result<Pair, String> {
    let (x, fs) = read_wav(&format!("{tsp}/48k/{}/{}.wav", u.speaker, u.id))?;
    if (fs - crate::FS).abs() > 0.5 {
        return Err(format!("{}: {fs} Hz, expected 48 kHz", u.id));
    }
    let tools = Tools::new(fs);
    let pairs = match_phones(&u.phones, synth_phones);
    let hi: Vec<usize> = pairs.iter().map(|p| p.0).collect();
    let si: Vec<usize> = pairs.iter().map(|p| p.1).collect();
    let hs = Sig::new(&x, fs, u.female());
    let ss = Sig::new(&said.x, crate::FS, u.female());
    let mut human = measure(&tools, &hs, &u.phones, &hi, u.female());
    let synth = measure(&tools, &ss, synth_phones, &si, u.female());
    // a pair is one category: the synth's stress (what the G2P said) for both voices
    for (h, s) in human.1.iter_mut().zip(&synth.1) {
        h.stressed = s.stressed;
    }
    Ok(Pair { id: u.id.clone(), speaker: u.speaker.clone(), female: u.female(), human, synth })
}

/// `chimera compare tsp [tsp-dir] [outdir] [--jobs n] [--dump]`
pub fn run(ctx: &Ctx, a: &[String]) -> Result<(), String> {
    let mut pos: Vec<&str> = Vec::new();
    let mut jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut dump = false;
    let mut speakers: Option<String> = None;
    let mut against: Option<String> = None;
    let mut it = a.iter();
    while let Some(x) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match x.as_str() {
            "--jobs" => jobs = val("--jobs")?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--speakers" => speakers = Some(val("--speakers")?),
            "--dump" => dump = true,
            "--against" => against = Some(val("--against")?),
            _ => pos.push(x),
        }
    }
    let default_dir = format!("{}/datasets/tsp", std::env::var("HOME").unwrap_or_default());
    let tsp = pos.first().copied().unwrap_or(&default_dir).to_string();
    let out_dir = pos.get(1).copied().unwrap_or("out/tsp").to_string();
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{out_dir}: {e}"))?;
    let all = load_utts()?;
    let utts: Vec<&Utt> = all
        .iter()
        .filter(|u| speakers.as_ref().is_none_or(|s| s.split(',').any(|k| k == u.speaker)))
        .collect();
    println!("TSP {tsp}: {} recordings, {jobs} jobs → {out_dir}", utts.len());
    let said = say_all(&utts, jobs)?;
    if dump {
        // the renders and their plan phones, for listening and for checking the timing
        for ((sid, female), s) in &said {
            let stem = format!("{out_dir}/{}-{sid}", if *female { "female" } else { "male" });
            let x: Vec<f32> = s.x.iter().map(|&v| v as f32).collect();
            ctx.write_wav(&format!("{stem}.wav"), &x)?;
            let phones: Vec<_> = s.phones.iter().map(|p| json!([p.ipa, p.stress, p.t0, p.t1])).collect();
            std::fs::write(format!("{stem}.json"), serde_json::to_string(&phones).unwrap())
                .map_err(|e| format!("{stem}.json: {e}"))?;
        }
        println!("wrote {} renders", said.len());
    }
    let texts: BTreeMap<String, String> = utts.iter().map(|u| (u.sentence().to_string(), u.text.clone())).collect();
    let g = g2p()?;
    let t = std::time::Instant::now();
    let synth_phones = align_synth(&g, &said, &texts, &format!("{out_dir}/mfa"), jobs)?;
    let aligner = aligner_id(&format!("{out_dir}/mfa"))?;
    println!("aligned {} of {} renders in {:.0} s", synth_phones.len(), said.len(), t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let next = AtomicUsize::new(0);
    let pairs: Mutex<Vec<Pair>> = Mutex::new(Vec::new());
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..jobs.max(1) {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(u) = utts.get(i) else { break };
                    let key = (u.sentence().to_string(), u.female());
                    let Some(sp) = synth_phones.get(&key) else {
                        errors.lock().unwrap().push(format!("{}: the render didn't align", u.id));
                        continue;
                    };
                    match measure_pair(&tsp, u, &said[&key], sp) {
                        Ok(p) => pairs.lock().unwrap().push(p),
                        Err(e) => errors.lock().unwrap().push(e),
                    }
                }
            });
        }
    });
    let mut pairs = pairs.into_inner().unwrap();
    pairs.sort_by(|a, b| a.id.cmp(&b.id));
    for e in errors.into_inner().unwrap() {
        eprintln!("{e}");
    }
    println!("measured {} recordings in {:.0} s", pairs.len(), t.elapsed().as_secs_f64());
    let scored = score(&pairs, &aligner);
    let mut out = scored.json;
    let mut md = report_md(&out);
    if let Some(path) = &against {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let old: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let (v, why, change) = verdict(&old, &out)?;
        let word = match v {
            Verdict::Better => "BETTER",
            Verdict::Worse => "WORSE",
            Verdict::Same => "NO CLEAR CHANGE",
        };
        md.push_str(&format!("\n## Verdict against {path}\n\n**{word}** (training score {change:+.3})\n"));
        for w in &why {
            md.push_str(&format!("- {w}\n"));
        }
        out["verdict"] = json!({ "against": path, "verdict": word, "change": r3(change), "why": why });
        println!("\nverdict against {path}: {word} (training score {change:+.3})");
        for w in &why {
            println!("  {w}");
        }
    }
    let score_path = format!("{out_dir}/score.json");
    std::fs::write(&score_path, serde_json::to_string_pretty(&out).unwrap()).map_err(|e| format!("{score_path}: {e}"))?;
    let md_path = format!("{out_dir}/report.md");
    std::fs::write(&md_path, &md).map_err(|e| format!("{md_path}: {e}"))?;
    println!(
        "score {} (training), {} (held-out); a real speaker scores {} (median, training speakers left out in turn) → {md_path}",
        fx(num(&out["train"]["score"]), 3),
        fx(num(&out["held"]["score"]), 3),
        fx(num(&out["human"]["train_median"]), 3)
    );
    Ok(())
}


// ---- the score

/// TSP's own suggested test speakers (report §3.3): lists 61–72.
pub fn held_out(speaker: &str) -> bool {
    speaker.ends_with('K') || speaker.ends_with('L')
}

/// Bump when a measure, a term or the score changes: the verdict refuses to compare across.
const JUDGE_VERSION: u32 = 2;
/// The scored groups, one share each. They hold the measures checked against the data and
/// against what the owner heard (docs/speech-comparison.md, "What is scored").
const GROUPS: [&str; 3] = ["level", "sibilants", "timing"];
/// Reported with costs on the same scale, but not scored until they are validated.
const DIAGNOSTICS: [&str; 5] = ["spectrum", "formants", "voice", "voicing", "intonation"];
/// A term needs this many training speakers of a sex with a value.
const MIN_SPEAKERS: usize = 5;
/// The verdict: a training score change of this much counts.
const MARGIN: f64 = 0.01;
/// The verdict: no scored term's cost may rise by more than this.
const TERM_SLACK: f64 = 0.5;
/// The verdict: no scored term may lose more than this share of the synth tokens it rests on.
const SUPPORT_SLACK: f64 = 0.05;

/// How a term summarises its tokens.
#[derive(Clone, Copy, PartialEq)]
enum Stat {
    Median,
    /// voiced shares
    Mean,
    /// the loudest tenth (pops are rare)
    P90,
    /// the shortest tenth (a vowel squeezed in a few contexts)
    P10,
}

/// How a term is measured: which tokens, which value, how it is summarised.
#[derive(Clone)]
struct TermDef {
    name: String,
    group: &'static str,
    /// the token's value, if this token counts for the term
    take: fn(&Tok, &str) -> Option<f64>,
    /// the phone the term is about ("" for a pooled term)
    cat: String,
    stat: Stat,
    /// compare on a log2 scale (frequencies)
    log: bool,
}

fn term_defs() -> Vec<TermDef> {
    use Stat::*;
    let mut d = Vec::new();
    let mut add = |name: String, group, cat: &str, take: fn(&Tok, &str) -> Option<f64>, stat, log| {
        d.push(TermDef { name, group, take, cat: cat.into(), stat, log })
    };
    for c in CONSONANTS {
        add(format!("level {c}"), "level", c, |t, c| (t.ipa == c).then_some(t.level), Median, false);
    }
    for c in STOPS {
        add(format!("burst {c}"), "level", c, |t, c| (t.ipa == c).then_some(t.burst), Median, false);
    }
    add("burst, loudest tenth".into(), "level", "", |t, _| STOPS.contains(&t.ipa.as_str()).then_some(t.burst), P90, false);
    // English speakers often leave a sentence's last stop unreleased ("truck."); the owner heard
    // the synth's release there as a tinny noise
    add("burst, final stops".into(), "level", "", |t, _| (STOPS.contains(&t.ipa.as_str()) && t.next == Next::End).then_some(t.burst), Median, false);
    for c in SIBILANTS {
        add(format!("centroid {c}"), "sibilants", c, |t, c| (t.ipa == c).then_some(t.centroid), Median, true);
    }
    for c in CONSONANTS {
        add(format!("duration {c}"), "timing", c, |t, c| (t.ipa == c).then_some(t.dur), Median, false);
    }
    add("duration stressed vowel before voiceless".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && t.stressed && t.next == Next::Voiceless).then_some(t.dur), Median, false);
    add("duration stressed vowel before voiced".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && t.stressed && t.next == Next::Voiced).then_some(t.dur), Median, false);
    add("duration stressed vowel before sonorant".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && t.stressed && t.next == Next::Sonorant).then_some(t.dur), Median, false);
    add("duration stressed vowel, final".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && t.stressed && t.next == Next::End).then_some(t.dur), Median, false);
    add("duration stressed vowels, shortest tenth".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && t.stressed).then_some(t.dur), P10, false);
    add("duration unstressed vowels".into(), "timing", "", |t, _| (is_vowel(&t.ipa) && !t.stressed).then_some(t.dur), Median, false);
    // diagnostics
    for v in MONOPHTHONGS {
        add(format!("F1 {v}"), "formants", v, |t, c| (t.ipa == c && t.stressed).then_some(t.f1), Median, true);
        add(format!("F2 {v}"), "formants", v, |t, c| (t.ipa == c && t.stressed).then_some(t.f2), Median, true);
    }
    for (k, b) in BAND_NAMES.iter().enumerate() {
        let take: fn(&Tok, &str) -> Option<f64> = match k {
            0 => |t, _| t.stressed.then_some(t.bands[0]),
            1 => |t, _| t.stressed.then_some(t.bands[1]),
            2 => |t, _| t.stressed.then_some(t.bands[2]),
            3 => |t, _| t.stressed.then_some(t.bands[3]),
            _ => |t, _| t.stressed.then_some(t.bands[4]),
        };
        add(format!("vowel band {b}"), "voice", "", take, Median, false);
    }
    add("H1-H2".into(), "voice", "", |t, _| t.stressed.then_some(t.h1h2), Median, false);
    for c in CONSONANTS {
        add(format!("voiced {c}"), "voicing", c, |t, c| (t.ipa == c).then_some(t.voiced), Mean, false);
    }
    add("voiced vowels".into(), "voicing", "", |t, _| is_vowel(&t.ipa).then_some(t.voiced), Mean, false);
    d
}

/// The spectrum terms (diagnostic): each consonant and monophthong.
fn spectrum_cats() -> Vec<&'static str> {
    CONSONANTS.iter().chain(MONOPHTHONGS.iter()).copied().collect()
}

/// One voice's values over a set of utterances: per term, and a cepstrum per spectrum category.
#[derive(Clone)]
struct Values {
    terms: BTreeMap<String, f64>,
    /// tokens behind each term's value
    support: BTreeMap<String, usize>,
    ceps: BTreeMap<&'static str, [f64; N_CEP]>,
}

fn cepstrum(logmel: &[f64; N_MEL]) -> [f64; N_CEP] {
    // as the singer judge's note cepstrum (a real cepstrum of ln|X|; c0, the level, excluded)
    std::array::from_fn(|q| {
        let q = q + 1;
        logmel.iter().enumerate().map(|(k, v)| v * (std::f64::consts::PI * q as f64 * (k as f64 + 0.5) / N_MEL as f64).cos()).sum::<f64>()
            / N_MEL as f64
    })
}

/// Values of one voice over its utterances `ms` (UttMeas, tokens).
fn values(defs: &[TermDef], ms: &[(&UttMeas, &[Tok])]) -> Values {
    let toks: Vec<&Tok> = ms.iter().flat_map(|m| m.1.iter()).collect();
    let mut terms = BTreeMap::new();
    let mut support = BTreeMap::new();
    for d in defs {
        let xs: Vec<f64> = toks.iter().filter_map(|t| (d.take)(t, &d.cat)).filter(|x| x.is_finite()).collect();
        let v = if xs.len() < MIN_TOKENS {
            f64::NAN
        } else {
            match d.stat {
                Stat::Median => median(&xs),
                Stat::Mean => mean(&xs),
                Stat::P90 => percentile(&xs, 0.9),
                Stat::P10 => percentile(&xs, 0.1),
            }
        };
        terms.insert(d.name.clone(), if d.log { v.log2() } else { v });
        support.insert(d.name.clone(), xs.len());
    }
    terms.insert("articulation rate".into(), median(&ms.iter().map(|m| m.0.rate).collect::<Vec<_>>()));
    terms.insert("f0 range".into(), median(&ms.iter().map(|m| m.0.f0_range).collect::<Vec<_>>()));
    support.insert("articulation rate".into(), ms.len());
    support.insert("f0 range".into(), ms.len());
    let mut ceps = BTreeMap::new();
    for c in spectrum_cats() {
        let mels: Vec<&[f64; N_MEL]> = toks.iter().filter(|t| t.ipa == c && t.mel.iter().all(|v| v.is_finite())).map(|t| &t.mel).collect();
        support.insert(format!("spectrum {c}"), mels.len());
        if mels.len() >= MIN_TOKENS {
            let avg: [f64; N_MEL] = std::array::from_fn(|k| mels.iter().map(|m| m[k]).sum::<f64>() / mels.len() as f64);
            ceps.insert(c, cepstrum(&avg));
        }
    }
    Values { terms, support, ceps }
}

/// The synth's values over several speakers' sentence sets, summarised as the human centre is:
/// per term the median over the sets of the synth's value on each (each set in its own lists),
/// so the synth and the speakers give the same kind of number. A term needs a value on at least
/// half the sets.
fn over_sets(sets: &[Values]) -> Values {
    let need = sets.len().div_ceil(2).max(1);
    let names: Vec<String> = sets.first().map(|v| v.terms.keys().cloned().collect()).unwrap_or_default();
    let mut terms = BTreeMap::new();
    let mut support = BTreeMap::new();
    for n in &names {
        let xs: Vec<f64> = sets.iter().map(|v| v.terms[n]).filter(|x| x.is_finite()).collect();
        terms.insert(n.clone(), if xs.len() >= need { median(&xs) } else { f64::NAN });
        support.insert(n.clone(), sets.iter().map(|v| v.support.get(n).copied().unwrap_or(0)).sum());
    }
    let mut ceps = BTreeMap::new();
    for c in spectrum_cats() {
        let cs: Vec<&[f64; N_CEP]> = sets.iter().filter_map(|v| v.ceps.get(c)).collect();
        let key = format!("spectrum {c}");
        support.insert(key.clone(), sets.iter().map(|v| v.support.get(&key).copied().unwrap_or(0)).sum());
        if cs.len() >= need {
            ceps.insert(c, mean_ceps(&cs));
        }
    }
    Values { terms, support, ceps }
}

fn group_of(name: &str) -> &'static str {
    match name {
        "articulation rate" => "timing",
        "f0 range" => "intonation",
        n if n.starts_with("spectrum ") => "spectrum",
        n => term_defs().into_iter().find(|d| d.name == n).map_or("?", |d| d.group),
    }
}

/// The smallest spread a term gets, about a just-noticeable difference: where every real speaker
/// agrees more closely than this (no voicing at all in /s/), a smaller difference still counts as
/// nothing, so a measure the speakers happen to share can't dominate the score.
fn spread_floor(name: &str) -> f64 {
    match name.split(' ').next().unwrap_or("") {
        "level" | "burst" | "burst," => 1.5,
        "vowel" | "H1-H2" => 1.0,
        // log2 frequency: 5 %
        "centroid" | "F1" | "F2" => 0.07,
        // share of the utterance's mean phone duration
        "duration" => 0.1,
        "voiced" => 0.1,
        // phones per second
        "articulation" => 0.5,
        // semitones
        "f0" => 1.0,
        _ => 0.0,
    }
}

fn mad_sd(xs: &[f64]) -> f64 {
    let m = median(xs);
    1.4826 * median(&xs.iter().map(|x| (x - m).abs()).collect::<Vec<_>>())
}

fn mean_ceps(cs: &[&[f64; N_CEP]]) -> [f64; N_CEP] {
    std::array::from_fn(|q| cs.iter().map(|c| c[q]).sum::<f64>() / cs.len() as f64)
}

/// The human reference of one sex for every term, from a set of training speakers.
struct Reference {
    /// value terms: (centre, spread, the spread of this sex alone, for the report)
    terms: BTreeMap<String, (f64, f64, f64)>,
    /// spectrum categories: (centre cepstrum, unit distance)
    ceps: BTreeMap<&'static str, ([f64; N_CEP], f64)>,
}

/// The reference for one sex: centres from that sex's speakers; value spreads from each
/// speaker's signed deviation from their own sex's centre, pooled over both sexes (19 speakers,
/// so a spread isn't small by chance), at least `spread_floor`; spectrum units from that sex.
fn reference(female: bool, speakers: &BTreeMap<String, Values>) -> Reference {
    let sex = |k: &str| k.starts_with('F');
    let names: Vec<String> = speakers.values().next().map(|v| v.terms.keys().cloned().collect()).unwrap_or_default();
    let centre = |name: &str, f: bool| {
        let xs: Vec<f64> = speakers.iter().filter(|(k, _)| sex(k) == f).map(|(_, v)| v.terms[name]).filter(|x| x.is_finite()).collect();
        if xs.len() >= MIN_SPEAKERS { median(&xs) } else { f64::NAN }
    };
    let mut terms = BTreeMap::new();
    for name in &names {
        let (cf, cm) = (centre(name, true), centre(name, false));
        let dev = |only: Option<bool>| -> Vec<f64> {
            speakers
                .iter()
                .filter(|(k, _)| only.is_none_or(|f| sex(k) == f))
                .map(|(k, v)| v.terms[name] - if sex(k) { cf } else { cm })
                .filter(|x| x.is_finite())
                .collect()
        };
        let spread = mad_sd(&dev(None)).max(spread_floor(name));
        terms.insert(name.clone(), (if female { cf } else { cm }, if spread > 0.0 { spread } else { f64::NAN }, mad_sd(&dev(Some(female)))));
    }
    let mut ceps = BTreeMap::new();
    for c in spectrum_cats() {
        let mine: Vec<(&String, &[f64; N_CEP])> =
            speakers.iter().filter(|(k, _)| sex(k) == female).filter_map(|(k, v)| v.ceps.get(c).map(|cc| (k, cc))).collect();
        if mine.len() < MIN_SPEAKERS {
            continue;
        }
        let all: Vec<&[f64; N_CEP]> = mine.iter().map(|m| m.1).collect();
        // a speaker's distance to the centre of the others: how far apart two real voices are
        let loo: Vec<f64> = mine
            .iter()
            .map(|(k, cc)| {
                let others: Vec<&[f64; N_CEP]> = mine.iter().filter(|m| m.0 != *k).map(|m| m.1).collect();
                mcd(cc, &mean_ceps(&others))
            })
            .collect();
        ceps.insert(c, (mean_ceps(&all), median(&loo)));
    }
    Reference { terms, ceps }
}

/// max(0, gap / unit − 1), or NaN when any of them is missing (`f64::max` would make a missing
/// measurement a perfect 0).
fn cost(gap: f64, unit: f64) -> f64 {
    if gap.is_finite() && unit.is_finite() && unit > 0.0 { (gap / unit - 1.0).max(0.0) } else { f64::NAN }
}

/// Each term's (value, cost) for one voice against a reference.
fn costs(v: &Values, r: &Reference) -> BTreeMap<String, (f64, f64)> {
    let mut out = BTreeMap::new();
    for (name, &(c, sd, _)) in &r.terms {
        let x = v.terms.get(name).copied().unwrap_or(f64::NAN);
        out.insert(name.clone(), (x, cost((x - c).abs(), sd)));
    }
    for (c, (centre, unit)) in &r.ceps {
        let (d, cost) = match v.ceps.get(c) {
            Some(cc) => {
                let d = mcd(cc, centre);
                (d, cost(d, *unit))
            }
            None => (f64::NAN, f64::NAN),
        };
        out.insert(format!("spectrum {c}"), (d, cost));
    }
    out
}

/// Group means of finite costs, for the scored groups and the diagnostics, and the score: the
/// mean of the scored groups.
fn fold(costs: &BTreeMap<String, (f64, f64)>) -> (f64, BTreeMap<&'static str, f64>) {
    let mut groups = BTreeMap::new();
    for g in GROUPS.iter().chain(DIAGNOSTICS.iter()) {
        let cs: Vec<f64> = costs.iter().filter(|(n, _)| group_of(n) == *g).map(|(_, c)| c.1).filter(|c| c.is_finite()).collect();
        if !cs.is_empty() {
            groups.insert(*g, cs.iter().sum::<f64>() / cs.len() as f64);
        }
    }
    let scored: Vec<f64> = GROUPS.iter().filter_map(|g| groups.get(g)).copied().collect();
    let score = if scored.len() == GROUPS.len() { scored.iter().sum::<f64>() / scored.len() as f64 } else { f64::NAN };
    (score, groups)
}

fn r3(x: f64) -> Value {
    if x.is_finite() { json!((x * 1000.0).round() / 1000.0) } else { Value::Null }
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// A cheap fingerprint of a file (FNV-1a), so the verdict can tell that two runs used the same
/// recordings' alignment and the same aligner model.
fn fingerprint(path: &str) -> String {
    fingerprint_bytes(&std::fs::read(path).unwrap_or_default())
}

fn fingerprint_bytes(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The paired tokens where the synth is furthest from the speaker on the same word: listening
/// targets. (stressed vowels by duration ratio, stops by burst difference)
fn worst(pairs: &[Pair], female: bool) -> Value {
    let mut vowels: Vec<(f64, &str, &Tok, &Tok)> = Vec::new();
    let mut bursts: Vec<(f64, &str, &Tok, &Tok)> = Vec::new();
    for p in pairs.iter().filter(|p| p.female == female && !held_out(&p.speaker)) {
        for (h, s) in p.human.1.iter().zip(&p.synth.1) {
            if is_vowel(&s.ipa) && s.stressed && h.secs > 0.0 {
                vowels.push((s.secs / h.secs, &p.id, h, s));
            }
            if STOPS.contains(&s.ipa.as_str()) && (s.burst - h.burst).is_finite() {
                bursts.push((s.burst - h.burst, &p.id, h, s));
            }
        }
    }
    vowels.sort_by(|a, b| a.0.total_cmp(&b.0));
    bursts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let row = |(x, id, h, s): &(f64, &str, &Tok, &Tok), ms: bool| {
        if ms {
            json!({ "recording": id, "phone": s.ipa, "synth_at": r3(s.t0), "synth_ms": (s.secs * 1000.0).round(), "speaker_ms": (h.secs * 1000.0).round(), "ratio": r3(*x) })
        } else {
            json!({ "recording": id, "phone": s.ipa, "synth_at": r3(s.t0), "synth_db": r3(s.burst), "speaker_db": r3(h.burst), "diff": r3(*x) })
        }
    };
    json!({
        "shortest_stressed_vowels": vowels.iter().take(10).map(|v| row(v, true)).collect::<Vec<_>>(),
        "loudest_bursts": bursts.iter().take(10).map(|v| row(v, false)).collect::<Vec<_>>(),
    })
}

struct Scored {
    json: Value,
}

/// Score the synth (and each real speaker, left out in turn) against the training speakers.
fn score(pairs: &[Pair], aligner: &str) -> Scored {
    let defs = term_defs();
    let utts = |pred: &dyn Fn(&Pair) -> bool, synth: bool| -> Vec<(&UttMeas, &[Tok])> {
        pairs.iter().filter(|p| pred(p)).map(|p| if synth { (&p.synth.0, &p.synth.1[..]) } else { (&p.human.0, &p.human.1[..]) }).collect()
    };
    let mut spk_names: Vec<&str> = pairs.iter().map(|p| p.speaker.as_str()).collect();
    spk_names.sort();
    spk_names.dedup();
    let human: BTreeMap<String, Values> =
        spk_names.iter().map(|k| (k.to_string(), values(&defs, &utts(&|p: &Pair| p.speaker == *k, false)))).collect();
    // the synth on each speaker's sentences
    let synth_sets: BTreeMap<String, Values> =
        spk_names.iter().map(|k| (k.to_string(), values(&defs, &utts(&|p: &Pair| p.speaker == *k, true)))).collect();
    let train: BTreeMap<String, Values> = human.iter().filter(|(k, _)| !held_out(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut out = json!({
        "judge": JUDGE_VERSION,
        "alignment": fingerprint(&repo_path("tools/tsp/alignment.tsv.gz")),
        "model": fingerprint(&format!("{}/.cache/chimera-mfa/mfa/pretrained_models/acoustic/english_us_arpa.zip", std::env::var("HOME").unwrap_or_default())),
        "aligner": aligner,
        "recordings": pairs.len(),
        "recording_set": fingerprint_bytes(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(",").as_bytes()),
    });
    let mut human_json = json!({});
    for (part, held) in [("train", false), ("held", true)] {
        let mut sexes = json!({});
        let mut sex_scores = Vec::new();
        for female in [true, false] {
            let sex = if female { "female" } else { "male" };
            let mine = |k: &str| k.starts_with(if female { 'F' } else { 'M' }) && held_out(k) == held;
            let r = reference(female, &train);
            let sets: Vec<Values> = synth_sets.iter().filter(|(k, _)| mine(k)).map(|(_, v)| v.clone()).collect();
            let sv = over_sets(&sets);
            let c = costs(&sv, &r);
            let (sc, groups) = fold(&c);
            sex_scores.push(sc);
            let terms: serde_json::Map<String, Value> = c
                .iter()
                .map(|(n, (x, cost))| {
                    let (centre, spread, own) = if let Some(cat) = n.strip_prefix("spectrum ") {
                        (f64::NAN, r.ceps.iter().find(|(k, _)| **k == cat).map_or(f64::NAN, |(_, v)| v.1), f64::NAN)
                    } else {
                        r.terms.get(n).copied().unwrap_or((f64::NAN, f64::NAN, f64::NAN))
                    };
                    let g = group_of(n);
                    (
                        n.clone(),
                        json!({
                            "group": g,
                            "scored": GROUPS.contains(&g),
                            "centre": r3(centre),
                            "spread": r3(spread),
                            "spread_this_sex": r3(own),
                            "synth": r3(*x),
                            "cost": r3(*cost),
                            "support": sv.support.get(n).copied().unwrap_or(0),
                        }),
                    )
                })
                .collect();
            sexes[sex] = json!({
                "score": r3(sc),
                "groups": groups.iter().map(|(g, v)| (g.to_string(), r3(*v))).collect::<serde_json::Map<_, _>>(),
                "terms": terms,
            });
            if !held {
                sexes[sex]["worst"] = worst(pairs, female);
            }
            // the real speakers scored the same way: a training speaker against the others
            for k in spk_names.iter().filter(|k| mine(k)) {
                let rk = if held {
                    reference(female, &train)
                } else {
                    let others: BTreeMap<String, Values> = train.iter().filter(|(n, _)| n.as_str() != *k).map(|(n, v)| (n.clone(), v.clone())).collect();
                    reference(female, &others)
                };
                let (hs, _) = fold(&costs(&human[*k], &rk));
                human_json[part][k.to_string()] = r3(hs);
            }
        }
        // the score of a part is the mean over the two sexes
        let both: Vec<f64> = sex_scores.iter().copied().filter(|x| x.is_finite()).collect();
        sexes["score"] = r3(if both.len() == 2 { both.iter().sum::<f64>() / 2.0 } else { f64::NAN });
        out[part] = sexes;
    }
    for part in ["train", "held"] {
        let xs: Vec<f64> = human_json[part].as_object().map(|o| o.values().map(num).collect()).unwrap_or_default();
        human_json[format!("{part}_median")] = r3(median(&xs));
    }
    out["human"] = human_json;
    Scored { json: out }
}

enum Verdict {
    Better,
    Worse,
    Same,
}

/// The verdict on the training score between two runs over the same recordings, by the same
/// judge. Scored terms only; diagnostics that moved a lot are noted, not counted.
fn verdict(old: &Value, new: &Value) -> Result<(Verdict, Vec<String>, f64), String> {
    for k in ["judge", "alignment", "model", "aligner", "recording_set"] {
        if old[k] != new[k] {
            return Err(format!("the earlier score has another {k} ({} here, {} there); rescore it with this build over the same speakers", new[k], old[k]));
        }
    }
    let mut why = Vec::new();
    for sex in ["female", "male"] {
        let (o, n) = (&old["train"][sex]["terms"], &new["train"][sex]["terms"]);
        for (name, t) in o.as_object().into_iter().flatten() {
            if t["scored"] != json!(true) {
                continue;
            }
            let nt = &n[name];
            let (a, b) = (num(&t["cost"]), num(&nt["cost"]));
            let (sa, sb) = (num(&t["support"]), num(&nt["support"]));
            if a.is_finite() && !b.is_finite() {
                why.push(format!("{sex} {name}: no value any more"));
            } else if a.is_finite() && b - a > TERM_SLACK {
                why.push(format!("{sex} {name}: cost {a:.2} → {b:.2}"));
            }
            if sb < sa * (1.0 - SUPPORT_SLACK) {
                why.push(format!("{sex} {name}: measured on {sb} tokens, was {sa}"));
            }
        }
    }
    let change = num(&new["train"]["score"]) - num(&old["train"]["score"]);
    let milli = |x: f64| (x * 1000.0).round() as i64;
    let v = if !why.is_empty() {
        Verdict::Worse
    } else if milli(change) <= -milli(MARGIN) {
        Verdict::Better
    } else if milli(change) >= milli(MARGIN) {
        why.push(format!("score up by {change:.3}"));
        Verdict::Worse
    } else {
        Verdict::Same
    };
    Ok((v, why, change))
}

/// One term table: name, people, spread, synth, cost.
fn term_table(md: &mut String, terms: &[(&String, &Value)]) {
    md.push_str("| term | people | spread | synth | cost |\n|---|---|---|---|---|\n");
    for (name, v) in terms {
        let log = name.starts_with("centroid") || name.starts_with('F');
        let show = |x: f64| if log && x.is_finite() { format!("{:.0} Hz", 2f64.powf(x)) } else { fx(x, 2) };
        let spread = num(&v["spread"]);
        let spread_s = if log && spread.is_finite() { format!("{:.0} %", 100.0 * (2f64.powf(spread) - 1.0)) } else { fx(spread, 2) };
        md.push_str(&format!("| {name} | {} | {} | {} | {} |\n", show(num(&v["centre"])), spread_s, show(num(&v["synth"])), fx(num(&v["cost"]), 2)));
    }
}

/// The report: the score, the human floor, then per sex the groups, the scored terms by cost,
/// the worst paired tokens, and the diagnostics by cost.
fn report_md(s: &Value) -> String {
    let mut md = String::from("# Speech against real voices (TSP)\n\n");
    md.push_str(&format!(
        "**Score {} on the training speakers' sentences, {} on the held-out speakers'.** 0 means every scored term is within one spread of the real speakers' centre. A real speaker, scored the same way against the other speakers, gets {} (median over training speakers; {} for the held-out speakers). Scored: {}. Diagnostics, not scored: {}.\n\n",
        fx(num(&s["train"]["score"]), 3),
        fx(num(&s["held"]["score"]), 3),
        fx(num(&s["human"]["train_median"]), 3),
        fx(num(&s["human"]["held_median"]), 3),
        GROUPS.join(", "),
        DIAGNOSTICS.join(", "),
    ));
    for sex in ["female", "male"] {
        let t = &s["train"][sex];
        md.push_str(&format!("## {sex} (score {})\n\n| group | cost |\n|---|---|\n", fx(num(&t["score"]), 3)));
        for g in GROUPS.iter().chain(DIAGNOSTICS.iter()) {
            let note = if DIAGNOSTICS.contains(g) { " (diagnostic)" } else { "" };
            md.push_str(&format!("| {g}{note} | {} |\n", fx(num(&t["groups"][*g]), 2)));
        }
        let mut terms: Vec<(&String, &Value)> = t["terms"].as_object().map(|o| o.iter().collect()).unwrap_or_default();
        terms.sort_by(|a, b| num(&b.1["cost"]).total_cmp(&num(&a.1["cost"])).then(a.0.cmp(b.0)));
        let (scored, diag): (Vec<_>, Vec<_>) = terms.into_iter().filter(|(_, v)| num(&v["cost"]) > 0.0).partition(|(_, v)| v["scored"] == json!(true));
        md.push_str("\n### Scored terms with a cost\n\n");
        term_table(&mut md, &scored);
        md.push_str("\n### Worst paired tokens (training sentences)\n\nStressed vowels, shortest against the speaker's same vowel:\n\n| recording | vowel | synth at | synth ms | speaker ms |\n|---|---|---|---|---|\n");
        for w in t["worst"]["shortest_stressed_vowels"].as_array().into_iter().flatten() {
            md.push_str(&format!("| {} | {} | {} s | {} | {} |\n", w["recording"].as_str().unwrap_or(""), w["phone"].as_str().unwrap_or(""), w["synth_at"], w["synth_ms"], w["speaker_ms"]));
        }
        md.push_str("\nStop bursts, loudest against the speaker's same stop (dB re the stressed vowels):\n\n| recording | stop | synth at | synth | speaker |\n|---|---|---|---|---|\n");
        for w in t["worst"]["loudest_bursts"].as_array().into_iter().flatten() {
            md.push_str(&format!("| {} | {} | {} s | {} | {} |\n", w["recording"].as_str().unwrap_or(""), w["phone"].as_str().unwrap_or(""), w["synth_at"], w["synth_db"], w["speaker_db"]));
        }
        md.push_str("\n### Diagnostics with a cost (not scored)\n\n");
        let top: Vec<_> = diag.into_iter().take(30).collect();
        term_table(&mut md, &top);
        md.push('\n');
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ph(ipa: &str) -> Phone {
        Phone { ipa: ipa.into(), stress: 0, t0: 0.0, t1: 0.1 }
    }

    #[test]
    fn a_render_is_aligned_over_a_noise_floor_like_the_recordings() {
        // half a second of a loud tone, then half a second of digital silence
        let fs = crate::FS;
        let x: Vec<f64> = (0..fs as usize)
            .map(|i| if (i as f64) < 0.5 * fs { 0.5 * (i as f64 * 0.05).sin() } else { 0.0 })
            .collect();
        let y = over_noise_floor(&x);
        let ms = |v: &[f32]| v.iter().map(|a| (*a as f64) * (*a as f64)).sum::<f64>() / v.len() as f64;
        let n = fs as usize / 2;
        let floor_db = 10.0 * (ms(&y[n..]) / ms(&y[..n])).log10();
        assert!((floor_db - ALIGN_FLOOR_DB).abs() < 0.5, "silence now sits {floor_db} dB below the tone");
        // the same render always aligns the same
        assert_eq!(y, over_noise_floor(&x));
    }

    #[test]
    fn a_missing_measurement_has_no_cost() {
        assert!(cost(f64::NAN, 1.0).is_nan());
        assert!(cost(1.0, f64::NAN).is_nan());
        assert!(cost(1.0, 0.0).is_nan());
        assert_eq!(cost(0.5, 1.0), 0.0);
        assert_eq!(cost(3.0, 1.0), 2.0);
        // a scored group with no cost leaves no score, rather than a perfect one
        let mut c = BTreeMap::new();
        c.insert("level ʃ".to_string(), (f64::NAN, f64::NAN));
        c.insert("centroid ʃ".to_string(), (1.0, 0.0));
        c.insert("duration k".to_string(), (1.0, 0.0));
        assert!(fold(&c).0.is_nan());
    }

    #[test]
    fn matching_keeps_only_the_same_phones_in_order() {
        // "the sheet" said ðə ʃit by the synth, ði ʃiːt (an inserted t) by a speaker
        let synth: Vec<Phone> = ["ð", "ə", "ʃ", "i", "t"].map(ph).to_vec();
        let human: Vec<Phone> = ["ð", "i", "ʃ", "i", "t", "t"].map(ph).to_vec();
        let m = match_phones(&human, &synth);
        assert!(m.iter().all(|&(h, s)| human[h].ipa == synth[s].ipa), "{m:?}");
        assert!(m.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1), "{m:?}");
        assert_eq!(m.len(), 4, "ð ʃ i t match, ə/i doesn't: {m:?}");
    }

    #[test]
    fn every_engine_phone_goes_to_arpabet_and_back() {
        // the synth's MFA dictionary must say exactly what the G2P says
        for ipa in VOWELS.iter().chain(CONSONANTS.iter()).chain(["ʒ"].iter()) {
            for stress in [0u8, 1, 2] {
                let s = Segment { ipa: ipa.to_string(), stress: Some(stress), ..Default::default() };
                let arpa = to_arpa(&s).unwrap();
                let back = arpabet_to_segments(&arpa).pop().unwrap();
                // ARPAbet has one unstressed AH and ER: an unstressed ʌ or ɝ comes back reduced
                let want = match (*ipa, stress) {
                    ("ʌ", 0) => "ə",
                    ("ɝ", 0) => "ɚ",
                    _ => ipa,
                };
                assert_eq!(back.ipa, want, "{ipa} {stress} → {arpa}");
                if is_vowel(ipa) && !["ə", "ɚ"].contains(&want) {
                    assert_eq!(back.stress, Some(stress), "{ipa} {stress} → {arpa}");
                }
            }
        }
        assert!(to_arpa(&Segment { ipa: "ɾ".into(), ..Default::default() }).is_err(), "allophones are resolved before the dictionary");
    }

    #[test]
    fn a_dictionary_word_keeps_its_reduced_function_word_form() {
        let g = crate::render::g2p().unwrap();
        let arpa = |w: &str| g.word(w, true).iter().map(|s| to_arpa(s).unwrap()).collect::<Vec<_>>().join(" ");
        assert_eq!(arpa("of"), "AH0 V");
        assert_eq!(arpa("sheet"), "SH IY1 T");
        assert_eq!(said_words(&g, "The hot-cross bun."), ["the", "hot", "cross", "bun"]);
    }

    fn scored(cost: f64, support: u64) -> Value {
        let term = json!({ "scored": true, "cost": cost, "support": support });
        json!({
            "judge": JUDGE_VERSION,
            "alignment": "a",
            "model": "m",
            "aligner": "mfa 3.4.3",
            "recording_set": "r",
            "train": {
                "score": cost,
                "female": { "terms": { "level ʃ": term, "F2 u": { "scored": false, "cost": cost, "support": support } } },
                "male": { "terms": { "level ʃ": term } },
            },
        })
    }

    #[test]
    fn the_verdict_wants_a_lower_score_and_no_term_much_worse() {
        let v = |old: Value, new: Value| match verdict(&old, &new).unwrap().0 {
            Verdict::Better => "better",
            Verdict::Worse => "worse",
            Verdict::Same => "same",
        };
        assert_eq!(v(scored(1.0, 1000), scored(0.99, 1000)), "better");
        assert_eq!(v(scored(1.0, 1000), scored(0.995, 1000)), "same");
        assert_eq!(v(scored(1.0, 1000), scored(1.01, 1000)), "worse");
        // a term measured on fewer tokens can't buy a better score
        assert_eq!(v(scored(1.0, 1000), scored(0.5, 940)), "worse");
        assert_eq!(v(scored(1.0, 1000), scored(0.5, 960)), "better");
        // a term much worse breaks it even when the score falls
        let mut new = scored(0.9, 1000);
        new["train"]["male"]["terms"]["level ʃ"]["cost"] = json!(1.0 + TERM_SLACK + 0.01);
        assert_eq!(v(scored(1.0, 1000), new), "worse");
        // a diagnostic doesn't decide
        let mut new = scored(0.9, 1000);
        new["train"]["female"]["terms"]["F2 u"]["cost"] = json!(5.0);
        assert_eq!(v(scored(1.0, 1000), new), "better");
        for k in ["recording_set", "judge", "alignment", "model", "aligner"] {
            let mut other = scored(1.0, 1000);
            other[k] = json!(99);
            assert!(verdict(&scored(1.0, 1000), &other).is_err(), "{k}");
        }
    }
}
