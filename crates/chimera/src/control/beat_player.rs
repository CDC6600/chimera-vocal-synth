//! Step sequencer for the beatboxer. Runs on the voice clock (in the AudioWorklet, or offline),
//! so hits are sample-accurate whatever the main thread is doing. Each step's gestures are built
//! when its hit comes within `horizon` of the clock: early enough for the slowest sound's set-up,
//! late enough that edits to the pattern are heard within a step or two.
//!
//! Real time: [`BeatPlayer::new`] reserves everything the player uses; after that `tick`,
//! `set_steps`, `set_tempo`, `start`/`stop`, `rebase`/`rewind`, `step_at` and `gestures` (into
//! a `Vec` with room for [`MAX_SOUND_GESTURES`]) allocate nothing. Call
//! [`crate::phonetics::beatbox::init`] (or construct the engine) before the first tick so the
//! phoneme tables are not parsed on the audio thread.

use super::score::Gesture;
use crate::anatomy::human::BodyMix;
use crate::math::{js_max, js_min, midi_to_hz};
use crate::phonetics::beatbox::{BASS_NOTE, BeatBuild, BeatStep, MAX_SOUND_GESTURES, beat_sound};

/// scheduled steps remembered for the playhead (well over a horizon's worth)
const MARKS: usize = 32;

#[derive(Clone, Debug)]
pub struct BeatPlayer {
    steps: Vec<Option<BeatStep>>,
    pub bpm: f64,
    /// 0..0.6: off-beat sixteenths are delayed by swing × half a step
    pub swing: f64,
    pub playing: bool,
    pub body: BodyMix,
    anchor_time: f64,
    anchor_step: i64,
    next: i64,
    /// step number and hit time of recently scheduled steps (−1 = empty slot)
    mark_n: [i64; MARKS],
    mark_t: [f64; MARKS],
    mark_pos: usize,
    /// one step's gestures, reused
    buf: Vec<Gesture>,
    /// following a host's transport: where its last block ended (beats)
    follow_end: Option<f64>,
}

impl Default for BeatPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl BeatPlayer {
    pub const STEPS_PER_BEAT: f64 = 4.0;
    /// schedule a step when its hit is this close (s); covers every sound's set-up time
    pub const HORIZON: f64 = 0.2;
    /// the longest pattern kept (longer ones are cut, as the worklet did)
    pub const MAX_STEPS: usize = 128;
    /// [`Self::follow`]: a position this many steps (or two blocks, if more) from where the last
    /// block ended is a jump; anything less is a host's unevenness
    const JUMP_STEPS: f64 = 0.25;
    /// [`Self::follow`]: positions this close (beats) are the same point, whatever rounding says
    const SAME_BEAT: f64 = 1e-9;

    pub fn new() -> Self {
        BeatPlayer {
            steps: Vec::with_capacity(Self::MAX_STEPS),
            bpm: 96.0,
            swing: 0.0,
            playing: false,
            body: BodyMix::MALE,
            anchor_time: 0.0,
            anchor_step: 0,
            next: 0,
            mark_n: [-1; MARKS],
            mark_t: [0.0; MARKS],
            mark_pos: 0,
            buf: Vec::with_capacity(MAX_SOUND_GESTURES),
            follow_end: None,
        }
    }

    pub fn steps(&self) -> &[Option<BeatStep>] {
        &self.steps
    }

    /// Replace the pattern (the first [`Self::MAX_STEPS`] steps). No allocation.
    pub fn set_steps(&mut self, steps: &[Option<BeatStep>]) {
        self.steps.clear();
        self.steps.extend_from_slice(&steps[..steps.len().min(Self::MAX_STEPS)]);
    }

    pub fn step_dur(&self) -> f64 {
        60.0 / self.bpm / Self::STEPS_PER_BEAT
    }

    fn step_time(&self, n: i64) -> f64 {
        let base = self.anchor_time + (n - self.anchor_step) as f64 * self.step_dur();
        if n % 2 == 1 { base + self.swing * 0.5 * self.step_dur() } else { base }
    }

