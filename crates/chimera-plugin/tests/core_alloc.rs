//! The audio thread's whole path must not allocate or free: notes, every command from the editor, a
//! body swap, new lyrics, telemetry, reset.
use chimera::anatomy::human::{ArticKey, ArticPatch, BodyMix};
use chimera::engine::Engine;
use chimera::phonetics::beatbox::{BeatStep, parse_beat};
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera::voice::human_voice::SourcePatch;
use chimera_plugin::core::{Core, Event, Mode, Prepared, Settings, Shared, Transport, UiNote, VoiceParams};
use chimera_plugin::g2p;
use chimera_plugin::speech::{SpeechSettings, plan_lines, plan_pad};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static COUNT: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        COUNT.with(|c| c.set((c.get().0 + 1, c.get().1)));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        COUNT.with(|c| c.set((c.get().0, c.get().1 + 1)));
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        COUNT.with(|c| c.set((c.get().0 + 1, c.get().1)));
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn params() -> VoiceParams {
    VoiceParams { vibrato: 35.0, vibrato_rate: 6.0, breath: 8.0, pressed: 0.5, bright: 0.25, breathy: 0.15, rough: 0.1, fry: 0.0, glide: 0.06, ring: 0.0, restart: true }
}

#[test]
fn the_audio_threads_path_does_not_allocate() {
    let shared = Shared::default();
    shared.restart(44100.0);
    let settings = Settings::default();
    // the largest airway is the default male's; start small to check the swap up to it
    let settings = Settings { body: BodyMix::CHILD, ..settings };
    let prepared = Prepared::of(&settings, &shared);
    let mut c = Core::new(44100.0, &settings, &prepared, params(), &shared);
    let mut buf = vec![0.0f32; 512];
    let block = |c: &mut Core, p: &VoiceParams, t: Transport, buf: &mut [f32], evs: &[Event]| {
        c.begin_block(p, t, &shared);
        for e in evs {
            c.event(*e);
        }
        c.render(buf);
        shared.watch();
        c.end_block(buf.len(), &shared);
        shared.feed_scope(buf);
    };
    let t = Transport { playing: true, position: Some(0.0), ..Default::default() };

    // what the main thread prepares (it may allocate) between the blocks that take it
    let voices = [BodyMix::MALE, BodyMix::FEMALE, BodyMix::new(0.5, 0.2)].map(|b| (Engine::prepare_voice(44100.0, b), b));
    let lyrics: Vec<_> = ["Strengths high - - hour", "sixths"].iter().map(|l| lyrics_to_syllables(l, g2p())).collect();
    let mut voices = voices.into_iter();
    let mut lyrics = lyrics.into_iter();
    // a long line: it has to fit what the engine reserved
    let paragraph = "The quick brown fox jumps over the lazy dog, and the dog, who had seen it all before, did not look up. ".repeat(20);
    let speeches: Vec<_> = ["One.\nTwo.", &paragraph].iter().map(|t| plan_lines(&SpeechSettings { text: t.to_string(), ..Default::default() }, BodyMix::MALE, g2p())).collect();
    assert!(speeches[1][0].plan.gestures.len() > 1000, "{} gestures", speeches[1][0].plan.gestures.len());
    let mut speeches = speeches.into_iter();
    let pads: Vec<_> = (0..5).map(|_| plan_pad("ˈɑpɑ", BodyMix::MALE, 1.0)).collect();
    let mut pads = pads.into_iter();
    let patterns: Vec<_> = ["B t Pf t", "B . . . m:43 . . . tss K ^K ! P! kch"].iter().map(|t| parse_beat(t)).collect();
    let mut patterns = patterns.into_iter();

    let mut total = (0usize, 0usize);
    let mut measured = |c: &mut Core, p: &VoiceParams, t: Transport, buf: &mut [f32], evs: &[Event]| {
        let before = COUNT.with(|c| c.get());
        block(c, p, t, buf, evs);
        let after = COUNT.with(|c| c.get());
        total.0 += after.0 - before.0;
        total.1 += after.1 - before.1;
    };

    // the very first block after `initialize` counts too
    measured(&mut c, &params(), t, &mut buf, &[]);
    for round in 0..5u8 {
        // a new body: the telemetry grows to fit it
        if let Some((v, b)) = voices.next() {
            shared.send_voice(v, b);
        }
        measured(&mut c, &params(), t, &mut buf, &[]);
        // new lyrics, handed over by the same lock
        if let Some(l) = lyrics.next() {
            shared.send_lyrics(l);
        }
        measured(&mut c, &params(), t, &mut buf, &[]);
        for k in 0..8u8 {
            let m = 50 + (k * 3 + round) % 20;
            measured(&mut c, &params(), t, &mut buf, &[Event::NoteOn { midi: m, velocity: 100.0 }, Event::Bend(0.6)]);
            measured(&mut c, &params(), t, &mut buf, &[]);
            measured(&mut c, &params(), t, &mut buf, &[Event::NoteOn { midi: m + 2, velocity: 60.0 }]);
            measured(&mut c, &params(), t, &mut buf, &[Event::NoteOff { midi: m }, Event::NoteOff { midi: m + 2 }]);
        }
        // the editor's commands
        shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: true });
        shared.edit(|p| {
            p.source = SourcePatch { voiced: Some(round % 2 == 0), f0: Some(200.0), ..Default::default() };
            p.artic = ArticPatch::default().with(ArticKey::Jaw, 1.0);
            p.tip_free = Some(true);
            p.cursor = Some(1);
        });
        measured(&mut c, &params(), t, &mut buf, &[]);
        shared.ui_note(UiNote { midi: 60, velocity: 96.0, on: false });
        // a parameter moved
        let moved = VoiceParams { breathy: 0.1 * round as f32, ring: 0.1 * round as f32, ..params() };
        measured(&mut c, &moved, t, &mut buf, &[]);
        // Play mode and back
        shared.edit(|p| {
            p.mode = Some(Mode::Play);
            p.hold = Some(true);
        });
        measured(&mut c, &params(), t, &mut buf, &[Event::NoteOn { midi: 57, velocity: 96.0 }]);
        measured(&mut c, &params(), t, &mut buf, &[Event::NoteOff { midi: 57 }]);
        // Speak: a note, the buttons, new plans, the chart, a long line
        shared.edit(|p| p.mode = Some(Mode::Speak));
        measured(&mut c, &params(), t, &mut buf, &[Event::NoteOn { midi: 64, velocity: 96.0 }]);
        measured(&mut c, &params(), t, &mut buf, &[Event::NoteOff { midi: 64 }, Event::NoteOn { midi: 40, velocity: 96.0 }, Event::Bend(0.3)]);
        shared.edit(|p| {
            p.speak_next = true;
            p.speech_cursor = Some(0);
        });
        measured(&mut c, &params(), t, &mut buf, &[]);
        shared.edit(|p| p.stop_speech = true);
        measured(&mut c, &params(), t, &mut buf, &[]);
        if let Some(lines) = speeches.next() {
            shared.send_speech(lines, true);
        }
        measured(&mut c, &params(), t, &mut buf, &[Event::NoteOn { midi: 60, velocity: 96.0 }]);
        measured(&mut c, &params(), t, &mut buf, &[]);
        if let Some(pad) = pads.next() {
            shared.send_pad(pad);
        }
        measured(&mut c, &params(), t, &mut buf, &[]);
        // Beatbox, following the host: steps, pads, both owners' keys, the close mic, a new pattern
        shared.edit(|p| p.mode = Some(Mode::Beatbox));
        let at = |beats: f64| Transport { playing: true, position: Some(beats), beats: Some(beats), tempo: Some(140.0), samples: 512 };
        measured(&mut c, &params(), at(0.0), &mut buf, &[Event::NoteOn { midi: 36, velocity: 110.0 }]);
        measured(&mut c, &params(), at(0.03), &mut buf, &[Event::NoteOn { midi: 64, velocity: 96.0 }, Event::NoteOn { midi: 38, velocity: 90.0 }]);
        shared.ui_note(UiNote { midi: 64, velocity: 96.0, on: true });
        shared.ui_hit(BeatStep::new("K"));
        if let Some(steps) = patterns.next() {
            shared.send_beat_steps(steps);
        }
        measured(&mut c, &params(), at(0.06), &mut buf, &[]);
        shared.ui_note(UiNote { midi: 64, velocity: 96.0, on: false });
        shared.edit(|p| {
            p.mic = Some(round % 2 == 0);
            p.beat_bpm = Some(100.0);
            p.beat_swing = Some(0.2);
        });
        // steps for a while, then a loop back, and a host stop
        let mut beats = 0.09;
        for _ in 0..40 {
            measured(&mut c, &params(), at(beats), &mut buf, &[]);
            beats += 0.027;
        }
        measured(&mut c, &params(), at(0.0), &mut buf, &[Event::NoteOff { midi: 64 }]);
        measured(&mut c, &params(), Transport::default(), &mut buf, &[Event::NoteOn { midi: 64, velocity: 96.0 }, Event::NoteOff { midi: 64 }]);
        // and running by itself while the host is stopped
        shared.edit(|p| p.beat_playing = Some(true));
        for _ in 0..10 {
            measured(&mut c, &params(), Transport::default(), &mut buf, &[Event::NoteOn { midi: 42, velocity: 96.0 }]);
        }
        shared.edit(|p| p.mode = Some(Mode::Sing));
        measured(&mut c, &params(), t, &mut buf, &[]);
        // the transport: a stop, a start, a loop
        measured(&mut c, &params(), Transport { playing: false, position: Some(1.0), ..Default::default() }, &mut buf, &[]);
        measured(&mut c, &params(), t, &mut buf, &[]);
        c.reset();
    }
    assert_eq!(total, (0, 0), "(allocations, frees) on the audio thread's path");
}
