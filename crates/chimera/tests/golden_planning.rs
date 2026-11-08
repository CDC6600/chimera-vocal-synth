//! The planners, singing, beatbox sounds and beat sequencer against the TypeScript engine
//! (fixtures from `bun tools/golden/planning.ts`). Plans are compared as the JSON the UI receives:
//! the same keys everywhere, strings and labels exact, `speed`/`releaseHold`/`tag` exact, other
//! numbers to 1e-9 relative (absolute floor 1e-9). The planners' expectations (utterances,
//! durations, phrases, songs, live notes) are re-blessed with `CHIMERA_BLESS=1` after an intended
//! change, like the audio fixtures. So are the beat sounds' gestures and the beat player's output,
//! which carry the calibrated vowel targets; the beat notation stays the TypeScript engine's.
mod common;
use chimera::anatomy::human::BODY_MALE;
use chimera::control::beat_player::BeatPlayer;
use chimera::control::score::Gesture;
use chimera::phonetics::Segment;
use chimera::phonetics::beatbox::{BEAT_PRESETS, BEAT_SOUNDS, BeatBuild, BeatStep, beat_ready, format_beat, parse_beat};
use chimera::phonetics::durations::{SegInfo, klatt_durations};
use chimera::phonetics::planner::{PhraseInput, PlanOptions, plan_phrases, plan_utterance};
use chimera::phonetics::sing::{MelismaOptions, NoteOnOptions, SongOptions, SungNote, plan_melisma, plan_note_off, plan_note_on, plan_song};
use chimera::voice::human_voice::{HumanVoice, SourcePatch};
use common::*;
use serde_json::{Value, json};

const TOL: f64 = 1e-9;
const EXACT: [&str; 3] = ["speed", "releaseHold", "tag"];

#[derive(Default)]
struct Stats {
    worst: f64,
    worst_at: String,
    numbers: usize,
}

/// Structural JSON comparison (`want` from TS, `got` from Rust).
fn cmp(path: &str, want: &Value, got: &Value, st: &mut Stats) {
    match (want, got) {
        (Value::Object(a), Value::Object(b)) => {
            // the same keys (in any order: JSON objects are maps)
            let mut ka: Vec<&String> = a.keys().collect();
            let mut kb: Vec<&String> = b.keys().collect();
            ka.sort();
            kb.sort();
            assert_eq!(ka, kb, "{path}: keys differ");
            for (k, va) in a {
                let p = format!("{path}.{k}");
                if EXACT.contains(&k.as_str()) {
                    assert_eq!(num(va), num(&b[k]), "{p}: must be exact");
                    st.numbers += 1;
                } else {
                    cmp(&p, va, &b[k], st);
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: length {} vs {}", a.len(), b.len());
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                cmp(&format!("{path}[{i}]"), x, y, st);
            }
        }
        (Value::Number(_), Value::Number(_)) => {
            let e = rel(num(want), num(got), TOL);
            st.numbers += 1;
            if e > st.worst {
                st.worst = e;
                st.worst_at = path.to_string();
            }
            assert!(e <= TOL, "{path}: {} vs {} (rel {e:.2e})", num(want), num(got));
        }
        _ => assert_eq!(want, got, "{path}"),
    }
}

fn report(name: &str, st: &Stats) {
    println!("{name:>12}: {} numbers, worst rel {:.2e} at {}", st.numbers, st.worst, if st.worst_at.is_empty() { "-" } else { &st.worst_at });
}

fn de<T: serde::de::DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"))
}

/// `CHIMERA_BLESS=1`: write the Rust planners' output as the new expectations of one section of
/// planning.json (its inputs stay). The tests run in parallel, so each rewrites the file under a
/// lock with only its own section changed.
fn bless(section: &str, field: &str, got: Vec<Value>) {
    bless_with(|f| {
        let items = f[section].as_array_mut().unwrap();
        assert_eq!(items.len(), got.len());
        for (item, v) in items.iter_mut().zip(got) {
            item[field] = v;
        }
    });
}

/// Rewrite planning.json under the lock, changing only what `edit` changes.
fn bless_with(edit: impl FnOnce(&mut Value)) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap();
    let mut f = load("planning");
    edit(&mut f);
    save_json("planning", &f);
}

fn val<T: serde::Serialize>(x: &T) -> Value {
    serde_json::to_value(x).unwrap()
}

#[test]
fn utterances_match_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let us = f["utterances"].as_array().unwrap();
    assert!(us.len() >= 60);
    let mut got = Vec::new();
    for u in us {
        let segs: Vec<Segment> = de(&u["segments"]);
        let opts: PlanOptions = de(&u["opts"]);
        let plan = val(&plan_utterance(&segs, &opts));
        if !blessing() {
            cmp(&format!("utterance {:?}", u["name"].as_str().unwrap()), &u["plan"], &plan, &mut st);
        }
        got.push(plan);
    }
    if blessing() {
        return bless("utterances", "plan", got);
    }
    report("utterances", &st);
}

