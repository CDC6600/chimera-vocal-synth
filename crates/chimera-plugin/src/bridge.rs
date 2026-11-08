//! What the page can ask of the plugin, with no webview in it: the editor serves this through a
//! custom protocol, and `examples/ui_harness.rs` over HTTP. Everything here runs on the main thread.
//! See docs/plugin.md, "The bridge".

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use chimera::anatomy::human::{ArticPatch, BodyMix};
use chimera::engine::{Engine, default_f0};
use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera::voice::human_voice::{SourcePatch, TELE_LABELS};
use nih_plug::params::internals::ParamPtr;
use nih_plug::prelude::{GuiContext, Params};
use serde::Deserialize;

use crate::core::{MIN_SAMPLE_RATE, Mode, Shared, UiNote};
use crate::{ChimeraParams, g2p};

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

const NOT_BUILT: &str = "<!doctype html><body style='background:#000;color:#fff;font:14px sans-serif;padding:2em'>The UI wasn't built into this plugin. Run <code>bun run build</code> before building it.</body>";

/// Scalars at the head of a `/poll` response (f64, little-endian), in order: revision,
/// syllablesVersion, cursor, sung, note, noteSerial, busyNs, audioNs, loadPeak, faulted, playing, the
/// telemetry's female, child, and the number of telemetry values. Those follow (see `TELE_HEADER`),
/// then the `TELE_LABELS` bytes of active labels. Keep in step with `src/ui/pluginState.ts`.
pub const POLL_HEADER: usize = 14;

/// The editor window's first size, and the limits of its grip (points).
pub const WINDOW_DEFAULT: (u32, u32) = (1280, 800);
pub const WINDOW_MIN: (u32, u32) = (720, 500);
pub const WINDOW_MAX: (u32, u32) = (3840, 2160);
/// The smallest viewport the page's layout fits without scrolling. A window smaller than this shows
/// the same layout scaled down. Checked by `tools/layout-lint.ts --plugin`.
pub const LAYOUT_MIN: (u32, u32) = (1120, 800);

/// The page zoom for a window of this size.
pub fn page_zoom((width, height): (u32, u32)) -> f64 {
    (width as f64 / LAYOUT_MIN.0 as f64).min(height as f64 / LAYOUT_MIN.1 as f64).min(1.0)
}

/// What the window's grip did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GripAction {
    Begin,
    /// the pointer moved: the editor reads where to
    Move,
    End,
    /// back to the first size
    Reset,
}

/// How the page's parameter changes and the window's grip reach the host, and the parameters' values.
pub trait ParamHost: Send + Sync {
    fn begin(&self, id: &str);
    fn set(&self, id: &str, plain: f32);
    fn end(&self, id: &str);
    /// Every parameter's plain value (a bool as 0 or 1).
    fn values(&self) -> Vec<(String, f32)>;
    /// The editor needs the host's view for this: see `editor.rs`.
    fn grip(&self, _action: GripAction) {}
    /// Called for every poll, on the main thread.
    fn tick(&self) {}
}

/// [`ParamHost`] through nih-plug's connection to the host.
pub struct GuiHost {
    pub context: Arc<dyn GuiContext>,
    map: HashMap<String, ParamPtr>,
    /// `map`'s pointers point into this
    _params: Arc<ChimeraParams>,
}

impl GuiHost {
    pub fn new(params: Arc<ChimeraParams>, context: Arc<dyn GuiContext>) -> GuiHost {
        let map = params.param_map().into_iter().map(|(id, ptr, _group)| (id, ptr)).collect();
        GuiHost { context, map, _params: params }
    }
}

impl ParamHost for GuiHost {
    fn begin(&self, id: &str) {
        if let Some(p) = self.map.get(id) {
            // SAFETY: `_params` keeps the parameter alive
            unsafe { self.context.raw_begin_set_parameter(*p) };
        }
    }
    fn set(&self, id: &str, plain: f32) {
        if let Some(p) = self.map.get(id) {
            // SAFETY: `_params` keeps the parameter alive
            unsafe { self.context.raw_set_parameter_normalized(*p, p.preview_normalized(plain)) };
        }
    }
    fn end(&self, id: &str) {
        if let Some(p) = self.map.get(id) {
            // SAFETY: `_params` keeps the parameter alive
            unsafe { self.context.raw_end_set_parameter(*p) };
        }
    }
    fn values(&self) -> Vec<(String, f32)> {
        // SAFETY: `_params` keeps the parameters alive
        self.map.iter().map(|(id, p)| (id.clone(), unsafe { p.unmodulated_plain_value() })).collect()
    }
}

