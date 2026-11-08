//! The instrument without a host: what it sings, and how it behaves on the audio thread.
use chimera::anatomy::human::{ArticKey, ArticPatch, BodyMix};
use chimera::engine::Engine;
use chimera::control::beat_player::BeatPlayer;
use chimera::phonetics::Syllable;
use chimera::phonetics::beatbox::{BEAT_SOUNDS, BeatStep, parse_beat};
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera_plugin::core::{BEAT_LATENCY, BeatSettings, Core, DEFAULT_LYRICS, Event, Mode, NOT_SPEAKING, Prepared, SAYING_PAD, Settings, Shared, Transport, UiNote, VoiceParams, drum_pad};
use chimera_plugin::g2p;
use chimera_plugin::speech::{SpeechSettings, plan_lines, plan_pad};
use std::sync::atomic::Ordering;

const FS: f64 = 44100.0;

fn params() -> VoiceParams {
    VoiceParams { vibrato: 35.0, vibrato_rate: 6.0, breath: 8.0, pressed: 0.5, bright: 0.25, breathy: 0.15, rough: 0.1, fry: 0.0, glide: 0.06, ring: 0.0, restart: true }
}

fn syllables(text: &str) -> Vec<Syllable> {
    lyrics_to_syllables(text, g2p())
}

fn core_with(settings: &Settings, shared: &Shared) -> Core {
    let prepared = Prepared::of(settings, shared);
    shared.restart(FS);
    let c = Core::new(FS, settings, &prepared, params(), shared);
    shared.adopt(prepared);
    c
}

fn core(shared: &Shared) -> Core {
    core_with(&Settings::default(), shared)
}

/// Run `samples` in blocks of `block`, taking each event on its sample, as `process` does.
fn run(c: &mut Core, shared: &Shared, events: &[(usize, Event)], samples: usize, block: impl Fn(usize) -> usize) -> Vec<f32> {
    let mut out = vec![0.0f32; samples];
    let (mut pos, mut next, mut k) = (0, 0, 0);
    while pos < samples {
        let n = block(k).min(samples - pos);
        k += 1;
        c.begin_block(&params(), Transport::default(), shared);
        let mut at = pos;
        while next < events.len() && events[next].0 < pos + n {
            let t = events[next].0;
            if t > at {
                c.render(&mut out[at..t]);
                at = t;
            }
            c.event(events[next].1);
            next += 1;
        }
        if at < pos + n {
            c.render(&mut out[at..pos + n]);
        }
        c.end_block(n, shared);
        pos += n;
    }
    out
}

fn on(midi: u8) -> Event {
    Event::NoteOn { midi, velocity: 96.0 }
}

fn off(midi: u8) -> Event {
    Event::NoteOff { midi }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |p, v| p.max(v.abs()))
}

fn song() -> Vec<(usize, Event)> {
    let s = FS as usize;
    vec![(1000, on(57)), (s / 2, off(57)), (s / 2 + 77, on(60)), (s, on(64)), (s + 2000, off(60)), (3 * s / 2, off(64)), (2 * s, on(62)), (2 * s + s / 3, Event::Bend(0.8)), (3 * s, off(62))]
}

#[test]
fn output_does_not_depend_on_the_hosts_block_size() {
    let shared = Shared::default();
    let n = 4 * FS as usize;
    let reference = run(&mut core(&shared), &shared, &song(), n, |_| 512);
    assert!(peak(&reference) > 0.01, "the song is silent");
    for (name, block) in [("64", (|_| 64) as fn(usize) -> usize), ("441", |_| 441), ("4096", |_| 4096), ("varying", |k| 32 + (k * 97) % 900)] {
        let out = run(&mut core(&shared), &shared, &song(), n, block);
        let first = out.iter().zip(&reference).position(|(a, b)| a.to_bits() != b.to_bits());
        assert_eq!(first, None, "blocks of {name} differ from blocks of 512");
    }
}

#[test]
fn watching_does_not_change_the_sound() {
    let n = 2 * FS as usize;
    let shared = Shared::default();
    let quiet = run(&mut core(&shared), &shared, &song(), n, |_| 256);
    let shared = Shared::default();
    let mut c = core(&shared);
    let mut out = Vec::new();
    // an editor polling all the way through, against a fresh core's identical event list
    for chunk in 0..(n / 4410) {
        shared.watch();
        let from = chunk * 4410;
        let evs: Vec<_> = song().into_iter().filter(|(t, _)| *t >= from && *t < from + 4410).map(|(t, e)| (t - from, e)).collect();
        out.extend(run(&mut c, &shared, &evs, 4410, |_| 256));
    }
    assert!(shared.telemetry.lock().unwrap().fresh, "telemetry was never collected");
    // run() re-blocks each chunk, so this also splits differently: still the same samples
    let first = out.iter().zip(&quiet).position(|(a, b)| a.to_bits() != b.to_bits());
    assert_eq!(first, None);
}

#[test]
fn a_fresh_core_repeats_a_bounce_exactly() {
    let n = 2 * FS as usize;
    let shared = Shared::default();
    let a = run(&mut core(&shared), &shared, &song(), n, |_| 256);
    let b = run(&mut core(&shared), &shared, &song(), n, |_| 256);
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn reset_silences_and_rewinds_and_the_song_sounds_again() {
    let shared = Shared::default();
    let mut c = core(&shared);
    let s = FS as usize;
    let first = run(&mut c, &shared, &[(0, on(57))], s / 2, |_| 256);
    assert_eq!(c.singer().cursor(), 1);
    c.reset();
    assert_eq!((c.singer().cursor(), c.singer().sounding_note()), (0, None));
    let quiet = run(&mut c, &shared, &[], s / 2, |_| 256);
    assert!(peak(&quiet[s / 4..]) < 1e-3, "still sounding after reset: {}", peak(&quiet[s / 4..]));
    let again = run(&mut c, &shared, &[(0, on(57))], s / 2, |_| 256);
    let (a, b) = (peak(&first), peak(&again));
    assert!((a - b).abs() < 0.25 * a, "not the same note after reset: peaks {a} and {b}");
}

/// nih-plug turns flush-to-zero on around `process`. The voice must stay stable and sound the same.
#[test]
fn flush_to_zero_leaves_the_voice_alone() {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    fn set_ftz(on: bool) {
        use std::arch::x86_64::{_MM_FLUSH_ZERO_OFF, _MM_FLUSH_ZERO_ON, _MM_SET_FLUSH_ZERO_MODE};
        unsafe { _MM_SET_FLUSH_ZERO_MODE(if on { _MM_FLUSH_ZERO_ON } else { _MM_FLUSH_ZERO_OFF }) }
    }
    #[cfg(target_arch = "aarch64")]
    fn set_ftz(on: bool) {
        unsafe {
            let mut fpcr: u64;
            std::arch::asm!("mrs {}, fpcr", out(reg) fpcr);
            if on {
                fpcr |= 1 << 24;
            } else {
                fpcr &= !(1 << 24);
            }
            std::arch::asm!("msr fpcr, {}", in(reg) fpcr);
        }
    }
    let n = 4 * FS as usize;
    let shared = Shared::default();
    let plain = run(&mut core(&shared), &shared, &song(), n, |_| 256);
    let mut c = core(&shared);
    set_ftz(true);
    let flushed = run(&mut c, &shared, &song(), n, |_| 256);
    set_ftz(false);
    assert!(flushed.iter().all(|x| x.is_finite()));
    let worst = plain.iter().zip(&flushed).fold(0.0f32, |w, (a, b)| w.max((a - b).abs()));
    assert!(worst < 1e-3 * peak(&plain), "flush-to-zero moved the output by {worst} (peak {})", peak(&plain));
}

#[test]
fn a_ui_key_cannot_release_a_host_key_on_the_same_pitch() {
    let shared = Shared::default();
    let mut c = core(&shared);
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60));
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: false });
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60), "the host still holds this key");
    run(&mut c, &shared, &[(0, off(60))], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), None);
}

