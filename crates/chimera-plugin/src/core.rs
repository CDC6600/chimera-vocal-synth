//! The instrument on the audio thread, with no plugin-API types in it, so it can be tested
//! anywhere. [`Shared`] is everything it shares with the editor. See docs/plugin.md.
//!
//! Real time: [`Core::begin_block`], [`Core::event`], [`Core::render`], [`Core::end_block`] and
//! [`Core::reset`] don't block, allocate or free. The audio thread only `try_lock`s, and what it
//! replaces it hands back through [`Pending`] for the main thread to free.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};

use chimera::anatomy::human::{ArticKey, ArticPatch, BodyMix};
use chimera::control::singer::{Singer, syllables_from};
use chimera::engine::{Engine, SUB_BLOCK};
use chimera::math::{midi_to_hz, pow};
use chimera::phonetics::Syllable;
use chimera::phonetics::sing::LiveSyllable;
use chimera::voice::human_voice::{HumanVoice, SourcePatch, TELE_LABELS, VoiceTelemetry};
use serde::{Deserialize, Serialize};

/// The lowest sample rate the engine is stable at (Hz): the airway gives NaN at 22.05 kHz.
pub const MIN_SAMPLE_RATE: f64 = 32000.0;
pub const DEFAULT_LYRICS: &str = "Hello there, I am a voice made of air.";
/// Notes from the on-screen keys waiting for the audio thread.
const MAX_UI_NOTES: usize = 256;
/// Pitch bend range, each way (semitones).
const BEND_RANGE: f64 = 2.0;
/// Telemetry snapshots a second, while someone is watching (as the browser's ~60 Hz).
const TELEMETRY_RATE: f64 = 60.0;
/// A poll keeps telemetry flowing for this long (s).
const WATCH_TIME: f64 = 0.25;

/// What the MIDI keys do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// each note sings the next syllable
    #[default]
    Sing,
    /// the keys set the pitch of the posture the user has set, and breathe it
    Play,
}

/// What a project saves besides the host parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub lyrics: String,
    pub body: BodyMix,
    pub mode: Mode,
    /// the articulators changed from neutral
    pub artic: ArticPatch,
    pub tip_free: bool,
    pub voiced: bool,
    /// the pitch set by hand, in Play mode (Hz)
    pub f0: f64,
    /// where the lyrics restart when the transport starts
    pub start_syllable: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { version: 1, lyrics: DEFAULT_LYRICS.to_string(), body: BodyMix::MALE, mode: Mode::Sing, artic: ArticPatch::default(), tip_free: true, voiced: true, f0: 110.0, start_syllable: 0 }
    }
}

/// The host parameters' values for one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    pub vibrato: f32,
    pub vibrato_rate: f32,
    pub breath: f32,
    pub pressed: f32,
    pub bright: f32,
    pub breathy: f32,
    pub rough: f32,
    pub fry: f32,
    pub glide: f32,
    pub ring: f32,
    pub restart: bool,
}

/// What the host says about the transport this block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    pub playing: bool,
    /// where it is (beats where the host gives them), to notice a loop or a seek
    pub position: Option<f64>,
}

/// A host event, at the point in the block [`Core::event`] is called.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// `velocity` in MIDI units (0–127)
    NoteOn { midi: u8, velocity: f64 },
    NoteOff { midi: u8 },
    /// all notes off, all sound off, a choke
    AllOff,
    /// 0..1, 0.5 centred
    Bend(f32),
}

/// A note from the on-screen keys.
#[derive(Clone, Copy, Debug)]
pub struct UiNote {
    pub midi: u8,
    pub velocity: f64,
    pub on: bool,
}