#[test]
fn klatt_durations_match_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let mut got = Vec::new();
    for (ci, c) in f["klatt"].as_array().unwrap().iter().enumerate() {
        let segs: Vec<Segment> = de(&c["segments"]);
        let infos: Vec<SegInfo> = de(&c["infos"]);
        // the info callback receives the segment: find its index by identity
        let durs = val(&klatt_durations(&segs, |s| infos[segs.iter().position(|x| std::ptr::eq(x, s)).unwrap()]));
        if !blessing() {
            cmp(&format!("klatt {ci}"), &c["durs"], &durs, &mut st);
        }
        got.push(durs);
    }
    if blessing() {
        return bless("klatt", "durs", got);
    }
    report("klatt", &st);
}

#[test]
fn phrases_match_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let mut got = Vec::new();
    for (i, p) in f["phrases"].as_array().unwrap().iter().enumerate() {
        let phrases: Vec<PhraseInput> = de(&p["phrases"]);
        let opts: PlanOptions = de(&p["opts"]);
        let plan = val(&plan_phrases(&phrases, &opts));
        if !blessing() {
            cmp(&format!("phrases {i}"), &p["plan"], &plan, &mut st);
        }
        got.push(plan);
    }
    if blessing() {
        return bless("phrases", "plan", got);
    }
    report("phrases", &st);
}

#[test]
fn songs_match_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let mut got = Vec::new();
    for s in f["songs"].as_array().unwrap() {
        let notes: Vec<SungNote> = de(&s["notes"]);
        let opts: SongOptions = de(&s["opts"]);
        let plan = val(&plan_song(&notes, &opts));
        if !blessing() {
            cmp(&format!("song {:?}", s["name"].as_str().unwrap()), &s["plan"], &plan, &mut st);
        }
        got.push(plan);
    }
    if blessing() {
        return bless("songs", "plan", got);
    }
    report("songs", &st);
}

#[test]
fn live_notes_match_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let mut legato = 0;
    let mut gestures = Vec::new();
    for (i, n) in f["notes"].as_array().unwrap().iter().enumerate() {
        let midi = num(&n["midi"]);
        let got = match n["kind"].as_str().unwrap() {
            "on" => {
                let o: NoteOnOptions = de(&n["opts"]);
                legato += o.legato_from.is_some() as usize;
                plan_note_on(&de(&n["syl"]), midi, &o)
            }
            "off" => plan_note_off(&de(&n["syl"]), midi, n["body"].as_str().unwrap().into()),
            "melisma" => plan_melisma(midi, &de::<MelismaOptions>(&n["opts"])),
            k => panic!("{k}"),
        };
        let got = val(&got);
        if !blessing() {
            cmp(&format!("note {i} ({})", n["kind"]), &n["gestures"], &got, &mut st);
        }
        gestures.push(got);
    }
    assert!(legato > 0);
    if blessing() {
        return bless("notes", "gestures", gestures);
    }
    report("notes", &st);
}

#[test]
fn beat_sounds_match_typescript() {
    let mut f = load("planning");
    let mut st = Stats::default();
    let bless_it = blessing();
    let mut cmp = |path: &str, want: &mut Value, got: Value, st: &mut Stats| if bless_it { *want = got } else { cmp(path, want, &got, st) };
    let sounds = f["sounds"].as_array_mut().unwrap();
    assert_eq!(sounds.len(), BEAT_SOUNDS.len());
    for (s, want) in BEAT_SOUNDS.iter().zip(sounds) {
        // the catalogue the UI receives
        let mut meta = want.as_object().unwrap().clone();
        meta.remove("builds");
        meta.remove("leadIn");
        self::cmp(&format!("sound {}", s.id), &Value::Object(meta), &val(s), &mut st);
        for b in want["builds"].as_array_mut().unwrap() {
            let body = b["body"].as_str().unwrap().to_string();
            let o: BeatBuild = de(&b["o"]);
            let mut out = Vec::new();
            (s.build)(body.as_str().into(), &o, &mut out);
            let path = format!("sound {} {body} {}", s.id, b["o"]);
            cmp(&path, &mut b["gestures"], val(&out), &mut st);
        }
        for body in ["male", "female", "child"] {
            cmp(&format!("leadIn {} {body}", s.id), &mut want["leadIn"][body], json!(BeatPlayer::lead_in(s.id, body.into())), &mut st);
        }
    }
    for body in ["male", "female", "child"] {
        cmp(&format!("ready {body}"), &mut f["ready"][body], val(&beat_ready(body.into())), &mut st);
    }
    if bless_it {
        let (sounds, ready) = (f["sounds"].clone(), f["ready"].clone());
        return bless_with(|g| {
            g["sounds"] = sounds;
            g["ready"] = ready;
        });
    }
    report("sounds", &st);
}

