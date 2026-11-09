use chimera::physics::CM_H2O;
use chimera::sources::glottis::ParametricGlottis;

fn cycles(attenuation: f64) -> Vec<(usize, f64)> {
    let mut source = ParametricGlottis::new(48000.0);
    source.params.f0 = 150.0;
    source.params.jitter = 0.0;
    source.params.shimmer = 0.0;
    source.params.drift = 0.0;
    source.params.chink = 0.0;
    source.params.alternating_attenuation = attenuation;
    let mut result = Vec::new();
    let mut start = 0;
    let mut maximum = 0.0_f64;
    let mut previous_phase = 0.0;
    for sample in 0..24000 {
        let area = source.next(10.0 * CM_H2O);
        let phase = source.cycle_phase();
        if phase < previous_phase {
            if sample > 12000 {
                result.push((sample - start, maximum));
            }
            start = sample;
            maximum = 0.0;
        }
        maximum = maximum.max(area);
        previous_phase = phase;
    }
    result
}

#[test]
fn alternate_pulse_attenuation_preserves_the_vocal_fold_period() {
    let normal = cycles(0.0);
    let throat = cycles(0.85);
    assert_eq!(normal.len(), throat.len());
    for ((normal_period, _), (throat_period, _)) in normal.iter().zip(&throat) {
        assert_eq!(normal_period, throat_period);
        assert!((319..=321).contains(throat_period));
    }
    for pair in throat.windows(2) {
        let low = pair[0].1.min(pair[1].1);
        let high = pair[0].1.max(pair[1].1);
        assert!((low / high - 0.15).abs() < 0.001);
    }
    for pair in normal.windows(2) {
        assert!((pair[0].1 / pair[1].1 - 1.0).abs() < 0.001);
    }
}
