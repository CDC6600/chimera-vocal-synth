//! The instrument without a host: what it sings, and how it behaves on the audio thread.
use chimera::anatomy::human::{ArticKey, ArticPatch, BodyMix};
use chimera::engine::Engine;
use chimera::phonetics::Syllable;
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera_plugin::core::{Core, DEFAULT_LYRICS, Event, Mode, Settings, Shared, Transport, UiNote, VoiceParams};
use chimera_plugin::g2p;
use std::sync::atomic::Ordering;

const FS: f64 = 44100.0;

fn params() -> VoiceParams {
    VoiceParams { vibrato: 35.0, vibrato_rate: 6.0, breath: 8.0, pressed: 0.5, bright: 0.25, breathy: 0.15, rough: 0.1, fry: 0.0, glide: 0.06, ring: 0.0, restart: true }
}

fn syllables(text: &str) -> Vec<Syllable> {
    lyrics_to_syllables(text, g2p())
}

fn core_with(settings: &Settings, shared: &Shared) -> Core {
    shared.restart(FS);
    let c = Core::new(FS, settings, &syllables(&settings.lyrics), params(), shared);
    *shared.syllables.lock().unwrap() = syllables(&settings.lyrics);
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
        c.begin_block(&params(), Transport { playing, position: Some(position) }, &shared);
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
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0) }, &shared);
    c.event(on(60));
    c.event(off(60));
    c.render(&mut block);
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0) }, &shared);
    c.render(&mut block);
    assert_eq!(c.singer().cursor(), 1);
    c.begin_block(&no_restart, Transport { playing: false, position: Some(0.0) }, &shared);
    c.begin_block(&no_restart, Transport { playing: true, position: Some(0.0) }, &shared);
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
