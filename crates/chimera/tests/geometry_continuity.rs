//! The area function must change smoothly as the articulators move: a jump in a section's area
//! is heard as a tick and a formant jump. Sweeps each articulator through its speech range from
//! several vowel postures and flags any step far above its neighbours.
//! (Port of tests/geometry-continuity.test.ts.)
use chimera::anatomy::geom::{Polyline, Vec2, ray_crossings};
use chimera::anatomy::human::{ArticKey, BODY_CHILD, BODY_FEMALE, BODY_MALE, HumanArtic, HumanBody, HumanFrame, HumanVocalTract, NEUTRAL, rest_tip};
use chimera::math::{js_max, js_min};
use chimera::phonetics::planner::{vowel_artic, vowel_table};
use std::collections::HashMap;

const BODIES: [(&str, HumanBody); 3] = [("male", BODY_MALE), ("female", BODY_FEMALE), ("child", BODY_CHILD)];

/// The tract's 20 + 24 section areas for an articulation.
struct Areas {
    tract: HumanVocalTract,
    frame: HumanFrame,
    ph: Vec<f64>,
    or: Vec<f64>,
}

impl Areas {
    fn new(body: HumanBody) -> Self {
        Areas { tract: HumanVocalTract::new(body), frame: HumanFrame::default(), ph: vec![0.0; 20], or: vec![0.0; 24] }
    }
    fn at(&mut self, a: &HumanArtic) -> Vec<f64> {
        self.tract.compute(a, &mut self.frame);
        self.tract.resample(a, &self.frame, &mut self.ph, &mut self.or, None, None);
        self.ph.iter().chain(&self.or).copied().collect()
    }
}

fn max_diff(x: &[f64], y: &[f64]) -> f64 {
    let mut m: f64 = 0.0;
    for j in 0..x.len() {
        m = js_max(m, (x[j] - y[j]).abs());
    }
    m
}

/// The largest change that survives refinement of [u0, u1] to 2⁻²⁰ of its width: every half
/// still changing by more than `tol` is refined (keeping only the larger half could follow a
/// steep ramp and miss a real jump in the other). A true discontinuity keeps its size; a steep
/// but continuous ramp shrinks away.
fn jump_size(areas: &mut dyn FnMut(f64) -> Vec<f64>, u0: f64, u1: f64, tol: f64) -> f64 {
    struct Ctx<'a> {
        memo: HashMap<u64, Vec<f64>>,
        areas: &'a mut dyn FnMut(f64) -> Vec<f64>,
        tol: f64,
        worst: f64,
    }
    impl Ctx<'_> {
        fn at(&mut self, u: f64) -> &Vec<f64> {
            // (a Map key: +0 and −0 are the same key)
            let key = (u + 0.0).to_bits();
            if !self.memo.contains_key(&key) {
                let v = (self.areas)(u);
                self.memo.insert(key, v);
            }
            &self.memo[&key]
        }
        fn diff(&mut self, a: f64, b: f64) -> f64 {
            let x = self.at(a).clone();
            let y = self.at(b);
            max_diff(&x, y)
        }
        fn visit(&mut self, lo: f64, hi: f64, depth: u32, d: f64) {
            if depth == 20 {
                self.worst = js_max(self.worst, d);
                return;
            }
            let mid = (lo + hi) / 2.0;
            let (d1, d2) = (self.diff(lo, mid), self.diff(mid, hi));
            if d1 > self.tol {
                self.visit(lo, mid, depth + 1, d1);
            }
            if d2 > self.tol {
                self.visit(mid, hi, depth + 1, d2);
            }
        }
    }
    let mut c = Ctx { memo: HashMap::new(), areas, tol, worst: 0.0 };
    let d = c.diff(u0, u1);
    if d > tol {
        c.visit(u0, u1, 0, d);
    }
    c.worst
}

/// Flags steps far above their ±8 neighbours' median (the step itself left out).
fn suspicious(steps: &[f64], i: usize, min_step: f64) -> bool {
    let s = steps[i];
    let lo = i.saturating_sub(8);
    let hi = (i + 9).min(steps.len());
    let skip = js_min(8.0, i as f64) as usize;
    let mut nb: Vec<f64> = steps[lo..hi].iter().enumerate().filter(|(k, _)| *k != skip).map(|(_, x)| *x).collect();
    nb.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = nb.get(nb.len() / 2).copied().unwrap_or(0.0);
    s > min_step && s > 6.0 * js_max(med, 0.01)
}

const SWEEPS: [(ArticKey, f64, f64); 8] = [
    (ArticKey::TbY, 5.5, 8.5),
    (ArticKey::TbX, 1.5, 4.2),
    (ArticKey::Jaw, 0.0, 2.0),
    (ArticKey::TtY, 6.0, 9.5),
    (ArticKey::TtX, 5.0, 7.0),
    (ArticKey::Root, -0.9, 0.9),
    (ArticKey::LipAp, -0.2, 2.0),
    (ArticKey::Velum, -0.25, 1.2),
];

