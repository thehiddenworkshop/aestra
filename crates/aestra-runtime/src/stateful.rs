//! A minimal, deterministic stateful particle reference (hybrid roadmap M5).
//!
//! This exists to validate the stateful execution and seek model against the analytic path — it is
//! not a shipping feature. It maintains persistent per-particle state (position, velocity, age,
//! lifetime), advances on the canonical fixed 60 Hz tick with semi-implicit Euler integration, and
//! extracts renderer-neutral presentation samples from that state.
//!
//! A clone of a [`StatefulSimulation`] is a checkpoint (GPU-resident checkpoints generalize this
//! later, §19). Backward seeks are never run in reverse: restore a checkpoint and replay forward
//! (§16).

use crate::{DEFAULT_PLAYBACK_TICK_RATE, ParticleSample, SimulationClass, SimulationStateLayout};

/// Fixed configuration for the prototype stateful integrator. Richer than a single speed/lifetime:
/// per-particle random speed and lifetime ranges, an authored launch direction with a spread cone, and
/// linear drag. Every axis is deterministic from `(seed, ordinal)` and — deliberately — trig-free, so
/// the GPU kernels can reproduce it bit-for-bit (`normalize` uses only IEEE-correctly-rounded `sqrt`
/// and division; the cone is `normalize(direction + spread * random_unit)`, not a trig cone).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatefulConfig {
    /// Constant acceleration applied to velocity each tick.
    pub gravity: [f32; 3],
    /// Particles spawned per tick (bounded by `capacity`).
    pub spawn_per_tick: u32,
    /// Initial particle speed, sampled per particle uniformly in `[min, max]`.
    pub speed: (f32, f32),
    /// Particle lifetime in seconds, sampled per particle uniformly in `[min, max]`.
    pub lifetime: (f32, f32),
    /// The base launch direction; normalized at spawn (a zero vector falls back to the random unit).
    pub direction: [f32; 3],
    /// Cone spread: `0` launches straight along `direction`, larger values blend in more of the
    /// per-particle random unit vector before renormalizing.
    pub spread: f32,
    /// Linear velocity damping per second (`v -= drag * v * dt` each tick); `0` disables it.
    pub drag: f32,
    /// Maximum live particles; spawning stops at this bound (bounded allocation).
    pub capacity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StateParticle {
    /// The particle's spawn ordinal — its stable identity, used to match against the GPU backend
    /// (whose parallel slot assignment need not match this reference's).
    id: u64,
    position: [f32; 3],
    velocity: [f32; 3],
    age: f32,
    lifetime: f32,
}

/// A minimal fixed-tick stateful simulation. Deterministic from `(config, seed)`; `Clone` is a
/// checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct StatefulSimulation {
    config: StatefulConfig,
    seed: u64,
    tick: u64,
    /// Total particles ever spawned; the ordinal that seeds each particle's deterministic launch.
    spawned: u64,
    particles: Vec<StateParticle>,
}

impl StatefulSimulation {
    /// The canonical fixed simulation timestep (seconds).
    pub const TICK_DT: f32 = 1.0 / DEFAULT_PLAYBACK_TICK_RATE as f32;

    pub fn new(config: StatefulConfig, seed: u64) -> Self {
        Self {
            config,
            seed,
            tick: 0,
            spawned: 0,
            particles: Vec::new(),
        }
    }

    /// The absolute fixed tick this simulation has reached.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of live particles.
    pub fn live_count(&self) -> usize {
        self.particles.len()
    }

    /// Every live particle as `(spawn ordinal, position)`, for identity-based conformance against a
    /// backend whose slot assignment differs from this reference's.
    pub fn alive_particles(&self) -> Vec<(u64, [f32; 3])> {
        self.particles
            .iter()
            .map(|particle| (particle.id, particle.position))
            .collect()
    }

    /// The persistent simulation-state layout this reference maintains (hybrid M4).
    pub fn state_layout() -> SimulationStateLayout {
        SimulationStateLayout::for_class(SimulationClass::Stateful)
    }