#[test]
fn a_key_already_held_by_the_other_owner_sings_nothing_new() {
    let shared = Shared::default();
    let mut c = core(&shared);
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 1);
    // the UI presses what the host holds
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 1, "the host's syllable must not be replaced");
    assert_eq!(shared.note_serial.load(Ordering::Relaxed), 1);
    // and the other way round: the host lifts, the UI still holds; the host presses it again
    run(&mut c, &shared, &[(0, off(60))], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60));
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 1);
    // a repeat from the same owner is a new note, which keeps the lyrics in step with the song
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 1);
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: false });
    run(&mut c, &shared, &[(0, off(60))], 256, |_| 256);
    run(&mut c, &shared, &[(0, on(60)), (10, off(60)), (20, on(60))], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 3);
    assert_eq!(shared.note_serial.load(Ordering::Relaxed), 3);
}

#[test]
fn closing_the_window_lifts_only_the_ui_keys() {
    let shared = Shared::default();
    let mut c = core(&shared);
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    shared.ui_note(UiNote { midi: 64, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(64));
    shared.clear_ui_keys();
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60), "glides back to the key the host still holds");
    assert!(c.singer().any_key_down());
}

#[test]
fn too_many_ui_notes_cannot_leave_one_stuck() {
    let shared = Shared::default();
    let mut c = core(&shared);
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60));
    // the host has stopped calling process, and the queue overflows before the release gets in
    for _ in 0..300 {
        shared.ui_note(UiNote { midi: 61, velocity: 96.0, on: true });
        shared.ui_note(UiNote { midi: 61, velocity: 96.0, on: false });
    }
    shared.clear_ui_keys(); // what overflow set anyway; a release afterward is honoured as usual
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), None);
    assert!(!c.singer().any_key_down());
}

#[test]
fn the_transport_restarts_the_lyrics_and_stops_the_notes() {
    let shared = Shared::default();
    let mut c = core(&shared);
    let mut block = vec![0.0f32; 256];
    let mut step = |c: &mut Core, playing: bool, position: f64, ev: Option<Event>| {
        c.begin_block(&params(), Transport { playing, position: Some(position), ..Default::default() }, &shared);
        if let Some(e) = ev {
            c.event(e);
        }
        c.render(&mut block);
        c.end_block(256, &shared);
    };
    step(&mut c, true, 0.0, Some(on(60)));
    step(&mut c, true, 0.1, Some(off(60)));
    step(&mut c, true, 0.2, Some(on(62)));
    assert_eq!(c.singer().cursor(), 2);
    // a loop: the position jumps back
    step(&mut c, true, 0.0, None);
    assert_eq!(c.singer().cursor(), 0, "a loop must put the lyrics back");
    step(&mut c, true, 0.1, Some(on(62)));
    assert_eq!(c.singer().cursor(), 1);
    // stop, with no note-offs from the host
    step(&mut c, false, 0.2, None);
    assert_eq!(c.singer().sounding_note(), None, "stopping must end the note");
    assert!(!c.singer().any_key_down());
    // a syllable picked while stopped is where playing starts from
    shared.edit(|p| p.cursor = Some(3));
    step(&mut c, false, 0.2, None);
    assert_eq!(c.singer().cursor(), 3);
    step(&mut c, true, 0.0, None);
    assert_eq!(c.singer().cursor(), 3);
    step(&mut c, true, 0.1, Some(on(60)));
    assert_eq!(c.singer().cursor(), 4);
    step(&mut c, false, 0.2, None);
    step(&mut c, true, 0.0, None);
    assert_eq!(c.singer().cursor(), 3, "playing again starts from the picked syllable");
}

#[test]
fn live_playing_can_keep_its_place_through_the_transport() {
    let shared = Shared::default();
    let mut c = core(&shared);
    let mut block = vec![0.0f32; 256];
    let no_restart = VoiceParams { restart: false, ..params() };
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0), ..Default::default() }, &shared);
    c.event(on(60));
    c.event(off(60));
    c.render(&mut block);
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0), ..Default::default() }, &shared);
    c.render(&mut block);
    assert_eq!(c.singer().cursor(), 1);
    c.begin_block(&no_restart, Transport { playing: false, position: Some(0.0), ..Default::default() }, &shared);
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0), ..Default::default() }, &shared);
    assert_eq!(c.singer().cursor(), 1);
}

#[test]
fn a_body_change_swaps_the_voice_and_hands_back_the_old_one() {
    let shared = Shared::default();
    let mut c = core(&shared);
    run(&mut c, &shared, &[(0, on(60))], 2048, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(60));
    let male = c.engine().info();
    shared.send_voice(Engine::prepare_voice(FS, BodyMix::FEMALE), BodyMix::FEMALE);
    let out = run(&mut c, &shared, &[], 2048, |_| 256);
    assert_eq!(c.engine().info().body, BodyMix::FEMALE);
    assert_ne!(c.engine().info().n_ph + c.engine().info().n_or, male.n_ph + male.n_or);
    assert_eq!(c.singer().sounding_note(), None, "the new voice has no score, so the note is over");
    assert!(out.iter().all(|x| x.is_finite()));
    // and it sings
    let out = run(&mut c, &shared, &[(0, on(69))], FS as usize / 2, |_| 256);
    assert!(peak(&out) > 0.01);
    // a voice for another sample rate is never offered to the audio thread
    shared.send_voice(Engine::prepare_voice(48000.0, BodyMix::CHILD), BodyMix::CHILD);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.engine().info().body, BodyMix::FEMALE);
}

#[test]
fn new_lyrics_keep_the_sounding_notes_own_coda() {
    let shared = Shared::default();
    let mut c = core(&shared);
    shared.send_lyrics(syllables("cat"));
    run(&mut c, &shared, &[(0, on(60))], 8192, |_| 256);
    shared.send_lyrics(syllables("ooh ah"));
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().cursor(), 0);
    assert_eq!(c.singer().sounding_note(), Some(60));
    // the coda of "cat" is scheduled when the note lifts
    run(&mut c, &shared, &[(0, off(60))], 256, |_| 256);
    assert!(c.engine().score().gestures().iter().any(|g| g.label_str() == "t"), "the /t/ of the old lyrics is gone");
}