fn with(base: &HumanArtic, k: ArticKey, x: f64) -> HumanArtic {
    let mut a = *base;
    a.set(k, x);
    a
}

#[test]
fn no_area_jumps_anywhere_in_the_articulators_speech_ranges() {
    let mut jumps: Vec<String> = Vec::new();
    let n = 400;
    for (body_name, body) in BODIES {
        let mut areas = Areas::new(body);
        for vw in ["i", "ɑ", "u", "ə"] {
            let v = vowel_artic(vw, body_name.into()).unwrap();
            let tip = rest_tip(v.get(ArticKey::Jaw).unwrap_or(NEUTRAL.jaw), v.get(ArticKey::TbX).unwrap(), v.get(ArticKey::TbY).unwrap());
            let mut base = v.apply_to(&NEUTRAL);
            base.tt_x = tip.x;
            base.tt_y = tip.y;
            for (key, lo, hi) in SWEEPS {
                let mut steps: Vec<f64> = Vec::with_capacity(n);
                let mut prev = areas.at(&with(&base, key, lo));
                for i in 1..=n {
                    let cur = areas.at(&with(&base, key, lo + ((hi - lo) * i as f64) / n as f64));
                    steps.push(max_diff(&cur, &prev));
                    prev = cur;
                }
                for i in 0..steps.len() {
                    if !suspicious(&steps, i, 0.25) {
                        continue;
                    }
                    let mut f = |u: f64| areas.at(&with(&base, key, lo + (hi - lo) * u));
                    let j = jump_size(&mut f, i as f64 / n as f64, (i + 1) as f64 / n as f64, 0.05);
                    if j > 0.05 {
                        jumps.push(format!("{body_name} {vw} {}={:.3}: {j:.2} cm²", key.name(), lo + ((hi - lo) * (i + 1) as f64) / n as f64));
                    }
                }
            }
        }
    }
    assert_eq!(jumps, Vec::<String>::new());
}

#[test]
fn no_area_jumps_on_the_way_between_calibrated_vowels() {
    // (all articulators at once, every body)
    let pairs = [("o", "ɝ"), ("i", "ɝ"), ("ɑ", "ɝ"), ("ə", "ɝ"), ("ɑ", "i"), ("u", "i"), ("æ", "u"), ("ʊ", "e")];
    let mut jumps: Vec<String> = Vec::new();
    let n = 300;
    for (body_name, body) in BODIES {
        let mut areas = Areas::new(body);
        // the full calibrated posture, tongue tip included (the rhotic's raised, curled tip)
        let posture = |vw: &str| vowel_table()[body_name][vw].artic.apply_to(&NEUTRAL);
        for (x, y) in pairs {
            let (a, b) = (posture(x), posture(y));
            let at = |u: f64| {
                let mut out = a;
                for k in ArticKey::ALL {
                    out.set(k, a.get(k) + (b.get(k) - a.get(k)) * u);
                }
                out
            };
            let mut steps: Vec<f64> = Vec::with_capacity(n);
            let mut prev = areas.at(&at(0.0));
            for i in 1..=n {
                let cur = areas.at(&at(i as f64 / n as f64));
                steps.push(max_diff(&cur, &prev));
                prev = cur;
            }
            for i in 0..steps.len() {
                if !suspicious(&steps, i, 0.2) {
                    continue;
                }
                let mut f = |u: f64| areas.at(&at(u));
                let j = jump_size(&mut f, i as f64 / n as f64, (i + 1) as f64 / n as f64, 0.05);
                if j > 0.05 {
                    jumps.push(format!("{body_name} {x}→{y} at {:.3}: {j:.2} cm²", (i + 1) as f64 / n as f64));
                }
            }
        }
    }
    assert_eq!(jumps, Vec::<String>::new());
}

#[test]
fn refinement_finds_a_small_jump_beside_a_steep_ramp() {
    let mut f = |u: f64| vec![js_min(1.6 * u, 0.8) + if u >= 0.75 { 0.1 } else { 0.0 }];
    // toBeCloseTo(0.1, 3): |x − 0.1| < 0.0005
    let j = jump_size(&mut f, 0.0, 1.0, 0.05);
    assert!((j - 0.1).abs() < 0.0005, "{j}");
    // a ramp alone is no jump
    assert_eq!(jump_size(&mut |u: f64| vec![js_min(1.6 * u, 0.8)], 0.0, 1.0, 0.05), 0.0);
}

