//! Singing: syllables on notes, through the same gestural planner as speech.
//!  - A note's vowel lands on the beat. Its onset consonants are sung just before it (they take
//!    their time from the previous note, as singers do), its coda just before the next onset.
//!  - Diphthongs sustain their first vowel and glide to the second at the end of the note.
//!  - A melisma ("-") carries the vowel onto further notes; the coda waits for the last of them.
//!  - Vibrato blooms a quarter of a second into a held vowel.
//!
//! Live playing uses open-ended notes: a key press sings the onset and starts a vowel that lasts
//! until the key is released (tagged gestures, ended with GestureScore.release), and the release
//! sings the glide and the coda.
//!
//! [`plan_song`] allocates and runs off the audio thread. The live planners come in two forms:
//! `*_into` over a [`LiveSyllable`] allocates nothing once its `Vec` has capacity (the plugin's
//! audio thread); the plain ones over a [`Syllable`] return a `Vec` (the browser's JSON API).

use super::inventory::phoneme;
use crate::anatomy::human::BodyMix;
use super::planner::{PlanOptions, TimedSegment, diphthong_parts, plan_timed, plan_utterance};
use super::{Intonation, Plan, PlannedSegment, Segment, Syllable};
use crate::control::score::{ControlKey, Gesture, Label, Targets};
use crate::math::{js_max, js_min};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// `440 · 2^((m − 69)/12)`, the same formula and operation order as `sing.ts`.
pub use crate::math::midi_to_hz;

/// sung consonants are a little shorter than spoken ones
fn cdur(ipa: &str, body: BodyMix, k: f64) -> f64 {
    phoneme(ipa, body).map_or(0.07, |d| d.dur) * k
}

fn consonants(segs: &[Segment], body: BodyMix, k: f64) -> Vec<Segment> {
    segs.iter().map(|c| Segment { ipa: c.ipa.clone(), dur: Some(cdur(&c.ipa, body, k)), ..Default::default() }).collect()
}

/// Sung consonants at their note's pitch: an onset consonant moves the pitch to the new note
/// before its vowel (as singers do), a coda holds the old one.
fn consonants_at(segs: &[Segment], body: BodyMix, k: f64, hz: f64) -> Vec<Segment> {
    let mut out = consonants(segs, body, k);
    for c in out.iter_mut() {
        c.f0 = Some(hz);
    }
    out
}

/// a sung vowel keeps at least this long when a short note squeezes its consonants (s)
const MIN_VOWEL: f64 = 0.05;
/// a squeezed consonant keeps at least this long (s); a flap is 16 ms
const MIN_CONSONANT: f64 = 0.012;

/// Fit the consonants between a vowel and the next one into `span`, the time from the vowel's
/// beat to the next vowel's (or to the end of a phrase's last note): when they leave the vowel
/// less than [`MIN_VOWEL`], they shrink in proportion (each down to [`MIN_CONSONANT`]), so the
/// next vowel still lands on its beat and a fast run doesn't drift later note by note.
fn fit(groups: [&mut [Segment]; 2], span: f64) {
    let [a, b] = groups;
    let cons = total(a) + total(b);
    let room = span - MIN_VOWEL;
    if cons <= room || cons <= 0.0 {
        return;
    }
    let scale = js_max(0.0, room) / cons;
    for c in a.iter_mut().chain(b.iter_mut()) {
        c.dur = Some(js_max(MIN_CONSONANT, c.dur.unwrap_or(0.0) * scale));
    }
}

fn total(segs: &[Segment]) -> f64 {
    segs.iter().fold(0.0, |s, c| s + c.dur.unwrap_or(0.0))
}

