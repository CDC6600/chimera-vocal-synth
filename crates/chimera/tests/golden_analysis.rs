//! The frequency-domain oracle and the analysis code against the TypeScript engine
//! (fixtures from `bun tools/golden/analysis.ts`): the UI's transfer-function/formant pipeline on
//! the human airway, simple tube networks, findPeaks, FFT, LPC, YIN and Nelder–Mead. The human
//! airway's cases follow the tract geometry, so `CHIMERA_BLESS=1` rewrites them after an intended
//! geometry change; everything else stays the TypeScript engine's.
mod common;
use chimera::airway::frequency::{Peak, find_peaks, linspace, transfer_function};
use chimera::airway::network::{AirwayNetwork, Area, PortSpec, TubeSpec};
use chimera::analysis::fft::{Fft, hamming, hann, next_pow2, power_spectrum_db};
use chimera::analysis::lpc::{FormantOpts, autocorr, estimate_formants, levinson, lpc_envelope_db, poly_roots};
use chimera::analysis::pitch::{YinOpts, yin_pitch};
use chimera::anatomy::human::{BodyName, HumanArtic};
use chimera::math::{log10, pow, sin};
use chimera::physics::{RHO, WALL_SOFT};
use chimera::util::optimize::{NelderMeadOpts, nelder_mead};
use chimera::voice::human_airway::HumanAirway;
use common::*;
use serde_json::{Value, json};

const TOL: f64 = 1e-9;

/// Worst error per area, printed at the end.
#[derive(Default)]
struct Report(Vec<(String, f64)>);
impl Report {
    fn check(&mut self, area: &str, label: &str, want: &[f64], got: &[f64], floor: f64) {
        let w = worst(label, want, got, floor);
        assert!(w < TOL, "{label}: worst relative difference {w:e}");
        match self.0.iter_mut().find(|(a, _)| a == area) {
            Some((_, x)) => *x = x.max(w),
            None => self.0.push((area.to_string(), w)),
        }
    }
    fn peaks(&mut self, area: &str, label: &str, want: &Value, got: &[Peak]) {
        let want = want.as_array().unwrap();
        assert_eq!(want.len(), got.len(), "{label}: peak count");
        for (w, g) in want.iter().zip(got) {
            let w = nums(w);
            self.check(area, label, &w[..2], &[g.freq, g.bw], 1.0);
            self.check(area, label, &w[2..], &[g.db], 1e-6);
        }
    }
    fn print(&self) {
        for (a, w) in &self.0 {
            println!("{a:>24}: worst relative difference {w:e}");
        }
    }
}

fn opt_num(v: &Value) -> Option<f64> {
    v.as_f64()
}
fn opt_vec(v: &Value) -> Option<Vec<f64>> {
    v.is_array().then(|| nums(v))
}

/// The networks of `networks()` in tools/golden/analysis.ts, in the same order: (net, inject, cut).
fn networks() -> Vec<(AirwayNetwork, usize, Vec<usize>)> {
    let mut out = Vec::new();
    for walls in [false, true] {
        let mut net = AirwayNetwork::new(96000.0);
        let t = net.add_tube(TubeSpec::new("tract", 40, 17.5, Area::Uniform(3.0)).wall(if walls { Some(WALL_SOFT) } else { None }));
        net.add_radiation(t.last_node());
        net.finalize();
        out.push((net, t.first_node(), vec![]));
    }
    {
        let mut net = AirwayNetwork::new(48000.0);
        let a = net.add_tube(TubeSpec::new("a", 12, 8.0, Area::Profile((0..12).map(|i| 1.0 + 0.3 * i as f64).collect())).wall(Some(WALL_SOFT)).radiating(0.25));
        let b = net.add_tube(TubeSpec::new("b", 10, 7.0, Area::Uniform(2.0)));
        let c = net.add_tube(TubeSpec::new("c", 14, 11.0, Area::Uniform(1.2)).wall(Some(WALL_SOFT)));
        net.connect(a.last_node(), b.first_node());
        net.connect(a.last_node(), c.first_node());
        net.add_radiation(b.last_node());
        net.add_radiation(c.last_node());
        net.finalize();
        out.push((net, a.first_node(), vec![]));
    }
    {
        let mut net = AirwayNetwork::new(48000.0);
        let lung = net.add_pressure_node("lung");
        let t = net.add_tube(TubeSpec::new("t", 20, 15.0, Area::Uniform(2.5)).wall(Some(WALL_SOFT)));
        let port = net.add_port(lung, t.first_node(), PortSpec::new(0.4, 0.5).resistance((0.5 * RHO * 35000.0) / 0.4));
        let cav = net.add_cavity("sinus", 12.0, None);
        net.add_port(t.node(8), cav, PortSpec::new(0.15, 0.6).kinetic(0.0, 0.0).no_half_sections());
        net.add_radiation(t.last_node());
        net.finalize();
        out.push((net.clone(), t.first_node(), vec![]));
        out.push((net, t.first_node(), vec![port]));
    }
    out
}

