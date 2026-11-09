//! The `chimera` binary end to end: a render writes a sound of the planned length, and the
//! calibrations reproduce the tables the engine embeds.

use chimera::math::js_round;
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PlanOptions, plan_utterance};
use std::process::Command;

const PHONETICS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../chimera/src/phonetics");

fn chimera(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_chimera"))
        .args(args)
        .output()
        .expect("run chimera");
    assert!(
        out.status.success(),
        "chimera {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn tmp(name: &str) -> String {
    format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"))
}

#[test]
fn render_track_keeps_audio_and_telemetry_on_the_same_clock() {
    let input = tmp("track.json");
    let output = tmp("track.wav");
    let tele = tmp("track.tele");
    let frames: Vec<_> = (0..50).map(|i| serde_json::json!({
        "f0": if i < 25 { 220.0 } else { 110.0 },
        "gate": if (5..45).contains(&i) { 1.0 } else { 0.0 }
    })).collect();
    std::fs::write(&input, serde_json::json!({
        "rate": 50, "source": {"glide": 0.01, "drift": 0}, "frames": frames
    }).to_string()).unwrap();
    chimera(&["render", "track", &input, &output, "male", "--tele", &tele, "--float"]);
    let wav = std::fs::read(&output).unwrap();
    let samples: Vec<f32> = wav[44..].chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
    assert_eq!(samples.len(), 48000, "a one-second track has no added lead or tail");
    assert!(samples.iter().all(|x| x.is_finite()));
    assert!(samples[..2400].iter().all(|x| x.abs() < 1e-6), "the first frame is silent");
    for (start, hz) in [(14400, 220.0), (33600, 110.0)] {
        let frame: Vec<_> = samples[start..start + 4096].iter().map(|x| *x as f64).collect();
        let pitch = chimera::analysis::pitch::yin_pitch(&frame, 48000.0, Default::default());
        assert!((pitch.f0 - hz).abs() < 5.0, "expected {hz} Hz, got {}", pitch.f0);
    }
    let bytes = std::fs::read(&tele).unwrap();
    assert_eq!(&bytes[..4], b"CHTL");
    let u = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    assert_eq!((u(8), u(12), u(16)), (48000, 768, 63));
    let len = u(20);
    assert_eq!(bytes.len(), 48 + 63 * len * 4);
    for i in 0..63 {
        let at = 48 + (i * len + 10) * 4;
        let clock = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert!((clock as f64 - i as f64 * 768.0 / 48000.0).abs() < 1e-6);
    }
    chimera(&["render", "track", &input, &output, "male", "--float"]);
    assert_eq!(wav, std::fs::read(output).unwrap(), "telemetry does not change the audio");
}

#[test]
fn render_track_rejects_invalid_timing_and_controls() {
    for (i, track) in [
        serde_json::json!({"rate": 0, "frames": [{"f0": 110, "gate": 1}]}),
        serde_json::json!({"rate": 59, "frames": [{"f0": 110, "gate": 1}]}),
        serde_json::json!({"rate": 50, "frames": []}),
        serde_json::json!({"rate": 50, "frames": [{"f0": 0, "gate": 1}]}),
        serde_json::json!({"rate": 50, "frames": [{"f0": 110, "gate": 2}]}),
        serde_json::json!({"rate": 50, "phonation": {"jitter": 0.2}, "frames": [{"f0": 110, "gate": 1}]}),
        serde_json::json!({"rate": 50, "phonation": {"alternatingAttenuation": 1.1}, "frames": [{"f0": 110, "gate": 1}]}),
        serde_json::json!({"rate": 50, "frames": [{"f0": 110, "gate": 1, "alternatingAttenuation": -0.1}]}),
    ].iter().enumerate() {
        let input = tmp(&format!("bad-track-{i}.json"));
        let output = tmp(&format!("bad-track-{i}.wav"));
        let _ = std::fs::remove_file(&output);
        std::fs::write(&input, track.to_string()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_chimera"))
            .args(["render", "track", &input, &output]).output().unwrap();
        assert!(!result.status.success());
        assert!(!std::path::Path::new(&output).exists());
    }
}

#[test]
fn render_track_can_sustain_regular_subharmonics_without_random_roughness() {
    let input = tmp("subharmonic-track.json");
    let output = tmp("subharmonic-track.wav");
    let frames: Vec<_> = (0..100).map(|_| serde_json::json!({"f0": 148.5, "gate": 1})).collect();
    std::fs::write(&input, serde_json::json!({
        "rate": 50,
        "source": {"roughness": 0, "drift": 0, "breathiness": 0.03, "tension": 0.8},
        "phonation": {"alternatingAttenuation": 0.85, "jitter": 0, "shimmer": 0},
        "frames": frames
    }).to_string()).unwrap();
    chimera(&["render", "track", &input, &output, "male", "--float"]);
    let bytes = std::fs::read(output).unwrap();
    let audio: Vec<f64> = bytes[44..].chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect();
    let pitch = chimera::analysis::pitch::yin_pitch(&audio[48000..52100], 48000.0,
        chimera::analysis::pitch::YinOpts { threshold: Some(0.08), ..Default::default() });
    assert!((pitch.f0 - 74.25).abs() < 1.0, "expected a 74.25 Hz repetition rate, got {}", pitch.f0);
    assert!(pitch.aperiodicity < 0.02, "the repeated pair of pulses must remain regular");
}

#[test]
fn render_track_follows_a_continuous_low_pitch_curve() {
    let input = tmp("continuous-pitch.json");
    let output = tmp("continuous-pitch.wav");
    let target = |t: f64| 65.0 + 12.0 * chimera::math::sin(chimera::math::TAU * 0.35 * t);
    let frames: Vec<_> = (0..800).map(|i| serde_json::json!({
        "f0": target(i as f64 / 200.0), "gate": 1
    })).collect();
    std::fs::write(&input, serde_json::json!({
        "rate": 200,
        "source": {"glide": 0.015, "roughness": 0, "drift": 0, "breathiness": 0.03},
        "frames": frames
    }).to_string()).unwrap();
    chimera(&["render", "track", &input, &output, "male", "--float"]);
    let bytes = std::fs::read(output).unwrap();
    let audio: Vec<f64> = bytes[44..].chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect();
    for t in [0.4, 0.7, 1.1, 1.5, 1.9, 2.3, 2.7, 3.1, 3.5] {
        let start = ((t - 0.05) * 48000.0) as usize;
        let pitch = chimera::analysis::pitch::yin_pitch(&audio[start..start + 4800], 48000.0,
            chimera::analysis::pitch::YinOpts { min_f0: Some(45.0), max_f0: Some(90.0), ..Default::default() });
        let expected = target(t - 0.015);
        let cents = 1200.0 * (pitch.f0 / expected).log2();
        assert!(cents.abs() < 35.0, "at {t}s: expected {expected} Hz, got {} Hz", pitch.f0);
    }
}

#[test]
fn render_track_honors_an_independent_tongue_tip_target() {
    let mut observed = Vec::new();
    for tip_free in [true, false] {
        let input = tmp(&format!("tip-{tip_free}.json"));
        let output = tmp(&format!("tip-{tip_free}.wav"));
        let tele = tmp(&format!("tip-{tip_free}.tele"));
        let frames: Vec<_> = (0..30).map(|_| serde_json::json!({
            "f0": 150, "gate": 1, "tipFree": tip_free,
            "artic": {"ttX": 5.8, "ttY": 8.5}
        })).collect();
        std::fs::write(&input, serde_json::json!({"rate": 50, "frames": frames}).to_string()).unwrap();
        chimera(&["render", "track", &input, &output, "male", "--tele", &tele]);
        let bytes = std::fs::read(tele).unwrap();
        let u = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let offset = 48 + ((u(16) - 1) * u(20) + 11 + 4) * 4;
        observed.push(f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()));
    }
    assert!((observed[1] - 8.5).abs() < 0.02, "the supplied tip height must persist");
    assert!((observed[0] - observed[1]).abs() > 0.3, "automatic tip placement must differ");
}

#[test]
fn render_track_can_focus_a_high_harmonic_while_retaining_the_low_voice() {
    let input = tmp("focused-tract.json");
    let output = tmp("focused-tract.wav");
    let articulation = serde_json::json!({
        "tbX": 2.81046, "tbY": 5.77136, "ttX": 6.60223, "ttY": 8.93349,
        "jaw": 0.67253, "lipAp": 0.20090, "lipProt": 0.55066, "lipWidth": 0.35,
        "root": 0.36976, "larynx": -0.28730, "epilarynx": 0.6, "ventricular": 0.5, "velum": 0
    });
    let frames: Vec<_> = (0..100).map(|_| serde_json::json!({
        "f0": 150, "gate": 1, "tipFree": false, "artic": articulation
    })).collect();
    std::fs::write(&input, serde_json::json!({
        "rate": 50, "frames": frames,
        "source": {"tension": 0.85, "brightness": 0.5, "pressure": 9.5, "breathiness": 0.025, "drift": 0},
        "phonation": {"alternatingAttenuation": 0.85, "jitter": 0, "shimmer": 0}
    }).to_string()).unwrap();
    chimera(&["render", "track", &input, &output, "male", "--float"]);
    let bytes = std::fs::read(output).unwrap();
    let samples: Vec<f64> = bytes[44..].chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect();
    let amplitude = |hz: f64| {
        let (mut real, mut imaginary) = (0.0, 0.0);
        for (i, sample) in samples[48000..72000].iter().enumerate() {
            let phase = chimera::math::TAU * hz * i as f64 / 48000.0;
            real += sample * chimera::math::cos(phase);
            imaginary += sample * chimera::math::sin(phase);
        }
        (real * real + imaginary * imaginary).sqrt() / 12000.0
    };
    let high = amplitude(1500.0);
    let rms = (samples[48000..72000].iter().map(|x| x * x).sum::<f64>() / 24000.0).sqrt();
    assert!(high > 2.0 * (amplitude(1350.0) * amplitude(1650.0)).sqrt(), "the target harmonic must stand above its neighbors");
    assert!(amplitude(150.0) > 0.15 * rms, "the low voice must remain strong relative to the whole signal");
    assert!(high > 0.15 * rms, "the focused harmonic must remain strong relative to the whole signal");
}

#[test]
fn render_speech_writes_a_sounding_wav_of_the_planned_length() {
    let stem = tmp("hello");
    let stdout = chimera(&["render", "speech", &stem, "ˈhɛloʊ"]);
    assert!(
        stdout.starts_with(&format!("wrote {stem}.wav  1.10 s  peak ")),
        "{stdout}"
    );

    let b = std::fs::read(format!("{stem}.wav")).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..16], b"WAVEfmt ");
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    assert_eq!(
        (u16_at(20), u16_at(22), u32_at(24), u16_at(34)),
        (1, 1, 48000, 16),
        "16-bit PCM mono 48 kHz"
    );
    let samples: Vec<i16> = b[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(u32_at(40) as usize, samples.len() * 2);

    // the tool renders until 0.35 s + 0.2 s after the plan's end (TS wrote 52 692 samples)
    let plan = plan_utterance(
        &tokenize_ipa("ˈhɛloʊ"),
        &PlanOptions {
            body: "male".into(),
            t0: 0.05,
            f0: Some(110.0),
            ..Default::default()
        },
    );
    let expected = js_round(48000.0 * (plan.end + 0.35 + 0.2)) as usize;
    assert_eq!(samples.len(), expected);
    assert_eq!(samples.len(), 52692);

    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!(peak > 3000, "peak {peak}");
    let loud = samples.iter().filter(|s| s.unsigned_abs() > 300).count();
    assert!(
        loud > samples.len() / 10,
        "only {loud} of {} samples above −40 dB",
        samples.len()
    );
}

