//! The plugin's UI without a DAW: the same [`Bridge`] over HTTP, with a real [`Core`] running on a
//! timer (its audio goes nowhere). This is how the UI gets tested on a machine with no Mac and no
//! host: see `tools/plugin-e2e.ts`. Run `bun run build` first, then
//! `cargo run -p chimera-plugin --release --example ui_harness -- 8549`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chimera::phonetics::syllables::lyrics_to_syllables;
use chimera_plugin::ChimeraParams;
use chimera_plugin::bridge::{Bridge, GripAction, ParamHost};
use chimera_plugin::core::{Core, Event, Settings, Shared, Transport, VoiceParams};
use chimera_plugin::g2p;
use nih_plug::prelude::Params;

/// Stands in for a host: parameter values, kept here, and a log the test can read.
struct FakeHost {
    values: Arc<Mutex<HashMap<String, f32>>>,
    /// what the host saw, as "begin id", "end id" or "grip Move"
    log: Arc<Mutex<Vec<String>>>,
}

impl ParamHost for FakeHost {
    fn begin(&self, id: &str) {
        self.log.lock().unwrap().push(format!("begin {id}"));
    }
    fn set(&self, id: &str, plain: f32) {
        self.values.lock().unwrap().insert(id.to_string(), plain);
    }
    fn end(&self, id: &str) {
        self.log.lock().unwrap().push(format!("end {id}"));
    }
    fn values(&self) -> Vec<(String, f32)> {
        self.values.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
    fn grip(&self, action: GripAction) {
        self.log.lock().unwrap().push(format!("grip {action:?}"));
    }
}

fn voice_params(v: &HashMap<String, f32>) -> VoiceParams {
    let g = |id: &str| v.get(id).copied().unwrap_or(0.0);
    VoiceParams { vibrato: g("vibrato"), vibrato_rate: g("vibrato_rate"), breath: g("breath"), pressed: g("pressed"), bright: g("bright"), breathy: g("breathy"), rough: g("rough"), fry: g("fry"), glide: g("glide"), ring: g("ring"), restart: g("restart") > 0.5 }
}

/// Makes the page think it's in the plugin: `window.ipc` is what wry defines there.
const IPC_SHIM: &str = "<script>window.ipc={postMessage:(m)=>fetch('/ipc',{method:'POST',body:m,keepalive:true})}</script>";

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "8549".into());
    let fs = 44100.0;
    let params = Arc::new(ChimeraParams::default());
    let shared = Arc::new(Shared::default());
    // the parameters' defaults, as a host would report them
    // SAFETY: `params` outlives the map
    let defaults: HashMap<String, f32> = params.param_map().into_iter().map(|(id, p, _)| (id, unsafe { p.unmodulated_plain_value() })).collect();
    let log = Arc::new(Mutex::new(Vec::new()));
    let values = Arc::new(Mutex::new(defaults));
    let host = FakeHost { values: values.clone(), log: log.clone() };
    let initial = values.lock().unwrap().clone();

    let settings = Settings::default();
    let syllables = lyrics_to_syllables(&settings.lyrics, g2p());
    shared.restart(fs);
    *shared.syllables.lock().unwrap() = syllables.clone();
    let mut core = Core::new(fs, &settings, &syllables, voice_params(&initial), &shared);
    let bridge = Arc::new(Bridge::new(params.clone(), shared.clone(), Box::new(host)));
    // a project the test restores, for the audio thread to come back from as `initialize` does
    let restore: Arc<Mutex<Option<Settings>>> = Arc::new(Mutex::new(None));

    // host-side notes and automation the test sends in, and whether the transport is playing
    let host_events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let playing = Arc::new(AtomicBool::new(false));

    // the audio thread: blocks of 256, in real time
    {
        let (shared, values, host_events, playing, restore, params) = (shared.clone(), values.clone(), host_events.clone(), playing.clone(), restore.clone(), params.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0.0f32; 256];
            let block = Duration::from_secs_f64(buf.len() as f64 / fs);
            let mut next = Instant::now();
            let mut position = 0.0;
            loop {
                let started = Instant::now();
                let voice = voice_params(&values.lock().unwrap());
                if let Some(s) = restore.lock().unwrap().take() {
                    let syllables = lyrics_to_syllables(&s.lyrics, g2p());
                    shared.restart(fs);
                    core = Core::new(fs, &s, &syllables, voice, &shared);
                    *shared.syllables.lock().unwrap() = syllables;
                    shared.syllables_version.fetch_add(1, Ordering::Relaxed);
                    *params.settings.write().unwrap() = s;
                }
                let on = playing.load(Ordering::Relaxed);
                position = if on { position + 0.01 } else { 0.0 };
                core.begin_block(&voice, Transport { playing: on, position: Some(position) }, &shared);
                for e in host_events.lock().unwrap().drain(..) {
                    core.event(e);
                }
                core.render(&mut buf);
                core.end_block(buf.len(), &shared);
                shared.note_load(started.elapsed().as_nanos() as u64, buf.len());
                next += block;
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        });
    }

    let server = tiny_http::Server::http(format!("127.0.0.1:{port}")).expect("port in use?");
    println!("ui harness on http://127.0.0.1:{port}/");
    for mut req in server.incoming_requests() {
        let url = req.url().split('?').next().unwrap_or("/").to_string();
        match url.as_str() {
            "/ipc" => {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                bridge.command(&body);
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            // the test acting as the host
            "/test/note" | "/test/off" | "/test/bend" => {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let x: f64 = body.trim().parse().unwrap_or(60.0);
                let ev = match url.as_str() {
                    "/test/note" => Event::NoteOn { midi: x as u8, velocity: 96.0 },
                    "/test/off" => Event::NoteOff { midi: x as u8 },
                    _ => Event::Bend(x as f32),
                };
                host_events.lock().unwrap().push(ev);
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            "/test/play" | "/test/stop" => {
                playing.store(url == "/test/play", Ordering::Relaxed);
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            "/test/param" => {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                if let Some((id, v)) = body.split_once('=') {
                    values.lock().unwrap().insert(id.to_string(), v.parse().unwrap_or(0.0));
                }
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            "/test/restore" => {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                *restore.lock().unwrap() = serde_json::from_str(&body).ok();
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            "/test/gestures" => {
                let text = log.lock().unwrap().join("\n");
                let _ = req.respond(tiny_http::Response::from_string(text));
            }
            "/test/close" => {
                bridge.close();
                let _ = req.respond(tiny_http::Response::empty(204));
            }
            _ => {
                let reply = bridge.request(&url);
                let mime = tiny_http::Header::from_bytes(&b"Content-Type"[..], reply.mime.as_bytes()).unwrap();
                let mut body = reply.body.into_owned();
                if url == "/" {
                    body = String::from_utf8_lossy(&body).replacen("<head>", &format!("<head>{IPC_SHIM}"), 1).into_bytes();
                }
                let _ = req.respond(tiny_http::Response::from_data(body).with_status_code(reply.status).with_header(mime));
            }
        }
    }
}
