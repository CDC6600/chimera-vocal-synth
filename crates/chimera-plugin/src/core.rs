//! The instrument on the audio thread, with no plugin-API types in it, so it can be tested
//! anywhere. [`Shared`] is everything it shares with the editor. See docs/plugin.md.
//!
//! Real time: [`Core::begin_block`], [`Core::event`], [`Core::render`], [`Core::end_block`] and
//! [`Core::reset`] don't block, allocate or free. The audio thread only `try_lock`s, and what it
//! replaces it hands back through [`Pending`] for the main thread to free.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};

use chimera::anatomy::human::{ArticKey, ArticPatch, BodyMix};
use chimera::control::score::{Gesture, Label};
use chimera::control::singer::{Singer, syllables_from};
use chimera::engine::{Engine, SUB_BLOCK};
use chimera::math::{midi_to_hz, pow};
use chimera::phonetics::Syllable;
use chimera::phonetics::beatbox::{BEAT_PRESETS, BeatStep, parse_beat};
use chimera::phonetics::sing::LiveSyllable;
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera::voice::human_voice::{HumanVoice, SourcePatch, TELE_LABELS, VoiceTelemetry};
use serde::{Deserialize, Serialize};

use crate::close_mic::CloseMic;
use crate::g2p;
use crate::speech::{SpeechSettings, SpokenLine, plan_lines};

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
/// Output samples kept for the editor's spectrum: more than a poll's worth at any sample rate.
const SCOPE_SAMPLES: usize = 16384;
/// The key that speaks at the body's natural pitch, and how far a key may move it (semitones).
const SPEECH_ROOT: f64 = 60.0;
const SPEECH_RANGE: f64 = 24.0;
/// A spoken line's time 0 comes this long after its note (s), as on the website.
const SPEECH_LEAD: f64 = 0.03;
/// What Beatbox reports as latency, and how far after its point on the timeline each hit lands (s):
/// a little more than the slowest sound's set-up (174 ms). See docs/plugin.md, "Beatbox".
pub const BEAT_LATENCY: f64 = 0.18;
/// Host notes from this one up hum, this many semitones down; below it they are pads.
const HUM_FROM: u8 = 60;
const HUM_DOWN: u8 = 24;
/// The tag of a hum held by a key (the singer's tags are positive), and how long it can last (s).
const HUM_TAG: f64 = -1.0;
const HUM_LEN: f64 = 3600.0;
const HUM: &str = "m";
/// A hit that isn't a step has no next step to last until (s), as on the website.
const BEAT_HIT_LEN: f64 = 0.18;
/// Pads pressed on the page waiting for the audio thread: more are dropped.
const MAX_UI_HITS: usize = 32;
/// [`Shared::spoken_line`]: nothing is being said.
pub const NOT_SPEAKING: i32 = -1;
/// [`Shared::spoken_line`]: the consonant chart's one-shot is being said.
pub const SAYING_PAD: i32 = -2;

/// What the MIDI keys do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// each note sings the next syllable
    #[default]
    Sing,
    /// the keys set the pitch of the posture the user has set, and breathe it
    Play,
    /// each note says the next line
    Speak,
    /// the sequencer follows the host, and notes are pads or a hum
    Beatbox,
}

/// The sound a host note plays in Beatbox: General MIDI's drum kit on the ten mouth sounds.
pub fn drum_pad(midi: u8) -> Option<&'static str> {
    Some(match midi {
        35 | 36 => "B",
        37 => "K",
        38 => "Pf",
        39 => "!",
        40 => "^K",
        41 | 43 | 45 | 47 | 48 | 50 => "P!",
        42 | 44 | 51 | 53 | 59 => "t",
        46 => "tss",
        49 | 52 | 55 | 57 => "kch",
        _ => return None,
    })
}

/// A saved beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BeatSettings {
    pub steps: Vec<Option<BeatStep>>,
    pub bpm: f64,
    pub swing: f64,
    /// the example it came from (none once edited)
    pub preset: Option<String>,
    /// the close-mic sound
    pub mic: bool,
    /// the sequencer follows the host's transport
    pub follow: bool,
}