fn vibrato_gesture(t0: f64, t1: f64, depth: f64, tag: Option<f64>) -> Gesture {
    let mut g = Gesture::new(t0 + 0.22, t1, 0.3, 0.06, 4.0, Targets::default().with(ControlKey::VIBRATO, depth)).label("vibrato");
    g.tag = tag;
    g
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SungNote {
    pub midi: f64,
    /// length in beats
    pub beats: f64,
    pub syl: Syllable,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongOptions {
    pub body: BodyMix,
    pub bpm: f64,
    /// plan time of the first note's beat (its onset consonants come before)
    pub t0: f64,
    /// vibrato depth (cents) on held vowels
    pub vibrato: f64,
}

fn melisma(s: &Syllable) -> bool {
    s.melisma == Some(true)
}

/// Plan a whole melody: one utterance per phrase, so each phrase gets its own breath.
pub fn plan_song(notes: &[SungNote], o: &SongOptions) -> Plan {
    let beat = 60.0 / o.bpm;
    let mut gestures: Vec<Gesture> = Vec::new();
    let mut segments: Vec<PlannedSegment> = Vec::new();
    let mut times: Vec<f64> = Vec::with_capacity(notes.len());
    let mut t = o.t0;
    for n in notes {
        times.push(t);
        t += n.beats * beat;
    }
    let mut end = t;
    let mut i = 0;
    while i < notes.len() {
        let mut j = i;
        while j < notes.len() - 1 && !notes[j].syl.phrase_end {
            j += 1;
        }
        let mut segs: Vec<Segment> = Vec::new();
        let mut vowel_spans: Vec<(f64, f64)> = Vec::new();
        // each note's onset and coda consonants, at the note's pitch (a coda at the pitch of the
        // note whose syllable it ends; a melisma has neither)
        let hz_of = |k: usize| midi_to_hz(notes[k].midi);
        let mut onsets: Vec<Vec<Segment>> = (i..=j).map(|k| if melisma(&notes[k].syl) { Vec::new() } else { consonants_at(&notes[k].syl.onset, o.body, 0.8, hz_of(k)) }).collect();
        let mut codas: Vec<Vec<Segment>> = (i..=j)
            .map(|k| if k < j && melisma(&notes[k + 1].syl) { Vec::new() } else { consonants_at(&last_of_chain(notes, k).coda, o.body, 0.8, hz_of(k)) })
            .collect();
        // the phrase's last vowel leaves a short breath before the next phrase. The call's last
        // phrase is followed by a rest (callers split phrases at rests), so the breath comes from
        // the rest: hold that note to its end, less the breath's release.
        let reserve = if j == notes.len() - 1 { 0.05 } else { 0.12 };
        let last_end = times[j] + notes[j].beats * beat - js_min(reserve, 0.3 * notes[j].beats * beat);
        for k in i..=j {
            let (span, next) = if k < j { (times[k + 1] - times[k], &mut onsets[k + 1 - i][..]) } else { (last_end - times[k], &mut [][..]) };
            fit([&mut codas[k - i][..], next], span);
        }
        let first_onset = total(&onsets[0]);
        for k in i..=j {
            let n = &notes[k];
            let hz = hz_of(k);
            let next_melisma = k < j && melisma(&notes[k + 1].syl);
            let coda = std::mem::take(&mut codas[k - i]);
            let onset = std::mem::take(&mut onsets[k - i]);
            // vowel from this note's beat to the next note's onset (less this note's coda)
            let note_end = if k < j { times[k + 1] - total(&onsets[k + 1 - i]) } else { last_end };
            // only consonants squeezed to their floor can leave less than MIN_VOWEL
            let vdur = js_max(0.02, note_end - times[k] - total(&coda));
            segs.extend(onset);
            let parts = diphthong_parts(&n.syl.nucleus.ipa);
            match parts {
                Some((a, b)) if !next_melisma => {
                    segs.push(Segment { ipa: a.to_string(), dur: Some(vdur * 0.8), f0: Some(hz), stress: Some(1), word_end: None });
                    segs.push(Segment { ipa: b.to_string(), dur: Some(vdur * 0.2), f0: Some(hz), stress: Some(1), word_end: None });
                }
                _ => {
                    let ipa = parts.map_or(n.syl.nucleus.ipa.clone(), |p| p.0.to_string());
                    segs.push(Segment { ipa, dur: Some(vdur), f0: Some(hz), stress: Some(1), word_end: None });
                }
            }
            segs.extend(coda);
            vowel_spans.push((times[k], times[k] + vdur));
        }
        if !segs.is_empty() {
            // the planner lays segments out from t0 + 0.06: start so the first vowel lands on its beat
            let plan = plan_utterance(&segs, &PlanOptions { body: o.body, t0: times[i] - first_onset - 0.06, f0: Some(hz_of(i)), intonation: Some(Intonation::Flat), sung: Some(true), ..Default::default() });
            gestures.extend_from_slice(&plan.gestures);
            segments.extend(plan.segments);
            for (a, b) in vowel_spans {
                if b - a > 0.3 {
                    gestures.push(vibrato_gesture(a, b, o.vibrato, None));
                }
            }
            end = js_max(end, plan.end);
        }
        i = j + 1;
    }
    Plan { gestures, segments, end }
}

/// The syllable whose coda ends a melisma chain starting at note k.
fn last_of_chain(notes: &[SungNote], k: usize) -> &Syllable {
    let mut b = k;
    while b > 0 && melisma(&notes[b].syl) {
        b -= 1;
    }
    &notes[b].syl
}

/// Live: gestures relative to "now" (the planner's 60 ms lead is mostly taken back).
const LIVE_T0: f64 = -0.045;
/// a held vowel lasts at most this long (s)
const HOLD: f64 = 30.0;

/// Consonants kept on each side of a live syllable's vowel (longer clusters are cut).
pub const MAX_CLUSTER: usize = 8;

/// Consonants held inline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cluster {
    n: u8,
    ipa: [Label; MAX_CLUSTER],
}

impl Cluster {
    fn from_segments(segs: &[Segment]) -> Cluster {
        let mut c = Cluster::default();
        for (slot, s) in c.ipa.iter_mut().zip(segs) {
            *slot = Label::new(&s.ipa);
            c.n += 1;
        }
        c
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.ipa[..self.n as usize].iter().map(|l| l.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

/// A [`Syllable`] as the live planners need it: `Copy`, so holding one costs no allocation and a
/// note's coda outlives a change of lyrics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveSyllable {
    pub onset: Cluster,
    /// vowel (a diphthong stays one nucleus: "aɪ")
    pub nucleus: Label,
    pub coda: Cluster,
    /// continue the previous syllable's vowel at a new pitch
    pub melisma: bool,
}

impl LiveSyllable {
    /// Sung when the lyrics are empty.
    pub fn ah() -> LiveSyllable {
        LiveSyllable { onset: Cluster::default(), nucleus: Label::new("ɑ"), coda: Cluster::default(), melisma: false }
    }
}

impl From<&Syllable> for LiveSyllable {
    fn from(s: &Syllable) -> LiveSyllable {
        LiveSyllable { onset: Cluster::from_segments(&s.onset), nucleus: Label::new(&s.nucleus.ipa), coda: Cluster::from_segments(&s.coda), melisma: s.melisma == Some(true) }
    }
}

/// [`NoteOnOptions`] over a [`LiveSyllable`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveNoteOn {
    pub body: BodyMix,
    pub vibrato: f64,
    pub tag: f64,
    /// the previous note of a legato pair: its syllable and MIDI note
    pub legato_from: Option<(LiveSyllable, f64)>,
}

/// The planner's input for one live note.
type LiveSegments<'a> = SmallVec<[TimedSegment<'a>; super::planner::INLINE_SEGMENTS]>;

/// Sung consonants, shorter than spoken ones (see [`cdur`]).
fn push_consonants<'a>(out: &mut LiveSegments<'a>, c: &'a Cluster, body: BodyMix, k: f64) {
    for ipa in c.iter() {
        out.push(TimedSegment { ipa, dur: cdur(ipa, body, k), stress: None, f0: None });
    }
}

