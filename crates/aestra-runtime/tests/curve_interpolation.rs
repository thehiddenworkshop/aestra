use aestra_core::{Curve, CurveInterpolation, CurveKey, ScalarRange};
use aestra_runtime::CompiledCurve;

#[test]
fn compiled_sampling_and_emission_integrals_match_each_mode() {
    for mode in [
        CurveInterpolation::Step,
        CurveInterpolation::Linear,
        CurveInterpolation::Smooth,
    ] {
        let mut curve = Curve::normalized(
            vec![
                CurveKey::new(0.0, 0.0),
                CurveKey::new(0.4, 1.0),
                CurveKey::new(1.0, 0.3),
            ],
            ScalarRange::new(2.0, 10.0),
        );
        curve.interpolation = mode;
        let compiled = CompiledCurve::compile(&curve);
        assert_eq!(compiled.interpolation(), mode);
        for time in [0.0, 0.1, 0.4, 0.7, 1.0] {
            assert!((compiled.sample(time) - curve.sample(time)).abs() < 1e-5);
            let numeric: f32 = (0..10000)
                .map(|i| curve.sample(time * (i as f32 + 0.5) / 10000.0) * time / 10000.0)
                .sum();
            assert!(
                (compiled.integral(time) - numeric).abs() < 0.002,
                "{mode:?} at {time}"
            );
        }
        curve.keys[1].time = 4.0;
        curve.keys[2].time = 8.0;
        let compiled = CompiledCurve::compile(&curve);
        for time in [0.0, 1.0, 4.0, 6.0, 8.0, 12.0] {
            assert!((compiled.sample_at(time) - curve.sample_at(time)).abs() < 1e-5);
        }
    }
}