impl Default for BeatSettings {
    fn default() -> Self {
        let p = &BEAT_PRESETS[0];
        BeatSettings { steps: parse_beat(p.text), bpm: p.bpm, swing: p.swing, preset: Some(p.name.to_string()), mic: true, follow: true }
    }
}

/// Who holds a key, in Beatbox: the same number means a different pitch from each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Host,
    Ui,
}

/// A held key humming.
#[derive(Clone, Copy, Debug)]
struct Hum {
    owner: Owner,
    midi: u8,
    /// the pitch it sounds (MIDI)
    pitch: f64,
    accent: f64,
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
    pub speech: SpeechSettings,
    pub beat: BeatSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { version: 1, lyrics: DEFAULT_LYRICS.to_string(), body: BodyMix::MALE, mode: Mode::Sing, artic: ArticPatch::default(), tip_free: true, voiced: true, f0: 110.0, start_syllable: 0, speech: SpeechSettings::default(), beat: BeatSettings::default() }
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
    /// where it is in beats, and its tempo, if the host says
    pub beats: Option<f64>,
    pub tempo: Option<f64>,
    /// samples in this block
    pub samples: usize,
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
    pub speech_cursor: Option<usize>,
    /// say the next line (the Speak button)
    pub speak_next: bool,
    pub stop_speech: bool,
    pub beat_bpm: Option<f64>,
    pub beat_swing: Option<f64>,
    pub beat_playing: Option<bool>,
    pub mic: Option<bool>,
    pub follow_host: Option<bool>,
    beat_steps: Option<Vec<Option<BeatStep>>>,
    hits: Vec<BeatStep>,
    voice: Option<(Box<HumanVoice>, BodyMix)>,
    syllables: Option<Vec<LiveSyllable>>,
    speech: Option<NewSpeech>,
    pad: Option<(Vec<Gesture>, u32)>,
    notes: Vec<UiNote>,
    /// release every key the UI holds (the window closed, or notes were dropped)
    clear_ui_keys: bool,
    /// what the audio thread replaced, for the main thread to free
    retired_voice: Option<Box<HumanVoice>>,
    retired_syllables: Option<Vec<LiveSyllable>>,
    retired_speech: Option<Vec<Vec<Gesture>>>,
    retired_pad: Option<Vec<Gesture>>,
    retired_steps: Option<Vec<Option<BeatStep>>>,
    /// the rate a new voice must have (0: not active)
    sample_rate: f64,
}

/// New plans for the spoken lines.
struct NewSpeech {
    plans: Vec<Vec<Gesture>>,
    generation: u32,
    /// the text changed: back to its first line
    reset: bool,
}

/// What's worked out from a project's text, off the audio thread.
pub struct Prepared {
    pub syllables: Vec<Syllable>,
    pub speech: Vec<SpokenLine>,
    pub generation: u32,
}

impl Prepared {
    pub fn of(settings: &Settings, shared: &Shared) -> Prepared {
        Prepared { syllables: lyrics_to_syllables(&settings.lyrics, g2p()), speech: plan_lines(&settings.speech, settings.body, g2p()), generation: shared.next_generation() }
    }
}

/// The spoken lines and the last one-shot, for the editor to show. Each set of plans has a
/// generation, so the page can tell a plan from the one really sounding.
#[derive(Default)]
pub struct SpeechDoc {
    pub lines: Vec<SpokenLine>,
    pub lines_generation: u32,
    pub pad: Option<SpokenLine>,
    pub pad_generation: u32,
}