/// What follows a held vowel: the diphthong's glide (at the note's pitch) and the coda.
fn push_tail<'a>(out: &mut LiveSegments<'a>, syl: &'a LiveSyllable, body: BodyMix, hz: f64) {
    if let Some((_, b)) = diphthong_parts(syl.nucleus.as_str()) {
        out.push(TimedSegment { ipa: b, dur: 0.07, stress: Some(1), f0: Some(hz) });
    }
    push_consonants(out, &syl.coda, body, 0.7);
}

/// Key pressed: sing the previous syllable's glide and coda if it is still sounding (legato),
/// then this syllable's onset, then its vowel held open-ended. Every gesture gets `tag`; the
/// pitch and vibrato get `tag + 1`, so a melisma can change pitch without ending the vowel.
/// Appends to `out`.
pub fn plan_note_on_into(syl: &LiveSyllable, midi: f64, o: &LiveNoteOn, out: &mut Vec<Gesture>) {
    let hz = midi_to_hz(midi);
    let mut segs = LiveSegments::new();
    if let Some((from, from_midi)) = &o.legato_from {
        push_tail(&mut segs, from, o.body, midi_to_hz(*from_midi));
    }
    push_consonants(&mut segs, &syl.onset, o.body, 0.7);
    let nucleus = syl.nucleus.as_str();
    segs.push(TimedSegment { ipa: diphthong_parts(nucleus).map_or(nucleus, |p| p.0), dur: HOLD, stress: Some(1), f0: Some(hz) });
    let first = out.len();
    let timing = plan_timed(segs.into_iter(), &PlanOptions { body: o.body, t0: LIVE_T0, f0: Some(hz), intonation: Some(Intonation::Flat), open_end: Some(true), ..Default::default() }, out, None);
    for g in &mut out[first..] {
        g.tag = Some(if g.targets.has(ControlKey::F0) { o.tag + 1.0 } else { o.tag });
    }
    out.push(vibrato_gesture(timing.last.0, timing.last.1, o.vibrato, Some(o.tag + 1.0)));
}