    pub fn start(&mut self, clock: f64) {
        self.playing = true;
        self.anchor_time = clock + Self::HORIZON;
        self.anchor_step = 0;
        self.next = 0;
        self.mark_n = [-1; MARKS];
    }

    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Change tempo without a jump: the next unscheduled step keeps its time.
    pub fn set_tempo(&mut self, bpm: f64, swing: f64) {
        let b = js_min(200.0, js_max(40.0, bpm));
        if self.playing && b != self.bpm {
            self.anchor_time += (self.next - self.anchor_step) as f64 * self.step_dur();
            self.anchor_step = self.next;
        }
        self.bpm = b;
        self.swing = js_min(0.6, js_max(0.0, swing));
    }

    /// The voice clock restarted (new voice after a body change): keep the beat's phase.
    pub fn rebase(&mut self, delta: f64) {
        self.anchor_time += delta;
        for t in self.mark_t.iter_mut() {
            *t += delta;
        }
    }

    /// The score holding the scheduled hits was replaced (new voice): schedule again every step
    /// not yet heard at `clock`, at the times they were given.
    pub fn rewind(&mut self, clock: f64) {
        let (mut first, mut first_t) = (-1i64, 0.0);
        for i in 0..MARKS {
            let n = self.mark_n[i];
            if n >= 0 && self.mark_t[i] >= clock && (first < 0 || n < first) {
                first = n;
                first_t = self.mark_t[i];
            }
        }
        if first < 0 {
            return;
        }
        self.anchor_step = first;
        self.anchor_time = first_t - if first % 2 == 1 { self.swing * 0.5 * self.step_dur() } else { 0.0 };
        self.next = first;
        for n in self.mark_n.iter_mut() {
            if *n >= first {
                *n = -1;
            }
        }
    }

    /// Step under the playhead at `clock`: the last scheduled step whose hit has come (−1: none).
    pub fn step_at(&self, clock: f64) -> i64 {
        if (!self.playing && self.follow_end.is_none()) || self.steps.is_empty() {
            return -1;
        }
        let (mut best, mut best_t) = (-1i64, f64::NEG_INFINITY);
        for i in 0..MARKS {
            let t = self.mark_t[i];
            if self.mark_n[i] >= 0 && t <= clock && t > best_t {
                best = self.mark_n[i];
                best_t = t;
            }
        }
        if best < 0 { -1 } else { best % self.steps.len() as i64 }
    }

    /// Schedule every step whose hit falls within the horizon: `schedule` receives each step's
    /// gestures (on the voice clock), in step order. No allocation.
    pub fn tick(&mut self, clock: f64, schedule: &mut impl FnMut(&[Gesture])) {
        if !self.playing || self.steps.is_empty() {
            return;
        }
        let mut buf = std::mem::take(&mut self.buf);
        for _guard in 0..64 {
            let hit = self.step_time(self.next);
            if hit > clock + Self::HORIZON {
                break;
            }
            self.mark(self.next, hit);
            if hit >= clock && self.step_gestures(self.next, hit, &mut buf) {
                schedule(&buf);
            }
            self.next += 1;
        }
        self.buf = buf;
    }

    fn mark(&mut self, n: i64, hit: f64) {
        self.mark_n[self.mark_pos] = n;
        self.mark_t[self.mark_pos] = hit;
        self.mark_pos = (self.mark_pos + 1) % MARKS;
    }

    /// Step `n`'s gestures for a hit at `hit`, into `buf`. False for a rest.
    fn step_gestures(&self, n: i64, hit: f64, buf: &mut Vec<Gesture>) -> bool {
        let l = self.steps.len() as i64;
        let Some(cell) = self.steps[(n % l) as usize] else { return false };
        // sustained sounds (hum, open hat) last until the next sound
        let mut rests = 0i64;
        while rests < l - 1 && self.steps[((n + 1 + rests) % l) as usize].is_none() {
            rests += 1;
        }
        buf.clear();
        self.gestures(&cell, hit, (1 + rests) as f64 * self.step_dur(), buf);
        true
    }

    /// Where step `n` falls on a host's timeline (beats), with swing on the odd steps.
    fn step_beat(&self, n: i64) -> f64 {
        let base = n as f64 / Self::STEPS_PER_BEAT;
        if n % 2 == 1 { base + self.swing * 0.5 / Self::STEPS_PER_BEAT } else { base }
    }