#[test]
fn calibrate_consonants_reproduces_the_committed_table() {
    let out = tmp("consonantTargets.json");
    chimera(&["calibrate", "consonants", "--out", &out]);
    let got = std::fs::read_to_string(&out).unwrap();
    let want =
        std::fs::read_to_string(format!("{PHONETICS}/consonantTargets.generated.json")).unwrap();
    assert!(
        got == want,
        "calibrate consonants differs from the committed table"
    );
}

/// About 100 s in release. Not byte-identical: the Nelder–Mead fit runs 1 800 + 2 × 1 000
/// iterations on a flat minimum, which amplifies last-ulp differences between V8's libm and Rust's
/// (in the airway geometry and the transfer function) into ~1e-4 changes of the fitted posture.
/// Three entries round the other way at the third decimal: male /ə/ root, child /u/ lipAp (the TS
/// tool gives the committed values), and female /a/ (the TS tool at the port's base commit does
/// not reproduce its committed values either; Rust gives what TS gives today). Everything else,
/// layout and key order included, is identical.
#[test]
#[ignore = "slow (~100 s): cargo test -p chimera-cli --release -- --ignored"]
fn calibrate_vowels_reproduces_the_committed_table() {
    let out = tmp("vowelTargets.json");
    let _ = std::fs::remove_file(&out);
    chimera(&["calibrate", "vowels", "all", "--out", &out]);
    let got = std::fs::read_to_string(&out).unwrap();
    let want = std::fs::read_to_string(format!("{PHONETICS}/vowelTargets.generated.json")).unwrap();
    let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    assert_eq!(g.len(), w.len(), "line count");
    let mut differing = Vec::new();
    for (i, (a, b)) in g.iter().zip(&w).enumerate() {
        if a == b {
            continue;
        }
        // only a posture value may differ, and only in the last digit or two
        let split = |l: &str| -> (String, f64) {
            let (k, v) = l
                .trim()
                .trim_end_matches(',')
                .split_once(": ")
                .unwrap_or_else(|| panic!("line {}: {a:?} vs {b:?}", i + 1));
            (
                k.to_string(),
                v.parse()
                    .unwrap_or_else(|_| panic!("line {}: {a:?} vs {b:?}", i + 1)),
            )
        };
        let ((ka, va), (kb, vb)) = (split(a), split(b));
        assert_eq!(ka, kb, "line {}", i + 1);
        assert!(
            [
                "\"tbX\"",
                "\"tbY\"",
                "\"jaw\"",
                "\"lipAp\"",
                "\"lipProt\"",
                "\"lipWidth\"",
                "\"root\"",
                "\"larynx\"",
                "\"ttX\"",
                "\"ttY\"",
                "\"ttCurl\""
            ]
            .contains(&ka.as_str()),
            "line {}: {a:?} vs {b:?}",
            i + 1
        );
        assert!((va - vb).abs() <= 0.0025, "line {}: {a:?} vs {b:?}", i + 1);
        differing.push(i + 1);
    }
    assert!(
        differing.len() <= 8,
        "{} posture values differ (lines {differing:?})",
        differing.len()
    );
}