/// What the editor has asked of the audio thread and it hasn't taken yet. Values merge, so a host
/// that stops processing can't make this grow; only notes queue, and they are bounded.
#[derive(Default)]
pub struct Pending {
    pub artic: ArticPatch,
    pub tip_free: Option<bool>,
    pub source: SourcePatch,
    pub hold: Option<bool>,
    pub mode: Option<Mode>,
    pub cursor: Option<usize>,
    voice: Option<(Box<HumanVoice>, BodyMix)>,
    syllables: Option<Vec<LiveSyllable>>,
    notes: Vec<UiNote>,
    /// release every key the UI holds (the window closed, or notes were dropped)
    clear_ui_keys: bool,
    /// what the audio thread replaced, for the main thread to free
    retired_voice: Option<Box<HumanVoice>>,
    retired_syllables: Option<Vec<LiveSyllable>>,
    /// the rate a new voice must have (0: not active)
    sample_rate: f64,
}

/// An f32 in an atomic (bit pattern).
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn store(&self, x: f32) {
        self.0.store(x.to_bits(), Ordering::Relaxed)
    }
    pub fn swap(&self, x: f32) -> f32 {
        f32::from_bits(self.0.swap(x.to_bits(), Ordering::Relaxed))
    }
}

/// The latest telemetry, for the editor.
pub struct TelemetryFrame {
    pub block: Vec<f64>,
    pub labels: [u8; TELE_LABELS],
    pub body: BodyMix,
    /// not read yet
    pub fresh: bool,
}

impl Default for TelemetryFrame {
    fn default() -> Self {
        TelemetryFrame { block: Vec::new(), labels: [0; TELE_LABELS], body: BodyMix::MALE, fresh: false }
    }
}

/// What the editor and the audio thread share.
#[derive(Default)]
pub struct Shared {
    pending: Mutex<Pending>,
    dirty: AtomicBool,
    pub telemetry: Mutex<TelemetryFrame>,
    /// samples of telemetry still wanted
    watch: AtomicI64,
    pub syllables: Mutex<Vec<Syllable>>,
    /// bumped when `syllables` changes
    pub syllables_version: AtomicU32,
    /// bumped when a state restore replaces everything
    pub revision: AtomicU32,
    pub cursor: AtomicI32,
    /// syllable sounding now (−1: none)
    pub sung: AtomicI32,
    /// MIDI note sounding now (−1: none)
    pub note: AtomicI32,
    pub playing: AtomicBool,
    /// time spent in `process` and audio time rendered since the editor last read them (ns), and
    /// the worst block (a fraction of its duration)
    pub busy_ns: AtomicU64,
    pub audio_ns: AtomicU64,
    pub load_peak: AtomicF32,
    sample_rate: AtomicU64,
    /// the engine stopped (a panic, or a non-finite sample): silent until it is re-created
    pub faulted: AtomicBool,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Change what the audio thread will take next. Main thread.
    pub fn edit(&self, f: impl FnOnce(&mut Pending)) {
        let mut p = self.lock();
        f(&mut p);
        self.dirty.store(true, Ordering::Release);
    }

    /// A note from the on-screen keys. If the queue is full, every UI key lifts instead, so a
    /// dropped release can't leave a note stuck.
    pub fn ui_note(&self, note: UiNote) {
        self.edit(|p| {
            if p.notes.len() < MAX_UI_NOTES {
                p.notes.push(note);
            } else {
                p.notes.clear();
                p.clear_ui_keys = true;
            }
        });
    }

    /// Lift every key the UI holds (the window closed).
    pub fn clear_ui_keys(&self) {
        self.edit(|p| {
            p.notes.clear();
            p.clear_ui_keys = true;
        });
    }

    /// Send a voice built off the audio thread. Dropped if it isn't for the current sample rate.
    pub fn send_voice(&self, voice: Box<HumanVoice>, body: BodyMix) {
        let mut p = self.lock();
        if voice.fs_out != p.sample_rate {
            drop(p);
            return;
        }
        let freed = (p.retired_voice.take(), p.voice.replace((voice, body)));
        self.dirty.store(true, Ordering::Release);
        drop(p);
        drop(freed); // outside the lock: the audio thread must not find it busy for this long
    }

