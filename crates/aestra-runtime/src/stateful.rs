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

pub use aestra_core::{Collider, ColliderShape};

/// The volume new particles spawn within (hybrid roadmap M6). Sampled per particle from deterministic
/// uniforms, trig-free so the GPU reproduces it bit-for-bit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpawnShape {
    /// All particles spawn at the emitter origin.
    Point,
    /// A filled sphere of the given radius (radial distance from `sqrt` of a uniform, so no `cbrt`).
    Sphere { radius: f32 },
    /// An axis-aligned box spanning `[-half_extents, half_extents]`.
    Box { half_extents: [f32; 3] },
}

/// The maximum colliders one stateful emitter carries (hybrid roadmap M10). Packed into the params
/// buffer, so kept small; enough for a ground plane plus a few obstacles.
pub const MAX_COLLIDERS: usize = 4;

/// Fixed configuration for the prototype stateful integrator. Richer than a single speed/lifetime:
/// per-particle random speed and lifetime ranges, an authored launch direction with a spread cone,
/// linear drag, a spawn shape, and value-noise turbulence. Every axis is deterministic from
/// `(seed, ordinal[, age])` and — deliberately — trig-free, so the GPU kernels can reproduce it
/// bit-for-bit (`normalize` and shapes use only IEEE-correctly-rounded `sqrt`/division; turbulence is
/// hash value noise with a polynomial smoothstep, never `sin`).
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
    /// The volume particles spawn within.
    pub shape: SpawnShape,
    /// Procedural turbulence strength: a per-particle, per-axis value-noise acceleration that evolves
    /// with the particle's age. `0` disables it.
    pub turbulence: f32,
    /// Collision primitives applied after integration each tick (hybrid roadmap M10). Only the first
    /// `collider_count` are active; the rest are ignored.
    pub colliders: [Collider; MAX_COLLIDERS],
    /// The number of active entries in `colliders` (`0` disables collision).
    pub collider_count: u32,
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

    /// The deterministic initial position sampled from the spawn shape for a spawn ordinal. Canonical
    /// for both this CPU reference and the GPU spawn kernel.
    pub fn launch_position(config: &StatefulConfig, seed: u64, ordinal: u64) -> [f32; 3] {
        launch_position(config, seed, ordinal)
    }

    /// The deterministic per-axis turbulence acceleration for a particle at `age`. Canonical for both
    /// this CPU reference and the GPU integrate kernel.
    pub fn turbulence_acceleration(seed: u64, ordinal: u64, age: f32, strength: f32) -> [f32; 3] {
        turbulence_acceleration(seed, ordinal, age, strength)
    }

    /// Resolves the config's colliders against a particle's post-integration `position`/`velocity`,
    /// returning whether it was killed. Canonical for both this CPU reference and the GPU death loop.
    pub fn resolve_colliders(
        config: &StatefulConfig,
        position: &mut [f32; 3],
        velocity: &mut [f32; 3],
    ) -> bool {
        resolve_colliders(
            &config.colliders,
            config.collider_count,
            position,
            velocity,
        )
    }

    /// Advances exactly one fixed tick: integrate alive particles, retire the dead, then spawn.
    pub fn advance_tick(&mut self) {
        let dt = Self::TICK_DT;
        let drag = self.config.drag;
        for particle in &mut self.particles {
            // Semi-implicit (symplectic) Euler with linear drag and value-noise turbulence: the
            // per-axis acceleration (gravity + turbulence at the current age) updates velocity first
            // (then damping), then position.
            let turbulence = turbulence_acceleration(
                self.seed,
                particle.id,
                particle.age,
                self.config.turbulence,
            );
            for (axis, &turb) in turbulence.iter().enumerate() {
                let acceleration = self.config.gravity[axis] + turb;
                let with_acceleration = particle.velocity[axis] + acceleration * dt;
                let damped = with_acceleration - drag * with_acceleration * dt;
                particle.velocity[axis] = damped;
                particle.position[axis] += damped * dt;
            }
            // Collision resolution against the authored colliders, in order. A killed particle is
            // retired immediately by forcing `age == lifetime` so the shared death check retires it
            // (matching the GPU death loop, which frees the slot on the same condition).
            let killed = resolve_colliders(
                &self.config.colliders,
                self.config.collider_count,
                &mut particle.position,
                &mut particle.velocity,
            );
            if killed {
                particle.age = particle.lifetime;
            } else {
                particle.age += dt;
            }
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        let room = (self.config.capacity as usize).saturating_sub(self.particles.len());
        let spawn = (self.config.spawn_per_tick as usize).min(room);
        for _ in 0..spawn {
            let ordinal = self.spawned;
            let velocity = launch_velocity(&self.config, self.seed, ordinal);
            let position = launch_position(&self.config, self.seed, ordinal);
            let lifetime = lerp(
                self.config.lifetime.0,
                self.config.lifetime.1,
                spawn_uniform(self.seed, ordinal, 1),
            );
            self.particles.push(StateParticle {
                id: ordinal,
                position,
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

/// The deterministic initial position for a particle, sampled from the emitter's spawn shape. Uses
/// only signed uniforms and `sqrt` — trig- and `cbrt`-free — so the GPU reproduces it bit-for-bit.
fn launch_position(config: &StatefulConfig, seed: u64, ordinal: u64) -> [f32; 3] {
    match config.shape {
        SpawnShape::Point => [0.0; 3],
        SpawnShape::Box { half_extents } => [
            spawn_signed(seed, ordinal, 2) * half_extents[0],
            spawn_signed(seed, ordinal, 3) * half_extents[1],
            spawn_signed(seed, ordinal, 4) * half_extents[2],
        ],
        SpawnShape::Sphere { radius } => {
            let vector = [
                spawn_signed(seed, ordinal, 2),
                spawn_signed(seed, ordinal, 3),
                spawn_signed(seed, ordinal, 4),
            ];
            let direction = normalize_or(vector, [0.0, 1.0, 0.0]);
            let distance = radius * spawn_uniform(seed, ordinal, 5).sqrt();
            [
                direction[0] * distance,
                direction[1] * distance,
                direction[2] * distance,
            ]
        }
    }
}

/// The per-axis turbulence acceleration for a particle at `age`: independent value noise per axis,
/// scaled by `strength`. Value noise (hash per integer cell, smoothstep-interpolated) is trig-free, so
/// it matches the GPU bit-for-bit, and it depends only on `(seed, ordinal, age)` — all persistent — so
/// a checkpoint restore reproduces it exactly.
fn turbulence_acceleration(seed: u64, ordinal: u64, age: f32, strength: f32) -> [f32; 3] {
    if strength == 0.0 {
        return [0.0; 3];
    }
    [
        strength * turbulence_noise(seed, ordinal, 10, age),
        strength * turbulence_noise(seed, ordinal, 11, age),
        strength * turbulence_noise(seed, ordinal, 12, age),
    ]
}

/// One axis of value noise in `[-1, 1)`: hash the integer cell of `age * FREQ` and the next, and
/// smoothstep-interpolate between them by the fractional part.
fn turbulence_noise(seed: u64, ordinal: u64, channel: u64, age: f32) -> f32 {
    const FREQ: f32 = 3.0;
    let t = age * FREQ;
    let cell = t.floor();
    let frac = t - cell;
    let cell = cell as u64;
    let a = unit_signed(turbulence_hash(seed, ordinal, channel, cell));
    let b = unit_signed(turbulence_hash(seed, ordinal, channel, cell + 1));
    let smooth = frac * frac * (3.0 - 2.0 * frac);
    a + (b - a) * smooth
}

fn turbulence_hash(seed: u64, ordinal: u64, channel: u64, cell: u64) -> u64 {
    let key = channel
        .wrapping_mul(0x1_0000)
        .wrapping_add(cell)
        .wrapping_add(1);
    splitmix64(
        seed ^ ordinal.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ key.wrapping_mul(0xD6E8_FEB8_6659_FD93),
    )
}

/// A deterministic per-particle signed uniform in `[-1, 1)` for a channel — `spawn_uniform` remapped.
fn spawn_signed(seed: u64, ordinal: u64, channel: u64) -> f32 {
    spawn_uniform(seed, ordinal, channel) * 2.0 - 1.0
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

/// Applies each active collider to a particle's post-integration `position`/`velocity`, in array
/// order, returning whether the particle was killed. Canonical for both this CPU reference and the GPU
/// death-loop kernel: every operation is `+ - * /`, comparison, or `sqrt` (all IEEE-correctly-rounded),
/// and it reads only the persistent position/velocity, so a checkpoint restore reproduces every bounce.
fn resolve_colliders(
    colliders: &[Collider; MAX_COLLIDERS],
    count: u32,
    position: &mut [f32; 3],
    velocity: &mut [f32; 3],
) -> bool {
    let count = (count as usize).min(MAX_COLLIDERS);
    for collider in &colliders[..count] {
        if resolve_collider(collider, position, velocity) {
            return true;
        }
    }
    false
}

/// Resolves a single collider against `position`/`velocity`. Returns `true` when the particle contacts
/// a `kill` collider (the caller retires it). Bounces push the particle back onto the surface along the
/// contact normal, reflect the inbound normal velocity scaled by `restitution`, and damp the tangential
/// velocity by `friction` — a single uniform response shared by every shape.
fn resolve_collider(collider: &Collider, position: &mut [f32; 3], velocity: &mut [f32; 3]) -> bool {
    // Each shape reports (contact, outward unit normal, penetration depth ≥ 0).
    let (contact, normal, penetration) = match collider.shape {
        ColliderShape::Plane { normal, distance } => {
            let signed = dot(normal, *position) - distance;
            (signed < 0.0, normal, -signed)
        }
        ColliderShape::Sphere { center, radius } => {
            let delta = [
                position[0] - center[0],
                position[1] - center[1],
                position[2] - center[2],
            ];
            let distance = dot(delta, delta).sqrt();
            let normal = normalize_or(delta, [0.0, 1.0, 0.0]);
            (distance < radius, normal, radius - distance)
        }
        ColliderShape::Aabb { min, max } => {
            let inside = position[0] > min[0]
                && position[0] < max[0]
                && position[1] > min[1]
                && position[1] < max[1]
                && position[2] > min[2]
                && position[2] < max[2];
            // Exit along the axis/face of least penetration; strict `<` breaks ties toward the earlier
            // axis and toward `min` before `max`, deterministically on both CPU and GPU.
            let mut best_penetration = f32::INFINITY;
            let mut best_normal = [0.0, 1.0, 0.0];
            for axis in 0..3 {
                let to_min = position[axis] - min[axis];
                if to_min < best_penetration {
                    best_penetration = to_min;
                    best_normal = axis_normal(axis, -1.0);
                }
                let to_max = max[axis] - position[axis];
                if to_max < best_penetration {
                    best_penetration = to_max;
                    best_normal = axis_normal(axis, 1.0);
                }
            }
            (inside, best_normal, best_penetration)
        }
    };

    if !contact {
        return false;
    }
    if collider.kill {
        return true;
    }

    // Push out of the collider along the contact normal, then apply the bounce response.
    for axis in 0..3 {
        position[axis] += normal[axis] * penetration;
    }
    let normal_speed = dot(*velocity, normal);
    let tangential = [
        velocity[0] - normal[0] * normal_speed,
        velocity[1] - normal[1] * normal_speed,
        velocity[2] - normal[2] * normal_speed,
    ];
    // Only reflect when moving into the surface; a particle already separating keeps its normal speed.
    let reflected_normal_speed = if normal_speed < 0.0 {
        -collider.restitution * normal_speed
    } else {
        normal_speed
    };
    let keep_tangential = 1.0 - collider.friction;
    for axis in 0..3 {
        velocity[axis] = tangential[axis] * keep_tangential + normal[axis] * reflected_normal_speed;
    }
    false
}

fn axis_normal(axis: usize, sign: f32) -> [f32; 3] {
    let mut normal = [0.0; 3];
    normal[axis] = sign;
    normal
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
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
            shape: SpawnShape::Sphere { radius: 3.0 },
            turbulence: 6.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
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
            shape: SpawnShape::Point,
            turbulence: 0.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
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

    #[test]
    fn spawn_shapes_place_particles_within_their_volume() {
        let seed = 0x5417;
        for ordinal in 0..64 {
            // Sphere: within the radius.
            let sphere = StatefulConfig {
                shape: SpawnShape::Sphere { radius: 5.0 },
                ..config()
            };
            let position = StatefulSimulation::launch_position(&sphere, seed, ordinal);
            let distance = (position[0].powi(2) + position[1].powi(2) + position[2].powi(2)).sqrt();
            assert!(
                distance <= 5.0 + 1e-3,
                "sphere spawn {distance} within radius 5"
            );

            // Box: each axis within its half extent.
            let boxed = StatefulConfig {
                shape: SpawnShape::Box {
                    half_extents: [4.0, 2.0, 6.0],
                },
                ..config()
            };
            let position = StatefulSimulation::launch_position(&boxed, seed, ordinal);
            assert!(position[0].abs() <= 4.0 + 1e-3);
            assert!(position[1].abs() <= 2.0 + 1e-3);
            assert!(position[2].abs() <= 6.0 + 1e-3);

            // Point: exactly the origin.
            let point = StatefulConfig {
                shape: SpawnShape::Point,
                ..config()
            };
            assert_eq!(
                StatefulSimulation::launch_position(&point, seed, ordinal),
                [0.0; 3]
            );
        }
    }

    #[test]
    fn turbulence_is_bounded_evolves_with_age_and_disables_at_zero() {
        let seed = 0x7B_u64;
        let ordinal = 11;
        // Bounded by strength and varies with age (value noise moves between cells).
        let mut samples = Vec::new();
        for step in 0..20 {
            let age = step as f32 * 0.1;
            let acceleration = StatefulSimulation::turbulence_acceleration(seed, ordinal, age, 8.0);
            for axis in acceleration {
                assert!(axis.abs() <= 8.0 + 1e-3, "turbulence within +/- strength");
            }
            samples.push(acceleration[0]);
        }
        assert!(
            samples.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "turbulence evolves with age"
        );
        assert_eq!(
            StatefulSimulation::turbulence_acceleration(seed, ordinal, 0.5, 0.0),
            [0.0; 3],
            "zero strength disables turbulence"
        );
    }

    fn ground_plane(restitution: f32, friction: f32) -> Collider {
        Collider {
            shape: ColliderShape::Plane {
                normal: [0.0, 1.0, 0.0],
                distance: 0.0,
            },
            restitution,
            friction,
            kill: false,
        }
    }

    #[test]
    fn plane_collider_bounces_particles_and_keeps_them_above_it() {
        // Particles launched downward under gravity hit the ground plane at y = 0, bounce, and never
        // settle below it. A perfectly elastic frictionless plane also flips downward velocity to up.
        let config = StatefulConfig {
            gravity: [0.0, -9.81, 0.0],
            spawn_per_tick: 2,
            speed: (5.0, 5.0),
            lifetime: (1000.0, 1000.0),
            direction: [0.0, -1.0, 0.0],
            spread: 0.0,
            drag: 0.0,
            shape: SpawnShape::Point,
            turbulence: 0.0,
            colliders: {
                let mut colliders = [Collider::NONE; MAX_COLLIDERS];
                colliders[0] = ground_plane(1.0, 0.0);
                colliders
            },
            collider_count: 1,
            capacity: 64,
        };
        let mut simulation = StatefulSimulation::new(config, 0xB0_1CE);
        simulation.advance_to_tick(400);
        assert!(simulation.live_count() > 0, "particles persist (no kill)");
        for (_, position) in simulation.alive_particles() {
            assert!(
                position[1] >= -1e-3,
                "particle stays on/above the plane: y = {}",
                position[1]
            );
        }
    }

    #[test]
    fn kill_collider_retires_particles_on_contact() {
        // A kill plane just below the spawn point removes downward-launched particles instead of
        // bouncing them; with a killing floor and downward launch the population cannot accumulate the
        // way a bouncing floor allows.
        let base = StatefulConfig {
            gravity: [0.0, -20.0, 0.0],
            spawn_per_tick: 1,
            speed: (2.0, 2.0),
            lifetime: (1000.0, 1000.0),
            direction: [0.0, -1.0, 0.0],
            spread: 0.0,
            drag: 0.0,
            shape: SpawnShape::Point,
            turbulence: 0.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: 4096,
        };
        let kill_floor = Collider {
            shape: ColliderShape::Plane {
                normal: [0.0, 1.0, 0.0],
                distance: -1.0,
            },
            restitution: 0.0,
            friction: 0.0,
            kill: true,
        };
        let killing = StatefulConfig {
            colliders: {
                let mut colliders = [Collider::NONE; MAX_COLLIDERS];
                colliders[0] = kill_floor;
                colliders
            },
            collider_count: 1,
            ..base
        };
        let mut bouncing_config = killing;
        bouncing_config.colliders[0].kill = false;
        bouncing_config.colliders[0].restitution = 1.0;

        let mut killing_sim = StatefulSimulation::new(killing, 7);
        let mut bouncing_sim = StatefulSimulation::new(bouncing_config, 7);
        killing_sim.advance_to_tick(120);
        bouncing_sim.advance_to_tick(120);
        assert!(
            killing_sim.live_count() < bouncing_sim.live_count(),
            "kill floor retires particles ({} live) vs bounce ({} live)",
            killing_sim.live_count(),
            bouncing_sim.live_count()
        );
    }

    #[test]
    fn friction_damps_tangential_velocity_on_bounce() {
        // A particle moving diagonally into a frictional plane loses horizontal speed on contact; a
        // frictionless plane preserves it. Restitution is 0 so the vertical component is killed and the
        // bounce is a pure slide.
        let mut position = [0.0, 0.0, 0.0];
        let mut velocity = [4.0, -3.0, 0.0];
        let mut frictionless = velocity;
        let mut frictionless_pos = position;

        let plane_friction = ground_plane(0.0, 0.5);
        let plane_free = ground_plane(0.0, 0.0);
        // Force contact: start just below the plane.
        position[1] = -0.5;
        frictionless_pos[1] = -0.5;

        resolve_collider(&plane_friction, &mut position, &mut velocity);
        resolve_collider(&plane_free, &mut frictionless_pos, &mut frictionless);

        assert!(
            velocity[0] < frictionless[0],
            "friction reduces horizontal speed: {} vs {}",
            velocity[0],
            frictionless[0]
        );
        assert!(
            (velocity[0] - frictionless[0] * 0.5).abs() < 1e-5,
            "friction 0.5 keeps half the tangential speed"
        );
        assert!(position[1] >= -1e-6, "push-out lifts to the surface");
    }
}