#[test]
fn beat_notation_matches_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let n = &f["notation"];
    cmp("presets", &n["presets"], &val(&BEAT_PRESETS), &mut st);
    for t in n["texts"].as_array().unwrap() {
        let steps = parse_beat(t["text"].as_str().unwrap());
        cmp(&format!("parse {:?}", t["text"]), &t["steps"], &val(&steps), &mut st);
        assert_eq!(format_beat(&steps), t["formatted"].as_str().unwrap());
    }
    for t in n["format"].as_array().unwrap() {
        let steps: Vec<Option<BeatStep>> = de(&t["steps"]);
        assert_eq!(format_beat(&steps), t["formatted"].as_str().unwrap());
    }
}

fn run_player(ops: &[Value]) -> Vec<Value> {
    let mut p = BeatPlayer::new();
    let mut out = Vec::new();
    for o in ops {
        match o["op"].as_str().unwrap() {
            "steps" => p.set_steps(&parse_beat(o["text"].as_str().unwrap())),
            "tempo" => p.set_tempo(num(&o["bpm"]), num(&o["swing"])),
            "start" => p.start(num(&o["clock"])),
            "stop" => p.stop(),
            "body" => p.body = o["body"].as_str().unwrap().into(),
            "rebase" => p.rebase(num(&o["delta"])),
            "rewind" => p.rewind(num(&o["clock"])),
            "stepAt" => out.push(json!(nums(&o["clocks"]).iter().map(|c| p.step_at(*c)).collect::<Vec<_>>())),
            "gestures" => {
                let cell: BeatStep = de(&o["cell"]);
                let len = if o["len"].is_null() { p.step_dur() } else { num(&o["len"]) };
                let mut gs = Vec::new();
                p.gestures(&cell, num(&o["hit"]), len, &mut gs);
                out.push(val(&gs));
            }
            "run" => {
                let (from, to, dt) = (num(&o["from"]), num(&o["to"]), num(&o["dt"]));
                let mut batches = Vec::new();
                let mut clock = from;
                while clock < to {
                    p.tick(clock, &mut |gs: &[Gesture]| batches.push(json!({ "clock": clock, "gestures": gs })));
                    clock += dt;
                }
                out.push(Value::Array(batches));
            }
            op => panic!("{op}"),
        }
    }
    out
}

#[test]
fn beat_player_matches_typescript() {
    let f = load("planning");
    let mut st = Stats::default();
    let mut got_all = Vec::new();
    for (i, s) in f["player"].as_array().unwrap().iter().enumerate() {
        let got = Value::Array(run_player(s["ops"].as_array().unwrap()));
        if !blessing() {
            cmp(&format!("player {i}"), &s["out"], &got, &mut st);
        }
        got_all.push(got);
    }
    if blessing() {
        return bless("player", "out", got_all);
    }
    report("player", &st);
}

/// A beatbox bar through the human voice, driven as the worklet drives it: tick before each
/// 128-sample block.
#[test]
fn beatbox_bar_audio_matches_typescript() {
    let f = load("planning");
    let a = &f["audio"];
    let fs = num(&a["fs"]);
    let n = a["samples"].as_u64().unwrap() as usize;
    let tempo_at = a["tempoAt"].as_u64().unwrap() as usize;
    chimera::phonetics::beatbox::init();
    let mut v = HumanVoice::new(fs, BODY_MALE, 2);
    v.set_source(&SourcePatch::f0(num(&a["f0"])));
    v.set_articulation(&beat_ready("male".into()), None);
    let mut p = BeatPlayer::new();
    p.set_steps(&parse_beat(a["text"].as_str().unwrap()));
    p.set_tempo(num(&a["bpm"]), num(&a["swing"]));
    p.start(v.clock);
    let mut out = vec![0.0f32; n];
    let (mut b, mut i) = (0usize, 0usize);
    while i < n {
        if b == tempo_at {
            p.set_tempo(num(&a["tempo"][0]), num(&a["tempo"][1]));
        }
        p.tick(v.clock, &mut |gs: &[Gesture]| v.schedule(gs));
        let end = n.min(i + 128);
        v.render(&mut out[i..end]);
        b += 1;
        i = end;
    }
    if blessing() {
        save_f32("beatbox-bar", &out);
        return;
    }
    let reference = load_f32("beatbox-bar");
    let d = compare_audio(&reference, &out, 1e-6, 480);
    println!("beatbox bar: peak {:.3} max err {:.2e} first >1e-6 at {:?} env {:.4} dB", d.peak, d.max_err, d.first_diverge, d.env_db);
    assert_close("beatbox-bar", &d);
}
