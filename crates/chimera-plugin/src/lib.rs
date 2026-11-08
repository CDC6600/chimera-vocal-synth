//! CHIMERA as a VST3/CLAP instrument (spike). MIDI notes sing the lyrics, one syllable per note;
//! on macOS the editor is a system webview (WKWebView) showing `ui/index.html`.
//!
//! Threads: the host's audio thread runs `process` (the engine and the Sing controller); the
//! editor runs on the host's main thread and talks to the audio side only through [`Shared`].

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

use chimera::control::singer::{Singer, syllables_from};
use chimera::engine::Engine;
use chimera::phonetics::Syllable;
use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::syllables::lyrics_to_syllables;
use nih_plug::prelude::*;

#[cfg(target_os = "macos")]
mod editor;

/// The lowest sample rate the engine is stable at (Hz).
const MIN_SAMPLE_RATE: f32 = 32000.0;

pub const DEFAULT_LYRICS: &str = "Hello there, I am a voice made of air.";

/// The pronouncing dictionary, parsed once (off the audio thread).
fn g2p() -> &'static EnglishG2P {
    static G2P: OnceLock<EnglishG2P> = OnceLock::new();
    G2P.get_or_init(|| {
        static GZ: &[u8] = include_bytes!("../../../public/data/cmudict.txt.gz");
        let mut text = String::new();
        let mut g = EnglishG2P::new();
        if flate2::read::GzDecoder::new(GZ).read_to_string(&mut text).is_ok() {
            g.load_dict(&text);
        }
        g
    })
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

/// What the editor and the audio thread share. The audio thread only ever `try_lock`s.
#[derive(Default)]
pub struct Shared {
    /// new syllables for the audio thread (taken at the start of the next block)
    pending: Mutex<Option<Vec<Syllable>>>,
    /// the syllables' texts, for the editor
    pub texts: Mutex<Vec<String>>,
    /// notes played on the editor's keys: (midi, on), drained by the audio thread
    pub ui_notes: Mutex<Vec<(u8, bool)>>,
    pub cursor: AtomicI32,
    /// syllable sounding now (−1: none)
    pub sung: AtomicI32,
    /// MIDI note sounding now (−1: none)
    pub note: AtomicI32,
    /// output level (peak-held RMS)
    pub level: AtomicF32,
    /// DSP load since the editor last read it: time spent in `process` and audio time rendered
    /// (ns), and the worst single block (fraction of its duration)
    pub busy_ns: AtomicU64,
    pub audio_ns: AtomicU64,
    pub load_peak: AtomicF32,
    pub sample_rate: AtomicF32,
    pub block: AtomicI32,
    /// the engine panicked: the plugin is silent until it is re-created
    pub faulted: AtomicBool,
}

impl Shared {
    /// Lyrics → syllables (G2P, allocates: call from the editor or `initialize`).
    pub fn set_lyrics(&self, params: &ChimeraParams, text: &str) {
        *params.lyrics.write().unwrap() = text.to_string();
        let syls = lyrics_to_syllables(text, g2p());
        *self.texts.lock().unwrap() = syls.iter().map(|s| format!("{}{}", s.text, if s.word_end { "" } else { "-" })).collect();
        *self.pending.lock().unwrap() = Some(syls);
    }
}

#[derive(Params)]
pub struct ChimeraParams {
    #[id = "gain"]
    pub gain: FloatParam,
    /// vibrato depth on held vowels (cents)
    #[id = "vibrato"]
    pub vibrato: FloatParam,
    #[persist = "lyrics"]
    pub lyrics: RwLock<String>,
}

impl Default for ChimeraParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new("Gain", 0.0, FloatRange::Linear { min: -30.0, max: 12.0 }).with_smoother(SmoothingStyle::Linear(20.0)).with_step_size(0.1).with_unit(" dB"),
            vibrato: FloatParam::new("Vibrato", 35.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_step_size(1.0).with_unit(" ct"),
            lyrics: RwLock::new(DEFAULT_LYRICS.to_string()),
        }
    }
}

pub struct Chimera {
    params: Arc<ChimeraParams>,
    shared: Arc<Shared>,
    engine: Option<Engine>,
    sing: Singer,
    mono: Vec<f32>,
    level: f32,
}

impl Default for Chimera {
    fn default() -> Self {
        Self { params: Arc::new(ChimeraParams::default()), shared: Arc::new(Shared::default()), engine: None, sing: Singer::new(), mono: Vec::new(), level: 0.0 }
    }
}