    /// Follow a host's transport instead of the voice clock: schedule the steps the position reaches
    /// in this block, which runs from `beats` to `beats_end` and starts at `clock`. Each hit lands
    /// `delay` seconds after its point on the timeline, which leaves its set-up room (the plugin
    /// reports that delay as latency). No allocation.
    ///
    /// A step's number only moves forward, so a host that repeats a position or reports an uneven
    /// one can't double a hit; only a real jump looks it up again. Steps wait for beat 0 during a
    /// pre-roll. See docs/plugin.md, "Beatbox".
    pub fn follow(&mut self, clock: f64, beats: f64, beats_end: f64, bpm: f64, delay: f64, schedule: &mut impl FnMut(&[Gesture])) {
        self.bpm = bpm;
        let jumped = match self.follow_end {
            None => true,
            Some(end) => (beats - end).abs() > js_max(2.0 * (beats_end - beats), Self::JUMP_STEPS / Self::STEPS_PER_BEAT),
        };
        if jumped {
            let mut n = js_max(0.0, (beats * Self::STEPS_PER_BEAT).floor() - 1.0) as i64;
            while self.step_beat(n) < beats - Self::SAME_BEAT {
                n += 1;
            }
            self.next = n;
        }
        self.follow_end = Some(beats_end);
        if self.steps.is_empty() {
            return;
        }
        let seconds_per_beat = 60.0 / bpm;
        let mut buf = std::mem::take(&mut self.buf);
        for _guard in 0..64 {
            let at = self.step_beat(self.next);
            // rounding mustn't fire a step at the very end of a block: if the next block is a loop's
            // start, that same point would fire again
            if at >= beats_end - Self::SAME_BEAT {
                break;
            }
            let hit = clock + (at - beats) * seconds_per_beat + delay;
            self.mark(self.next, hit);
            if self.step_gestures(self.next, hit, &mut buf) {
                schedule(&buf);
            }
            self.next += 1;
        }
        self.buf = buf;
    }

    pub fn following(&self) -> bool {
        self.follow_end.is_some()
    }

    /// Stop following. The next [`Self::follow`] looks its step up afresh.
    pub fn stop_following(&mut self) {
        self.follow_end = None;
        self.mark_n = [-1; MARKS];
    }

    /// The steps scheduled lately: number and hit time, in no particular order.
    pub fn recent_hits(&self) -> impl Iterator<Item = (i64, f64)> + '_ {
        self.mark_n.iter().zip(self.mark_t.iter()).filter(|(n, _)| **n >= 0).map(|(n, t)| (*n, *t))
    }

    /// Gestures for one sound whose acoustic hit lands at `hit` on the voice clock, appended to
    /// `out` (`len`: how long the sound may last; the TS default was one step, [`Self::step_dur`]).
    /// No allocation when `out` has room for [`MAX_SOUND_GESTURES`] more.
    pub fn gestures(&self, cell: &BeatStep, hit: f64, len: f64, out: &mut Vec<Gesture>) {
        let Some(s) = beat_sound(cell.id.as_str()) else { return };
        let start = out.len();
        (s.build)(self.body, &BeatBuild { f0: midi_to_hz(cell.note.unwrap_or(BASS_NOTE)), len, accent: cell.accent.unwrap_or(1.0) }, out);
        let off = hit - s.latency;
        for g in out[start..].iter_mut() {
            g.t0 += off;
            g.t1 += off;
        }
    }

    /// Set-up time a sound needs before its hit (s): when a pad press can sound at the earliest.
    /// `scratch` is cleared and used to build the sound (no allocation when it has room).
    pub fn lead_in_with(id: &str, body: BodyMix, scratch: &mut Vec<Gesture>) -> f64 {
        let Some(s) = beat_sound(id) else { return 0.0 };
        scratch.clear();
        (s.build)(body, &BeatBuild { f0: 100.0, len: 0.15, accent: 1.0 }, scratch);
        let mut lead = 0.0;
        for g in scratch.iter() {
            lead = js_max(lead, -(g.t0 - g.ramp_in));
        }
        lead + s.latency
    }

    /// [`Self::lead_in_with`] on a fresh buffer (allocates; for the UI and tests).
    pub fn lead_in(id: &str, body: BodyMix) -> f64 {
        Self::lead_in_with(id, body, &mut Vec::with_capacity(MAX_SOUND_GESTURES))
    }
}
