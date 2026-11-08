//! What a note says in Speak mode: the text's lines, planned ahead on the main thread. The audio
//! thread only copies a line's gestures into the score. See docs/plugin.md, "Speak".

use chimera::anatomy::human::BodyMix;
use chimera::engine::default_f0;
use chimera::phonetics::english::{EnglishG2P, Phrase};
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::planner::{PhraseInput, PlanOptions, plan_phrases};
use chimera::phonetics::{Intonation, Plan};
use serde::{Deserialize, Serialize};

/// The text to say and how, as a project saves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpeechSettings {
    pub text: String,
    /// the text is IPA
    pub ipa: bool,
    pub rate: f64,
    /// where the lines restart when the transport starts
    pub start_line: usize,
}

impl Default for SpeechSettings {
    fn default() -> Self {
        SpeechSettings { text: "Hello. Can you hear me?".to_string(), ipa: false, rate: 1.0, start_line: 0 }
    }
}

/// One line, planned, with what the page needs to draw it.
#[derive(Clone, Debug, Serialize)]
pub struct SpokenLine {
    pub text: String,
    pub phrases: Vec<Phrase>,
    pub plan: Plan,
}

/// The same rules as `ipaToPhrases` on the website: `|`, `.`, `?` and `!` separate phrases.
fn ipa_phrases(text: &str) -> Vec<Phrase> {
    text.split(['|', '.', '?', '!']).map(str::trim).filter(|p| !p.is_empty()).map(|p| Phrase { segments: tokenize_ipa(p), intonation: Intonation::Statement, pause: 0.3, words: Vec::new() }).collect()
}

fn plan(text: &str, phrases: Vec<Phrase>, body: BodyMix, rate: f64) -> SpokenLine {
    let input: Vec<PhraseInput> = phrases.iter().map(|p| PhraseInput { segments: p.segments.clone(), intonation: Some(p.intonation), pause: Some(p.pause) }).collect();
    let plan = plan_phrases(&input, &PlanOptions { body, t0: 0.0, rate: Some(rate), f0: Some(default_f0(body)), ..Default::default() });
    SpokenLine { text: text.to_string(), phrases, plan }
}

/// Every line of the text with something to say. Allocates: not on the audio thread.
pub fn plan_lines(s: &SpeechSettings, body: BodyMix, g2p: &EnglishG2P) -> Vec<SpokenLine> {
    let mut lines = Vec::new();
    for line in s.text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let phrases = if s.ipa { ipa_phrases(line) } else { g2p.to_phrases(line) };
        if !phrases.is_empty() {
            lines.push(plan(line, phrases, body, s.rate));
        }
    }
    lines
}

/// One sound from the consonant chart, in flat intonation, as on the website.
pub fn plan_pad(ipa: &str, body: BodyMix, rate: f64) -> SpokenLine {
    plan(ipa, vec![Phrase { segments: tokenize_ipa(ipa), intonation: Intonation::Flat, pause: 0.25, words: Vec::new() }], body, rate)
}
