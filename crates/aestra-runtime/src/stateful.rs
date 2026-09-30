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

pub use aestra_core::{Collider, ColliderShape, HomingLostPolicy};

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

/// Where an emitter's spawns land in effect space: the emitter transform. Stateful particles simulate
/// in effect space (gravity, colliders and followed fields are effect-space), so the transform places
/// only what a spawn creates — the shape sample becomes `translation + rotation(scale × local)` and the
/// launch velocity is rotated (not scaled). Moving an emitter therefore moves where new particles
/// appear while the ones in flight keep their motion, like a world-space emitter. Trig-free (explicit
/// quaternion rotation of a unit `rotation`), so the GPU spawn kernel reproduces it bit-for-bit;
/// [`SpawnPlacement::IDENTITY`] is skipped entirely on both sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPlacement {
    pub translation: [f32; 3],
    /// A unit quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl SpawnPlacement {
    pub const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };

    /// A shape-local spawn position placed in effect space.
    pub fn point(&self, local: [f32; 3]) -> [f32; 3] {
        let rotated = self.vector([
            local[0] * self.scale[0],
            local[1] * self.scale[1],
            local[2] * self.scale[2],
        ]);
        [
            self.translation[0] + rotated[0],
            self.translation[1] + rotated[1],
            self.translation[2] + rotated[2],
        ]
    }

    /// A launch direction/velocity rotated into effect space: `v + w·t + q × t` with `t = 2 q × v`,
    /// in the exact operation order the GPU kernel uses.
    pub fn vector(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.rotation;
        let tx = 2.0 * (y * v[2] - z * v[1]);
        let ty = 2.0 * (z * v[0] - x * v[2]);
        let tz = 2.0 * (x * v[1] - y * v[0]);
        [
            v[0] + w * tx + (y * tz - z * ty),
            v[1] + w * ty + (z * tx - x * tz),
            v[2] + w * tz + (x * ty - y * tx),
        ]
    }
}

impl Default for SpawnPlacement {
    fn default() -> Self {
        Self::IDENTITY
    }
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
    /// The emitter transform placing each spawn in effect space.
    pub placement: SpawnPlacement,
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
    /// Homing steering (host bindings HB7), when a Homing module is enabled. Its target is an input
    /// that changes over time: [`StatefulSimulation::set_homing_target`].
    pub homing: Option<HomingConfig>,
}

/// How homing particles steer (host bindings HB7). See [`steer_homing`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomingConfig {
    /// The speed particles settle at.
    pub speed: f32,
    /// How fast the speed changes toward `speed`, per second; `0` sets it at once.
    pub acceleration: f32,
    /// The share of the way from the current heading to the target's direction turned per second
    /// (at most all of it in one tick).
    pub turn_rate: f32,
    /// Particles this close to the target have arrived: they retire.
    pub arrival_radius: f32,
    /// What particles do while the target is lost.
    pub lost: HomingLostPolicy,
}

/// Where the target is for a tick, in the particles' space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomingTarget {
    pub position: [f32; 3],
    /// Its velocity, to lead it (zero when unknown).
    pub velocity: [f32; 3],
}

/// Why a homing particle retired (host bindings HB7/HB9): it reached the target, or the target was
/// lost under [`HomingLostPolicy::Kill`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomingRetire {
    Arrived,
    Lost,
}

/// A change in whether the host supplies the homing target (host bindings HB9): the runtime events
/// `target_lost` / `target_acquired`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetChange {
    /// The target was supplied and no longer is; where it was last seen.
    Lost(HomingTarget),
    /// The target is supplied again (or for the first time).
    Acquired(HomingTarget),
}

/// Resolves the target a tick steers toward from the host's input: the input when there is one,
/// else — with [`HomingLostPolicy::KeepLastPosition`] — the last one seen, standing still. Shared by
/// this reference and the GPU host, so both lose a target the same way. It also notes when the input
/// appears or disappears: [`Self::take_change`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HomingTracker {
    last: Option<HomingTarget>,
    present: bool,
    change: Option<TargetChange>,
}

