//! The live singer runs on a plugin's audio thread: once warm, notes, legato, melismas, glides and
//! releases must not allocate or free, for any body (a body between presets blends its targets).
use chimera::anatomy::human::BodyMix;
use chimera::control::singer::{Singer, syllables_from};
use chimera::engine::Engine;
use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::sing::{NoteOnOptions, plan_note_on};
use chimera::phonetics::syllables::lyrics_to_syllables;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static FREES: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREES.with(|n| n.set(n.get() + 1));
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn counts() -> (usize, usize) {
    (ALLOCS.with(|n| n.get()), FREES.with(|n| n.get()))
}

/// A long run of what a performer does, on every kind of syllable.
fn perform(s: &mut Singer, e: &mut Engine, buf: &mut [f32]) {
    for round in 0..4u8 {
        for k in 0..12u8 {
            let m = 48 + (k * 5 + round) % 24;
            s.note_on(e, m, 40.0 + k as f64 * 7.0);
            e.render(buf);
            match k % 4 {
                // legato into the next note, then release the older one: a glide
                0 => {
                    s.note_on(e, m + 3, 96.0);
                    e.render(buf);
                    s.note_off(e, m);
                    e.render(buf);
                    s.note_off(e, m + 3);
                }
                // the older key comes back when the newer one lifts
                1 => {
                    s.note_on(e, m + 7, 110.0);
                    e.render(buf);
                    s.note_off(e, m + 7);
                    e.render(buf);
                    s.note_off(e, m);
                }
                _ => s.note_off(e, m),
            }
            e.render(buf);
        }
        s.stop_all(e);
    }
}

#[test]
fn the_singer_does_not_allocate() {
    // clusters, diphthongs, a rhotic, an affricate, codas, melismas, a phrase end
    let lyrics = "Strengths high - - hour church, thy joy air - sixths bright. Twelfths around";
    let syllables = syllables_from(&lyrics_to_syllables(lyrics, &EnglishG2P::new()));
    assert!(syllables.iter().any(|s| s.melisma), "the lyrics must exercise melismas");
    assert!(syllables.iter().any(|s| !s.coda.is_empty()), "the lyrics must exercise codas");
    for body in [BodyMix::MALE, BodyMix::CHILD, BodyMix::new(0.4, 0.3)] {
        let mut e = Engine::new(44100.0);
        e.set_body(body);
        let mut s = Singer::new();
        s.body = body;
        let _none = s.set_syllables(syllables.clone());
        let mut buf = vec![0.0f32; 512];
        e.render(&mut buf); // warm
        let before = counts();
        perform(&mut s, &mut e, &mut buf);
        let after = counts();
        assert_eq!(after, before, "(allocations, frees) while singing with body {body:?}");
    }
}

/// The counter works: the same planning through the browser's `Vec` API is seen allocating.
#[test]
fn the_counter_sees_allocations() {
    let syl = lyrics_to_syllables("strengths", &EnglishG2P::new()).remove(0);
    let before = counts();
    let gs = plan_note_on(&syl, 57.0, &NoteOnOptions { body: BodyMix::MALE, vibrato: 30.0, tag: 2.0, legato_from: None });
    assert!(!gs.is_empty());
    assert!(counts().0 > before.0);
}

#[test]
fn notes_sound() {
    let mut e = Engine::new(44100.0);
    let mut s = Singer::new();
    let _ = s.set_syllables(syllables_from(&lyrics_to_syllables("hello there", &EnglishG2P::new())));
    let mut buf = vec![0.0f32; 22050];
    s.note_on(&mut e, 57, 96.0);
    e.render(&mut buf);
    let peak = buf.iter().fold(0.0f32, |p, x| p.max(x.abs()));
    assert!(peak > 0.01, "silent: peak {peak}");
    assert_eq!((s.sung(), s.cursor(), s.sounding_note()), (Some(0), 1, Some(57)));
    s.note_off(&mut e, 57);
    assert_eq!((s.sung(), s.sounding_note()), (None, None));
    // nothing sounds a second after the coda
    for _ in 0..2 {
        e.render(&mut buf);
    }
    let peak = buf.iter().fold(0.0f32, |p, x| p.max(x.abs()));
    assert!(peak < 1e-3, "still sounding: peak {peak}");
}
