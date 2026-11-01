use super::*;

#[test]
fn seeded_fields_are_continuous_periodic_bounded_and_filter_to_the_mean() {
    for basis in [NoiseBasis::Value, NoiseBasis::Gradient] {
        for dimensions in [2, 3] {
            let settings = NoiseSettings {
                basis,
                dimensions,
                scale: 1.0,
                seed: 17,
                octaves: 5,
                ..Default::default()
            };
            let mut minimum = 1.0f32;
            let mut maximum = 0.0f32;
            for i in 0..512 {
                let p = [i as f32 * 0.071 - 17.0, -2.75, i as f32 * 0.013];
                let a = sample(p, &settings).unwrap();
                assert!((0.0..=1.0).contains(&a));
                assert_eq!(sample(p, &settings), Some(a));
                minimum = minimum.min(a);
                maximum = maximum.max(a);
            }
            assert!(maximum - minimum > 0.1);
            let a = sample([-1.00001, 0.27, 0.41], &settings).unwrap();
            let b = sample([-0.99999, 0.27, 0.41], &settings).unwrap();
            assert!((a - b).abs() < 0.001);
            let p = [-1.25, 0.5, 0.75];
            assert_eq!(
                sample(p, &settings),
                sample([p[0] + NOISE_PERIOD, p[1], p[2]], &settings)
            );
            assert_ne!(
                sample(p, &settings),
                sample(
                    p,
                    &NoiseSettings {
                        seed: 19,
                        ..settings
                    }
                )
            );
            assert_eq!(
                sample(
                    p,
                    &NoiseSettings {
                        footprint: 1.0,
                        amplitude: 2.0,
                        ..settings
                    }
                ),
                Some(1.0)
            );
            if dimensions == 2 {
                assert_eq!(sample(p, &settings), sample([p[0], p[1], 100.0], &settings));
            }
        }
    }
    assert!(sample([f32::NAN, 0.0, 0.0], &NoiseSettings::default()).is_none());
}