impl Chimera {
    fn handle(&mut self, ev: &NoteEvent<()>) {
        let Some(e) = self.engine.as_mut() else { return };
        match *ev {
            NoteEvent::NoteOn { note, velocity, .. } => self.sing.note_on(e, note, (velocity * 127.0) as f64),
            NoteEvent::NoteOff { note, .. } => self.sing.note_off(e, note),
            NoteEvent::Choke { .. } => self.sing.stop_all(e),
            NoteEvent::MidiCC { cc: 120 | 123, .. } => self.sing.stop_all(e),
            _ => {}
        }
    }

    /// Render `out` (mono), splitting the block at each event so notes start on their sample.
    fn render(&mut self, n: usize, context: &mut impl ProcessContext<Self>) {
        let mut at = 0;
        while let Some(ev) = context.next_event() {
            let t = (ev.timing() as usize).min(n);
            if t > at {
                let (e, out) = (self.engine.as_mut().unwrap(), &mut self.mono[at..t]);
                e.render(out);
                at = t;
            }
            self.handle(&ev);
        }
        if at < n {
            self.engine.as_mut().unwrap().render(&mut self.mono[at..n]);
        }
    }
}

impl Plugin for Chimera {
    const NAME: &'static str = "CHIMERA";
    const VENDOR: &'static str = "CHIMERA";
    const URL: &'static str = "https://github.com/jamesondh/chimera-vocal-synth";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout { main_input_channels: None, main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() },
        AudioIOLayout { main_input_channels: None, main_output_channels: NonZeroU32::new(1), ..AudioIOLayout::const_default() },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    #[cfg(target_os = "macos")]
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::WebEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        // everything that allocates happens here, off the audio thread
        let lyrics = self.params.lyrics.read().unwrap().clone();
        self.shared.set_lyrics(&self.params, &lyrics);
        if let Some(s) = self.shared.pending.lock().unwrap().take() {
            let _ = self.sing.set_syllables(syllables_from(&s));
        }
        // the airway goes unstable below MIN_SAMPLE_RATE (NaN at 22.05 kHz): stay silent there
        // rather than refusing to activate, which some hosts' plugin scans treat as an error
        self.engine = (config.sample_rate >= MIN_SAMPLE_RATE).then(|| Engine::new(config.sample_rate as f64));
        if self.engine.is_none() {
            nih_error!("CHIMERA needs a sample rate of at least {MIN_SAMPLE_RATE} Hz; it stays silent");
        }
        self.mono = vec![0.0; config.max_buffer_size as usize];
        self.shared.sample_rate.store(config.sample_rate);
        self.shared.block.store(config.max_buffer_size as i32, Ordering::Relaxed);
        self.shared.ui_notes.lock().unwrap().reserve(256);
        true
    }

    fn reset(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            self.sing.stop_all(e);
            e.stop_speech();
        }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let started = Instant::now();
        let n = buffer.samples();
        let out = buffer.as_slice();
        if self.shared.faulted.load(Ordering::Relaxed) || self.engine.is_none() || n > self.mono.len() {
            for ch in out.iter_mut() {
                ch.fill(0.0);
            }
            return ProcessStatus::KeepAlive;
        }

        let ok = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(mut p) = self.shared.pending.try_lock()
                && let Some(s) = p.take()
            {
                // spike: frees the old lyrics on the audio thread
                util::permit_alloc(|| drop(self.sing.set_syllables(syllables_from(&s))));
            }
            self.sing.vibrato = self.params.vibrato.value() as f64;
            let shared = self.shared.clone();
            if let Ok(mut q) = shared.ui_notes.try_lock() {
                for (note, on) in q.drain(..) {
                    let ev = if on { NoteEvent::NoteOn { timing: 0, voice_id: None, channel: 0, note, velocity: 96.0 / 127.0 } } else { NoteEvent::NoteOff { timing: 0, voice_id: None, channel: 0, note, velocity: 0.0 } };
                    self.handle(&ev);
                }
            }
            self.render(n, context);
        }));
        // a panic, or a non-finite sample (an unstable airway): silence, for good, rather than
        // sending NaN into the host's mix bus
        if ok.is_err() || !self.mono[..n].iter().all(|x| x.is_finite()) {
            self.shared.faulted.store(true, Ordering::Relaxed);
            for ch in out.iter_mut() {
                ch.fill(0.0);
            }
            return ProcessStatus::KeepAlive;
        }

        let mut sum = 0.0f32;
        for x in self.mono[..n].iter_mut() {
            *x *= util::db_to_gain_fast(self.params.gain.smoothed.next());
            sum += *x * *x;
        }
        for ch in out.iter_mut() {
            ch.copy_from_slice(&self.mono[..n]);
        }

        let rms = (sum / n.max(1) as f32).sqrt();
        self.level = rms.max(self.level * 0.9);
        self.shared.level.store(self.level);
        let busy = started.elapsed().as_nanos() as u64;
        let audio = (n as f64 / self.shared.sample_rate.load() as f64 * 1e9) as u64;
        self.shared.busy_ns.fetch_add(busy, Ordering::Relaxed);
        self.shared.audio_ns.fetch_add(audio, Ordering::Relaxed);
        let used = busy as f32 / audio.max(1) as f32;
        if used > self.shared.load_peak.load() {
            self.shared.load_peak.store(used);
        }
        self.shared.cursor.store(self.sing.cursor() as i32, Ordering::Relaxed);
        self.shared.sung.store(self.sing.sung().map_or(-1, |i| i as i32), Ordering::Relaxed);
        self.shared.note.store(self.sing.sounding_note().map_or(-1, |m| m as i32), Ordering::Relaxed);
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Chimera {
    const CLAP_ID: &'static str = "com.github.jamesondh.chimera";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Physical-modelling singing voice");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Mono, ClapFeature::Stereo];
}