#[test]
fn play_mode_breathes_the_posture_at_the_keys_pitch() {
    let shared = Shared::default();
    let settings = Settings { mode: Mode::Play, ..Default::default() };
    let mut c = core_with(&settings, &shared);
    let quiet = run(&mut c, &shared, &[], 4096, |_| 256);
    assert!(peak(&quiet) < 1e-3, "breathing with no key down");
    let out = run(&mut c, &shared, &[(0, on(57))], FS as usize / 2, |_| 256);
    assert!(peak(&out) > 0.01, "silent with a key down");
    assert_eq!(c.engine().voice().source.f0, 220.0);
    run(&mut c, &shared, &[(0, on(64)), (64, off(64))], 256, |_| 256);
    assert_eq!(c.engine().voice().source.f0, 220.0, "falls back to the key still held");
    run(&mut c, &shared, &[(0, off(57))], FS as usize / 2, |_| 256);
    // hold keeps the breath on with no key, and going back to Sing ends it
    shared.edit(|p| p.hold = Some(true));
    let held = run(&mut c, &shared, &[], FS as usize / 2, |_| 256);
    assert!(peak(&held) > 0.01, "hold should breathe");
    shared.edit(|p| p.mode = Some(Mode::Sing));
    run(&mut c, &shared, &[], FS as usize, |_| 256);
    let after = run(&mut c, &shared, &[], 4096, |_| 256);
    assert!(peak(&after) < 1e-3, "still breathing after leaving Play: {}", peak(&after));
}

#[test]
fn a_restored_project_comes_back() {
    let shared = Shared::default();
    let settings = Settings {
        lyrics: "one two three".into(),
        body: BodyMix::CHILD,
        mode: Mode::Play,
        artic: ArticPatch::default().with(ArticKey::Jaw, 1.7),
        voiced: false,
        f0: 333.0,
        start_syllable: 2,
        ..Default::default()
    };
    // through the JSON the host stores
    let settings: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    let c = core_with(&settings, &shared);
    let v = c.engine().voice();
    assert_eq!(c.engine().info().body, BodyMix::CHILD);
    assert_eq!(v.source.f0, 333.0);
    assert!(!v.source.voiced);
    assert_eq!(v.artic.target.jaw, 1.7);
    assert_eq!(c.singer().cursor(), 2);
    // and a project saved by an older build, with less in it, still loads
    let old: Settings = serde_json::from_str(r#"{"version":1,"lyrics":"la"}"#).unwrap();
    assert_eq!((old.lyrics.as_str(), old.voiced, old.f0), ("la", true, 110.0));
    assert_eq!(Settings::default().lyrics, DEFAULT_LYRICS);
}

#[test]
fn a_restart_drops_what_was_queued_for_the_old_core() {
    let shared = Shared::default();
    let mut c = core(&shared);
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    shared.send_lyrics(syllables("stale words"));
    let revision = shared.revision.load(Ordering::Relaxed);
    shared.restart(FS);
    assert_eq!(shared.revision.load(Ordering::Relaxed), revision + 1);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), None);
    // and what is published after it still gets through
    shared.ui_note(UiNote { midi: 62, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.singer().sounding_note(), Some(62));
}