/// A tiny dataset in CSD's layout: two phrases (a gap between) and a melisma, with a stereo
/// 16-bit 44.1 kHz "singer" that is a pure tone at each note's pitch.
fn mini_csd(dir: &str, syllables: [&str; 4]) {
    mini_song(dir, "x01a", syllables, 0.0);
}

/// One song of the tiny dataset; the singer's second harmonic has amplitude `h2` (re the first).
fn mini_song(dir: &str, id: &str, syllables: [&str; 4], h2: f64) {
    let notes = [
        (0.3, 0.8, 69.0),
        (0.8, 1.3, 69.0),
        (1.3, 1.8, 71.0),
        (2.1, 2.7, 72.0),
    ];
    std::fs::create_dir_all(format!("{dir}/english/csv")).unwrap();
    std::fs::create_dir_all(format!("{dir}/english/wav")).unwrap();
    let mut csv = String::from("start,end,pitch,syllable\r\n");
    for ((a, b, m), s) in notes.iter().zip(syllables) {
        csv += &format!("{a:.4},{b:.4},{m},{s}\r\n");
    }
    std::fs::write(format!("{dir}/english/csv/{id}.csv"), csv).unwrap();
    let fs = 44100.0;
    let n = (3.0 * fs) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f64 / fs;
        let v = notes
            .iter()
            .find(|(a, b, _)| t >= *a && t < *b)
            .map_or(0.0, |(_, _, m)| {
                let ph = 2.0 * std::f64::consts::PI * 440.0 * 2f64.powf((m - 69.0) / 12.0) * t;
                0.3 * (ph.sin() + h2 * (2.0 * ph).sin())
            });
        let s = ((v * 32767.0).round() as i16).to_le_bytes();
        data.extend_from_slice(&s);
        data.extend_from_slice(&s);
    }
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    for v in [1u16, 2] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&44100u32.to_le_bytes());
    b.extend_from_slice(&(44100u32 * 4).to_le_bytes());
    for v in [4u16, 16] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(format!("{dir}/english/wav/{id}.wav"), b).unwrap();
}