    /// The deterministic 64-bit spawn hash (splitmix64). Exposed as the *canonical* definition so the
    /// GPU spawn kernel (which emulates `u64` with `u32` pairs) can be conformance-checked against it.
    pub fn splitmix64(input: u64) -> u64 {
        splitmix64(input)
    }

    /// The deterministic launch direction (components in `[-1, 1]`) for a particle's spawn ordinal.
    /// Canonical for both this CPU reference and the GPU spawn kernel.
    pub fn launch_direction(seed: u64, ordinal: u64) -> [f32; 3] {
        launch_direction(seed, ordinal)
    }

    /// The full per-particle launch velocity (direction cone × sampled speed) for a spawn ordinal.
    /// Canonical for both this CPU reference and the GPU spawn kernel.
    pub fn launch_velocity(config: &StatefulConfig, seed: u64, ordinal: u64) -> [f32; 3] {
        launch_velocity(config, seed, ordinal)
    }

    /// The deterministic per-particle uniform in `[0, 1)` for a channel (speed = 0, lifetime = 1).
    /// Canonical for both this CPU reference and the GPU spawn kernel.
    pub fn spawn_uniform(seed: u64, ordinal: u64, channel: u64) -> f32 {
        spawn_uniform(seed, ordinal, channel)
    }

