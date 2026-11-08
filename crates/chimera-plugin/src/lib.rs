//! CHIMERA as an audio plugin: MIDI notes sing the lyrics, one syllable per note. nih-plug makes it a
//! CLAP plugin and `clap-wrapper` adds VST3 and AUv2 to the same binary. On macOS the editor is a
//! system webview (WKWebView) showing the React UI. See docs/plugin.md.

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera::voice::human_voice::DEFAULT_SOURCE;
use nih_plug::prelude::*;

pub mod bridge;
pub mod core;
#[cfg(target_os = "macos")]
mod editor;

use self::core::{Core, Event, MIN_SAMPLE_RATE, Settings, Shared, Transport, VoiceParams};

/// The pronouncing dictionary, parsed once (off the audio thread).
pub fn g2p() -> &'static EnglishG2P {
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

/// The host parameters. Their IDs are permanent once released: saved projects refer to them.
#[derive(Params)]
pub struct ChimeraParams {
    #[id = "gain"]
    pub gain: FloatParam,
    /// vibrato depth on held vowels (cents)
    #[id = "vibrato"]
    pub vibrato: FloatParam,
    #[id = "vibrato_rate"]
    pub vibrato_rate: FloatParam,
    /// lung pressure (cmH2O)
    #[id = "breath"]
    pub breath: FloatParam,
    #[id = "pressed"]
    pub pressed: FloatParam,
    #[id = "bright"]
    pub bright: FloatParam,
    #[id = "breathy"]
    pub breathy: FloatParam,
    #[id = "rough"]
    pub rough: FloatParam,
    #[id = "fry"]
    pub fry: FloatParam,
    #[id = "glide"]
    pub glide: FloatParam,
    /// the singer's ring: the tube narrowed just above the folds
    #[id = "ring"]
    pub ring: FloatParam,
    /// transport start or a loop puts the lyrics back at the start syllable
    #[id = "restart"]
    pub restart: BoolParam,
    #[persist = "settings"]
    pub settings: RwLock<Settings>,
}