impl HomingTracker {
    pub fn resolve(
        &mut self,
        policy: HomingLostPolicy,
        input: Option<HomingTarget>,
    ) -> Option<HomingTarget> {
        match (input, self.present) {
            (Some(target), false) => self.change = Some(TargetChange::Acquired(target)),
            (None, true) => self.change = self.last.map(TargetChange::Lost),
            _ => {}
        }
        self.present = input.is_some();
        match input {
            Some(target) => {
                self.last = Some(target);
                Some(target)
            }
            None if policy == HomingLostPolicy::KeepLastPosition => {
                self.last.map(|last| HomingTarget {
                    position: last.position,
                    velocity: [0.0; 3],
                })
            }
            None => None,
        }
    }

    /// The latest appearance or loss of the target since the last call, if any.
    pub fn take_change(&mut self) -> Option<TargetChange> {
        self.change.take()
    }
}

/// A particle event the simulation raised in its last tick (host bindings HB9b): which particle (its
/// spawn ordinal), where, and how fast it was going.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleEvent {
    pub ordinal: u64,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
}

/// The host's world geometry a stateful emitter's `World` colliders collide with (host bindings
/// HB10), and where the effect sits in it: particles simulate in effect space, the volume is in world
/// space.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleWorld {
    pub volume: std::sync::Arc<crate::SdfVolume>,
    /// The effect-to-world affine, 3×4 rows.
    pub world_from_effect: [[f32; 4]; 3],
}

impl ParticleWorld {
    /// Whether an effect-space `position` is within `radius` of the world, the effect-space outward
    /// normal there, and how deep: the position taken to world space, the volume's distance and
    /// normal there, the normal brought back to effect space (by the transposed linear part) and the
    /// distance scaled by the effect's mean axis scale. The GPU's world collider mirrors it operation
    /// for operation.
    pub fn contact(&self, radius: f32, position: [f32; 3]) -> (bool, [f32; 3], f32) {
        placed_contact(self.world_from_effect, radius, position, |world| {
            (self.volume.sample(world), self.volume.normal(world))
        })
    }
}

/// The host's physics scene a stateful emitter's `Physics` colliders collide with (host bindings
/// HB10), and where the effect sits in it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticlePhysics {
    pub scene: std::sync::Arc<crate::PhysicsScene>,
    /// The effect-to-world affine, 3×4 rows.
    pub world_from_effect: [[f32; 4]; 3],
}

impl ParticlePhysics {
    /// [`ParticleWorld::contact`] against the scene's nearest proxy.
    pub fn contact(&self, radius: f32, position: [f32; 3]) -> (bool, [f32; 3], f32) {
        placed_contact(self.world_from_effect, radius, position, |world| {
            self.scene.distance_normal(world)
        })
    }
}

/// An effect-space contact against a world-space distance field: the position taken to world space,
/// the field's distance and normal there, the normal brought back by the transposed linear part and
/// the distance scaled by the mean axis scale.
fn placed_contact(
    r: [[f32; 4]; 3],
    radius: f32,
    position: [f32; 3],
    field: impl Fn([f32; 3]) -> (f32, [f32; 3]),
) -> (bool, [f32; 3], f32) {
    let world: [f32; 3] = std::array::from_fn(|i| {
        r[i][0] * position[0] + r[i][1] * position[1] + r[i][2] * position[2] + r[i][3]
    });
    let (distance, n) = field(world);
    let normal: [f32; 3] =
        std::array::from_fn(|j| r[0][j] * n[0] + r[1][j] * n[1] + r[2][j] * n[2]);
    let length_squared = dot(normal, normal);
    if length_squared <= 1e-12 {
        return (false, [0.0, 1.0, 0.0], 0.0);
    }
    let length = length_squared.sqrt();
    let normal = normal.map(|component| component / length);
    let column = |j: usize| (r[0][j] * r[0][j] + r[1][j] * r[1][j] + r[2][j] * r[2][j]).sqrt();
    let scale = (column(0) + column(1) + column(2)) / 3.0;
    let distance = distance / scale;
    (distance < radius, normal, radius - distance)
}