    /// Advances exactly one fixed tick: integrate alive particles, retire the dead, then spawn.
    pub fn advance_tick(&mut self) {
        let dt = Self::TICK_DT;
        let drag = self.config.drag;
        for particle in &mut self.particles {
            // Semi-implicit (symplectic) Euler with linear drag: velocity first (gravity, then
            // damping), then position.
            for axis in 0..3 {
                let with_gravity = particle.velocity[axis] + self.config.gravity[axis] * dt;
                let damped = with_gravity - drag * with_gravity * dt;
                particle.velocity[axis] = damped;
                particle.position[axis] += damped * dt;
            }
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        let room = (self.config.capacity as usize).saturating_sub(self.particles.len());
        let spawn = (self.config.spawn_per_tick as usize).min(room);
        for _ in 0..spawn {
            let ordinal = self.spawned;
            let velocity = launch_velocity(&self.config, self.seed, ordinal);
            let lifetime = lerp(
                self.config.lifetime.0,
                self.config.lifetime.1,
                spawn_uniform(self.seed, ordinal, 1),
            );
            self.particles.push(StateParticle {
                id: ordinal,
                position: [0.0; 3],
                velocity,
                age: 0.0,
                lifetime,
            });
            self.spawned += 1;
        }
        self.tick += 1;
    }

    /// Advances forward to an absolute tick. Panics on a backward seek — restore a checkpoint and
    /// replay forward instead; the simulation is never run with negative `dt` (§16).
    pub fn advance_to_tick(&mut self, target: u64) {
        assert!(
            target >= self.tick,
            "stateful simulation cannot run backwards; restore a checkpoint and replay forward"
        );
        while self.tick < target {
            self.advance_tick();
        }
    }

    /// Extracts renderer-neutral presentation samples from the persistent state — the boundary the
    /// 48-byte presentation ABI sits behind.
    pub fn present(&self, out: &mut Vec<ParticleSample>) {
        out.clear();
        for (index, particle) in self.particles.iter().enumerate() {
            let normalized_age = if particle.lifetime > 0.0 {
                (particle.age / particle.lifetime).clamp(0.0, 1.0)
            } else {
                0.0
            };
            out.push(ParticleSample {
                emitter_index: 0,
                particle_index: index as u32,
                position: particle.position,
                size: 1.0,
                rotation: 0.0,
                color: [1.0, 1.0, 1.0, 1.0],
                normalized_age,
            });
        }
    }
}

/// The full launch velocity for a particle: an authored direction blended with the per-particle random
/// unit vector to a spread cone, renormalized, and scaled by a per-particle random speed. Canonical for
/// both this CPU reference and the GPU spawn kernel — trig-free so they match bit-for-bit.
fn launch_velocity(config: &StatefulConfig, seed: u64, ordinal: u64) -> [f32; 3] {
    let random_unit = launch_direction(seed, ordinal);
    // Cone: base direction + spread * random unit, renormalized. Falls back to the random unit when
    // the base direction is zero (matching the GPU, which normalizes the same mixed vector).
    let mixed = [
        config.direction[0] + config.spread * random_unit[0],
        config.direction[1] + config.spread * random_unit[1],
        config.direction[2] + config.spread * random_unit[2],
    ];
    let direction = normalize_or(mixed, random_unit);
    let speed = lerp(
        config.speed.0,
        config.speed.1,
        spawn_uniform(seed, ordinal, 0),
    );
    [
        direction[0] * speed,
        direction[1] * speed,
        direction[2] * speed,
    ]
}

/// A deterministic launch direction (components in `[-1, 1]`) from the seed and the particle's spawn
/// ordinal. Uses splitmix64 so the same `(seed, ordinal)` always yields the same direction — the
/// determinism the whole seek model relies on.
fn launch_direction(seed: u64, ordinal: u64) -> [f32; 3] {
    let base = splitmix64(seed ^ ordinal.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    [
        unit_signed(base),
        unit_signed(splitmix64(base)),
        unit_signed(splitmix64(base ^ 0xD1B5_4A32_D192_ED03)),
    ]
}

/// A deterministic per-particle uniform in `[0, 1)` for a named `channel` (speed = 0, lifetime = 1).
/// Salted distinctly from the direction hash so the samples are independent. Canonical for both sides.
fn spawn_uniform(seed: u64, ordinal: u64, channel: u64) -> f32 {
    let hash = splitmix64(
        seed ^ ordinal.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (channel.wrapping_add(1)).wrapping_mul(0xD6E8_FEB8_6659_FD93),
    );
    unit01(hash)
}

/// Maps a hash to `[0, 1)` using 24 bits of precision.
fn unit01(hash: u64) -> f32 {
    (hash >> 40) as f32 / (1u64 << 24) as f32
}

fn lerp(min: f32, max: f32, t: f32) -> f32 {
    min + (max - min) * t
}

/// Normalizes `v`, or returns `fallback` when `v` is (near) zero. Uses only `sqrt` and division, both
/// IEEE-correctly-rounded, so the GPU reproduces it exactly.
fn normalize_or(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length_squared = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if length_squared > 1e-12 {
        let length = length_squared.sqrt();
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        fallback
    }
}

fn splitmix64(input: u64) -> u64 {
    let mut z = input.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Maps a hash to `[-1, 1)` using 24 bits of precision — ample for a direction.
fn unit_signed(hash: u64) -> f32 {
    let unit = (hash >> 40) as f32 / (1u64 << 24) as f32;
    unit * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> StatefulConfig {
        StatefulConfig {
            gravity: [0.0, -9.81, 0.0],
            spawn_per_tick: 4,
            speed: (10.0, 14.0),
            lifetime: (1.2, 1.8),
            direction: [0.0, 1.0, 0.0],
            spread: 0.4,
            drag: 0.5,
            capacity: 128,
        }
    }

    #[test]
    fn identical_seeds_produce_identical_fixed_tick_state() {
        let (mut a, mut b) = (
            StatefulSimulation::new(config(), 0x1234),
            StatefulSimulation::new(config(), 0x1234),
        );
        a.advance_to_tick(150);
        b.advance_to_tick(150);
        assert_eq!(
            a, b,
            "the stateful reference is deterministic from its seed"
        );
        assert!(a.live_count() > 0, "particles are alive at tick 150");
    }

    #[test]
    fn advance_to_tick_matches_tick_by_tick_replay() {
        let mut jumped = StatefulSimulation::new(config(), 7);
        jumped.advance_to_tick(120);
        let mut stepped = StatefulSimulation::new(config(), 7);
        for _ in 0..120 {
            stepped.advance_tick();
        }
        assert_eq!(jumped, stepped);
    }

    #[test]
    fn a_checkpoint_clone_replays_to_the_same_state_as_uninterrupted_simulation() {
        // Uninterrupted forward run to tick 200.
        let mut uninterrupted = StatefulSimulation::new(config(), 99);
        uninterrupted.advance_to_tick(200);

        // Run to tick 80, snapshot (clone), then replay the snapshot forward to 200.
        let mut live = StatefulSimulation::new(config(), 99);
        live.advance_to_tick(80);
        let mut checkpoint = live.clone();
        checkpoint.advance_to_tick(200);

        assert_eq!(
            checkpoint, uninterrupted,
            "restoring a checkpoint and replaying forward reaches the uninterrupted state"
        );
    }

    #[test]
    fn presentation_extraction_reflects_integrated_motion() {
        let mut simulation = StatefulSimulation::new(config(), 3);
        simulation.advance_to_tick(30);
        let mut samples = Vec::new();
        simulation.present(&mut samples);
        assert_eq!(samples.len(), simulation.live_count());
        // Integration has moved older particles off the spawn origin (the freshest batch, spawned
        // this tick at age 0, is still at the origin — it integrates next tick).
        assert!(
            samples.iter().any(|sample| sample.position != [0.0; 3]),
            "integrated particles have left the origin"
        );
        assert!(
            samples
                .iter()
                .all(|sample| (0.0..=1.0).contains(&sample.normalized_age)),
        );
    }

    #[test]
    #[should_panic(expected = "cannot run backwards")]
    fn seeking_backwards_panics_instead_of_integrating_negative_dt() {
        let mut simulation = StatefulSimulation::new(config(), 1);
        simulation.advance_to_tick(60);
        simulation.advance_to_tick(30);
    }

    #[test]
    fn capacity_bounds_the_live_particle_count() {
        let bounded = StatefulConfig {
            capacity: 10,
            lifetime: (1000.0, 1000.0),
            ..config()
        };
        let mut simulation = StatefulSimulation::new(bounded, 5);
        simulation.advance_to_tick(100);
        assert!(
            simulation.live_count() <= 10,
            "capacity bounds live particles"
        );
    }

    #[test]
    fn richer_dynamics_vary_per_particle_and_launch_along_the_cone() {
        // Speed and lifetime are sampled per particle in range, and the launch direction is a
        // renormalized cone around `direction`, so particles are not identical and the samples stay in
        // their authored bounds.
        let seed = 0x51ED;
        let mut speeds = Vec::new();
        let mut lifetimes = Vec::new();
        for ordinal in 0..64 {
            let velocity = StatefulSimulation::launch_velocity(&config(), seed, ordinal);
            let speed = (velocity[0].powi(2) + velocity[1].powi(2) + velocity[2].powi(2)).sqrt();
            assert!(
                (10.0 - 1e-3..=14.0 + 1e-3).contains(&speed),
                "sampled speed {speed} is within [10, 14]"
            );
            let lifetime = lerp(
                config().lifetime.0,
                config().lifetime.1,
                StatefulSimulation::spawn_uniform(seed, ordinal, 1),
            );
            assert!((1.2..=1.8).contains(&lifetime));
            // spread 0.4 around +Y keeps the dominant component pointing up.
            assert!(velocity[1] > 0.0, "the cone launches upward along +Y");
            speeds.push(speed);
            lifetimes.push(lifetime);
        }
        assert!(
            speeds.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "speeds vary per particle"
        );
        assert!(
            lifetimes.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "lifetimes vary per particle"
        );
    }

    #[test]
    fn drag_slows_particles_relative_to_no_drag() {
        // With gravity disabled and a straight-up launch, drag must reduce the height reached.
        let base = StatefulConfig {
            gravity: [0.0, 0.0, 0.0],
            spawn_per_tick: 1,
            speed: (20.0, 20.0),
            lifetime: (100.0, 100.0),
            direction: [0.0, 1.0, 0.0],
            spread: 0.0,
            drag: 0.0,
            capacity: 8,
        };
        let dragged = StatefulConfig { drag: 2.0, ..base };
        let mut without = StatefulSimulation::new(base, 1);
        let mut with = StatefulSimulation::new(dragged, 1);
        without.advance_to_tick(60);
        with.advance_to_tick(60);
        let height = |sim: &StatefulSimulation| sim.alive_particles()[0].1[1];
        assert!(
            height(&with) < height(&without),
            "drag reduces the height reached ({} vs {})",
            height(&with),
            height(&without)
        );
    }
}