fn percent(name: &str, default: f64) -> FloatParam {
    FloatParam::new(name, default as f32, FloatRange::Linear { min: 0.0, max: 1.0 }).with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for ChimeraParams {
    fn default() -> Self {
        let s = DEFAULT_SOURCE;
        Self {
            gain: FloatParam::new("Gain", 0.0, FloatRange::Linear { min: -30.0, max: 12.0 }).with_smoother(SmoothingStyle::Linear(20.0)).with_step_size(0.1).with_unit(" dB"),
            vibrato: FloatParam::new("Vibrato", 35.0, FloatRange::Linear { min: 0.0, max: 120.0 }).with_step_size(1.0).with_unit(" ct"),
            vibrato_rate: FloatParam::new("Vibrato Rate", s.vibrato_rate as f32, FloatRange::Linear { min: 3.5, max: 7.5 }).with_step_size(0.1).with_unit(" Hz"),
            breath: FloatParam::new("Breath", s.pressure as f32, FloatRange::Linear { min: 2.0, max: 22.0 }).with_step_size(0.1).with_unit(" cmH2O"),
            pressed: percent("Pressed", s.tension),
            bright: percent("Bright", s.brightness),
            breathy: percent("Breathy", s.breathiness),
            rough: percent("Rough", s.roughness),
            fry: percent("Fry", s.fry),
            glide: FloatParam::new("Glide", s.glide as f32, FloatRange::Skewed { min: 0.005, max: 0.6, factor: FloatRange::skew_factor(-2.0) }).with_unit(" ms").with_value_to_string(Arc::new(|v| format!("{:.0}", v * 1000.0))).with_string_to_value(Arc::new(|t| t.trim().trim_end_matches("ms").trim().parse::<f32>().ok().map(|v| v / 1000.0))),
            ring: percent("Ring", 0.0),
            restart: BoolParam::new("Restart Lyrics On Play", true),
            settings: RwLock::new(Settings::default()),
        }
    }
}

impl ChimeraParams {
    fn voice(&self) -> VoiceParams {
        VoiceParams {
            vibrato: self.vibrato.value(),
            vibrato_rate: self.vibrato_rate.value(),
            breath: self.breath.value(),
            pressed: self.pressed.value(),
            bright: self.bright.value(),
            breathy: self.breathy.value(),
            rough: self.rough.value(),
            fry: self.fry.value(),
            glide: self.glide.value(),
            ring: self.ring.value(),
            restart: self.restart.value(),
        }
    }
}

pub struct Chimera {
    params: Arc<ChimeraParams>,
    shared: Arc<Shared>,
    /// None until active, and below [`MIN_SAMPLE_RATE`]
    core: Option<Core>,
    mono: Vec<f32>,
}

impl Default for Chimera {
    fn default() -> Self {
        Self { params: Arc::new(ChimeraParams::default()), shared: Arc::new(Shared::default()), core: None, mono: Vec::new() }
    }
}

/// A host note event as the core sees it.
fn core_event(ev: &NoteEvent<()>) -> Option<Event> {
    Some(match *ev {
        NoteEvent::NoteOn { note, velocity, .. } => Event::NoteOn { midi: note, velocity: (velocity * 127.0) as f64 },
        NoteEvent::NoteOff { note, .. } => Event::NoteOff { midi: note },
        NoteEvent::Choke { .. } | NoteEvent::MidiCC { cc: 120 | 123, .. } => Event::AllOff,
        NoteEvent::MidiPitchBend { value, .. } => Event::Bend(value),
        _ => return None,
    })
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

    // Runs where the host activates or restores, with `process` held off by nih-plug's plugin lock.
    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        let fs = config.sample_rate as f64;
        let settings = self.params.settings.read().unwrap_or_else(|e| e.into_inner()).clone();
        let syllables = lyrics_to_syllables(&settings.lyrics, g2p());
        self.shared.restart(fs);
        // below this the airway goes unstable: stay silent rather than refusing to activate, which
        // some hosts' plugin scans treat as an error
        self.core = (fs >= MIN_SAMPLE_RATE).then(|| Core::new(fs, &settings, &syllables, self.params.voice(), &self.shared));
        if self.core.is_none() {
            nih_error!("CHIMERA needs a sample rate of at least {MIN_SAMPLE_RATE} Hz; it stays silent");
        }
        *self.shared.syllables.lock().unwrap_or_else(|e| e.into_inner()) = syllables;
        self.shared.syllables_version.fetch_add(1, Ordering::Relaxed);
        self.mono = vec![0.0; config.max_buffer_size as usize];
        true
    }

    fn reset(&mut self) {
        if let Some(core) = self.core.as_mut() {
            core.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let started = Instant::now();
        let n = buffer.samples();
        let out = buffer.as_slice();
        let (shared, params) = (self.shared.clone(), self.params.clone());
        let (Some(core), false, true) = (self.core.as_mut(), shared.faulted.load(Ordering::Relaxed), n <= self.mono.len()) else {
            for ch in out.iter_mut() {
                ch.fill(0.0);
            }
            return ProcessStatus::KeepAlive;
        };
        let mono = &mut self.mono[..n];

        let ok = catch_unwind(AssertUnwindSafe(|| {
            let t = context.transport();
            let transport = Transport { playing: t.playing, position: t.pos_beats().or_else(|| t.pos_samples().map(|s| s as f64)) };
            core.begin_block(&params.voice(), transport, &shared);
            // split the block at each event, so notes start on their sample
            let mut at = 0;
            while let Some(ev) = context.next_event() {
                let t = (ev.timing() as usize).min(n);
                if t > at {
                    core.render(&mut mono[at..t]);
                    at = t;
                }
                if let Some(e) = core_event(&ev) {
                    core.event(e);
                }
            }
            if at < n {
                core.render(&mut mono[at..n]);
            }
            core.end_block(n, &shared);
        }));
        // a panic, or a non-finite sample (an unstable airway): silence, for good, rather than
        // sending NaN into the host's mix bus
        if ok.is_err() || !mono.iter().all(|x| x.is_finite()) {
            shared.faulted.store(true, Ordering::Relaxed);
            for ch in out.iter_mut() {
                ch.fill(0.0);
            }
            return ProcessStatus::KeepAlive;
        }

        for x in mono.iter_mut() {
            *x *= util::db_to_gain_fast(params.gain.smoothed.next());
        }
        for ch in out.iter_mut() {
            ch.copy_from_slice(mono);
        }
        shared.note_load(started.elapsed().as_nanos() as u64, n);
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