/// Melisma, live: the held vowel moves to a new pitch (release tag + 1 first). Appends to `out`.
pub fn plan_melisma_into(midi: f64, o: &MelismaOptions, out: &mut Vec<Gesture>) {
    let MelismaOptions { vibrato, tag } = *o;
    out.push(Gesture::new(0.0, HOLD, 0.05, 0.06, 6.0, Targets::default().with(ControlKey::F0, midi_to_hz(midi))).label("pitch").tag(tag + 1.0));
    out.push(vibrato_gesture(0.0, HOLD, vibrato, Some(tag + 1.0)));
}

/// Key released: the diphthong's glide and the coda (release the note's tags first). Appends to
/// `out`; nothing when the syllable ends on its vowel.
pub fn plan_note_off_into(syl: &LiveSyllable, midi: f64, body: BodyMix, out: &mut Vec<Gesture>) {
    let hz = midi_to_hz(midi);
    let mut segs = LiveSegments::new();
    push_tail(&mut segs, syl, body, hz);
    if segs.is_empty() {
        return;
    }
    plan_timed(segs.into_iter(), &PlanOptions { body, t0: LIVE_T0, f0: Some(hz), intonation: Some(Intonation::Flat), ..Default::default() }, out, None);
}

/// The previous note of a legato pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegatoFrom {
    pub syl: Syllable,
    pub midi: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteOnOptions {
    pub body: BodyMix,
    pub vibrato: f64,
    pub tag: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legato_from: Option<LegatoFrom>,
}

/// [`plan_note_on_into`] over a [`Syllable`].
pub fn plan_note_on(syl: &Syllable, midi: f64, o: &NoteOnOptions) -> Vec<Gesture> {
    let mut out = Vec::new();
    let legato_from = o.legato_from.as_ref().map(|l| (LiveSyllable::from(&l.syl), l.midi));
    plan_note_on_into(&syl.into(), midi, &LiveNoteOn { body: o.body, vibrato: o.vibrato, tag: o.tag, legato_from }, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MelismaOptions {
    pub vibrato: f64,
    pub tag: f64,
}

/// [`plan_melisma_into`].
pub fn plan_melisma(midi: f64, o: &MelismaOptions) -> Vec<Gesture> {
    let mut out = Vec::with_capacity(2);
    plan_melisma_into(midi, o, &mut out);
    out
}

/// [`plan_note_off_into`] over a [`Syllable`].
pub fn plan_note_off(syl: &Syllable, midi: f64, body: BodyMix) -> Vec<Gesture> {
    let mut out = Vec::new();
    plan_note_off_into(&syl.into(), midi, body, &mut out);
    out
}