/// Commands from the page carry the revision it last read: one from before a restore is dropped.
#[test]
fn a_command_from_before_a_restore_is_dropped() {
    use chimera_plugin::bridge::{Bridge, ParamHost};
    use chimera_plugin::ChimeraParams;
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Host(Arc<Mutex<Vec<String>>>);
    impl ParamHost for Host {
        fn begin(&self, id: &str) {
            self.0.lock().unwrap().push(format!("begin {id}"));
        }
        fn set(&self, id: &str, _plain: f32) {
            self.0.lock().unwrap().push(format!("set {id}"));
        }
        fn end(&self, id: &str) {
            self.0.lock().unwrap().push(format!("end {id}"));
        }
        fn values(&self) -> Vec<(String, f32)> {
            Vec::new()
        }
    }
    let params = Arc::new(ChimeraParams::default());
    let shared = Arc::new(Shared::default());
    let log = Host::default();
    let bridge = Bridge::new(params.clone(), shared.clone(), Box::new(log.clone()));
    shared.restart(FS);
    let rev = shared.revision.load(Ordering::Relaxed);
    let lyrics = || params.settings.read().unwrap().lyrics.clone();

    bridge.command(&format!(r#"{{"rev":{rev},"type":"lyrics","text":"current"}}"#));
    assert_eq!(lyrics(), "current");
    bridge.command(&format!(r#"{{"rev":{rev},"type":"param","id":"fry","value":0.5}}"#));
    assert_eq!(*log.0.lock().unwrap(), ["begin fry", "set fry"]);

    shared.restart(FS);
    bridge.command(&format!(r#"{{"rev":{rev},"type":"lyrics","text":"stale"}}"#));
    assert_eq!(lyrics(), "current", "a stale edit must not touch the restored project");
    bridge.command(&format!(r#"{{"rev":{rev},"type":"param","id":"fry","value":0.9}}"#));
    assert_eq!(log.0.lock().unwrap().len(), 2, "nor move a restored parameter");
    // but an open gesture still ends, or the VST3 layer would ignore automation on it
    bridge.command(&format!(r#"{{"rev":{rev},"type":"paramEnd","id":"fry"}}"#));
    assert_eq!(log.0.lock().unwrap().last().unwrap(), "end fry");
    // a command without a revision doesn't parse, so can't slip past this
    bridge.command(r#"{"type":"lyrics","text":"no revision"}"#);
    assert_eq!(lyrics(), "current");
}

#[test]
fn pitch_bend_moves_the_voice_and_centres() {
    let shared = Shared::default();
    let mut c = core(&shared);
    run(&mut c, &shared, &[(0, on(57))], FS as usize, |_| 256);
    let f0 = |c: &mut Core, shared: &Shared| {
        shared.watch();
        run(c, shared, &[], 8192, |_| 256);
        let block = std::mem::take(&mut shared.telemetry.lock().unwrap().block);
        block[3] // see TELE_HEADER
    };
    let plain = f0(&mut c, &shared);
    run(&mut c, &shared, &[(0, Event::Bend(1.0))], FS as usize / 2, |_| 256);
    let up = f0(&mut c, &shared);
    let semitones = 12.0 * (up / plain).log2();
    assert!((semitones - 2.0).abs() < 0.5, "a full bend should be about two semitones, got {semitones}");
}

/// A window smaller than the layout shows the whole layout, zoomed out, whichever side is short.
#[test]
fn a_small_window_zooms_out_and_a_large_one_doesnt() {
    use chimera_plugin::bridge::{LAYOUT_MIN, WINDOW_DEFAULT, WINDOW_MAX, WINDOW_MIN, page_zoom};
    assert_eq!(page_zoom(WINDOW_DEFAULT), 1.0, "the first size needs no zoom");
    assert_eq!(page_zoom(WINDOW_MAX), 1.0);
    assert_eq!(page_zoom((LAYOUT_MIN.0 / 2, LAYOUT_MIN.1 * 2)), 0.5);
    assert_eq!(page_zoom((LAYOUT_MIN.0 * 2, LAYOUT_MIN.1 / 2)), 0.5);
    let z = page_zoom(WINDOW_MIN);
    assert!(z > 0.5 && z < 1.0, "the smallest window stays legible: {z}");
    // what's laid out never needs less room than the layout's
    for (w, h) in [WINDOW_MIN, (900, 700), (2000, 600), (1000, 1400)] {
        let z = page_zoom((w, h));
        assert!(w as f64 / z >= LAYOUT_MIN.0 as f64 - 1e-9 && h as f64 / z >= LAYOUT_MIN.1 as f64 - 1e-9);
    }
}

/// Found by review: `initialize` read the settings before taking the transaction, so an edit landing
/// in between was saved and shown but never reached the audio thread.
#[test]
fn an_edit_during_a_rebuild_reaches_the_rebuilt_core() {
    use chimera_plugin::{ChimeraParams, rebuild};
    use std::sync::Arc;
    let params = Arc::new(ChimeraParams::default());
    let shared = Arc::new(Shared::default());
    shared.restart(FS);
    // a command from the page, in the middle of its transaction
    let command = shared.transaction();
    let (p, s) = (params.clone(), shared.clone());
    let voice = params.voice();
    let activating = std::thread::spawn(move || drop(rebuild(&p, &s, voice, FS)));
    // long enough for a rebuild that reads first to have read
    std::thread::sleep(std::time::Duration::from_millis(100));
    params.settings.write().unwrap().lyrics = "no".to_string();
    drop(command);
    activating.join().unwrap();
    let sung = shared.syllables.lock().unwrap();
    assert_eq!(sung.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["no"], "the rebuilt instrument has the edit");
}

// ---------------------------------------------------------------- Speak

fn speaking(text: &str, shared: &Shared) -> Core {
    let settings = Settings { mode: Mode::Speak, speech: SpeechSettings { text: text.to_string(), ..Default::default() }, ..Default::default() };
    core_with(&settings, shared)
}

fn spoken(shared: &Shared) -> i32 {
    shared.spoken_line.load(Ordering::Relaxed)
}

#[test]
fn each_note_says_the_next_line_and_goes_round() {
    let shared = Shared::default();
    let mut c = speaking("Hello there.\n\n  One more line.  \nThe last.", &shared);
    assert_eq!(shared.speech.lock().unwrap().lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Hello there.", "One more line.", "The last."], "blank lines say nothing");
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), NOT_SPEAKING, "nothing said yet");
    for expect in [0, 1, 2, 0] {
        let serial = shared.spoken_serial.load(Ordering::Relaxed);
        let out = run(&mut c, &shared, &[(0, on(60)), (2000, off(60))], (FS * 0.5) as usize, |_| 256);
        assert_eq!(spoken(&shared), expect);
        assert_ne!(shared.spoken_serial.load(Ordering::Relaxed), serial, "the page can tell a new line started");
        assert!(peak(&out) > 0.001, "line {expect} sounds: {}", peak(&out));
        assert!(c.engine().score().size() > 0, "the note's release doesn't end the line");
    }
}

#[test]
fn a_new_note_cuts_the_line_sounding() {
    let shared = Shared::default();
    let mut c = speaking("The quick brown fox jumps over the lazy dog.\nNo.", &shared);
    run(&mut c, &shared, &[(0, on(60))], 2048, |_| 256);
    let long = c.engine().score().size();
    run(&mut c, &shared, &[(0, on(62))], 2048, |_| 256);
    assert!(c.engine().score().size() < long, "only the short line is left");
    assert_eq!(spoken(&shared), 1);
}

#[test]
fn the_key_transposes_like_a_sampler() {
    let shared = Shared::default();
    let mut c = speaking("Hello.", &shared);
    let bend = |c: &Core| c.engine().voice().bend;
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(bend(&c), 1.0, "C4 speaks at the natural pitch");
    run(&mut c, &shared, &[(0, on(72))], 256, |_| 256);
    assert!((bend(&c) - 2.0).abs() < 1e-9, "an octave up: {}", bend(&c));
    run(&mut c, &shared, &[(0, Event::Bend(0.75))], 256, |_| 256);
    assert!((bend(&c) - 2.0 * 2f64.powf(2.0 / 24.0 * 2.0 / 2.0)).abs() < 1e-3, "the wheel moves it further: {}", bend(&c));
    // far keys stay within two octaves, and the voice stays well behaved there
    for midi in [0, 127] {
        let out = run(&mut c, &shared, &[(0, on(midi))], (FS * 0.6) as usize, |_| 256);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 4.0), "key {midi} peaks at {}", peak(&out));
    }
    run(&mut c, &shared, &[(0, Event::Bend(0.5))], 256, |_| 256);
    assert!((bend(&c) - 4.0).abs() < 1e-9, "at most two octaves: {}", bend(&c));
}

#[test]
fn the_transport_brings_the_lines_back_and_a_stop_stops_them() {
    let shared = Shared::default();
    let mut c = speaking("One.\nTwo.\nThree.", &shared);
    let block = |c: &mut Core, t: Transport, evs: &[Event]| {
        c.begin_block(&params(), t, &shared);
        for e in evs {
            c.event(*e);
        }
        c.render(&mut [0.0; 256]);
        c.end_block(256, &shared);
    };
    let at = |p: f64| Transport { playing: true, position: Some(p), ..Default::default() };
    // pick the second line while stopped: that's where playing starts
    shared.edit(|p| p.speech_cursor = Some(1));
    block(&mut c, Transport::default(), &[]);
    assert_eq!(c.speech_cursor(), 1);
    block(&mut c, at(0.0), &[on(60)]);
    assert_eq!(spoken(&shared), 1);
    block(&mut c, at(0.1), &[on(60)]);
    assert_eq!(spoken(&shared), 2);
    // a loop
    block(&mut c, at(0.0), &[]);
    assert_eq!(c.speech_cursor(), 1, "back to the start line");
    block(&mut c, at(0.1), &[on(60)]);
    assert!(c.engine().score().size() > 0);
    block(&mut c, Transport::default(), &[]);
    assert_eq!(spoken(&shared), NOT_SPEAKING);
    assert_eq!(c.engine().score().size(), 0, "a stop stops the speech");
}

#[test]
fn new_plans_keep_the_place_unless_the_words_changed() {
    let shared = Shared::default();
    let mut c = speaking("One.\nTwo.\nThree.", &shared);
    let speech = |text: &str| SpeechSettings { text: text.to_string(), ..Default::default() };
    run(&mut c, &shared, &[(0, on(60)), (10, on(61))], 256, |_| 256);
    assert_eq!(c.speech_cursor(), 2);
    let before = shared.spoken_generation.load(Ordering::Relaxed);
    assert_eq!(before, shared.speech.lock().unwrap().lines_generation, "what's said is from these plans");
    // a new rate, or a new body: the same words
    shared.send_speech(plan_lines(&SpeechSettings { rate: 1.5, ..speech("One.\nTwo.\nThree.") }, BodyMix::MALE, g2p()), false);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.speech_cursor(), 2);
    assert_ne!(shared.speech.lock().unwrap().lines_generation, before);
    assert_eq!(shared.spoken_generation.load(Ordering::Relaxed), before, "and what's sounding is still the old plan: the page must not draw the new one over it");
    // fewer lines: the place is kept in range
    shared.send_speech(plan_lines(&speech("One.\nTwo."), BodyMix::MALE, g2p()), false);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.speech_cursor(), 1);
    // new words
    shared.send_speech(plan_lines(&speech("Other words.\nAnd more."), BodyMix::MALE, g2p()), true);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.speech_cursor(), 0);
    // no words at all: a note does nothing
    shared.send_speech(plan_lines(&speech("  \n "), BodyMix::MALE, g2p()), true);
    run(&mut c, &shared, &[], 256, |_| 256);
    let serial = shared.spoken_serial.load(Ordering::Relaxed);
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(shared.spoken_serial.load(Ordering::Relaxed), serial, "no line to say");
}

#[test]
fn the_speak_button_the_stop_button_and_the_chart() {
    let shared = Shared::default();
    let mut c = speaking("One.\nTwo.", &shared);
    shared.edit(|p| p.speak_next = true);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), 0);
    assert_eq!(c.engine().voice().bend, 1.0);
    shared.edit(|p| p.stop_speech = true);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), NOT_SPEAKING);
    assert_eq!(c.speech_cursor(), 1, "stop keeps the place");
    shared.send_pad(plan_pad("ˈɑpɑ", BodyMix::MALE, 1.0));
    let out = run(&mut c, &shared, &[], (FS * 0.3) as usize, |_| 256);
    assert_eq!(spoken(&shared), SAYING_PAD);
    assert_eq!(shared.spoken_generation.load(Ordering::Relaxed), shared.speech.lock().unwrap().pad_generation);
    assert!(peak(&out) > 0.001);
    assert_eq!(c.speech_cursor(), 1, "a pad isn't a line");
    // in another mode none of these does anything
    shared.edit(|p| p.mode = Some(Mode::Sing));
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), NOT_SPEAKING);
    shared.edit(|p| p.speak_next = true);
    shared.send_pad(plan_pad("ˈɑpɑ", BodyMix::MALE, 1.0));
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), NOT_SPEAKING);
}