#[test]
fn compare_csd_sings_the_annotation_on_the_recordings_timeline() {
    let dir = tmp("csd-mini");
    let out = tmp("csd-mini-out");
    mini_csd(&dir, ["l_ii", "ii", "d_ou", "m_ai_n"]);
    let stdout = chimera(&[
        "compare", "csd", &dir, &out, "--songs", "x01a", "--jobs", "1",
    ]);
    assert!(stdout.contains("x01a    4 notes"), "{stdout}");

    // the render has the recording's length (3 s at 48 kHz, 16-bit mono)
    let synth = std::fs::metadata(format!("{out}/x01a-synth.wav"))
        .unwrap()
        .len();
    assert_eq!(synth, 44 + 3 * 48000 * 2);

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{out}/report.json")).unwrap())
            .unwrap();
    let notes = report["songs"][0]["notes"].as_array().unwrap();
    let ipa: Vec<&str> = notes.iter().map(|n| n["ipa"].as_str().unwrap()).collect();
    assert_eq!(ipa, ["li", "i", "doʊ", "maɪn"]);
    let flag = |k: &str| {
        notes
            .iter()
            .map(|n| n[k].as_bool().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(flag("melisma"), [false, true, false, false]);
    assert_eq!(flag("phraseStart"), [true, false, false, true]);
    // the "singer" is a pure tone on the score, so its pitch error is ~0
    for n in notes {
        let c = n["human"]["cents"].as_f64().unwrap();
        assert!(c.abs() < 3.0, "{n}");
    }
    // the synth sings every note near its pitch
    for n in notes {
        let c = n["synth"]["cents"].as_f64().unwrap();
        assert!(c.abs() < 60.0, "{n}");
    }
    assert!(
        std::fs::read_to_string(format!("{out}/report.md"))
            .unwrap()
            .contains("| measure |")
    );
}

#[test]
fn compare_csd_scores_training_and_held_out_songs_and_judges_a_rerun() {
    // six songs whose singer differs in H1−H2, so that term has a spread; x05a is held out
    let dir = tmp("csd-score");
    let ids = ["x01a", "x02a", "x03a", "x04a", "x05a", "x06a"];
    for (i, id) in ids.iter().enumerate() {
        mini_song(
            &dir,
            id,
            ["l_ii", "ii", "d_ou", "m_ai_n"],
            0.1 + 0.15 * i as f64,
        );
    }
    let songs = ids.join(",");
    let (first, second) = (tmp("csd-score-1"), tmp("csd-score-2"));
    chimera(&[
        "compare", "csd", &dir, &first, "--songs", &songs, "--jobs", "3",
    ]);
    let score: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{first}/score.json")).unwrap())
            .unwrap();
    let names = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        names(&score["train"]["songs"]),
        ["x01a", "x02a", "x03a", "x04a", "x06a"]
    );
    assert_eq!(names(&score["heldout"]["songs"]), ["x05a"]);
    let h1h2 = &score["train"]["terms"][5];
    assert_eq!(h1h2["name"], "H1-H2");
    assert!(h1h2["spread"].as_f64().unwrap() > 0.0, "{h1h2}");
    assert!(score["train"]["score"].is_number(), "{score}");
    assert_eq!(score["train"]["guards"]["notes"], 20);

    // the engine is deterministic: the same build scored again is no change
    let stdout = chimera(&[
        "compare",
        "csd",
        &dir,
        &second,
        "--songs",
        &songs,
        "--jobs",
        "3",
        "--against",
        &format!("{first}/score.json"),
    ]);
    assert!(stdout.contains("NO CLEAR CHANGE"), "{stdout}");
    // a run over other songs can't be judged against it
    let other = Command::new(env!("CARGO_BIN_EXE_chimera"))
        .args([
            "compare",
            "csd",
            &dir,
            &tmp("csd-score-3"),
            "--songs",
            "x01a,x02a,x03a,x04a,x05a",
            "--against",
            &format!("{first}/score.json"),
        ])
        .output()
        .unwrap();
    assert!(!other.status.success());
    assert!(String::from_utf8_lossy(&other.stderr).contains("other songs"));
}

#[test]
fn compare_csd_refuses_an_unmapped_phoneme() {
    let dir = tmp("csd-bad");
    mini_csd(&dir, ["l_ii", "ii", "d_qq", "m_ai_n"]);
    let out = Command::new(env!("CARGO_BIN_EXE_chimera"))
        .args([
            "compare",
            "csd",
            &dir,
            &tmp("csd-bad-out"),
            "--songs",
            "x01a",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unmapped CSD phonemes: qq"));
}

#[test]
fn render_notes_artic_tracks_follow_the_sound_without_changing_it() {
    let melody = tmp("melody.json");
    std::fs::write(
        &melody,
        r#"{"bpm": 120, "t0": 0, "lyrics": "Take my hand", "notes": [[0, 1], [67, 1], [63, 0.5], [60, 1.5]]}"#,
    )
    .unwrap();
    let (plain, tracked, tracks) = (tmp("notes-plain"), tmp("notes-tracked"), tmp("notes-artic.json"));
    chimera(&["render", "notes", &melody, &plain, "female"]);
    let stdout = chimera(&["render", "notes", &melody, &tracked, "female", "--artic", &tracks]);
    let a = std::fs::read(format!("{plain}.wav")).unwrap();
    let b = std::fs::read(format!("{tracked}.wav")).unwrap();
    assert!(a == b, "--artic changed the sound");

    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&tracks).unwrap()).unwrap();
    assert_eq!(j["rate"], 100);
    let keys: Vec<&str> = j["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    assert_eq!(keys.len(), 17);
    let frames = j["frames"].as_array().unwrap();
    // one frame at t = 0 and one per 480 samples rendered
    let samples = (a.len() - 44) / 2;
    assert_eq!(frames.len(), 1 + samples.div_ceil(480), "{stdout}");
    assert!(frames.iter().all(|f| f.as_array().unwrap().len() == 17));
    // the lips close for the /m/ of "my" (its note starts at 1 s) and open for its vowel
    let lip = keys.iter().position(|k| *k == "lipAp").unwrap();
    let ap = |i: usize| frames[i][lip].as_f64().unwrap();
    let closed = (85..105).map(ap).fold(f64::MAX, f64::min);
    let open = (105..125).map(ap).fold(f64::MIN, f64::max);
    assert!(closed < 0.1 && open > 0.5, "lipAp closed {closed}, open {open}");
}

#[test]
fn render_notes_tele_records_the_live_telemetry_without_changing_the_sound() {
    let melody = tmp("tele-melody.json");
    std::fs::write(
        &melody,
        r#"{"bpm": 120, "t0": 0, "lyrics": "Take my hand", "notes": [[0, 1], [67, 1], [63, 0.5], [60, 1.5]]}"#,
    )
    .unwrap();
    let (plain, tracked, tele, tracks) = (tmp("tele-plain"), tmp("tele-tracked"), tmp("voice.tele"), tmp("tele-artic.json"));
    chimera(&["render", "notes", &melody, &plain, "female"]);
    // both recordings at once: each keeps its own cadence
    let stdout = chimera(&["render", "notes", &melody, &tracked, "female", "--tele", &tele, "--artic", &tracks]);
    let a = std::fs::read(format!("{plain}.wav")).unwrap();
    let b = std::fs::read(format!("{tracked}.wav")).unwrap();
    assert!(a == b, "--tele changed the sound");
    let samples = (a.len() - 44) / 2;
    let artic: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&tracks).unwrap()).unwrap();
    assert_eq!(artic["frames"].as_array().unwrap().len(), 1 + samples.div_ceil(480));

    let t = std::fs::read(&tele).unwrap();
    assert_eq!(&t[..4], b"CHTL");
    let u = |i: usize| u32::from_le_bytes(t[4 + 4 * i..8 + 4 * i].try_into().unwrap()) as usize;
    let f = |o: usize| f32::from_le_bytes(t[o..o + 4].try_into().unwrap());
    let (version, rate, hop, frames, len, n_artic, n_ph, n_or, n_scope) = (u(0), u(1), u(2), u(3), u(4), u(5), u(6), u(7), u(8));
    assert_eq!((version, rate, hop, n_artic, n_scope), (1, 48000, 768, 17, 32));
    assert_eq!((f(40), f(44)), (1.0, 0.0), "body mix: female");
    // the female airway at 48 kHz, two times oversampled
    assert_eq!((n_ph, n_or), (15, 16));
    assert_eq!(len, 11 + n_artic + 3 * (n_ph + n_or) + 2 * n_scope);
    // frame 0 before any sound, then one per whole hop
    assert_eq!(frames, 1 + samples / hop, "{stdout}");
    assert_eq!(t.len(), 48 + 4 * frames * len);
    let at = |i: usize, k: usize| f(48 + 4 * (i * len + k));
    // the clock (field 10) is the frame's time
    for i in [0, 1, frames / 2, frames - 1] {
        assert!((at(i, 10) as f64 - (i * hop) as f64 / 48000.0).abs() < 1e-4, "frame {i} clock {}", at(i, 10));
    }
    // silent before the first note (a beat's rest: it starts at 0.5 s), then "Take" voiced on
    // G4 (392 Hz): frames 38-53 are 0.61-0.85 s
    let out_rms = |i: usize| at(i, 6);
    assert!((1..20).all(|i| out_rms(i) < 1e-4), "noise before the first note");
    let sung = 38..54;
    assert!(sung.clone().all(|i| out_rms(i) > 1e-2 && at(i, 4) > 0.5), "not voiced while singing");
    let f0 = sung.map(|i| at(i, 3)).sum::<f32>() / 16.0;
    assert!((f0 - 392.0).abs() < 12.0, "f0 {f0}");
    // the articulators agree with --artic's (lipAp at 1.0 s: artic frame 100, tele frame 62.5)
    let keys: Vec<&str> = artic["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    let lip = keys.iter().position(|k| *k == "lipAp").unwrap();
    let a_lip = artic["frames"][96][lip].as_f64().unwrap(); // 0.96 s = 60 tele hops
    assert!((at(60, 11 + lip) as f64 - a_lip).abs() < 1e-3, "lipAp {} vs {a_lip}", at(60, 11 + lip));
}