fn gestures_of(lines: &[SpokenLine]) -> Vec<Vec<Gesture>> {
    lines.iter().map(|l| l.plan.gestures.clone()).collect()
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
    /// held while the project's settings and `pending` change together, so an edit from the page
    /// and a restore can't interleave. Never touched on the audio thread.
    transaction: Mutex<()>,
    pub telemetry: Mutex<TelemetryFrame>,
    /// the output since the editor last read it, for its spectrum (never grows past its capacity)
    pub scope: Mutex<Vec<f32>>,
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
    /// bumped by every press, so the editor can tell a new note from the same pitch held
    pub note_serial: AtomicU32,
    pub speech: Mutex<SpeechDoc>,
    generations: AtomicU32,
    /// the line a note says next
    pub speech_cursor: AtomicI32,
    /// bumped whenever what's being said changes, and written last: read it first
    pub spoken_serial: AtomicU32,
    /// the line being said ([`NOT_SPEAKING`], [`SAYING_PAD`])
    pub spoken_line: AtomicI32,
    /// the generation of the plan being said
    pub spoken_generation: AtomicU32,
    /// the voice clock at the plan's time 0 (f64 bits)
    pub spoken_t0: AtomicU64,
    /// the step under the playhead (−1: none)
    pub beat_step: AtomicI32,
    /// the pattern is running, and whether that's the host's doing
    pub beat_running: AtomicBool,
    pub beat_following: AtomicBool,
    /// the host's tempo (f64 bits; 0: not known)
    pub host_bpm: AtomicU64,
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

    /// See [`Shared::transaction`](#structfield.transaction). Don't call into the host under this.
    pub fn transaction(&self) -> std::sync::MutexGuard<'_, ()> {
        self.transaction.lock().unwrap_or_else(|e| e.into_inner())
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

    /// A number no set of speech plans has had.
    pub fn next_generation(&self) -> u32 {
        self.generations.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// New plans for the lines: for the audio thread, and for the editor to show.
    pub fn send_speech(&self, lines: Vec<SpokenLine>, reset: bool) {
        let generation = self.next_generation();
        let plans = gestures_of(&lines);
        {
            let mut doc = self.speech.lock().unwrap_or_else(|e| e.into_inner());
            doc.lines = lines;
            doc.lines_generation = generation;
        }
        let mut p = self.lock();
        let reset = reset || p.speech.as_ref().is_some_and(|s| s.reset);
        let freed = (p.retired_speech.take(), p.speech.replace(NewSpeech { plans, generation, reset }));
        self.dirty.store(true, Ordering::Release);
        drop(p);
        drop(freed);
    }

    /// A new pattern for the sequencer.
    pub fn send_beat_steps(&self, steps: Vec<Option<BeatStep>>) {
        let mut p = self.lock();
        let freed = (p.retired_steps.take(), p.beat_steps.replace(steps));
        self.dirty.store(true, Ordering::Release);
        drop(p);
        drop(freed);
    }

    /// A pad pressed on the page. If too many are waiting (a host that stopped processing), it's lost.
    pub fn ui_hit(&self, step: BeatStep) {
        self.edit(|p| {
            if p.hits.len() < MAX_UI_HITS {
                p.hits.push(step);
            }
        });
    }

    /// Say this once, now, if Speak is the mode (the consonant chart).
    pub fn send_pad(&self, line: SpokenLine) {
        let generation = self.next_generation();
        let gestures = line.plan.gestures.clone();
        {
            let mut doc = self.speech.lock().unwrap_or_else(|e| e.into_inner());
            doc.pad = Some(line);
            doc.pad_generation = generation;
        }
        let mut p = self.lock();
        let freed = (p.retired_pad.take(), p.pad.replace((gestures, generation)));
        self.dirty.store(true, Ordering::Release);
        drop(p);
        drop(freed);
    }

    /// What was prepared for a new [`Core`], for the editor to show.
    pub fn adopt(&self, prepared: Prepared) {
        *self.syllables.lock().unwrap_or_else(|e| e.into_inner()) = prepared.syllables;
        self.syllables_version.fetch_add(1, Ordering::Relaxed);
        let mut doc = self.speech.lock().unwrap_or_else(|e| e.into_inner());
        doc.lines = prepared.speech;
        doc.lines_generation = prepared.generation;
    }

    /// Start afresh, as `initialize` does: nothing queued is for the new [`Core`].
    pub fn restart(&self, sample_rate: f64) {
        let fresh = Pending { sample_rate, notes: Vec::with_capacity(MAX_UI_NOTES), hits: Vec::with_capacity(MAX_UI_HITS), ..Default::default() };
        let mut p = self.lock();
        let old = std::mem::replace(&mut *p, fresh);
        // under the same lock: an edit published right after must keep its notification
        self.dirty.store(false, Ordering::Release);
        drop(p);
        drop(old);
        let mut scope = self.scope.lock().unwrap_or_else(|e| e.into_inner());
        scope.clear();
        scope.reserve_exact(SCOPE_SAMPLES);
        drop(scope);
        self.sample_rate.store(sample_rate.to_bits(), Ordering::Relaxed);
        self.faulted.store(false, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// The current sample rate (0: not active).
    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    /// Keep this block of output for the editor, if someone is looking. On the audio thread: a busy
    /// lock or a slow editor loses some of the display, never time.
    pub fn feed_scope(&self, block: &[f32]) {
        if self.watch.load(Ordering::Relaxed) <= 0 {
            return;
        }
        if let Ok(mut scope) = self.scope.try_lock() {
            let room = scope.capacity() - scope.len();
            scope.extend_from_slice(&block[..block.len().min(room)]);
        }
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
    /// presses so far (see [`Shared::note_serial`])
    presses: u32,
    /// each line's gestures, and the generation of those plans
    speech: Vec<Vec<Gesture>>,
    speech_generation: u32,
    speech_cursor: usize,
    speech_start: usize,
    /// the last one-shot, kept until the main thread can free it
    pad: Vec<Gesture>,
    /// what's being said, as [`Shared`] shows it
    spoken_serial: u32,
    spoken_line: i32,
    spoken_generation: u32,
    spoken_t0: f64,
    /// pitch ratios: from the key that said this line, and the pitch wheel
    key_bend: f64,
    wheel_bend: f64,
    /// the sequencer is following the host's transport
    following: bool,
    follow_host: bool,
    /// the user's tempo: following the host overwrites the sequencer's
    beat_bpm: f64,
    mic_on: bool,
    close_mic: CloseMic,
    /// keys humming in Beatbox, oldest first
    hums: Vec<Hum>,
    /// samples until the next telemetry snapshot
    tele_due: i64,
    tele_every: i64,
}

impl Core {
    /// Build the instrument from a project's state. Allocates: not on the audio thread.
    pub fn new(sample_rate: f64, settings: &Settings, prepared: &Prepared, params: VoiceParams, shared: &Shared) -> Core {
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
        let _ = singer.set_syllables(syllables_from(&prepared.syllables));
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
            presses: 0,
            speech_cursor: settings.speech.start_line.min(prepared.speech.len().saturating_sub(1)),
            speech_start: settings.speech.start_line.min(prepared.speech.len().saturating_sub(1)),
            speech: gestures_of(&prepared.speech),
            speech_generation: prepared.generation,
            pad: Vec::new(),
            spoken_serial: shared.spoken_serial.load(Ordering::Relaxed).wrapping_add(1),
            spoken_line: NOT_SPEAKING,
            spoken_generation: 0,
            spoken_t0: 0.0,
            key_bend: 1.0,
            wheel_bend: 1.0,
            following: false,
            follow_host: settings.beat.follow,
            beat_bpm: settings.beat.bpm,
            mic_on: settings.beat.mic,
            close_mic: CloseMic::new(sample_rate),
            hums: Vec::with_capacity(2 * 128),
            tele_due: 0,
            tele_every: (sample_rate / TELEMETRY_RATE) as i64,
        };
        core.engine.set_source(&SourcePatch { f0: Some(settings.f0), voiced: Some(settings.voiced), ..Default::default() });
        core.engine.set_artic(&settings.artic, Some(settings.tip_free));
        core.engine.set_beat_steps(&settings.beat.steps);
        core.engine.set_beat_tempo(Some(settings.beat.bpm), Some(settings.beat.swing));
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

    /// The line a note says next.
    pub fn speech_cursor(&self) -> usize {
        self.speech_cursor
    }

    /// The latency to report to the host: only Beatbox has any.
    pub fn latency_samples(&self) -> u32 {
        if self.mode == Mode::Beatbox { (BEAT_LATENCY * self.engine.sample_rate).round() as u32 } else { 0 }
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
        self.presses = self.presses.wrapping_add(1);
        match self.mode {
            Mode::Sing => self.singer.note_on(&mut self.engine, midi, velocity),
            Mode::Play => {
                self.play_keys.retain(|&k| k != midi);
                self.play_keys.push(midi);
                self.engine.set_source(&SourcePatch::f0(midi_to_hz(midi as f64)));
                self.update_gate();
            }
            Mode::Speak => {
                let semitones = (midi as f64 - SPEECH_ROOT).clamp(-SPEECH_RANGE, SPEECH_RANGE);
                self.say_next(pow(2.0, semitones / 12.0));
            }
            // by owner: see `beat_key`
            Mode::Beatbox => {}
        }
    }

    fn lift(&mut self, midi: u8) {
        match self.mode {
            Mode::Sing => self.singer.note_off(&mut self.engine, midi),
            // a line is a one-shot: it ends by itself, or when the next one cuts it
            Mode::Speak => {}
            Mode::Beatbox => {}
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
        if self.mode == Mode::Speak {
            self.stop_speaking();
        }
        if self.mode == Mode::Beatbox {
            self.hums.clear();
            let now = self.engine.clock();
            self.engine.release_at(HUM_TAG, now);
        }
        self.update_gate();
    }

    /// When a sound asked for now should land: `BEAT_LATENCY` on, while the host's timeline is
    /// running and it compensates; otherwise as soon as the mouth can make it.
    fn beat_time(&mut self, id: &str) -> f64 {
        let wait = if self.transport.playing { BEAT_LATENCY } else { self.engine.beat_lead(id) };
        self.engine.clock() + wait
    }

    /// A sound from a pad, or from a host note on the drum map.
    fn beat_hit(&mut self, step: BeatStep) {
        let hit = self.beat_time(step.id.as_str());
        self.engine.beat_hit_at(&step, hit, BEAT_HIT_LEN, None);
    }

    /// Hum this key's pitch, in place of whatever was humming.
    fn start_hum(&mut self, hum: Hum) {
        let hit = self.beat_time(HUM);
        self.engine.release_at(HUM_TAG, hit);
        let step = BeatStep { id: Label::new(HUM), note: Some(hum.pitch), accent: Some(hum.accent) };
        self.engine.beat_hit_at(&step, hit, HUM_LEN, Some(HUM_TAG));
    }

    /// The hums changed: the newest key held hums, or nothing does. `top` is what was humming.
    fn resolve_hums(&mut self, top: Option<(Owner, u8)>) {
        let now = self.hums.last().copied();
        match now {
            Some(h) if top != Some((h.owner, h.midi)) => self.start_hum(h),
            Some(_) => {}
            None if top.is_some() => {
                let t = self.beat_time(HUM);
                self.engine.release_at(HUM_TAG, t);
            }
            None => {}
        }
    }

    /// A key went down or up in Beatbox. The two owners' numbers mean different pitches, so the
    /// shared press and release masks don't apply here.
    fn beat_key(&mut self, owner: Owner, midi: u8, velocity: f64, on: bool) {
        let accent = (velocity / 100.0).clamp(0.4, 1.25);
        let pitch = match owner {
            Owner::Ui => midi as f64,
            Owner::Host if midi >= HUM_FROM => (midi - HUM_DOWN) as f64,
            Owner::Host => {
                if on && let Some(id) = drum_pad(midi) {
                    self.presses = self.presses.wrapping_add(1);
                    self.beat_hit(BeatStep { id: Label::new(id), note: None, accent: Some(accent) });
                }
                return;
            }
        };
        let top = self.hums.last().map(|h| (h.owner, h.midi));
        self.hums.retain(|h| !(h.owner == owner && h.midi == midi));
        if on {
            self.presses = self.presses.wrapping_add(1);
            self.hums.push(Hum { owner, midi, pitch, accent });
            // a repeat of the key humming is a new note
            if top == Some((owner, midi)) {
                let h = self.hums[self.hums.len() - 1];
                self.start_hum(h);
                return;
            }
        }
        self.resolve_hums(top);
    }

    /// Follow the host's transport with the sequencer, or stop following it.
    fn follow_beat(&mut self, t: Transport) {
        let host = if self.mode == Mode::Beatbox && self.follow_host && t.playing { t.beats.zip(t.tempo).filter(|(_, bpm)| *bpm > 0.0) } else { None };
        match host {
            Some((beats, bpm)) => {
                if !self.following {
                    self.engine.set_beat_playing(false);
                    self.following = true;
                }
                let end = beats + t.samples as f64 / self.engine.sample_rate * bpm / 60.0;
                self.engine.beat_follow(beats, end, bpm, BEAT_LATENCY);
            }
            None if self.following => self.stop_following(),
            None => {}
        }
    }

    fn stop_following(&mut self) {
        self.following = false;
        self.engine.beat_stop_following();
        self.engine.set_beat_tempo(Some(self.beat_bpm), None);
    }

    fn mark_spoken(&mut self, line: i32, generation: u32, t0: f64) {
        self.spoken_serial = self.spoken_serial.wrapping_add(1);
        self.spoken_line = line;
        self.spoken_generation = generation;
        self.spoken_t0 = t0;
    }

    /// Say the next line, at this pitch ratio. The whole contour moves through the bend, so a plan
    /// isn't rebuilt per note.
    fn say_next(&mut self, key_bend: f64) {
        let Some(gestures) = self.speech.get(self.speech_cursor) else { return };
        self.key_bend = key_bend;
        self.engine.set_bend(self.key_bend * self.wheel_bend);
        let t0 = self.engine.schedule(gestures, SPEECH_LEAD, true);
        self.mark_spoken(self.speech_cursor as i32, self.speech_generation, t0);
        self.speech_cursor = (self.speech_cursor + 1) % self.speech.len();
    }

    fn stop_speaking(&mut self) {
        self.engine.stop_speech();
        if self.spoken_line != NOT_SPEAKING {
            self.mark_spoken(NOT_SPEAKING, 0, 0.0);
        }
        self.key_bend = 1.0;
        self.engine.set_bend(self.wheel_bend);
    }

    fn lift_ui_keys(&mut self) {
        if self.mode == Mode::Beatbox {
            let top = self.hums.last().map(|h| (h.owner, h.midi));
            self.hums.retain(|h| h.owner != Owner::Ui);
            self.resolve_hums(top);
            return;
        }
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
        if let Some(new) = p.speech.take() {
            p.retired_speech = Some(std::mem::replace(&mut self.speech, new.plans));
            self.speech_generation = new.generation;
            let last = self.speech.len().saturating_sub(1);
            self.speech_cursor = if new.reset { 0 } else { self.speech_cursor.min(last) };
            self.speech_start = if new.reset { 0 } else { self.speech_start.min(last) };
        }
        if let Some(mode) = p.mode.take()
            && mode != self.mode
        {
            self.stop_notes();
            if self.mode == Mode::Beatbox {
                self.stop_following();
                self.engine.set_beat_playing(false);
            }
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
        if let Some(i) = p.speech_cursor.take() {
            self.speech_cursor = i.min(self.speech.len().saturating_sub(1));
            if !self.transport.playing {
                self.speech_start = self.speech_cursor;
            }
        }
        if let Some((gestures, generation)) = p.pad.take() {
            let old = std::mem::replace(&mut self.pad, gestures);
            p.retired_pad = Some(old);
            if self.mode == Mode::Speak {
                self.key_bend = 1.0;
                self.engine.set_bend(self.wheel_bend);
                let t0 = self.engine.schedule(&self.pad, SPEECH_LEAD, true);
                self.mark_spoken(SAYING_PAD, generation, t0);
            }
        }
        if let Some(steps) = p.beat_steps.take() {
            self.engine.set_beat_steps(&steps);
            p.retired_steps = Some(steps);
        }
        if p.beat_bpm.is_some() || p.beat_swing.is_some() {
            self.beat_bpm = p.beat_bpm.unwrap_or(self.beat_bpm);
            self.engine.set_beat_tempo(p.beat_bpm.take(), p.beat_swing.take());
        }
        if let Some(on) = p.mic.take() {
            self.mic_on = on;
        }
        if let Some(on) = p.follow_host.take() {
            self.follow_host = on;
        }
        if let Some(on) = p.beat_playing.take()
            && self.mode == Mode::Beatbox
            && !self.following
        {
            self.engine.set_beat_playing(on);
        }
        let Pending { hits, .. } = &mut *p;
        for step in hits.drain(..) {
            if self.mode == Mode::Beatbox {
                self.beat_hit(step);
            }
        }
        if std::mem::take(&mut p.speak_next) && self.mode == Mode::Speak {
            self.say_next(1.0);
        }
        if std::mem::take(&mut p.stop_speech) && self.mode == Mode::Speak {
            self.stop_speaking();
        }
        if p.clear_ui_keys {
            p.clear_ui_keys = false;
            self.lift_ui_keys();
        }
        let Pending { notes, .. } = &mut *p;
        for n in notes.drain(..) {
            if self.mode == Mode::Beatbox {
                self.beat_key(Owner::Ui, n.midi & 127, n.velocity, n.on);
                continue;
            }
            let bit = 1u128 << (n.midi & 127);
            if n.on {
                self.ui_keys |= bit;
                if self.host_keys & bit == 0 {
                    self.press(n.midi & 127, n.velocity);
                }
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
            self.speech_cursor = self.speech_start;
        }
        if was.playing && !t.playing {
            self.stop_notes();
        }
        self.transport = t;
        self.follow_beat(t);
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
            Event::NoteOn { midi, velocity } if self.mode == Mode::Beatbox => self.beat_key(Owner::Host, midi & 127, velocity, true),
            Event::NoteOff { midi } if self.mode == Mode::Beatbox => self.beat_key(Owner::Host, midi & 127, 0.0, false),
            Event::NoteOn { midi, velocity } => {
                let bit = 1u128 << (midi & 127);
                self.host_keys |= bit;
                // a repeat from the same owner is a new note; a key the UI already holds isn't
                if self.ui_keys & bit == 0 {
                    self.press(midi & 127, velocity);
                }
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
            Event::Bend(value) => {
                self.wheel_bend = pow(2.0, (value as f64 - 0.5) * 2.0 * BEND_RANGE / 12.0);
                self.engine.set_bend(self.key_bend * self.wheel_bend);
            }
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        self.engine.render(out);
        if self.mode == Mode::Beatbox && self.mic_on {
            self.close_mic.process(out);
        }
    }

    /// End of a block of `samples`: tell the editor where things are.
    pub fn end_block(&mut self, samples: usize, shared: &Shared) {
        shared.cursor.store(self.singer.cursor() as i32, Ordering::Relaxed);
        shared.sung.store(self.singer.sung().map_or(-1, |i| i as i32), Ordering::Relaxed);
        let note = match self.mode {
            Mode::Sing => self.singer.sounding_note().map_or(-1, |m| m as i32),
            Mode::Play => self.play_keys.last().map_or(-1, |m| *m as i32),
            Mode::Speak | Mode::Beatbox => -1,
        };
        shared.note.store(note, Ordering::Relaxed);
        shared.note_serial.store(self.presses, Ordering::Relaxed);
        shared.speech_cursor.store(self.speech_cursor as i32, Ordering::Relaxed);
        shared.spoken_line.store(self.spoken_line, Ordering::Relaxed);
        shared.spoken_generation.store(self.spoken_generation, Ordering::Relaxed);
        shared.spoken_t0.store(self.spoken_t0.to_bits(), Ordering::Relaxed);
        shared.spoken_serial.store(self.spoken_serial, Ordering::Release);
        shared.beat_step.store(self.engine.beat_step() as i32, Ordering::Relaxed);
        shared.beat_running.store(self.following || self.engine.beat().playing, Ordering::Relaxed);
        shared.beat_following.store(self.following, Ordering::Relaxed);
        shared.host_bpm.store(self.transport.tempo.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
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
        self.stop_following();
        self.engine.set_beat_playing(false);
        self.engine.stop_speech();
        self.key_bend = 1.0;
        self.wheel_bend = 1.0;
        self.engine.set_bend(1.0);
        self.singer.set_cursor(self.start_syllable);
        self.speech_cursor = self.speech_start;
        self.transport = Transport::default();
    }
}