#[test]
fn a_new_body_stops_the_line_and_replans_for_its_airway() {
    use chimera_plugin::bridge::{Bridge, ParamHost};
    use chimera_plugin::ChimeraParams;
    use std::sync::Arc;
    struct Host;
    impl ParamHost for Host {
        fn begin(&self, _: &str) {}
        fn set(&self, _: &str, _: f32) {}
        fn end(&self, _: &str) {}
        fn values(&self) -> Vec<(String, f32)> {
            Vec::new()
        }
    }
    let params = Arc::new(ChimeraParams::default());
    let shared = Arc::new(Shared::default());
    let bridge = Bridge::new(params.clone(), shared.clone(), Box::new(Host));
    let mut c = chimera_plugin::rebuild(&params, &shared, params.voice(), FS).unwrap();
    let rev = shared.revision.load(Ordering::Relaxed);
    let send = |json: String| bridge.command(&json.replacen('{', &format!("{{\"rev\":{rev},"), 1));
    send(r#"{"type":"mode","mode":"speak"}"#.to_string());
    send(r#"{"type":"speech","text":"Hello there.\nGoodbye."}"#.to_string());
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert_eq!(spoken(&shared), 0);
    let male = shared.speech.lock().unwrap().lines[0].plan.gestures.clone();
    send(r#"{"type":"body","body":{"female":1,"child":0}}"#.to_string());
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(spoken(&shared), NOT_SPEAKING, "the new voice has no score");
    assert_eq!(c.speech_cursor(), 1, "and the place is kept");
    assert_ne!(shared.speech.lock().unwrap().lines[0].plan.gestures, male, "a line is planned for its airway");
    // what the page reads
    let reply = bridge.request("/speech/1");
    assert_eq!(reply.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(v["generation"], shared.speech.lock().unwrap().lines_generation);
    assert!(v["plan"]["gestures"].as_array().is_some_and(|g| !g.is_empty()) && v["phrases"][0]["words"][0] == "goodbye", "{v}");
    assert_eq!(bridge.request("/speech/9").status, 404);
    assert_eq!(bridge.request("/speech/pad").status, 404, "no pad yet");
    send(r#"{"type":"sayIpa","ipa":"ˈɑtɑ"}"#.to_string());
    assert_eq!(bridge.request("/speech/pad").status, 200);
}

/// Saved projects from before Speak still load, and their Speak settings survive a round trip.
#[test]
fn settings_from_before_speak_still_load() {
    let old = r#"{"version":1,"lyrics":"la","body":{"female":1,"child":0},"mode":"play","artic":{},"tipFree":false,"voiced":true,"f0":200,"startSyllable":2}"#;
    let s: Settings = serde_json::from_str(old).unwrap();
    assert_eq!((s.lyrics.as_str(), s.mode, s.start_syllable), ("la", Mode::Play, 2));
    assert_eq!(s.speech, SpeechSettings::default());
    let edited = Settings { mode: Mode::Speak, speech: SpeechSettings { text: "Two\nlines".into(), ipa: true, rate: 1.5, start_line: 1 }, ..s };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(back, edited);
}

// ---------------------------------------------------------------- the live spectrum

/// The audio thread keeps its output for the editor only while someone is looking, never more than
/// what was reserved, and the reader gets the samples in order.
#[test]
fn the_output_reaches_the_editor_only_while_it_watches() {
    let shared = Shared::default();
    shared.restart(FS);
    let cap = shared.scope.lock().unwrap().capacity();
    assert!(cap as f64 > 192000.0 / 15.0, "room for a slow poll at any rate: {cap}");
    shared.feed_scope(&[1.0; 256]);
    assert_eq!(shared.scope.lock().unwrap().len(), 0, "nobody's looking");
    shared.watch();
    let block: Vec<f32> = (0..256).map(|i| i as f32).collect();
    shared.feed_scope(&block);
    assert_eq!(*shared.scope.lock().unwrap(), block);
    // a window that stopped reading: full, and no bigger
    for _ in 0..cap {
        shared.feed_scope(&block);
    }
    let scope = shared.scope.lock().unwrap();
    assert_eq!((scope.len(), scope.capacity()), (cap, cap));
}

// ---------------------------------------------------------------- Beatbox

fn beatboxing(pattern: &str, shared: &Shared) -> Core {
    // in the ready posture, as the mode command leaves it
    let artic = chimera::phonetics::beatbox::beat_ready(BodyMix::MALE);
    let settings = Settings { mode: Mode::Beatbox, artic, beat: BeatSettings { steps: parse_beat(pattern), mic: false, ..Default::default() }, ..Default::default() };
    core_with(&settings, shared)
}

/// The host, playing from `beats` for this many samples: each block's position and tempo.
struct Host {
    beats: f64,
    bpm: f64,
}

impl Host {
    fn block(&mut self, c: &mut Core, shared: &Shared, out: &mut [f32], events: &[Event]) {
        c.begin_block(&params(), Transport { playing: true, position: Some(self.beats), beats: Some(self.beats), tempo: Some(self.bpm), samples: out.len() }, shared);
        for e in events {
            c.event(*e);
        }
        c.render(out);
        c.end_block(out.len(), shared);
        self.beats += out.len() as f64 / FS * self.bpm / 60.0;
    }

    fn play(&mut self, c: &mut Core, shared: &Shared, seconds: f64) -> Vec<f32> {
        let mut out = vec![0.0f32; (seconds * FS) as usize];
        for block in out.chunks_mut(512) {
            self.block(c, shared, block, &[]);
        }
        out
    }
}

/// Where each burst of sound starts (s): louder than the threshold after a quiet stretch.
fn onsets(x: &[f32], threshold: f32) -> Vec<f64> {
    let mut found = Vec::new();
    let mut quiet = usize::MAX;
    for (i, s) in x.iter().enumerate() {
        if s.abs() >= threshold {
            if quiet > (0.05 * FS) as usize {
                found.push(i as f64 / FS);
            }
            quiet = 0;
        } else if quiet != usize::MAX {
            quiet += 1;
        }
    }
    found
}

#[test]
fn every_sound_fits_in_the_reported_latency() {
    for s in BEAT_SOUNDS.iter() {
        for body in [BodyMix::MALE, BodyMix::FEMALE, BodyMix::CHILD] {
            let lead = BeatPlayer::lead_in(s.id, body);
            assert!(lead <= BEAT_LATENCY, "{} needs {lead}", s.id);
        }
    }
}

#[test]
fn only_beatbox_has_latency() {
    let shared = Shared::default();
    let mut c = beatboxing("B . . .", &shared);
    assert_eq!(c.latency_samples(), (BEAT_LATENCY * FS).round() as u32);
    for (mode, expect) in [(Mode::Sing, 0), (Mode::Beatbox, (BEAT_LATENCY * FS).round() as u32), (Mode::Speak, 0), (Mode::Play, 0)] {
        shared.edit(|p| p.mode = Some(mode));
        run(&mut c, &shared, &[], 256, |_| 256);
        assert_eq!(c.latency_samples(), expect, "{mode:?}");
    }
}

/// The whole point: after the host takes off the reported latency, a kick on the beat is on the beat,
/// including the very first one.
#[test]
fn hits_land_on_the_grid_after_the_reported_latency() {
    let shared = Shared::default();
    let mut c = beatboxing("B . . . . . . .", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    let out = host.play(&mut c, &shared, 4.2);
    let threshold = peak(&out) * 0.25;
    let hits = onsets(&out, threshold);
    eprintln!("kicks at {hits:?} (peak {})", peak(&out));
    assert!(hits.len() >= 4, "{hits:?}");
    for (i, t) in hits.iter().enumerate() {
        // on the beat, every two beats (one second at 120 bpm), after the latency
        let grid = i as f64 + BEAT_LATENCY;
        assert!((t - grid).abs() < 0.02, "kick {i} at {t}, grid {grid}: {hits:?}");
    }
    // the first has had its full set-up, so it's a real kick. Only roughly: a kick's peak depends on
    // exactly when it falls (0.11 to 0.25 across delays, on the website's sequencer too)
    let level = |i: usize| {
        let at = (hits[i] * FS) as usize;
        peak(&out[at..at + (0.1 * FS) as usize])
    };
    assert!(level(0) > 0.35 * level(1), "first {}, second {}", level(0), level(1));
}

#[test]
fn a_loop_keeps_the_beat_going_and_stays_on_the_grid() {
    let shared = Shared::default();
    let mut c = beatboxing("B . . . . . . .", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    let mut all = Vec::new();
    // a one-bar loop, four times
    for _ in 0..4 {
        host.beats = 0.0;
        all.extend(host.play(&mut c, &shared, 2.0));
    }
    let hits = onsets(&all, peak(&all) * 0.25);
    eprintln!("kicks over a loop at {hits:?}");
    assert_eq!(hits.len(), 8, "{hits:?}");
    for (i, t) in hits.iter().enumerate() {
        assert!((t - (i as f64 + BEAT_LATENCY)).abs() < 0.02, "kick {i} at {t}: {hits:?}");
    }
}

#[test]
fn the_host_stopping_silences_what_was_on_its_way() {
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    host.play(&mut c, &shared, 0.5);
    assert!(c.engine().score().size() > 0);
    assert!(shared.beat_running.load(Ordering::Relaxed) && shared.beat_following.load(Ordering::Relaxed));
    c.begin_block(&params(), Transport::default(), &shared);
    c.render(&mut [0.0; 256]);
    c.end_block(256, &shared);
    assert_eq!(c.engine().score().size(), 0);
    assert!(!shared.beat_running.load(Ordering::Relaxed));
    assert_eq!(shared.beat_step.load(Ordering::Relaxed), -1);
}

#[test]
fn the_button_runs_the_beat_while_the_host_is_stopped_and_the_host_takes_over() {
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    let tempo = c.engine().beat().bpm;
    shared.edit(|p| p.beat_playing = Some(true));
    let out = run(&mut c, &shared, &[], (FS * 0.6) as usize, |_| 256);
    assert!(peak(&out) > 0.001 && shared.beat_running.load(Ordering::Relaxed) && !shared.beat_following.load(Ordering::Relaxed));
    let mut host = Host { beats: 0.0, bpm: 150.0 };
    host.play(&mut c, &shared, 0.1);
    assert!(shared.beat_following.load(Ordering::Relaxed), "the host takes over");
    assert!(!c.engine().beat().playing);
    assert_eq!(c.engine().beat().bpm, 150.0);
    // pressing the button meanwhile changes nothing
    shared.edit(|p| p.beat_playing = Some(true));
    host.play(&mut c, &shared, 0.1);
    assert!(!c.engine().beat().playing);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert_eq!(c.engine().beat().bpm, tempo, "the user's tempo is back once the host stops");
    // told not to follow, the host's playing doesn't start it
    shared.edit(|p| p.follow_host = Some(false));
    host.play(&mut c, &shared, 0.1);
    assert!(!shared.beat_running.load(Ordering::Relaxed), "not following");
    // a host that gives no position or tempo can't be followed
    shared.edit(|p| p.follow_host = Some(true));
    c.begin_block(&params(), Transport { playing: true, position: Some(0.0), beats: None, tempo: None, samples: 256 }, &shared);
    c.render(&mut [0.0; 256]);
    c.end_block(256, &shared);
    assert!(!shared.beat_following.load(Ordering::Relaxed));
}

#[test]
fn drum_notes_play_their_pads_on_the_general_midi_map() {
    assert_eq!(drum_pad(36), Some("B"));
    assert_eq!(drum_pad(38), Some("Pf"));
    assert_eq!(drum_pad(42), Some("t"));
    assert_eq!(drum_pad(46), Some("tss"));
    assert_eq!(drum_pad(49), Some("kch"));
    assert_eq!(drum_pad(34), None);
    // every tom, not just the low ones
    assert!([41, 43, 45, 47, 48, 50].iter().all(|m| drum_pad(*m) == Some("P!")));
    // everything named on the map is a sound that exists
    for m in 0..60 {
        if let Some(id) = drum_pad(m) {
            assert!(chimera::phonetics::beatbox::beat_sound(id).is_some(), "{m} -> {id}");
        }
    }
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    let size = |c: &Core| c.engine().score().size();
    run(&mut c, &shared, &[(0, on(30)), (10, on(54))], 256, |_| 256);
    assert_eq!(size(&c), 0, "notes off the map do nothing");
    run(&mut c, &shared, &[(0, on(36)), (10, off(36))], 256, |_| 256);
    assert!(size(&c) > 0, "a kick");
}

/// While the host plays, a note's hit goes the latency after it, to land on the grid; while it's
/// stopped, as soon as the mouth can make it, because someone is auditioning.
#[test]
fn a_note_waits_for_the_grid_only_while_the_host_plays() {
    let latest_start = |c: &Core| c.engine().score().gestures().iter().map(|g| g.t0 - g.ramp_in).fold(f64::INFINITY, f64::min) - c.engine().clock();
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    run(&mut c, &shared, &[(0, on(36))], 16, |_| 16);
    let stopped = latest_start(&c);
    assert!(stopped.abs() < 0.01, "stopped: starts now, {stopped}");
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    host.block(&mut c, &shared, &mut [0.0; 16], &[on(36)]);
    let playing = latest_start(&c);
    let lead = BeatPlayer::lead_in("B", BodyMix::MALE);
    assert!((playing - (BEAT_LATENCY - lead)).abs() < 0.01, "playing: its set-up starts {} early, got {playing}", BEAT_LATENCY - lead);
}

fn hum_pitches(c: &Core) -> Vec<f64> {
    use chimera::control::score::ControlKey;
    let clock = c.engine().clock();
    c.engine().score().gestures().iter().filter(|g| g.tag == Some(-1.0) && g.t1 > clock + 1.0).filter_map(|g| g.targets.get(ControlKey::F0)).collect()
}

/// Found by review: host note 60 hums C2 and on-screen 60 hums C4, and the shared press masks would
/// have treated them as one key.
#[test]
fn the_hosts_and_the_pages_keys_hum_separately() {
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    let c2 = chimera::math::midi_to_hz(36.0);
    let c4 = chimera::math::midi_to_hz(60.0);
    let close = |got: Vec<f64>, want: f64, what: &str| assert!(got.len() == 1 && (got[0] - want).abs() < 1e-6, "{what}: {got:?}, want {want}");
    run(&mut c, &shared, &[(0, on(60))], 16, |_| 16);
    close(hum_pitches(&c), c2, "host C4 hums two octaves down");
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 16, |_| 16);
    close(hum_pitches(&c), c4, "the page's C4 isn't suppressed by it");
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: false });
    run(&mut c, &shared, &[], 16, |_| 16);
    close(hum_pitches(&c), c2, "and back to the host's, still held");
    run(&mut c, &shared, &[(0, off(60))], 16, |_| 16);
    assert!(hum_pitches(&c).is_empty(), "nothing left humming");
    // the other way round, and a window closing with a key down
    shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
    run(&mut c, &shared, &[], 16, |_| 16);
    close(hum_pitches(&c), c4, "the page's");
    run(&mut c, &shared, &[(0, on(60))], 16, |_| 16);
    close(hum_pitches(&c), c2, "the host's isn't suppressed either");
    run(&mut c, &shared, &[(0, off(60))], 16, |_| 16);
    close(hum_pitches(&c), c4, "back to the page's");
    shared.clear_ui_keys();
    run(&mut c, &shared, &[], 16, |_| 16);
    assert!(hum_pitches(&c).is_empty(), "the window closed");
    // a hum is heard, and ends when let go
    let out = run(&mut c, &shared, &[(0, on(72)), ((FS * 0.8) as usize, off(72))], (FS * 1.6) as usize, |_| 256);
    let window = |a: f64, b: f64| peak(&out[(a * FS) as usize..(b * FS) as usize]);
    assert!(window(0.4, 0.7) > 0.001, "humming: {}", window(0.4, 0.7));
    assert!(window(1.3, 1.6) < 0.2 * window(0.4, 0.7), "let go: {} after {}", window(1.3, 1.6), window(0.4, 0.7));
}

#[test]
fn leaving_beatbox_stops_the_beat_and_the_close_mic() {
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    shared.edit(|p| p.mic = Some(true));
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    let loud = host.play(&mut c, &shared, 0.6);
    assert!(peak(&loud) > 0.0);
    shared.edit(|p| p.mode = Some(Mode::Play));
    host.play(&mut c, &shared, 0.05);
    assert!(!shared.beat_running.load(Ordering::Relaxed));
    assert_eq!(shared.beat_step.load(Ordering::Relaxed), -1);
    assert_eq!(c.engine().score().size(), 0);
}

/// The chain's response and its stability, where a filter would show trouble.
#[test]
fn the_close_mic_lifts_the_low_end_tames_peaks_and_stays_stable() {
    use chimera_plugin::close_mic::CloseMic;
    for fs in [44100.0, 48000.0, 96000.0, 192000.0] {
        let tone = |hz: f64, amp: f32| -> f32 {
            let mut mic = CloseMic::new(fs);
            let mut x: Vec<f32> = (0..(fs * 0.5) as usize).map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / fs).sin() as f32).collect();
            mic.process(&mut x);
            assert!(x.iter().all(|v| v.is_finite()));
            peak(&x[x.len() / 2..])
        };
        // quiet enough to pass the compressor: the shelf gives +9 dB at the bottom, nothing mid-band
        let (low, mid) = (tone(50.0, 0.005), tone(1000.0, 0.005));
        let db = 20.0 * (low / mid).log10();
        assert!((6.0..10.0).contains(&db), "{fs}: low end {db} dB over mid");
        // a loud sound is turned down far more than a quiet one is
        let quiet_gain = tone(1000.0, 0.005) / 0.005;
        let loud_gain = tone(1000.0, 0.8) / 0.8;
        assert!(loud_gain < quiet_gain * 0.25, "{fs}: gain {loud_gain} against {quiet_gain}");
        assert!(tone(1000.0, 1.0) < 2.0, "{fs}: a full-scale tone stays in range");
    }
}

/// Saved projects from before Beatbox still load; a beat survives a round trip.
#[test]
fn a_beat_is_saved_with_the_project() {
    let old = r#"{"version":1,"lyrics":"la","mode":"speak"}"#;
    let s: Settings = serde_json::from_str(old).unwrap();
    assert_eq!(s.beat, BeatSettings::default());
    assert!(s.beat.mic && s.beat.follow && !s.beat.steps.is_empty());
    let edited = Settings { mode: Mode::Beatbox, beat: BeatSettings { steps: parse_beat("B . m:43 ."), bpm: 77.0, swing: 0.3, preset: None, mic: false, follow: false }, ..s };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(back, edited);
}

/// The mode and the posture are one command, so a stale one changes neither.
#[test]
fn entering_beatbox_sets_its_posture_in_the_same_command() {
    use chimera::phonetics::beatbox::beat_ready;
    use chimera_plugin::bridge::{Bridge, ParamHost};
    use chimera_plugin::ChimeraParams;
    use std::sync::Arc;
    struct Host;
    impl ParamHost for Host {
        fn begin(&self, _: &str) {}
        fn set(&self, _: &str, _: f32) {}
        fn end(&self, _: &str) {}
        fn values(&self) -> Vec<(String, f32)> {
            Vec::new()
        }
    }
    let params = Arc::new(ChimeraParams::default());
    let shared = Arc::new(Shared::default());
    let bridge = Bridge::new(params.clone(), shared.clone(), Box::new(Host));
    let mut c = chimera_plugin::rebuild(&params, &shared, params.voice(), FS).unwrap();
    let rev = shared.revision.load(Ordering::Relaxed);
    let jaw = |s: &Settings| s.artic.get(ArticKey::Jaw);
    let ready = beat_ready(BodyMix::MALE).get(ArticKey::Jaw);
    assert_ne!(jaw(&params.settings.read().unwrap()), ready);
    // from before a restore: neither happens
    shared.restart(FS);
    bridge.command(&format!(r#"{{"rev":{rev},"type":"mode","mode":"beatbox"}}"#));
    assert_eq!(params.settings.read().unwrap().mode, Mode::Sing);
    assert_ne!(jaw(&params.settings.read().unwrap()), ready);
    // current: both, and the audio thread gets both at once
    let rev = shared.revision.load(Ordering::Relaxed);
    bridge.command(&format!(r#"{{"rev":{rev},"type":"mode","mode":"beatbox"}}"#));
    assert_eq!(params.settings.read().unwrap().mode, Mode::Beatbox);
    assert_eq!(jaw(&params.settings.read().unwrap()), ready);
    c.begin_block(&params.voice(), Transport::default(), &shared);
    assert_eq!(c.latency_samples(), (BEAT_LATENCY * FS).round() as u32);
    assert_eq!(c.engine().voice().artic.target.jaw, ready.unwrap());
    // the beat's commands reach the settings and the sequencer
    bridge.command(&format!(r#"{{"rev":{rev},"type":"beat","steps":[{{"id":"B"}},null,{{"id":"m","note":43}}],"preset":"Mine","bpm":99,"swing":0.2}}"#));
    bridge.command(&format!(r#"{{"rev":{rev},"type":"mic","on":false}}"#));
    bridge.command(&format!(r#"{{"rev":{rev},"type":"followHost","on":false}}"#));
    let beat = params.settings.read().unwrap().beat.clone();
    assert_eq!((beat.steps.len(), beat.preset.as_deref(), beat.bpm, beat.swing, beat.mic, beat.follow), (3, Some("Mine"), 99.0, 0.2, false, false));
    c.begin_block(&params.voice(), Transport::default(), &shared);
    assert_eq!((c.engine().beat().steps().len(), c.engine().beat().bpm, c.engine().beat().swing), (3, 99.0, 0.2));
}

// Found by code review: transitions in Beatbox.

#[test]
fn leaving_beatbox_takes_a_pads_hit_with_it() {
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    // stopped, with no sequencer running to cancel it
    run(&mut c, &shared, &[(0, on(36))], 16, |_| 16);
    assert!(c.engine().score().size() > 0, "a kick on its way");
    shared.edit(|p| p.mode = Some(Mode::Play));
    run(&mut c, &shared, &[], 16, |_| 16);
    assert_eq!(c.engine().score().size(), 0, "it mustn't sound in the next mode");
}

#[test]
fn stopping_the_beat_keeps_a_held_hum() {
    let c2 = chimera::math::midi_to_hz(36.0);
    let hums_c2 = |c: &Core| {
        let got = hum_pitches(c);
        got.len() == 1 && (got[0] - c2).abs() < 1e-6
    };
    // turning Follow off while the host plays
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    host.block(&mut c, &shared, &mut [0.0; 256], &[on(60)]);
    assert!(hums_c2(&c), "{:?}", hum_pitches(&c));
    shared.edit(|p| p.follow_host = Some(false));
    host.block(&mut c, &shared, &mut [0.0; 256], &[]);
    assert!(hums_c2(&c), "still humming: {:?}", hum_pitches(&c));
    // the Stop button, with the host stopped
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    shared.edit(|p| p.beat_playing = Some(true));
    run(&mut c, &shared, &[(0, on(60))], 256, |_| 256);
    assert!(c.engine().beat().playing && hums_c2(&c));
    shared.edit(|p| p.beat_playing = Some(false));
    run(&mut c, &shared, &[], 256, |_| 256);
    assert!(!c.engine().beat().playing);
    assert!(hums_c2(&c), "still humming: {:?}", hum_pitches(&c));
    // but the host stopping lifts every key, as in every mode
    let shared = Shared::default();
    let mut c = beatboxing("B t Pf t", &shared);
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    host.block(&mut c, &shared, &mut [0.0; 256], &[on(60)]);
    run(&mut c, &shared, &[], 256, |_| 256);
    assert!(hum_pitches(&c).is_empty());
}

#[test]
fn a_pad_pressed_just_before_the_host_starts_is_for_its_timeline() {
    let shared = Shared::default();
    let mut c = beatboxing(". . . .", &shared);
    run(&mut c, &shared, &[], 16, |_| 16);
    shared.ui_hit(BeatStep::new("B"));
    let mut host = Host { beats: 0.0, bpm: 120.0 };
    host.block(&mut c, &shared, &mut [0.0; 16], &[]);
    let start = c.engine().score().gestures().iter().map(|g| g.t0 - g.ramp_in).fold(f64::INFINITY, f64::min) - c.engine().clock();
    let lead = BeatPlayer::lead_in("B", BodyMix::MALE);
    assert!((start - (BEAT_LATENCY - lead)).abs() < 0.01, "it lands on the grid: its set-up starts {} from now, got {start}", BEAT_LATENCY - lead);
}

/// A host answers a latency change by restarting the plugin: nothing but the page's edits is lost.
#[test]
fn a_restart_in_beatbox_comes_back_the_same() {
    use chimera_plugin::ChimeraParams;
    let params = ChimeraParams::default();
    let shared = Shared::default();
    params.settings.write().unwrap().mode = Mode::Beatbox;
    params.settings.write().unwrap().beat = BeatSettings { steps: parse_beat("B . m:43 ."), bpm: 77.0, swing: 0.3, preset: None, mic: false, follow: true };
    let rev = shared.revision.load(Ordering::Relaxed);
    let c = chimera_plugin::rebuild(&params, &shared, params.voice(), FS).unwrap();
    assert_ne!(shared.revision.load(Ordering::Relaxed), rev, "the page will read the state again");
    assert_eq!(c.latency_samples(), (BEAT_LATENCY * FS).round() as u32);
    assert_eq!((c.engine().beat().steps().len(), c.engine().beat().bpm, c.engine().beat().swing), (4, 77.0, 0.3));
    assert!(!c.engine().beat().playing && !c.engine().beat().following());
}
