//! Fit a narrow vocal-tract resonance using tongue, lip, and pharynx controls.
//! `cargo run --release -p chimera --example overtone -- 1500 2200 3000 > postures.json`

use chimera::airway::frequency::transfer_function;
use chimera::anatomy::human::{ArticKey, BODY_MALE, HumanArtic, NEUTRAL};
use chimera::phonetics::planner::vowel_table;
use chimera::util::optimize::{NelderMeadOpts, nelder_mead};
use chimera::voice::human_airway::HumanAirway;

const KEYS: [ArticKey; 9] = [ArticKey::TbX, ArticKey::TbY, ArticKey::TtX, ArticKey::TtY,
    ArticKey::Jaw, ArticKey::LipAp, ArticKey::LipProt, ArticKey::Root, ArticKey::Larynx];
const LO: [f64; 9] = [1.8, 5.0, 4.8, 6.4, 0.15, 0.2, 0.0, -0.85, -1.0];
const HI: [f64; 9] = [4.3, 8.4, 6.8, 9.8, 1.8, 1.5, 1.4, 0.85, 1.0];

fn articulation(p: &[f64]) -> HumanArtic {
    let mut a = HumanArtic { lip_width: 0.35, epilarynx: 0.6, ventricular: 0.5, velum: 0.0, ..NEUTRAL };
    for (key, value) in KEYS.iter().zip(p) { a.set(*key, *value); }
    a
}

fn main() {
    let targets: Vec<f64> = std::env::args().skip(1).map(|s| s.parse().expect("frequency in Hz")).collect();
    assert!(!targets.is_empty() && targets.iter().all(|f| (800.0..=3500.0).contains(f)), "supply target frequencies from 800 to 3500 Hz");
    let mut air = HumanAirway::new(96000.0, BODY_MALE);
    let mut output = Vec::new();
    let mut previous: Option<Vec<f64>> = None;
    for target in targets {
        let mut freqs = vec![target - 300.0, target - 150.0, target - 75.0, target,
            target + 75.0, target + 150.0, target + 300.0];
        freqs.extend((700..=4200).step_by(175).map(|f| f as f64));
        let mut cost = |p: &[f64]| {
            air.apply(&articulation(p), 0.0);
            let minimum = air.ph_areas.iter().chain(&air.or_areas).copied().fold(f64::INFINITY, f64::min);
            if minimum < 0.12 || air.tract.contact_tip < 0.0 || air.tract.contact_body < 0.0 {
                return 1000.0 + (0.12 - minimum).max(0.0) * 1000.0
                    + (-air.tract.contact_tip).max(0.0) * 100.0
                    + (-air.tract.contact_body).max(0.0) * 100.0;
            }
            let h = transfer_function(&air.net.linear_elements(), air.pharynx.first_node(), &freqs, &[air.glottis_port]);
            let center = h.pressure_db[3];
            let near = (h.pressure_db[2] + h.pressure_db[4]) * 0.5;
            let far = (h.pressure_db[1] + h.pressure_db[5]) * 0.5;
            // Approximate the source's spectral falloff when comparing distant resonances.
            // This weights the fit only; the rendered sound receives no spectral filter.
            let focused = center - 30.0 * chimera::math::log10(target);
            let competing = freqs.iter().zip(&h.pressure_db).skip(7)
                .filter(|(f, _)| (**f - target).abs() > 300.0)
                .map(|(f, db)| db - 30.0 * chimera::math::log10(*f))
                .fold(f64::NEG_INFINITY, f64::max);
            let continuity = previous.as_ref().map_or(0.0, |old| p.iter().zip(old).enumerate()
                .map(|(i, (x, y))| ((x - y) / (HI[i] - LO[i])).powi(2)).sum::<f64>());
            -(center - near) - 0.4 * (center - far) - 0.03 * center + 3.0 * continuity
                + 3.0 * (competing - focused + 3.0).max(0.0)
        };
        let mut seeds: Vec<Vec<f64>> = Vec::new();
        if let Some(p) = &previous { seeds.push(p.clone()); }
        for vowel in &["i", "u", "o", "ɑ", "ɝ", "ə"] {
            let patch = &vowel_table()["male"][*vowel].artic;
            let mut a = NEUTRAL;
            for (key, value) in patch.iter() { a.set(key, value); }
            if target < 1600.0 {
                seeds.push(KEYS.iter().map(|key| a.get(*key)).collect());
            }
            a.tt_x = 5.9;
            a.tt_y = 8.7;
            seeds.push(KEYS.iter().map(|key| a.get(*key)).collect());
        }
        let mut best = None;
        for seed in seeds {
            let fit = nelder_mead(&mut cost, &seed, NelderMeadOpts {
                lo: Some(LO.to_vec()), hi: Some(HI.to_vec()),
                step: Some(vec![0.2, 0.2, 0.15, 0.15, 0.1, 0.15, 0.15, 0.15, 0.15]),
                max_iter: Some(1200), ..Default::default()
            });
            if best.as_ref().is_none_or(|old: &chimera::util::optimize::NelderMeadResult| fit.fx < old.fx) { best = Some(fit); }
        }
        let best = best.unwrap();
        let a = articulation(&best.x);
        air.apply(&a, 0.0);
        let h = transfer_function(&air.net.linear_elements(), air.pharynx.first_node(), &freqs, &[air.glottis_port]);
        let contrast = h.pressure_db[3] - (h.pressure_db[2] + h.pressure_db[4]) * 0.5;
        eprintln!("{target:.0} Hz: adjacent-band contrast {contrast:.1} dB, cost {:.2}", best.fx);
        output.push(serde_json::json!({"target": target, "contrast": contrast, "artic": a, "freqs": freqs, "db": h.pressure_db}));
        previous = Some(best.x);
    }
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
