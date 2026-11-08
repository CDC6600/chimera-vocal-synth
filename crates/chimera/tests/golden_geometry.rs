//! The anatomy model against the TypeScript engine: grids, area functions, contacts, outlines.
//! Re-blessed with `CHIMERA_BLESS=1` after an intended change to the geometry (the articulations
//! stay; what they produce is rewritten).
mod common;
use chimera::anatomy::geom::Vec2;
use chimera::anatomy::human::{BodyName, HumanArtic, HumanFrame, HumanVocalTract};
use common::*;
use serde_json::json;

fn flat(ps: &[Vec2]) -> Vec<f64> {
    ps.iter().flat_map(|p| [p.x, p.y]).collect()
}

#[test]
fn geometry_matches_typescript() {
    let mut cases = load("geometry");
    let bless_it = blessing();
    let mut tracts: std::collections::HashMap<String, HumanVocalTract> = Default::default();
    let mut f = HumanFrame::default();
    let mut worst_all = 0.0f64;
    for (ci, c) in cases.as_array_mut().unwrap().iter_mut().enumerate() {
        let body = c["body"].as_str().unwrap();
        let artic: HumanArtic = serde_json::from_value(c["artic"].clone()).unwrap();
        let t = tracts.entry(body.to_string()).or_insert_with(|| HumanVocalTract::new(BodyName::parse(body).body()));
        t.compute(&artic, &mut f);
        let label = format!("case {ci} ({body})");
        let (n_ph, n_or) = (18, 18);
        let (mut ph, mut or, mut ph_min, mut or_min) = (vec![0.0; n_ph], vec![0.0; n_or], vec![0.0; n_ph], vec![0.0; n_or]);
        if bless_it {
            c["grid"] = json!(f.grid.iter().map(|r| json!([r.region, r.d, r.area, r.s])).collect::<Vec<_>>());
            c["scalars"] = json!([f.length, f.pharynx_length, f.teeth_s, f.lip_s, f.velum_port, f.lip_area, f.tongue_radius, f.tip.x, f.tip.y, f.split_index as f64, t.contact_tip, t.contact_body, t.contact_tip_n.x, t.contact_tip_n.y, t.contact_body_n.x, t.contact_body_n.y]);
            for (key, pts) in [("outer", &f.outer), ("inner", &f.inner), ("tongue", &f.tongue), ("velum", &f.velum), ("upperLip", &f.upper_lip), ("lowerLip", &f.lower_lip)] {
                if c[key].is_array() {
                    c[key] = json!(flat(pts));
                }
            }
            let (ph_len, or_len) = t.resample(&artic, &f, &mut ph, &mut or, Some(&mut ph_min), Some(&mut or_min));
            c["resample"] = json!({ "phLen": ph_len, "orLen": or_len, "ph": ph, "or": or, "phMin": ph_min, "orMin": or_min });
            continue;
        }
        let grid = c["grid"].as_array().unwrap();
        assert_eq!(grid.len(), f.grid.len(), "{label}: grid size");
        let mut w = 0.0f64;
        for (g, r) in grid.iter().zip(&f.grid) {
            let e = nums(g);
            assert_eq!(e[0] as u8, r.region, "{label}: region");
            w = w.max(worst(&label, &e[1..], &[r.d, r.area, r.s], 1e-6));
        }
        let s = nums(&c["scalars"]);
        let mine = [f.length, f.pharynx_length, f.teeth_s, f.lip_s, f.velum_port, f.lip_area, f.tongue_radius, f.tip.x, f.tip.y, f.split_index as f64, t.contact_tip, t.contact_body, t.contact_tip_n.x, t.contact_tip_n.y, t.contact_body_n.x, t.contact_body_n.y];
        w = w.max(worst(&format!("{label} scalars"), &s, &mine, 1e-6));
        for (key, pts) in [("outer", &f.outer), ("inner", &f.inner), ("tongue", &f.tongue), ("velum", &f.velum), ("upperLip", &f.upper_lip), ("lowerLip", &f.lower_lip)] {
            if c[key].is_array() {
                w = w.max(worst(&format!("{label} {key}"), &nums(&c[key]), &flat(pts), 1e-6));
            }
        }
        let rs = &c["resample"];
        let (ph_len, or_len) = t.resample(&artic, &f, &mut ph, &mut or, Some(&mut ph_min), Some(&mut or_min));
        w = w.max(worst(&format!("{label} lengths"), &[num(&rs["phLen"]), num(&rs["orLen"])], &[ph_len, or_len], 1e-6));
        for (key, arr) in [("ph", &ph), ("or", &or), ("phMin", &ph_min), ("orMin", &or_min)] {
            w = w.max(worst(&format!("{label} {key}"), &nums(&rs[key]), arr, 1e-6));
        }
        assert!(w < 1e-9, "{label}: worst relative difference {w:e}");
        worst_all = worst_all.max(w);
    }
    if bless_it {
        return save_json("geometry", &cases);
    }
    println!("geometry: {} cases, worst relative difference {worst_all:e}", cases.as_array().unwrap().len());
}
