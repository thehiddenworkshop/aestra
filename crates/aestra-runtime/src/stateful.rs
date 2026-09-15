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

/// Fixed configuration for the prototype stateful integrator. Deliberately tiny.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatefulConfig {
    /// Constant acceleration applied to velocity each tick.
    pub gravity: [f32; 3],
    /// Particles spawned per tick (bounded by `capacity`).
    pub spawn_per_tick: u32,
    /// Initial particle speed along its deterministic launch direction.
    pub initial_speed: f32,
    /// Particle lifetime in seconds.
    pub lifetime: f32,
    /// Maximum live particles; spawning stops at this bound (bounded allocation).
    pub capacity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StateParticle {
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

    /// The persistent simulation-state layout this reference maintains (hybrid M4).
    pub fn state_layout() -> SimulationStateLayout {
        SimulationStateLayout::for_class(SimulationClass::Stateful)
    }

    /// Advances exactly one fixed tick: integrate alive particles, retire the dead, then spawn.
    pub fn advance_tick(&mut self) {
        let dt = Self::TICK_DT;
        for particle in &mut self.particles {
            // Semi-implicit (symplectic) Euler: update velocity first, then position.
            for axis in 0..3 {
                particle.velocity[axis] += self.config.gravity[axis] * dt;
                particle.position[axis] += particle.velocity[axis] * dt;
            }
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        let room = (self.config.capacity as usize).saturating_sub(self.particles.len());
        let spawn = (self.config.spawn_per_tick as usize).min(room);
        for _ in 0..spawn {
            let direction = launch_direction(self.seed, self.spawned);
            let velocity = [
                direction[0] * self.config.initial_speed,
                direction[1] * self.config.initial_speed,
                direction[2] * self.config.initial_speed,
            ];
            self.particles.push(StateParticle {
                position: [0.0; 3],
                velocity,
                age: 0.0,
                lifetime: self.config.lifetime,
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
            initial_speed: 12.0,
            lifetime: 1.5,
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
            lifetime: 1000.0,
            ..config()
        };
        let mut simulation = StatefulSimulation::new(bounded, 5);
        simulation.advance_to_tick(100);
        assert!(
            simulation.live_count() <= 10,
            "capacity bounds live particles"
        );
    }
}