nih_export_clap!(Chimera);
clap_wrapper::export_vst3!();
clap_wrapper::export_auv2!();

#[cfg(test)]
mod tests {
    use super::*;

    /// Sing a few notes through the controller at `fs`; returns the peak and whether all samples are finite.
    fn sing_at(fs: f64) -> (f32, bool) {
        let mut e = Engine::new(fs);
        let mut s = Singer::new();
        let _ = s.set_syllables(syllables_from(&lyrics_to_syllables("hello there", g2p())));
        let mut out = vec![0.0f32; (fs * 0.25) as usize];
        let (mut peak, mut finite) = (0.0f32, true);
        let mut run = |e: &mut Engine| {
            e.render(&mut out);
            for x in &out {
                finite &= x.is_finite();
                peak = peak.max(x.abs());
            }
        };
        for (i, midi) in [60u8, 64, 67].into_iter().enumerate() {
            s.note_on(&mut e, midi, 96.0);
            run(&mut e);
            run(&mut e);
            if i == 1 {
                s.note_on(&mut e, 72, 110.0); // legato
                run(&mut e);
                s.note_off(&mut e, 72);
            }
            s.note_off(&mut e, midi);
            run(&mut e);
        }
        (peak, finite)
    }

    #[test]
    fn sings_at_host_rates() {
        for fs in [44100.0, 48000.0, 88200.0, 96000.0, 192000.0] {
            let (peak, finite) = sing_at(fs);
            assert!(finite, "non-finite output at {fs} Hz");
            assert!(peak > 0.01, "silent at {fs} Hz (peak {peak})");
        }
    }

    #[test]
    #[ignore = "survey: which sample rates the engine survives"]
    fn rate_survey() {
        for fs in [1000.0, 8000.0, 11025.0, 16000.0, 22050.0, 32000.0, 44100.0, 384000.0, 768000.0] {
            println!("{fs:>8} Hz: {:?}", sing_at(fs));
        }
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore = "measurement: start-up and per-note costs"]
    fn costs() {
        let t = Instant::now();
        let _ = g2p();
        println!("dictionary parse: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let mut e = Engine::new(44100.0);
        println!("Engine::new: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
        let mut s = Singer::new();
        let _ = s.set_syllables(syllables_from(&lyrics_to_syllables(DEFAULT_LYRICS, g2p())));
        let mut buf = vec![0.0f32; 256];
        let block = 256.0 / 44100.0;
        let (mut worst, mut total, mut blocks, mut plan_worst) = (0.0f64, 0.0f64, 0, 0.0f64);
        for i in 0..400 {
            if i % 40 == 0 {
                let t = Instant::now();
                s.note_on(&mut e, 60 + (i / 40) as u8, 96.0);
                plan_worst = plan_worst.max(t.elapsed().as_secs_f64());
            }
            if i % 40 == 30 {
                let t = Instant::now();
                s.note_off(&mut e, 60 + (i / 40) as u8);
                plan_worst = plan_worst.max(t.elapsed().as_secs_f64());
            }
            let t = Instant::now();
            e.render(&mut buf);
            let d = t.elapsed().as_secs_f64();
            worst = worst.max(d);
            total += d;
            blocks += 1;
        }
        println!("render: mean {:.0}% worst {:.0}% of a 256-sample block; worst note on/off plan {:.2} ms ({:.0}% of a block)", 100.0 * total / blocks as f64 / block, 100.0 * worst / block, plan_worst * 1e3, 100.0 * plan_worst / block);
    }
}
