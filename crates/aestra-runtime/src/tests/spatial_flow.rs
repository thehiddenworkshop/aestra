use super::*;
#[test]
fn shared_fields_are_finite_bounded_coherent_and_curl_has_small_divergence() {
    for mode in [FlowMode::Noise, FlowMode::Curl] {
        for octaves in 1..=3 {
            let f = SpatialFlow {
                mode,
                octaves,
                strength: 2.0,
                ..Default::default()
            };
            for i in 0..128 {
                let p = [i as f32 * 0.173 - 8.0, 2.17, -0.91];
                let a = sample(p, 1.25, f).unwrap();
                assert_eq!(sample(p, 1.25, f), Some(a));
                assert!(a.into_iter().all(|v| v.is_finite() && v.abs() <= 2.001));
                let b = sample([p[0] + 0.0001, p[1], p[2]], 1.25, f).unwrap();
                assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 0.003));
                if mode == FlowMode::Curl {
                    let h = 1.0 / (128.0 * (1u32 << (octaves - 1)) as f32 * f.scale);
                    let mut divergence = 0.0;
                    for axis in 0..3 {
                        let mut a = p;
                        let mut b = p;
                        a[axis] += h;
                        b[axis] -= h;
                        divergence += (sample(a, 1.25, f).unwrap()[axis]
                            - sample(b, 1.25, f).unwrap()[axis])
                            / (2.0 * h);
                    }
                    assert!(
                        divergence.abs() < 0.003,
                        "octaves={octaves}, divergence={divergence}"
                    );
                }
            }
        }
    }
}
#[test]
fn world_sampling_and_acceleration_transform_respect_translation_rotation_and_scale() {
    let flow = SpatialFlow {
        space: FlowSpace::World,
        ..Default::default()
    };
    let pose = [
        [0.0, -2.0, 0.0, 10.0],
        [3.0, 0.0, 0.0, -5.0],
        [0.0, 0.0, 0.5, 1.0],
    ];
    let local = [2.0, 1.0, -3.0];
    let world = [8.0, 1.0, -0.5];
    let expected = sample(world, 2.0, flow).unwrap();
    let actual = acceleration(local, 2.0, flow, pose).unwrap();
    let reconstructed = [-2.0 * actual[1], 3.0 * actual[0], 0.5 * actual[2]];
    for (a, b) in expected.into_iter().zip(reconstructed) {
        assert!((a - b).abs() < 0.00001);
    }
    assert!(inverse_linear([[0.0; 4]; 3]).is_none());
    // A small but well-conditioned uniform scale is not singular merely because
    // its determinant is small in absolute units.
    let small = [
        [0.001, 0.0, 0.0, 0.0],
        [0.0, 0.001, 0.0, 0.0],
        [0.0, 0.0, 0.001, 0.0],
    ];
    let actual = acceleration([200.0, 100.0, -300.0], 2.0, flow, small).unwrap();
    let expected = sample([0.2, 0.1, -0.3], 2.0, flow).unwrap();
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a * 0.001 - b).abs() < 0.00001);
    }
    assert!(sample([f32::NAN, 0.0, 0.0], 0.0, flow).is_none());
}

#[test]
fn moving_deposits_keep_identity_drift_coherently_pause_and_restart_without_extra_storage() {
    use crate::{MAX_COLLIDERS, SpawnPlacement, SpawnShape, StatefulConfig, StatefulSimulation};
    let config = StatefulConfig {
        gravity: [0.0; 3],
        spawn_per_tick: 1,
        speed: (0.0, 0.0),
        lifetime: (3.0, 3.0),
        direction: [0.0, 1.0, 0.0],
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        spread: 0.0,
        drag: 0.3,
        shape: SpawnShape::Point,
        placement: SpawnPlacement::IDENTITY,
        turbulence: 0.0,
        colliders: [aestra_core::Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 256,
        homing: None,
    };
    let run = || {
        let mut sim = StatefulSimulation::new(config, 42);
        sim.set_spatial_flow(Some(SpatialFlow {
            strength: 2.0,
            ..Default::default()
        }))
        .unwrap();
        for tick in 0..120 {
            sim.set_placement(SpawnPlacement {
                translation: [tick as f32 * 0.05, 0.0, 0.0],
                ..SpawnPlacement::IDENTITY
            });
            sim.advance_tick();
        }
        sim
    };
    let sim = run();
    let paused = sim.clone();
    assert_eq!(sim, paused, "no tick means no flow evolution");
    assert_eq!(
        sim,
        run(),
        "restart restores the same shared field and trajectories"
    );
    let points = sim.alive_particles();
    assert_eq!(points.len(), 120);
    assert!(
        points[0].1[0].abs() < 2.0,
        "old smoke stays near its deposited path"
    );
    assert!(
        points[119].1[0] > 5.9,
        "new deposits follow the moving emitter"
    );
    assert!(
        points[0].1[1].abs() > 0.001 || points[0].1[2].abs() > 0.001,
        "flow changes particle motion"
    );
    let mut continued = sim.clone();
    continued.advance_to_tick(240);
    let mut checkpoint = sim;
    checkpoint.advance_to_tick(240);
    assert_eq!(
        continued, checkpoint,
        "checkpoint restoration retains the field and fixed clock"
    );
}
