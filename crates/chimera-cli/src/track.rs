//! Replay control frames without quantizing pitch or timing to musical notes.

use crate::render::{TELE_HOP, TeleRecorder};
use crate::{Ctx, FS, arg, body, body_mix, peak, wav_name};
use chimera::anatomy::human::ArticPatch;
use chimera::voice::human_voice::{HumanVoice, SourcePatch};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Track {
    rate: usize,
    #[serde(default)]
    source: SourcePatch,
    #[serde(default)]
    phonation: Phonation,
    frames: Vec<Frame>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Phonation {
    diplophonia: Option<f64>,
    alternating_attenuation: Option<f64>,
    jitter: Option<f64>,
    shimmer: Option<f64>,
}

impl Phonation {
    fn apply(&self, voice: &mut HumanVoice) {
        let p = &mut voice.glottis.params;
        if let Some(x) = self.diplophonia { p.diplophonia = x; }
        if let Some(x) = self.alternating_attenuation { p.alternating_attenuation = x; }
        if let Some(x) = self.jitter { p.jitter = x; }
        if let Some(x) = self.shimmer { p.shimmer = x; }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Frame {
    f0: f64,
    gate: f64,
    #[serde(default)]
    artic: ArticPatch,
    #[serde(default = "default_tip_free")]
    tip_free: bool,
    alternating_attenuation: Option<f64>,
}

fn default_tip_free() -> bool { true }

impl Track {
    fn validate(&self) -> Result<(), String> {
        for (name, value, max) in [
            ("diplophonia", self.phonation.diplophonia, 1.0),
            ("alternatingAttenuation", self.phonation.alternating_attenuation, 1.0),
            ("jitter", self.phonation.jitter, 0.1),
            ("shimmer", self.phonation.shimmer, 1.0),
        ] {
            if let Some(x) = value && !(0.0..=max).contains(&x) {
                return Err(format!("phonation {name} must be 0..{max}"));
            }
        }
        if self.rate == 0 || self.rate > 1000 || FS as usize % self.rate != 0 {
            return Err("track rate must be 1..1000 Hz and divide 48000 exactly".into());
        }
        if self.frames.is_empty() || self.frames.len() > self.rate * 600 {
            return Err("track must contain frames for at most 600 seconds".into());
        }
        for (i, frame) in self.frames.iter().enumerate() {
            if !(20.0..=2000.0).contains(&frame.f0) || !(0.0..=1.5).contains(&frame.gate) {
                return Err(format!("frame {i}: f0 must be 20..2000 Hz and gate must be 0..1.5"));
            }
            if let Some(x) = frame.alternating_attenuation && !(0.0..=1.0).contains(&x) {
                return Err(format!("frame {i}: alternatingAttenuation must be 0..1"));
            }
        }
        Ok(())
    }
}

pub fn render(ctx: &Ctx, args: &[String]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut tele = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--tele" {
            tele = Some(it.next().ok_or("--tele needs a file")?);
        } else if a.starts_with("--") {
            return Err(format!("unknown track option {a}"));
        } else {
            positional.push(a.clone());
        }
    }
    if positional.len() < 2 || positional.len() > 3 {
        return Err("render track <in.json> <out> [male|female|child] [--tele voice.tele]".into());
    }
    let input = &positional[0];
    let body_name = arg(&positional, 2, "male");
    let text = std::fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let track: Track = serde_json::from_str(&text).map_err(|e| format!("{input}: {e}"))?;
    track.validate()?;
    let mut voice = HumanVoice::new(FS, body(body_name)?, 2);
    voice.set_source(&track.source);
    let mut recorder = tele.map(|_| TeleRecorder::new(&mut voice));
    let hop = FS as usize / track.rate;
    let mut out = vec![0.0; track.frames.len() * hop];
    let mut next_tele = TELE_HOP;
    for (i, frame) in track.frames.iter().enumerate() {
        voice.set_source(&SourcePatch::f0(frame.f0));
        // set_source derives these from roughness, so reapply the independent controls.
        track.phonation.apply(&mut voice);
        voice.glottis.params.alternating_attenuation = frame.alternating_attenuation
            .or(track.phonation.alternating_attenuation).unwrap_or(0.0);
        voice.set_gate(frame.gate);
        voice.set_articulation(&frame.artic, Some(frame.tip_free));
        let mut pos = i * hop;
        let end = pos + hop;
        while pos < end {
            let stop = if recorder.is_some() { end.min(next_tele) } else { end };
            for block in out[pos..stop].chunks_mut(128) {
                voice.render(block);
            }
            pos = stop;
            if let Some(r) = &mut recorder
                && pos == next_tele
            {
                r.snap(&mut voice)?;
                next_tele += TELE_HOP;
            }
        }
    }
    if out.iter().any(|x| !x.is_finite()) {
        return Err("track produced non-finite audio".into());
    }
    let output = wav_name(&positional[1]);
    ctx.write_wav(&output, &out)?;
    if let (Some(path), Some(r)) = (tele, recorder) {
        std::fs::write(path, r.bytes(body_mix(body_name)?)).map_err(|e| format!("{path}: {e}"))?;
    }
    println!("{output}: {:.2} s, peak {:.3}, {} control frames at {} Hz",
        out.len() as f64 / FS, peak(&out), track.frames.len(), track.rate);
    Ok(())
}