    /// New lyrics: for the audio thread, and for the editor to show.
    pub fn send_lyrics(&self, syllables: Vec<Syllable>) {
        let live = syllables_from(&syllables);
        *self.syllables.lock().unwrap_or_else(|e| e.into_inner()) = syllables;
        self.syllables_version.fetch_add(1, Ordering::Relaxed);
        let mut p = self.lock();
        let freed = (p.retired_syllables.take(), p.syllables.replace(live));
        self.dirty.store(true, Ordering::Release);
        drop(p);
        drop(freed);
    }

    /// Start afresh, as `initialize` does: nothing queued is for the new [`Core`].
    pub fn restart(&self, sample_rate: f64) {
        let fresh = Pending { sample_rate, notes: Vec::with_capacity(MAX_UI_NOTES), ..Default::default() };
        let old = std::mem::replace(&mut *self.lock(), fresh);
        drop(old);
        self.dirty.store(false, Ordering::Release);
        self.sample_rate.store(sample_rate.to_bits(), Ordering::Relaxed);
        self.faulted.store(false, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// The current sample rate (0: not active).
    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    /// Someone is looking: collect telemetry for a little longer.
    pub fn watch(&self) {
        self.watch.store((WATCH_TIME * self.sample_rate()) as i64, Ordering::Relaxed);
    }

    /// Account for one block's cost.
    pub fn note_load(&self, busy_ns: u64, samples: usize) {
        let audio = (samples as f64 / self.sample_rate() * 1e9) as u64;
        self.busy_ns.fetch_add(busy_ns, Ordering::Relaxed);
        self.audio_ns.fetch_add(audio, Ordering::Relaxed);
        let used = busy_ns as f32 / audio.max(1) as f32;
        if used > self.load_peak.load() {
            self.load_peak.store(used);
        }
    }
}

pub struct Core {
    engine: Engine,
    singer: Singer,
    mode: Mode,
    params: VoiceParams,
    /// the Hold breath toggle (Play)
    hold: bool,
    /// keys down in Play, oldest first
    play_keys: Vec<u8>,
    /// keys down per owner, as bit masks: a pitch lifts when its last owner lets go
    host_keys: u128,
    ui_keys: u128,
    start_syllable: usize,
    transport: Transport,
    tele: VoiceTelemetry,
    block: Vec<f64>,
    labels: [u8; TELE_LABELS],
    /// samples until the next telemetry snapshot
    tele_due: i64,
    tele_every: i64,
}

impl Core {
    /// Build the instrument from a project's state. Allocates: not on the audio thread.
    pub fn new(sample_rate: f64, settings: &Settings, syllables: &[Syllable], params: VoiceParams, shared: &Shared) -> Core {
        // a new male engine is the largest airway at this rate, so telemetry sized for it fits any
        let mut engine = Engine::new(sample_rate);
        let mut tele = VoiceTelemetry::default();
        let mut block = Vec::new();
        let info = engine.info();
        tele.reserve(info.n_ph + info.n_or, &mut block);
        // the other buffer of the swap with the editor
        let mut frame = shared.telemetry.lock().unwrap_or_else(|e| e.into_inner());
        frame.block = Vec::with_capacity(block.capacity());
        frame.fresh = false;
        drop(frame);

        engine.set_body(settings.body);
        let mut singer = Singer::new();
        singer.body = settings.body;
        let _ = singer.set_syllables(syllables_from(syllables));
        singer.set_cursor(settings.start_syllable);
        let mut core = Core {
            engine,
            singer,
            mode: settings.mode,
            params,
            hold: false,
            play_keys: Vec::with_capacity(128),
            host_keys: 0,
            ui_keys: 0,
            start_syllable: settings.start_syllable,
            transport: Transport::default(),
            tele,
            block,
            labels: [0; TELE_LABELS],
            tele_due: 0,
            tele_every: (sample_rate / TELEMETRY_RATE) as i64,
        };
        core.engine.set_source(&SourcePatch { f0: Some(settings.f0), voiced: Some(settings.voiced), ..Default::default() });
        core.engine.set_artic(&settings.artic, Some(settings.tip_free));
        core.apply_params();
        // a voice's first control step sizes some of its buffers: not on the audio thread
        core.engine.render(&mut [0.0; SUB_BLOCK]);
        core
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn singer(&self) -> &Singer {
        &self.singer
    }

    fn apply_params(&mut self) {
        let p = self.params;
        self.singer.vibrato = p.vibrato as f64;
        // while singing, the notes' own gestures carry the vibrato: a base depth would shake the onset
        let depth = if self.mode == Mode::Play { p.vibrato as f64 } else { 0.0 };
        self.engine.set_source(&SourcePatch {
            vibrato_depth: Some(depth),
            vibrato_rate: Some(p.vibrato_rate as f64),
            pressure: Some(p.breath as f64),
            tension: Some(p.pressed as f64),
            brightness: Some(p.bright as f64),
            breathiness: Some(p.breathy as f64),
            roughness: Some(p.rough as f64),
            fry: Some(p.fry as f64),
            glide: Some(p.glide as f64),
            ..Default::default()
        });
        self.engine.set_artic(&ArticPatch::default().with(ArticKey::Epilarynx, p.ring as f64), None);
    }

    fn update_gate(&mut self) {
        let on = self.mode == Mode::Play && (self.hold || !self.play_keys.is_empty());
        self.engine.set_gate(if on { 1.0 } else { 0.0 });
    }

    fn press(&mut self, midi: u8, velocity: f64) {
        match self.mode {
            Mode::Sing => self.singer.note_on(&mut self.engine, midi, velocity),
            Mode::Play => {
                self.play_keys.retain(|&k| k != midi);
                self.play_keys.push(midi);
                self.engine.set_source(&SourcePatch::f0(midi_to_hz(midi as f64)));
                self.update_gate();
            }
        }
    }

    fn lift(&mut self, midi: u8) {
        match self.mode {
            Mode::Sing => self.singer.note_off(&mut self.engine, midi),
            Mode::Play => {
                self.play_keys.retain(|&k| k != midi);
                // fall back to the most recent note still held (legato)
                if let Some(&last) = self.play_keys.last() {
                    self.engine.set_source(&SourcePatch::f0(midi_to_hz(last as f64)));
                }
                self.update_gate();
            }
        }
    }

    /// Stop everything the keys are doing.
    fn stop_notes(&mut self) {
        self.singer.stop_all(&mut self.engine);
        self.play_keys.clear();
        self.host_keys = 0;
        self.ui_keys = 0;
        self.update_gate();
    }

    fn lift_ui_keys(&mut self) {
        let mine = self.ui_keys;
        self.ui_keys = 0;
        for midi in 0..128u8 {
            let bit = 1u128 << midi;
            if mine & bit != 0 && self.host_keys & bit == 0 {
                self.lift(midi);
            }
        }
    }

    /// Take what the editor has sent, if the lock is free.
    fn intake(&mut self, shared: &Shared) {
        if !shared.dirty.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut p) = shared.pending.try_lock() else { return };
        shared.dirty.store(false, Ordering::Release);
        if let Some((mut voice, body)) = p.voice.take() {
            // the new voice has no score: a sung note would hang, and its coda has nowhere to go
            self.stop_notes();
            self.engine.install_voice(&mut voice, body);
            self.singer.body = body;
            p.retired_voice = Some(voice);
        }
        if let Some(syllables) = p.syllables.take() {
            p.retired_syllables = Some(self.singer.set_syllables(syllables));
            self.start_syllable = 0;
        }
        if let Some(mode) = p.mode.take()
            && mode != self.mode
        {
            self.stop_notes();
            self.mode = mode;
            self.hold = false;
            self.update_gate();
            self.apply_params();
        }
        if p.source != SourcePatch::default() {
            self.engine.set_source(&p.source);
            p.source = SourcePatch::default();
        }
        if !p.artic.is_empty() || p.tip_free.is_some() {
            self.engine.set_artic(&p.artic, p.tip_free);
            p.artic = ArticPatch::default();
            p.tip_free = None;
        }
        if let Some(hold) = p.hold.take() {
            self.hold = hold;
            self.update_gate();
        }
        if let Some(i) = p.cursor.take() {
            self.singer.set_cursor(i);
            // picked while stopped: that is where playing starts from
            if !self.transport.playing {
                self.start_syllable = self.singer.cursor();
            }
        }
        if p.clear_ui_keys {
            p.clear_ui_keys = false;
            self.lift_ui_keys();
        }
        let Pending { notes, .. } = &mut *p;
        for n in notes.drain(..) {
            let bit = 1u128 << (n.midi & 127);
            if n.on {
                self.ui_keys |= bit;
                self.press(n.midi & 127, n.velocity);
            } else {
                let held = self.ui_keys & bit != 0;
                self.ui_keys &= !bit;
                if held && self.host_keys & bit == 0 {
                    self.lift(n.midi & 127);
                }
            }
        }
    }

    fn follow_transport(&mut self, t: Transport, shared: &Shared) {
        let was = self.transport;
        let started = t.playing && !was.playing;
        let jumped = t.playing && was.playing && matches!((t.position, was.position), (Some(now), Some(last)) if now < last - 1e-3);
        if (started || jumped) && self.params.restart {
            self.singer.set_cursor(self.start_syllable);
        }
        if was.playing && !t.playing {
            self.stop_notes();
        }
        self.transport = t;
        shared.playing.store(t.playing, Ordering::Relaxed);
    }

    /// Start of a block: what the editor sent, the host parameters, the transport.
    pub fn begin_block(&mut self, params: &VoiceParams, transport: Transport, shared: &Shared) {
        self.intake(shared);
        if *params != self.params {
            self.params = *params;
            self.apply_params();
        }
        self.follow_transport(transport, shared);
    }

    /// A host event. Render up to its sample first.
    pub fn event(&mut self, ev: Event) {
        match ev {
            Event::NoteOn { midi, velocity } => {
                self.host_keys |= 1u128 << (midi & 127);
                self.press(midi & 127, velocity);
            }
            Event::NoteOff { midi } => {
                let bit = 1u128 << (midi & 127);
                let held = self.host_keys & bit != 0;
                self.host_keys &= !bit;
                if held && self.ui_keys & bit == 0 {
                    self.lift(midi & 127);
                }
            }
            Event::AllOff => self.stop_notes(),
            Event::Bend(value) => self.engine.set_bend(pow(2.0, (value as f64 - 0.5) * 2.0 * BEND_RANGE / 12.0)),
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        self.engine.render(out);
    }

    /// End of a block of `samples`: tell the editor where things are.
    pub fn end_block(&mut self, samples: usize, shared: &Shared) {
        shared.cursor.store(self.singer.cursor() as i32, Ordering::Relaxed);
        shared.sung.store(self.singer.sung().map_or(-1, |i| i as i32), Ordering::Relaxed);
        let note = match self.mode {
            Mode::Sing => self.singer.sounding_note().map_or(-1, |m| m as i32),
            Mode::Play => self.play_keys.last().map_or(-1, |m| *m as i32),
        };
        shared.note.store(note, Ordering::Relaxed);
        let n = samples as i64;
        if shared.watch.fetch_sub(n, Ordering::Relaxed) <= 0 {
            shared.watch.store(0, Ordering::Relaxed);
            return;
        }
        self.tele_due -= n;
        if self.tele_due > 0 {
            return;
        }
        self.tele_due = self.tele_every;
        self.engine.telemetry(&mut self.tele);
        self.tele.write_flat(&mut self.block, &mut self.labels);
        if let Ok(mut frame) = shared.telemetry.try_lock() {
            std::mem::swap(&mut frame.block, &mut self.block);
            frame.labels = self.labels;
            frame.body = self.engine.info().body;
            frame.fresh = true;
        }
    }

    /// Back to the start of the lyrics, with nothing sounding. Not a full DSP reset: see
    /// docs/plugin.md.
    pub fn reset(&mut self) {
        self.stop_notes();
        self.engine.stop_speech();
        self.engine.set_bend(1.0);
        self.singer.set_cursor(self.start_syllable);
        self.transport = Transport::default();
    }
}