#[test]
fn analysis_matches_typescript() {
    let mut g = load("analysis");
    let mut rep = Report::default();
    let bless_it = blessing();

    // ---- the UI's transfer function and formants (src/ui/analysis.ts VoiceModel.transfer)
    let ui_freqs: Vec<f64> = (0..360).map(|i| 60.0 * pow(8000.0 / 60.0, i as f64 / 359.0)).collect();
    rep.check("ui freqs", "uiFreqs", &nums(&g["uiFreqs"]), &ui_freqs, 1.0);
    let mut n_formants = 0;
    for h in g["human"].as_array_mut().unwrap() {
        let body = h["body"].as_str().unwrap().to_string();
        let mut airway = HumanAirway::new(96000.0, BodyName::parse(&body).body());
        for c in h["cases"].as_array_mut().unwrap() {
            let label = format!("{body} {}", c["name"].as_str().unwrap());
            let artic: HumanArtic = serde_json::from_value(c["artic"].clone()).unwrap();
            airway.apply(&artic, 0.0);
            let el = airway.net.linear_elements();
            let tf = transfer_function(&el, airway.pharynx.first_node(), &ui_freqs, &[airway.glottis_port]);
            let db: Vec<f64> = tf.mag.iter().map(|m| 20.0 * log10(m + 1e-12)).collect();
            let peaks = find_peaks(&ui_freqs, &tf.mag, 150.0);
            let formants: Vec<f64> = peaks.iter().map(|p| p.freq).take(5).collect();
            n_formants += formants.len();
            if bless_it {
                c["mag"] = json!(tf.mag);
                c["pressureDb"] = json!(tf.pressure_db);
                c["db"] = json!(db);
                c["peaks"] = json!(peaks.iter().map(|p| [p.freq, p.bw, p.db]).collect::<Vec<_>>());
                c["formants"] = json!(formants);
                continue;
            }
            rep.check("human |H|", &label, &nums(&c["mag"]), &tf.mag, 1e-12);
            rep.check("human pressure dB", &label, &nums(&c["pressureDb"]), &tf.pressure_db, 1e-6);
            rep.check("human UI dB", &label, &nums(&c["db"]), &db, 1e-6);
            rep.peaks("human peaks", &label, &c["peaks"], &peaks);
            rep.check("human formants", &label, &nums(&c["formants"]), &formants, 1.0);
        }
    }
    assert!(n_formants >= 90, "formant readouts: {n_formants}");
    if bless_it {
        save_json("analysis", &g);
    }

    // ---- simple networks
    let net_freqs = linspace(20.0, 6000.0, 700);
    rep.check("linspace", "netFreqs", &nums(&g["netFreqs"]), &net_freqs, 1.0);
    let nets = g["nets"].as_array().unwrap();
    let built = networks();
    assert_eq!(nets.len(), built.len());
    for (c, (net, inject, cut)) in nets.iter().zip(&built) {
        let label = c["name"].as_str().unwrap();
        let tf = transfer_function(&net.linear_elements(), *inject, &net_freqs, cut);
        rep.check("network |H|", label, &nums(&c["mag"]), &tf.mag, 1e-12);
        rep.check("network pressure dB", label, &nums(&c["pressureDb"]), &tf.pressure_db, 1e-6);
        rep.peaks("network peaks", label, &c["peaks"], &find_peaks(&net_freqs, &tf.mag, 80.0));
    }

    // ---- findPeaks edge cases
    for c in g["peakCases"].as_array().unwrap() {
        let label = c["name"].as_str().unwrap();
        rep.peaks("findPeaks", label, &c["peaks"], &find_peaks(&nums(&c["freqs"]), &nums(&c["mag"]), num(&c["minFreq"])));
    }

    // ---- signals: the TypeScript engine's sustained male voice (voice-sustain-male.f32, from
    // sample `offset`), frozen in its own file. This tests the analysis code on a fixed signal; the
    // voice fixtures themselves are re-blessed whenever the sound changes.
    let vc = &g["voice"];
    let voice_frame: Vec<f64> = load_f32("analysis-voice-frame").iter().map(|&x| x as f64).collect();
    assert_eq!(voice_frame.len(), vc["len"].as_u64().unwrap() as usize);
    let voice_frame = &voice_frame[..];
    let noise = nums(&g["lpc"]["noise"]);

    // ---- FFT, windows, spectra
    let f = &g["fft"];
    let (mut re, mut im) = (nums(&f["input"]), vec![0.0; 1024]);
    Fft::new(1024).transform(&mut re, &mut im);
    rep.check("fft", "fft re", &nums(&f["re"]), &re, 1e-9);
    rep.check("fft", "fft im", &nums(&f["im"]), &im, 1e-9);
    let s = &f["small"];
    let (mut sr, mut si) = (nums(&s["re"]), nums(&s["im"]));
    Fft::new(8).transform(&mut sr, &mut si);
    rep.check("fft", "fft8 re", &nums(&s["outRe"]), &sr, 1e-12);
    rep.check("fft", "fft8 im", &nums(&s["outIm"]), &si, 1e-12);
    rep.check("windows", "hann", &nums(&f["hann"]), &hann(64), 1e-12);
    rep.check("windows", "hamming", &nums(&f["hamming"]), &hamming(101), 1e-12);
    rep.check("powerSpectrumDb", "psd noise", &nums(&f["psdNoise"]), &power_spectrum_db(&noise, &Fft::new(1024), &hann(1000)), 1e-6);
    rep.check("powerSpectrumDb", "psd voice", &nums(&f["psdVoice"]), &power_spectrum_db(voice_frame, &Fft::new(2048), &hamming(2048)), 1e-6);
    for p in f["nextPow2"].as_array().unwrap() {
        let p = nums(p);
        assert_eq!(next_pow2(p[0]) as f64, p[1], "nextPow2({})", p[0]);
    }

    // ---- LPC
    let lpc = &g["lpc"];
    let vowel16 = nums(&lpc["vowel16"]);
    for c in lpc["cases"].as_array().unwrap() {
        let label = c["name"].as_str().unwrap();
        let frame: &[f64] = if label.starts_with("vowel16") {
            &vowel16
        } else if label.starts_with("noise") {
            &noise
        } else {
            voice_frame
        };
        let sr = num(&c["sr"]);
        let order = c["order"].as_u64().unwrap() as usize;
        let r = autocorr(frame, order);
        // coefficients and the error of near-singular systems amplify the autocorrelation's
        // last-ulp differences, so they compare with a floor of 1e-12 of r[0]
        rep.check("autocorr", label, &nums(&c["autocorr"]), &r, 1e-12 * r[0].abs());
        let l = levinson(&r, order);
        rep.check("levinson", label, &nums(&c["a"]), &l.a, 1e-12);
        rep.check("levinson", label, &[num(&c["err"])], &[l.err], 1e-12 * r[0].abs());
        let (rr, ri) = poly_roots(&l.a, 400);
        rep.check("polyRoots", label, &nums(&c["rootsRe"]), &rr, 1e-12);
        rep.check("polyRoots", label, &nums(&c["rootsIm"]), &ri, 1e-12);
        rep.check("lpcEnvelopeDb", label, &nums(&c["envelope"]), &lpc_envelope_db(&l.a, l.err, sr, &linspace(0.0, sr / 2.0, 200)), 1e-6);
        let o = &c["opts"];
        let opts = FormantOpts { order: o["order"].as_u64().map(|x| x as usize), pre_emph: opt_num(&o["preEmph"]), max_bw: opt_num(&o["maxBw"]), min_freq: opt_num(&o["minFreq"]) };
        let fm = estimate_formants(frame, sr, opts);
        let want = c["formants"].as_array().unwrap();
        assert_eq!(want.len(), fm.len(), "{label}: formant count");
        for (w, got) in want.iter().zip(&fm) {
            rep.check("estimateFormants", label, &nums(w), &[got.freq, got.bw], 1.0);
        }
    }
    let k = &lpc["known"];
    let c = nums(&k["c"]);
    let (rr, ri) = poly_roots(&c, 400);
    rep.check("polyRoots", "known", &nums(&k["re"]), &rr, 1e-12);
    rep.check("polyRoots", "known", &nums(&k["im"]), &ri, 1e-12);
    let (rr, ri) = poly_roots(&c, 5);
    rep.check("polyRoots", "known, 5 iterations", &nums(&k["short"]["re"]), &rr, 1e-12);
    rep.check("polyRoots", "known, 5 iterations", &nums(&k["short"]["im"]), &ri, 1e-12);

    // ---- YIN
    for c in g["yin"].as_array().unwrap() {
        let label = c["name"].as_str().unwrap();
        let frame = if c["frame"].is_array() { nums(&c["frame"]) } else { voice_frame.to_vec() };
        let o = &c["opts"];
        let p = yin_pitch(&frame, num(&c["sr"]), YinOpts { min_f0: opt_num(&o["minF0"]), max_f0: opt_num(&o["maxF0"]), threshold: opt_num(&o["threshold"]) });
        // ("undefined": TS read past the array where no lag was searched; NaN in Rust)
        let ap = if c["aperiodicity"] == "undefined" { f64::NAN } else { num(&c["aperiodicity"]) };
        rep.check("yinPitch", label, &[num(&c["f0"]), ap], &[p.f0, p.aperiodicity], 1e-12);
    }

    // ---- Nelder–Mead
    for c in g["nelderMead"].as_array().unwrap() {
        let label = c["name"].as_str().unwrap();
        let mut calls = 0usize;
        let rosen = |x: &[f64]| pow(1.0 - x[0], 2.0) + 100.0 * pow(x[1] - pow(x[0], 2.0), 2.0);
        let bowl = |x: &[f64]| pow(x[0] - 3.0, 2.0) + 2.0 * pow(x[1] + 1.0, 2.0) + 0.5 * pow(x[2] - 0.2, 2.0) + sin(3.0 * x[0]) * 0.1;
        let f = |x: &[f64]| {
            calls += 1;
            if label.starts_with("rosenbrock") { rosen(x) } else { bowl(x) }
        };
        let o = &c["opts"];
        let opts = NelderMeadOpts { step: opt_vec(&o["step"]), lo: opt_vec(&o["lo"]), hi: opt_vec(&o["hi"]), max_iter: o["maxIter"].as_u64().map(|x| x as usize), tol: opt_num(&o["tol"]) };
        let r = nelder_mead(f, &nums(&c["x0"]), opts);
        assert_eq!(r.iters as u64, c["iters"].as_u64().unwrap(), "{label}: iterations");
        assert_eq!(calls as u64, c["calls"].as_u64().unwrap(), "{label}: evaluations");
        rep.check("nelderMead", label, &nums(&c["x"]), &r.x, 1e-12);
        rep.check("nelderMead", label, &[num(&c["fx"])], &[r.fx], 1e-12);
    }
    rep.print();
}