/// A command from the page, and the state revision the page had when it sent it: a command from
/// before a restore was meant for a project that's gone.
#[derive(Debug, Deserialize)]
struct Envelope {
    rev: u32,
    #[serde(flatten)]
    command: Command,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Command {
    Note { midi: u8, on: bool, velocity: Option<f64> },
    Lyrics { text: String },
    Cursor { index: usize },
    Body { body: BodyMix },
    Mode { mode: Mode },
    Artic { values: ArticPatch, #[serde(rename = "tipFree")] tip_free: Option<bool> },
    /// the voice settings that aren't host parameters (`voiced`, `f0`)
    Source { values: SourcePatch },
    Hold { on: bool },
    /// a host parameter moved (the gesture begins with its first value)
    Param { id: String, value: f32 },
    ParamEnd { id: String },
    Grip { action: GripAction },
}

/// A response to a request.
pub struct Reply {
    pub status: u16,
    pub mime: &'static str,
    pub body: Cow<'static, [u8]>,
}

impl Reply {
    fn json(value: &impl serde::Serialize) -> Reply {
        Reply { status: 200, mime: "application/json", body: Cow::Owned(serde_json::to_vec(value).unwrap_or_default()) }
    }
}

pub struct Bridge {
    params: Arc<ChimeraParams>,
    shared: Arc<Shared>,
    host: Box<dyn ParamHost>,
    /// parameters with a gesture open: ended when the window closes, because the page's timer dies
    /// with it and clap-wrapper's VST3 layer ignores automation on a parameter whose gesture is open
    open_gestures: Mutex<Vec<String>>,
}

impl Bridge {
    pub fn new(params: Arc<ChimeraParams>, shared: Arc<Shared>, host: Box<dyn ParamHost>) -> Bridge {
        Bridge { params, shared, host, open_gestures: Mutex::new(Vec::new()) }
    }

    /// Answer a request for this path.
    pub fn request(&self, path: &str) -> Reply {
        match path {
            "/poll" => Reply { status: 200, mime: "application/octet-stream", body: Cow::Owned(self.poll()) },
            "/state" => self.state(),
            "/params" => Reply::json(&self.param_values()),
            "/syllables" => Reply::json(&*self.shared.syllables.lock().unwrap_or_else(|e| e.into_inner())),
            _ => Self::asset(path),
        }
    }

    fn asset(path: &str) -> Reply {
        let path = if path == "/" { "/index.html" } else { path };
        match ASSETS.iter().find(|(p, _, _)| *p == path) {
            Some((_, mime, bytes)) => Reply { status: 200, mime, body: Cow::Borrowed(bytes) },
            None if ASSETS.is_empty() => Reply { status: 200, mime: "text/html; charset=utf-8", body: Cow::Borrowed(NOT_BUILT.as_bytes()) },
            None => Reply { status: 404, mime: "text/plain; charset=utf-8", body: Cow::Borrowed(b"not found") },
        }
    }

    fn param_values(&self) -> HashMap<String, f32> {
        self.host.values().into_iter().collect()
    }

    /// The whole state, for the page to start from.
    fn state(&self) -> Reply {
        let s = &self.shared;
        // read the revision first: a restore during this reply then shows as a change to read again
        let revision = s.revision.load(Ordering::Relaxed);
        let version = s.syllables_version.load(Ordering::Relaxed);
        Reply::json(&serde_json::json!({
            "revision": revision,
            "syllablesVersion": version,
            "settings": *self.params.settings.read().unwrap_or_else(|e| e.into_inner()),
            "params": self.param_values(),
            "syllables": *s.syllables.lock().unwrap_or_else(|e| e.into_inner()),
            "cursor": s.cursor.load(Ordering::Relaxed),
            "sampleRate": s.sample_rate(),
        }))
    }

    fn poll(&self) -> Vec<u8> {
        self.host.tick();
        let s = &self.shared;
        s.watch();
        let mut frame = s.telemetry.lock().unwrap_or_else(|e| e.into_inner());
        let n = if frame.fresh { frame.block.len() } else { 0 };
        let mut out = Vec::with_capacity(8 * (POLL_HEADER + n) + TELE_LABELS);
        let header: [f64; POLL_HEADER] = [
            s.revision.load(Ordering::Relaxed) as f64,
            s.syllables_version.load(Ordering::Relaxed) as f64,
            s.cursor.load(Ordering::Relaxed) as f64,
            s.sung.load(Ordering::Relaxed) as f64,
            s.note.load(Ordering::Relaxed) as f64,
            s.note_serial.load(Ordering::Relaxed) as f64,
            s.busy_ns.swap(0, Ordering::Relaxed) as f64,
            s.audio_ns.swap(0, Ordering::Relaxed) as f64,
            s.load_peak.swap(0.0) as f64,
            s.faulted.load(Ordering::Relaxed) as u8 as f64,
            s.playing.load(Ordering::Relaxed) as u8 as f64,
            frame.body.female,
            frame.body.child,
            n as f64,
        ];
        for x in header {
            out.extend_from_slice(&x.to_le_bytes());
        }
        if n > 0 {
            for x in &frame.block {
                out.extend_from_slice(&x.to_le_bytes());
            }
            out.extend_from_slice(&frame.labels);
            frame.fresh = false;
        }
        out
    }

    /// A command from the page (JSON). One that doesn't parse is ignored.
    pub fn command(&self, json: &str) {
        let Ok(Envelope { rev, command }) = serde_json::from_str::<Envelope>(json) else { return };
        let current = rev == self.shared.revision.load(Ordering::Relaxed);
        match command {
            // the host's, so outside the transaction; an open gesture must end even if stale
            Command::Param { id, value } => {
                if current {
                    let mut open = self.open_gestures.lock().unwrap_or_else(|e| e.into_inner());
                    if !open.contains(&id) {
                        self.host.begin(&id);
                        open.push(id.clone());
                    }
                    drop(open);
                    self.host.set(&id, value);
                }
            }
            Command::ParamEnd { id } => {
                let mut open = self.open_gestures.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(i) = open.iter().position(|x| *x == id) {
                    open.remove(i);
                    drop(open);
                    self.host.end(&id);
                }
            }
            // the host's too; it isn't part of a project, so a restore doesn't make it stale
            Command::Grip { action } => self.host.grip(action),
            command => {
                let _together = self.shared.transaction();
                // checked again now that no restore can be under way
                if rev == self.shared.revision.load(Ordering::Relaxed) {
                    self.apply(command);
                }
            }
        }
    }

    /// A command that changes the project: its settings and what the audio thread takes next.
    fn apply(&self, cmd: Command) {
        let shared = &self.shared;
        let settings = || self.params.settings.write().unwrap_or_else(|e| e.into_inner());
        match cmd {
            Command::Note { midi, on, velocity } => shared.ui_note(UiNote { midi: midi.min(127), velocity: velocity.unwrap_or(96.0), on }),
            Command::Lyrics { text } => {
                let syllables = lyrics_to_syllables(&text, g2p());
                {
                    let mut s = settings();
                    s.lyrics = text;
                    s.start_syllable = 0;
                }
                shared.send_lyrics(syllables);
            }
            Command::Cursor { index } => {
                if !shared.playing.load(Ordering::Relaxed) {
                    settings().start_syllable = index;
                }
                shared.edit(|p| p.cursor = Some(index));
            }
            Command::Body { body } => {
                let body = BodyMix::new(body.female, body.child);
                {
                    let mut s = settings();
                    s.body = body;
                    s.f0 = default_f0(body);
                }
                let fs = shared.sample_rate();
                if fs >= MIN_SAMPLE_RATE {
                    shared.send_voice(Engine::prepare_voice(fs, body), body);
                }
            }
            Command::Mode { mode } => {
                settings().mode = mode;
                shared.edit(|p| p.mode = Some(mode));
            }
            Command::Artic { values, tip_free } => {
                {
                    let mut s = settings();
                    s.artic.merge(&values);
                    if let Some(t) = tip_free {
                        s.tip_free = t;
                    }
                }
                shared.edit(|p| {
                    p.artic.merge(&values);
                    if tip_free.is_some() {
                        p.tip_free = tip_free;
                    }
                });
            }
            Command::Source { values } => {
                {
                    let mut s = settings();
                    if let Some(f) = values.f0 {
                        s.f0 = f;
                    }
                    if let Some(v) = values.voiced {
                        s.voiced = v;
                    }
                }
                shared.edit(|p| p.source.merge(&values));
            }
            Command::Hold { on } => shared.edit(|p| p.hold = Some(on)),
            Command::Param { .. } | Command::ParamEnd { .. } | Command::Grip { .. } => {}
        }
    }

    /// The window closed: end what the page can no longer end.
    pub fn close(&self) {
        for id in std::mem::take(&mut *self.open_gestures.lock().unwrap_or_else(|e| e.into_inner())) {
            self.host.end(&id);
        }
        self.shared.clear_ui_keys();
    }
}
