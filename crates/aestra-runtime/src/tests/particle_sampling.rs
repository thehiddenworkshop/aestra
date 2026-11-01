use super::*;
use crate::particle_sampling::*;
use aestra_core::{EmitterShape, VelocityDistribution, particle_sampling::*};

#[test]
fn fibonacci_shells_keep_exact_counts_coverage_seed_and_jitter_contracts() {
    let mut settings = ParticleSampling {
        directions: SamplingMode::Stratified,
        positions: SamplingMode::Stratified,
        jitter: 0.0,
        seed: u32::MAX,
        ..Default::default()
    };
    for count in [1, 2, 3, 7, 64, 800] {
        let sample = |s, i| {
            direction(
                s,
                42,
                SamplingCohort {
                    index: i,
                    count,
                    first: 0,
                },
                VelocityDistribution::Sphere,
                [0.0, 1.0, 0.0],
                0.0,
            )
        };
        let points: Vec<_> = (0..count).map(|i| sample(settings, i)).collect();
        assert_eq!(points.len(), count as usize);
        let mean: [f32; 3] =
            std::array::from_fn(|a| points.iter().map(|p| p[a]).sum::<f32>() / count as f32);
        assert!(mean.iter().all(|v| v.abs() < 2.0 / count as f32));
        for (i, p) in points.iter().enumerate() {
            assert!((p.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 2e-6);
            assert_eq!(*p, sample(settings, i as u32));
            for q in &points[..i] {
                let d2 = p.iter().zip(q).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
                assert!(d2 > 0.3 / count as f32, "count={count}, near duplicate");
            }
        }
        assert_ne!(
            points[0],
            sample(
                ParticleSampling {
                    seed: 1,
                    ..settings
                },
                0
            )
        );
    }
    settings.jitter = 1.0;
    for i in 0..800 {
        let c = SamplingCohort {
            index: i,
            count: 800,
            first: 123,
        };
        let uvw = coordinates(settings, SamplingMode::Stratified, 0, c, 1);
        assert!(uvw.iter().all(|v| v.is_finite() && *v >= 0.0 && *v < 1.0));
        assert!((i as f32 / 800.0..(i + 1) as f32 / 800.0).contains(&uvw[0]));
    }
    // Integer fixed-point azimuth does not lose precision for late large ordinals.
    let c = SamplingCohort::sequence(u32::MAX, 64);
    assert_eq!(c.index, 63);
    assert_eq!(c.first, u32::MAX - 63);
}

#[test]
fn uniform_and_stratified_positions_have_volume_area_and_support_contracts() {
    let count = 8192;
    for mode in [SamplingMode::Uniform, SamplingMode::Stratified] {
        let settings = ParticleSampling {
            positions: mode,
            jitter: 0.0,
            ..Default::default()
        };
        let mut radial_cube = 0.0;
        let mut disk_radius_squared = 0.0;
        let mut cone_height_cube = 0.0;
        for i in 0..count {
            let c = SamplingCohort {
                index: i,
                count,
                first: 0,
            };
            let p = position(settings, 42, c, EmitterShape::Sphere { radius: 1.0 });
            let r2 = p.iter().map(|v| v * v).sum::<f32>();
            assert!(r2 <= 1.00001);
            radial_cube += r2.powf(1.5) / count as f32;
            let p = position(settings, 42, c, EmitterShape::Circle { radius: 1.0 });
            assert_eq!(p[2], 0.0);
            disk_radius_squared += (p[0] * p[0] + p[1] * p[1]) / count as f32;
            let p = position(
                settings,
                42,
                c,
                EmitterShape::Cone {
                    radius: 1.0,
                    depth: 1.0,
                },
            );
            assert!(p[0] * p[0] + p[2] * p[2] <= p[1] * p[1] + 1e-6);
            cone_height_cube += p[1].powi(3) / count as f32;
            for shape in [
                EmitterShape::Point,
                EmitterShape::Ring { radius: 1.0 },
                EmitterShape::Hemisphere { radius: 1.0 },
                EmitterShape::Box {
                    half_extents: [1.0; 3],
                },
                EmitterShape::Cylinder {
                    radius: 1.0,
                    depth: 2.0,
                },
            ] {
                let p = position(settings, 42, c, shape);
                assert!(p.iter().all(|v| v.is_finite() && v.abs() <= 1.00001));
                if matches!(shape, EmitterShape::Hemisphere { .. }) {
                    assert!(p[1] >= 0.0);
                }
            }
            for velocity in VelocityDistribution::ALL.into_iter().skip(1) {
                let d = direction(
                    ParticleSampling {
                        directions: mode,
                        ..settings
                    },
                    42,
                    c,
                    velocity,
                    [0.0, 1.0, 0.0],
                    60.0,
                );
                assert!(d.iter().all(|v| v.is_finite()));
                let len = d.iter().map(|v| v * v).sum::<f32>();
                assert!(len <= 1.00001);
                if velocity == VelocityDistribution::Cone {
                    assert!(d[1] >= 30f32.to_radians().cos() - 1e-6);
                }
            }
        }
        for mean in [radial_cube, disk_radius_squared, cone_height_cube] {
            assert!((mean - 0.5).abs() < 0.02, "{mode:?}: {mean}");
        }
    }
}

#[test]
fn event_cohorts_keep_requested_layout_under_changing_counts_capacity_and_restore() {
    let config = StatefulConfig {
        gravity: [0.0; 3],
        spawn_per_tick: 0,
        speed: (10.0, 12.0),
        lifetime: (2.0, 3.0),
        direction: [0.0, 1.0, 0.0],
        velocity_distribution: VelocityDistribution::Sphere,
        spread: 0.0,
        drag: 0.0,
        shape: SpawnShape::Point,
        placement: SpawnPlacement::IDENTITY,
        turbulence: 0.0,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 19,
        homing: None,
    };
    let s = ParticleSampling {
        directions: SamplingMode::Stratified,
        positions: SamplingMode::Legacy,
        jitter: 0.1,
        ..Default::default()
    };
    let mut sim = StatefulSimulation::new(config, 42);
    sim.set_particle_sampling(Some(s)).unwrap();
    let events = [
        ParticleEvent {
            ordinal: 4,
            position: [1.0, 2.0, 3.0],
            velocity: [0.0; 3],
        },
        ParticleEvent {
            ordinal: 2,
            position: [-1.0, 2.0, 3.0],
            velocity: [0.0; 3],
        },
    ];
    sim.spawn_from_events(&events, 7, 0.0);
    assert_eq!(sim.live_count(), 14);
    for (i, p) in sim.particles.iter().enumerate() {
        let c = SamplingCohort {
            index: i as u32 % 7,
            count: 7,
            first: i as u32 - i as u32 % 7,
        };
        let d = direction(
            s,
            42,
            c,
            VelocityDistribution::Sphere,
            config.direction,
            0.0,
        );
        let speed = config.speed.0
            + (config.speed.1 - config.speed.0)
                * StatefulSimulation::spawn_uniform(42, i as u64, 0);
        assert_eq!(p.velocity, d.map(|v| v * speed));
        assert_eq!(p.position, events[if i < 7 { 1 } else { 0 }].position);
        let life = config.lifetime.0
            + (config.lifetime.1 - config.lifetime.0)
                * StatefulSimulation::spawn_uniform(42, i as u64, 1);
        assert_eq!(p.lifetime, life);
    }
    let checkpoint = sim.clone();
    sim.spawn_from_events(&events, 3, 0.0);
    assert_eq!(sim.live_count(), 19);
    let mut restored = checkpoint;
    restored.spawn_from_events(&events, 3, 0.0);
    assert_eq!(sim, restored);
    // The truncated second event keeps its requested count of 3, not the 2 accepted slots.
    for p in &sim.particles[17..] {
        let c = SamplingCohort {
            index: p.id as u32 - 17,
            count: 3,
            first: 17,
        };
        let d = direction(
            s,
            42,
            c,
            VelocityDistribution::Sphere,
            config.direction,
            0.0,
        );
        let speed = config.speed.0
            + (config.speed.1 - config.speed.0) * StatefulSimulation::spawn_uniform(42, p.id, 0);
        assert_eq!(p.velocity, d.map(|v| v * speed));
    }
}