/// The slot of each trigger in the per-tick event lists.
fn trigger_index(trigger: aestra_core::EventTrigger) -> usize {
    match trigger {
        aestra_core::EventTrigger::OnSpawn => 0,
        aestra_core::EventTrigger::OnDeath => 1,
        aestra_core::EventTrigger::OnCollision => 2,
    }
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
    /// The homing target the next ticks steer toward (host bindings HB7), as the host last set it.
    homing_input: Option<HomingTarget>,
    homing_tracker: HomingTracker,
    /// A host's `stop_emitting` / `kill` (event system E2b).
    cutoffs: crate::EmissionCutoffs,
    /// Particles that reached their homing target so far (the `impact` event, host bindings HB9).
    arrivals: u64,
    /// The spawn, death and collision events of the last tick (host bindings HB9b).
    events: [Vec<ParticleEvent>; 3],
    /// The world `World` colliders collide with (host bindings HB10); none collides without one.
    world: Option<ParticleWorld>,
    /// The physics scene `Physics` colliders collide with (host bindings HB10).
    physics: Option<ParticlePhysics>,
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
            homing_input: None,
            homing_tracker: HomingTracker::default(),
            cutoffs: crate::EmissionCutoffs::NONE,
            arrivals: 0,
            events: Default::default(),
            world: None,
            physics: None,
        }
    }

    /// Sets where the homing target is for the following ticks (host bindings HB7); `None` while it is
    /// lost. Ignored without a homing config.
    pub fn set_homing_target(&mut self, target: Option<HomingTarget>) {
        self.homing_input = target;
    }

    /// Sets the world `World` colliders collide with from the next tick (host bindings HB10).
    pub fn set_world(&mut self, world: Option<ParticleWorld>) {
        self.world = world;
    }

    /// Sets the physics scene `Physics` colliders collide with from the next tick (host bindings
    /// HB10).
    pub fn set_physics(&mut self, physics: Option<ParticlePhysics>) {
        self.physics = physics;
    }

    /// Where a host's `stop_emitting` and `kill` inputs cut emission (event system E2b): from
    /// `stop_tick` nothing spawns; at `kill_tick` every particle retires without a death event.
    pub fn set_cutoffs(&mut self, cutoffs: crate::EmissionCutoffs) {
        self.cutoffs = cutoffs;
    }

    /// Moves where the following ticks' spawns land (an attached emitter, host bindings HB7b).
    /// Particles already in flight keep their motion, so a moving placement leaves a wake.
    pub fn set_placement(&mut self, placement: SpawnPlacement) {
        self.config.placement = placement;
    }

    /// The events of `trigger` the last tick raised (host bindings HB9b): every spawn (not those made
    /// by [`Self::spawn_from_events`]), every retirement, every contact with a collider.
    pub fn events(&self, trigger: aestra_core::EventTrigger) -> &[ParticleEvent] {
        &self.events[trigger_index(trigger)]
    }

    /// Spawns `count` particles per event (host bindings HB9b), after the tick, exactly as the GPU's
    /// event spawn does: events in ordinal order, each repeated `count` times, at most
    /// [`crate::PARTICLE_EVENT_CAPACITY`] and as many as there is room for; each becomes the next
    /// ordinal, at the event's position, with `inherit` × its velocity plus the launch velocity the
    /// ordinal samples (no shape, no placement: the event is already where it happened), and a
    /// lifetime in range.
    pub fn spawn_from_events(&mut self, events: &[ParticleEvent], count: u32, inherit: f32) {
        let mut sorted = events.to_vec();
        sorted.sort_by_key(|event| event.ordinal);
        let room = (self.config.capacity as usize).saturating_sub(self.particles.len());
        let records = sorted
            .iter()
            .flat_map(|event| std::iter::repeat_n(event, count as usize))
            .take((crate::PARTICLE_EVENT_CAPACITY as usize).min(room));
        for event in records {
            let ordinal = self.spawned;
            let launch = launch_velocity(&self.config, self.seed, ordinal);
            let velocity =
                std::array::from_fn(|axis| inherit * event.velocity[axis] + launch[axis]);
            let lifetime = lerp(
                self.config.lifetime.0,
                self.config.lifetime.1,
                spawn_uniform(self.seed, ordinal, 1),
            );
            self.particles.push(StateParticle {
                id: ordinal,
                position: event.position,
                velocity,
                age: 0.0,
                lifetime,
            });
            self.spawned += 1;
        }
    }

    /// How many particles have reached their homing target so far (host bindings HB9).
    pub fn arrivals(&self) -> u64 {
        self.arrivals
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
            Surroundings::default(),
            position,
            velocity,
        )
    }

    /// Advances exactly one fixed tick: integrate alive particles, retire the dead, then spawn.
    pub fn advance_tick(&mut self) {
        let dt = Self::TICK_DT;
        let drag = self.config.drag;
        for events in &mut self.events {
            events.clear();
        }
        // A host `kill` (event system E2b): from its tick every particle retires, raising nothing,
        // and nothing spawns.
        if self.cutoffs.kill_tick.is_some_and(|kill| self.tick >= kill) {
            self.particles.clear();
            self.tick += 1;
            return;
        }
        let [spawned_events, deaths, collisions] = &mut self.events;
        let homing = self.config.homing.map(|homing| {
            let target = self.homing_tracker.resolve(homing.lost, self.homing_input);
            (homing, target)
        });
        for particle in &mut self.particles {
            // Homing (HB7) steers the velocity before the forces act on it.
            if let Some((homing, target)) = &homing
                && let Some(retire) = steer_homing(
                    homing,
                    target.as_ref(),
                    particle.position,
                    &mut particle.velocity,
                    dt,
                )
            {
                if retire == HomingRetire::Arrived {
                    self.arrivals += 1;
                }
                particle.age = particle.lifetime;
                deaths.push(ParticleEvent {
                    ordinal: particle.id,
                    position: particle.position,
                    velocity: particle.velocity,
                });
                continue;
            }
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
            let before = (particle.position, particle.velocity);
            let killed = resolve_colliders(
                &self.config.colliders,
                self.config.collider_count,
                Surroundings {
                    world: self.world.as_ref(),
                    physics: self.physics.as_ref(),
                },
                &mut particle.position,
                &mut particle.velocity,
            );
            let event = ParticleEvent {
                ordinal: particle.id,
                position: particle.position,
                velocity: particle.velocity,
            };
            // A contact: a kill, or a bounce that moved or redirected the particle.
            if killed || before != (particle.position, particle.velocity) {
                collisions.push(event);
            }
            if killed {
                particle.age = particle.lifetime;
            } else {
                particle.age += dt;
            }
            if particle.age >= particle.lifetime {
                deaths.push(event);
            }
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        let room = (self.config.capacity as usize).saturating_sub(self.particles.len());
        let stopped = self.cutoffs.stop_tick.is_some_and(|stop| self.tick >= stop);
        let spawn = if stopped {
            0
        } else {
            (self.config.spawn_per_tick as usize).min(room)
        };
        for _ in 0..spawn {
            let ordinal = self.spawned;
            let mut velocity = launch_velocity(&self.config, self.seed, ordinal);
            let mut position = launch_position(&self.config, self.seed, ordinal);
            let placement = self.config.placement;
            if placement != SpawnPlacement::IDENTITY {
                position = placement.point(position);
                velocity = placement.vector(velocity);
            }
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
            spawned_events.push(ParticleEvent {
                ordinal,
                position,
                velocity,
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

/// Steers one particle's `velocity` toward `target` for a tick of `dt` (host bindings HB7),
/// returning why it retires, if it does (it arrived, or the target is lost under
/// [`HomingLostPolicy::Kill`]).
/// The direction to aim along leads the target by the time to reach it at `speed`; the heading turns
/// `min(turn_rate × dt, 1)` of the way there, renormalized; the speed moves toward `speed` by at
/// most `acceleration × dt`. Without a target the particle keeps its heading. Only `+ - * /`,
/// `min`/`max`, comparisons and `sqrt` — the GPU's `aestra_homing_steer` reproduces it.
pub fn steer_homing(
    config: &HomingConfig,
    target: Option<&HomingTarget>,
    position: [f32; 3],
    velocity: &mut [f32; 3],
    dt: f32,
) -> Option<HomingRetire> {
    let speed_now = dot(*velocity, *velocity).sqrt();
    let heading = normalize_or(*velocity, [0.0, 1.0, 0.0]);
    let direction = match target {
        Some(target) => {
            let to = [
                target.position[0] - position[0],
                target.position[1] - position[1],
                target.position[2] - position[2],
            ];
            let distance = dot(to, to).sqrt();
            if distance <= config.arrival_radius {
                return Some(HomingRetire::Arrived);
            }
            let reach = distance / config.speed.max(1e-6);
            let aim = [
                target.position[0] + target.velocity[0] * reach - position[0],
                target.position[1] + target.velocity[1] * reach - position[1],
                target.position[2] + target.velocity[2] * reach - position[2],
            ];
            let desired = normalize_or(aim, heading);
            let turn = (config.turn_rate * dt).min(1.0);
            let blended = [
                heading[0] + (desired[0] - heading[0]) * turn,
                heading[1] + (desired[1] - heading[1]) * turn,
                heading[2] + (desired[2] - heading[2]) * turn,
            ];
            normalize_or(blended, desired)
        }
        None if config.lost == HomingLostPolicy::Kill => return Some(HomingRetire::Lost),
        None => heading,
    };
    let speed = if config.acceleration > 0.0 {
        let step = config.acceleration * dt;
        speed_now + (config.speed - speed_now).clamp(-step, step)
    } else {
        config.speed
    };
    for axis in 0..3 {
        velocity[axis] = direction[axis] * speed;
    }
    None
}

/// Applies each active collider to a particle's post-integration `position`/`velocity`, in array
/// order, returning whether the particle was killed. Canonical for both this CPU reference and the GPU
/// death-loop kernel: every operation is `+ - * /`, comparison, or `sqrt` (all IEEE-correctly-rounded),
/// and it reads only the persistent position/velocity, so a checkpoint restore reproduces every bounce.
/// The host's scene a tick's colliders may reach: its world SDF and its physics colliders.
#[derive(Clone, Copy, Default)]
struct Surroundings<'a> {
    world: Option<&'a ParticleWorld>,
    physics: Option<&'a ParticlePhysics>,
}

fn resolve_colliders(
    colliders: &[Collider; MAX_COLLIDERS],
    count: u32,
    world: Surroundings<'_>,
    position: &mut [f32; 3],
    velocity: &mut [f32; 3],
) -> bool {
    let count = (count as usize).min(MAX_COLLIDERS);
    for collider in &colliders[..count] {
        if resolve_collider(collider, world, position, velocity) {
            return true;
        }
    }
    false
}

/// Resolves a single collider against `position`/`velocity`. Returns `true` when the particle contacts
/// a `kill` collider (the caller retires it). Bounces push the particle back onto the surface along the
/// contact normal, reflect the inbound normal velocity scaled by `restitution`, and damp the tangential
/// velocity by `friction` — a single uniform response shared by every shape.
fn resolve_collider(
    collider: &Collider,
    world: Surroundings<'_>,
    position: &mut [f32; 3],
    velocity: &mut [f32; 3],
) -> bool {
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
        ColliderShape::World { radius } => match world.world {
            Some(world) => world.contact(radius, *position),
            None => (false, [0.0, 1.0, 0.0], 0.0),
        },
        ColliderShape::Physics { radius } => match world.physics {
            Some(physics) => physics.contact(radius, *position),
            None => (false, [0.0, 1.0, 0.0], 0.0),
        },
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
            placement: SpawnPlacement::IDENTITY,
            turbulence: 6.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: 128,
            homing: None,
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
            placement: SpawnPlacement::IDENTITY,
            turbulence: 0.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: 8,
            homing: None,
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
    fn the_placement_moves_new_spawns_into_effect_space() {
        // A quarter turn about +Z maps +Y to -X; the shape sample is scaled before rotating.
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let placement = SpawnPlacement {
            translation: [30.0, 7.0, -2.0],
            rotation: [0.0, 0.0, half, half],
            scale: [2.0, 2.0, 2.0],
        };
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4);
        assert!(near(placement.vector([0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]));
        assert!(near(placement.point([0.0, 1.0, 0.0]), [28.0, 7.0, -2.0]));

        let config = StatefulConfig {
            placement,
            gravity: [0.0; 3],
            drag: 0.0,
            turbulence: 0.0,
            spread: 0.0,
            ..config()
        };
        let mut placed = StatefulSimulation::new(config, 3);
        placed.advance_to_tick(1);
        for particle in &placed.particles {
            // Spawned this tick: at the placed shape sample, launched along the rotated direction.
            let local = StatefulSimulation::launch_position(&config, 3, particle.id);
            assert_eq!(particle.position, placement.point(local));
            assert!(particle.velocity[0] < 0.0 && particle.velocity[1].abs() < 1e-3);
        }
        assert!(placed.live_count() > 0);
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
            placement: SpawnPlacement::IDENTITY,
            turbulence: 0.0,
            colliders: {
                let mut colliders = [Collider::NONE; MAX_COLLIDERS];
                colliders[0] = ground_plane(1.0, 0.0);
                colliders
            },
            collider_count: 1,
            capacity: 64,
            homing: None,
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
            placement: SpawnPlacement::IDENTITY,
            turbulence: 0.0,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: 4096,
            homing: None,
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

        resolve_collider(
            &plane_friction,
            Surroundings::default(),
            &mut position,
            &mut velocity,
        );
        resolve_collider(
            &plane_free,
            Surroundings::default(),
            &mut frictionless_pos,
            &mut frictionless,
        );

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

    #[test]
    fn the_tracker_reports_the_target_appearing_and_vanishing_once_each() {
        let seen = HomingTarget {
            position: [1.0, 2.0, 3.0],
            velocity: [0.0; 3],
        };
        let mut tracker = HomingTracker::default();
        tracker.resolve(HomingLostPolicy::KeepDirection, None);
        assert_eq!(
            tracker.take_change(),
            None,
            "never supplied: nothing to lose"
        );
        tracker.resolve(HomingLostPolicy::KeepDirection, Some(seen));
        assert_eq!(tracker.take_change(), Some(TargetChange::Acquired(seen)));
        tracker.resolve(HomingLostPolicy::KeepDirection, Some(seen));
        assert_eq!(tracker.take_change(), None, "still there");
        tracker.resolve(HomingLostPolicy::KeepDirection, None);
        tracker.resolve(HomingLostPolicy::KeepDirection, None);
        assert_eq!(
            tracker.take_change(),
            Some(TargetChange::Lost(seen)),
            "where it was last seen, once"
        );
        assert_eq!(tracker.take_change(), None);
    }

    #[test]
    fn homing_particles_retire_for_arriving_or_for_a_lost_target_and_only_arrivals_count() {
        let config = HomingConfig {
            speed: 10.0,
            acceleration: 0.0,
            turn_rate: 5.0,
            arrival_radius: 1.0,
            lost: HomingLostPolicy::Kill,
        };
        let target = HomingTarget {
            position: [0.5, 0.0, 0.0],
            velocity: [0.0; 3],
        };
        let mut velocity = [0.0, 1.0, 0.0];
        assert_eq!(
            steer_homing(&config, Some(&target), [0.0; 3], &mut velocity, 0.1),
            Some(HomingRetire::Arrived)
        );
        assert_eq!(
            steer_homing(&config, None, [0.0; 3], &mut velocity, 0.1),
            Some(HomingRetire::Lost)
        );
        let far = HomingTarget {
            position: [50.0, 0.0, 0.0],
            velocity: [0.0; 3],
        };
        assert_eq!(
            steer_homing(&config, Some(&far), [0.0; 3], &mut velocity, 0.1),
            None
        );

        let stateful = StatefulConfig {
            gravity: [0.0; 3],
            spawn_per_tick: 2,
            speed: (4.0, 6.0),
            lifetime: (3.0, 3.5),
            direction: [0.0, 1.0, 0.0],
            spread: 0.5,
            drag: 0.0,
            shape: SpawnShape::Point,
            turbulence: 0.0,
            placement: SpawnPlacement::IDENTITY,
            colliders: [Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: 256,
            homing: Some(HomingConfig {
                speed: 20.0,
                acceleration: 40.0,
                arrival_radius: 1.5,
                ..config
            }),
        };
        let mut simulation = StatefulSimulation::new(stateful, 7);
        simulation.set_homing_target(Some(HomingTarget {
            position: [0.0, 3.0, 0.0],
            velocity: [0.0; 3],
        }));
        simulation.advance_to_tick(90);
        let arrived = simulation.arrivals();
        assert!(arrived > 100, "{arrived} of 180 arrived");
        // Losing the target kills the rest under `Kill`: none of those count.
        simulation.set_homing_target(None);
        simulation.advance_to_tick(91);
        assert_eq!(simulation.arrivals(), arrived);
        assert!(simulation.live_count() <= 2, "only this tick's spawns live");
    }
}