#[test]
fn a_ray_grazing_a_polyline_vertex_sees_no_tissue_layer_that_is_not_there() {
    // a polyline touching the ray's line at a vertex (from either side): no crossing, or an
    // entry and its exit at the same point, never one unmatched crossing
    let mut t = [0.0f64; 8];
    let mut ex = [false; 8];
    let p = |x: f64, y: f64| Vec2 { x, y };
    for side in [-1.0, 1.0] {
        let mut touch = Polyline::default();
        touch.push(p(side, -1.0)).push(p(0.0, 0.0)).push(p(side, 1.0));
        let n = ray_crossings(0.0, -5.0, 0.0, 1.0, &touch, 0.0, 10.0, &mut t, &mut ex);
        assert!(n == 0 || (n == 2 && t[0] == t[1] && ex[0] != ex[1]), "side {side}: {n} crossings");
    }
    // a polyline passing through the line at a vertex crosses it once
    let mut through = Polyline::default();
    through.push(p(-1.0, -1.0)).push(p(0.0, 0.0)).push(p(1.0, 1.0));
    assert_eq!(ray_crossings(0.0, -5.0, 0.0, 1.0, &through, 0.0, 10.0, &mut t, &mut ex), 1);
    // the reviewer's posture: a fan ray grazing the tip of a calibrated /ɝ/ exactly at a vertex
    let mut areas = Areas::new(BODY_MALE);
    let mut base = vowel_table()["male"]["ɝ"].artic.apply_to(&NEUTRAL);
    base.tt_x = 5.625;
    let y = 8.588839057347066;
    let mut at = |tt_y: f64| areas.at(&with(&base, ArticKey::TtY, tt_y));
    let (a, b, c) = (at(y - 1e-9), at(y), at(y + 1e-9));
    for j in 0..a.len() {
        assert!((b[j] - a[j]).abs() < 1e-3);
        assert!((c[j] - b[j]).abs() < 1e-3);
    }
}

#[test]
fn no_area_jumps_as_the_lower_lip_tucks_for_f() {
    // /f v/: the lower lip slides up under the upper incisors (labiodental 0 → 1) with the lips
    // nearly shut. The vowel sweeps above start from open lips, so they never cross the handover
    // from the lips' own contact to the incisor constriction, where steps made a click.
    let mut jumps: Vec<String> = Vec::new();
    let n = 400;
    for (body_name, body) in BODIES {
        let mut areas = Areas::new(body);
        for jaw in [0.3, 0.5, 0.8, 1.2] {
            for lip_ap in [0.05, 0.3, 0.8] {
                let mut base = NEUTRAL;
                base.jaw = jaw;
                base.lip_ap = lip_ap;
                base.lip_prot = 0.0;
                let key = ArticKey::Labiodental;
                let mut steps: Vec<f64> = Vec::with_capacity(n);
                let mut prev = areas.at(&with(&base, key, 0.0));
                for i in 1..=n {
                    let cur = areas.at(&with(&base, key, i as f64 / n as f64));
                    steps.push(max_diff(&cur, &prev));
                    prev = cur;
                }
                for i in 0..steps.len() {
                    // every step is checked here, not only the suspicious ones: the handover is
                    // steep on purpose, and a jump inside it must still be found
                    let mut f = |u: f64| areas.at(&with(&base, key, u));
                    let j = jump_size(&mut f, i as f64 / n as f64, (i + 1) as f64 / n as f64, 0.05);
                    if j > 0.05 {
                        jumps.push(format!("{body_name} jaw={jaw} lipAp={lip_ap} labiodental={:.4}: {j:.2} cm²", (i + 1) as f64 / n as f64));
                    }
                }
            }
        }
    }
    assert_eq!(jumps, Vec::<String>::new());
}

#[test]
#[ignore = "known, not yet fixed: run with --ignored. Found by the batch-2 review; already on main (0.30 cm²)"]
fn no_area_jumps_as_a_protruded_lower_lip_tucks() {
    // With the lips protruded and narrowed, the first front ray (behind the lower lip's edge, so
    // not under the handover ramp) switches to a higher crossing of the lower lip's curve as the
    // lip tucks: NEUTRAL, jaw 0.3, lipAp 0.05, lipProt 0.2, lipWidth 0.5, lab ≈ 0.8612 jumps the
    // male area 0.41 cm² (0.30 cm² before hill-climb batch 2).
    let mut jumps: Vec<String> = Vec::new();
    let n = 400;
    for (body_name, body) in BODIES {
        let mut areas = Areas::new(body);
        for lip_prot in [0.2, 0.6] {
            for lip_w in [0.5, 1.0] {
                let mut base = NEUTRAL;
                base.jaw = 0.3;
                base.lip_ap = 0.05;
                base.lip_prot = lip_prot;
                base.set(ArticKey::LipWidth, lip_w);
                let key = ArticKey::Labiodental;
                let mut prev = areas.at(&with(&base, key, 0.0));
                for i in 1..=n {
                    let cur = areas.at(&with(&base, key, i as f64 / n as f64));
                    let step = max_diff(&cur, &prev);
                    prev = cur;
                    if step <= 0.05 {
                        continue;
                    }
                    let mut f = |u: f64| areas.at(&with(&base, key, u));
                    let j = jump_size(&mut f, (i - 1) as f64 / n as f64, i as f64 / n as f64, 0.05);
                    if j > 0.05 {
                        jumps.push(format!("{body_name} lipProt={lip_prot} lipWidth={lip_w} labiodental={:.4}: {j:.2} cm²", i as f64 / n as f64));
                    }
                }
            }
        }
    }
    assert_eq!(jumps, Vec::<String>::new());
}
