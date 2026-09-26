//! Engine-neutral GPU ABI definitions and artifact lowering.
//!
//! This crate translates compiled Aestra effects into packed data suitable for
//! GPU simulation and rendering. It intentionally contains no Bevy, WGPU,
//! windowing, ECS, shader loading, dispatch, or drawing integration.

mod compute_program;
mod host_bindings;
pub mod material;
pub mod mesh_bounds;
pub mod particle_attributes;
pub mod ribbon_bounds;
pub mod shader;
pub mod volume;

pub use compute_program::{BindingUse, ProgramInterface, check_program_block, program_interface};
pub use host_bindings::{
    GpuHostBindings, HOST_BINDING_BOUND, HOST_BINDING_HEADER_WORDS, HOST_BINDINGS_WGSL,
    host_binding_value_offsets, host_binding_value_word,
};

use aestra_core::{
    BlendMode, EmitterShape, FlipbookPlaybackMode, FlipbookTimeSource, PropertyEvaluationDomain,
    ScalarRange, Vec3Range,
};
use aestra_runtime::{
    CompiledCurve, CompiledGradient, CompiledVec3Curve, EffectInstance, ExecutionPlan, Instruction,
    MaterialColorPlan, RendererPlanKind, RuntimeValue, ScalarSource, SimulationClass,
    SimulationStateLayout, VectorSource,
};
use encase::ShaderType;
use glam::{Mat4, Quat, UVec2, UVec3, Vec2, Vec3, Vec4};
use thiserror::Error;

pub const MAX_CURVE_KEYS: usize = 8;
/// Storage bindings used by the simulation shader and its host layout.
pub const SIMULATION_STORAGE_BINDING_COUNT: u32 = 8;
/// Samples in the per-emitter inverse-emission table used to seed curve-driven
/// spawn-time reconstruction (see `aestra_simulation.wesl`). More samples give the
/// GPU a tighter starting bracket, so fewer refinement iterations are needed.
pub const SPAWN_INVERSE_SAMPLES: usize = 32;
pub const MAX_FLIPBOOK_FRAMES: usize = 64;
#[derive(Clone, Copy, ShaderType)]
pub struct GpuTrailCullParams {
    pub clip_from_world: Mat4,
    pub renderer_index: u32,
    pub instance_count: u32,
    pub epoch: u32,
    pub _padding: u32,
}
/// Eight triangles per semicircular endpoint; must match the trail vertex shader.
pub const TRAIL_CAP_SEGMENTS: u32 = 8;

pub fn trail_draw_instances(renderer: &GpuRenderer) -> u32 {
    let caps = if renderer.flipbook_flags & 2 != 0 {
        2 * TRAIL_CAP_SEGMENTS
    } else {
        0
    };
    renderer.playback_mode * (renderer.frame_count.saturating_sub(1) + caps)
}

pub const WORKGROUP_SIZE: u32 = 64;
const INDIRECT_DRAW_WORDS: usize = 4;
pub const INDIRECT_DRAW_BYTES: u64 = (INDIRECT_DRAW_WORDS * std::mem::size_of::<u32>()) as u64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GpuArtifactError {
    #[error(
        "trail history supports one renderer per emitter, at most 256 parents, parent capacity–1024 trail owners, 2–64 points, and 1,048,576 total particle/history records"
    )]
    TrailLimit,
    #[error("emitter '{0}' has no {1} instruction")]
    MissingInstruction(String, &'static str),
    #[error("{kind} has {actual} keys; the GPU profile supports at most {maximum}")]
    KeyLimit {
        kind: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("flipbook '{name}' has {actual} frames; the GPU profile supports at most {maximum}")]
    FlipbookFrameLimit {
        name: String,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuCurve {
    pub keys: [Vec2; MAX_CURVE_KEYS],
    pub count: u32,
    /// x encodes CurveInterpolation (0 smooth, 1 linear, 2 step); y/z reserved.
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradientKey {
    pub color: Vec4,
    pub time: f32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradient {
    pub keys: [GpuGradientKey; MAX_CURVE_KEYS],
    pub count: u32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuEmitter {
    pub slot_offset: u32,
    pub max_particles: u32,
    pub burst_count: u32,
    pub shape_kind: u32,
    pub start_time: f32,
    pub duration: f32,
    pub source_offset: f32,
    pub source_duration: f32,
    pub spawn_rate: Vec2,
    pub spawn_rate_source: u32,
    pub seed_index: u32,
    pub spawn_rate_curve: GpuCurve,
    pub shape_radius: f32,
    pub shape_depth: f32,
    pub shape_extent_z: f32,
    pub spread_radians: f32,
    pub drag: Vec2,
    pub drag_source: u32,
    /// Omitted presentation attributes; zero retains full-reference readback.
    pub omitted_attributes: u32,
    pub drag_curve: GpuCurve,
    pub direction: Vec3,
    pub _direction_padding: f32,
    pub lifetime: Vec2,
    pub speed: Vec2,
    pub angular_velocity: Vec2,
    pub _range_padding: Vec2,
    pub gravity: Vec3,
    pub gravity_source: u32,
    pub gravity_max: Vec3,
    pub _gravity_max_padding: f32,
    pub gravity_curves: [GpuCurve; 3],
    pub turbulence: Vec2,
    pub turbulence_source: u32,
    /// Strand count for deterministic ribbon linking; zero disables, trails use one.
    pub _turbulence_padding: u32,
    pub turbulence_curve: GpuCurve,
    pub translation: Vec3,
    pub max_scale: f32,
    pub rotation: Vec4,
    pub scale: Vec3,
    /// Non-zero when this emitter is simulated by the stateful GPU path (hybrid roadmap M6); the
    /// analytic `simulate` skips its slots so analytic and stateful emitters can share one effect's
    /// buffers. (Occupies the former transform padding word — no layout change.)
    pub stateful: u32,
    pub size: GpuCurve,
    pub opacity: GpuCurve,
    pub color: GpuGradient,
    /// Inverse-emission table for a curve-driven spawn rate: `spawn_inverse[k]` is the
    /// spawn time (in `[0, source_duration]`) at which cumulative emission reaches the
    /// fraction `k/(SPAWN_INVERSE_SAMPLES-1)` of `spawn_inverse_total`. Zeroed for
    /// non-curve spawn rates, which do not need reconstruction.
    pub spawn_inverse: [f32; SPAWN_INVERSE_SAMPLES],
    /// Total emission over the emitter's source duration — the denominator for the
    /// spawn-inverse fraction. Zero when the table is unused.
    pub spawn_inverse_total: f32,
    pub _spawn_inverse_padding: Vec3,
    pub trail_offset: u32,
    pub trail_points: u32,
    pub trail_interval: f32,
    pub trail_lifetime: f32,
    pub trail_capacity: u32,
    pub trail_sampling: u32,
    pub trail_distance: f32,
    pub trail_tolerance: f32,
}

/// One authored presentation path for an emitter.
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct GpuRenderer {
    pub emitter_index: u32,
    pub blend_mode: u32,
    pub softness: f32,
    pub textured: u32,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub tint: Vec4,
    pub particle_color: u32,
    pub renderer_kind: u32,
    pub frame_count: u32,
    /// Flipbook playback mode, or the resolved owner budget for Trail (kind 4).
    pub playback_mode: u32,
    /// Flipbook flags; for Trail, 0 = Stretch and 1 = Tile UVs.
    pub flipbook_flags: u32,
    pub frame_rate: f32,
    /// x: omitted particle reads; y: strip width (f32 bits); z: trail history offset.
    pub attribute_flags: UVec3,
    /// Flipbook rectangles; Trail uses only frames[0].x for world-space tile length.
    pub frames: [Vec4; MAX_FLIPBOOK_FRAMES],
}

/// Selects the renderer record used by one indirect draw.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderParams {
    pub renderer_index: u32,
    pub alive_offset: u32,
    /// x enables the trail compact-index list in the existing alive-index binding.
    pub _padding: UVec2,
    /// Emitter rotation and relative scale; particle size already includes maximum emitter scale.
    pub mesh_from_local: Mat4,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGlobals {
    pub time: f32,
    pub total_slots: u32,
    pub seed: u32,
    pub emitter_count: u32,
    pub duration: f32,
    pub continuous: u32,
    pub _padding: UVec2,
    /// World-space trail recording. `_padding.x` is the discontinuity epoch.
    pub world_from_effect: Mat4,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderGlobals {
    pub world_from_effect: Mat4,
    pub time: f32,
    pub seed: u32,
    pub _padding: Vec2,
}

/// Stable storage/readback ABI shared with the GPU simulation shader. 48 bytes:
/// `emitter_index` and `alive` are packed into one word (`emitter_index << 16 | alive`),
/// and ribbon/trail scratch lives in the separate `aux` buffer, so the record holds
/// only live presentation state.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuParticle {
    pub color: Vec4,
    pub position: Vec3,
    pub size: f32,
    pub rotation: f32,
    pub normalized_age: f32,
    /// `emitter_index << 16 | (alive & 0xffff)`. `alive` carries the trail tri-state (0/1/2).
    pub packed_emitter_alive: u32,
    pub particle_index: u32,
}

#[derive(Debug, Clone)]
pub struct GpuEffectArtifact {
    pub emitters: Vec<GpuEmitter>,
    pub renderers: Vec<GpuRenderer>,
    pub particles: Vec<GpuParticle>,
    pub total_slots: u32,
    pub bounds_half_extents: Vec3,
    /// Persistent simulation-state sizing for stateful emitters (hybrid M4/M6). Empty for analytic
    /// effects. The render backend allocates its state buffer from this.
    pub simulation_state: GpuSimulationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum GpuBlend {
    Alpha = 0,
    Additive = 1,
    Multiply = 2,
}

/// The dynamic, per-frame portion of a GPU effect artifact: emitter and renderer
/// inputs plus the slot count and bounds, but not the capacity-sized particle
/// scratch buffer. Building this avoids allocating and zeroing `Vec<GpuParticle>`
/// on the per-frame update path, where only emitter and renderer inputs change.
#[derive(Debug, Clone)]
pub struct GpuEffectDynamics {
    /// Geometry-independent bounds inputs, indexed by compiled emitter (including disabled ones).
    pub mesh_bounds: Vec<mesh_bounds::MeshParticleBounds>,
    /// Camera-facing bounds inputs, indexed by compiled emitter (including disabled ones).
    pub ribbon_bounds: Vec<ribbon_bounds::RibbonParticleBounds>,
    pub emitters: Vec<GpuEmitter>,
    pub renderers: Vec<GpuRenderer>,
    pub total_slots: u32,
    pub storage_records: u32,
    pub bounds_half_extents: Vec3,
    /// Persistent simulation-state storage required by stateful/staged emitters (hybrid roadmap
    /// M4/M6), kept separate from the presentation particle buffer. Empty for analytic effects.
    pub simulation_state: GpuSimulationState,
}

/// The persistent simulation-state storage a stateful/staged effect needs on the GPU, kept separate
/// from the 48-byte presentation particle buffer (hybrid roadmap M4/M6). `records == 0` for a fully
/// analytic effect, so analytic effects allocate no simulation-state buffer. The render backend
/// allocates `records * stride` `f32`s of persistent storage from this.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuSimulationState {
    /// `f32` components of persistent state per stateful slot (0 when the effect is analytic).
    pub stride: u32,
    /// Total persistent-state records — the combined slot capacity of the enabled stateful emitters.
    pub records: u32,
}

/// The deterministic spawn RNG for the stateful GPU backend (hybrid roadmap M6), as reusable WGSL
/// functions. WGSL has no native `u64`, so splitmix64 is emulated with `u32` pairs
/// (`vec2<u32>` = `(lo, hi)`). This is the canonical GPU counterpart of
/// `aestra_runtime::StatefulSimulation::{splitmix64, launch_direction}`, conformance-checked against
/// it, and included by the GPU spawn shader. Prepend it to a shader module and call
/// `spawn_launch_direction(seed, ordinal)`.
pub const STATEFUL_SPAWN_RNG_WGSL: &str = r#"
// u64 emulated as vec2<u32> = (lo, hi).
fn aestra_mul_u32_full(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xFFFFu; let a1 = a >> 16u;
    let b0 = b & 0xFFFFu; let b1 = b >> 16u;
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = p01 + p10;
    let mid_carry = select(0u, 1u, mid < p01);
    let lo = p00 + (mid << 16u);
    let lo_carry = select(0u, 1u, lo < p00);
    let hi = p11 + (mid >> 16u) + (mid_carry << 16u) + lo_carry;
    return vec2<u32>(lo, hi);
}
fn aestra_u64_add(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let lo = a.x + b.x;
    let carry = select(0u, 1u, lo < a.x);
    return vec2<u32>(lo, a.y + b.y + carry);
}
fn aestra_u64_mul(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let ll = aestra_mul_u32_full(a.x, b.x);
    let cross = a.x * b.y + a.y * b.x;
    return vec2<u32>(ll.x, ll.y + cross);
}
fn aestra_u64_shr(a: vec2<u32>, s: u32) -> vec2<u32> {
    if (s == 0u) { return a; }
    if (s < 32u) {
        return vec2<u32>((a.x >> s) | (a.y << (32u - s)), a.y >> s);
    }
    return vec2<u32>(a.y >> (s - 32u), 0u);
}
fn aestra_splitmix64(input: vec2<u32>) -> vec2<u32> {
    var z = aestra_u64_add(input, vec2<u32>(0x7F4A7C15u, 0x9E3779B9u));
    z = aestra_u64_mul(z ^ aestra_u64_shr(z, 30u), vec2<u32>(0x1CE4E5B9u, 0xBF58476Du));
    z = aestra_u64_mul(z ^ aestra_u64_shr(z, 27u), vec2<u32>(0x133111EBu, 0x94D049BBu));
    return z ^ aestra_u64_shr(z, 31u);
}
fn aestra_unit_signed(h: vec2<u32>) -> f32 {
    let v = aestra_u64_shr(h, 40u).x;
    return f32(v) / f32(1u << 24u) * 2.0 - 1.0;
}
fn spawn_launch_direction(seed: vec2<u32>, ordinal: vec2<u32>) -> vec3<f32> {
    let base = aestra_splitmix64(seed ^ aestra_u64_mul(ordinal, vec2<u32>(0x7F4A7C15u, 0x9E3779B9u)));
    return vec3<f32>(
        aestra_unit_signed(base),
        aestra_unit_signed(aestra_splitmix64(base)),
        aestra_unit_signed(aestra_splitmix64(base ^ vec2<u32>(0xD192ED03u, 0xD1B54A32u)))
    );
}
fn aestra_unit01(h: vec2<u32>) -> f32 {
    let v = aestra_u64_shr(h, 40u).x;
    return f32(v) / f32(1u << 24u);
}
// A deterministic per-particle uniform in [0, 1) for a named channel (speed = 0, lifetime = 1), salted
// distinctly from the direction hash. Mirrors aestra_runtime::StatefulSimulation::spawn_uniform.
fn aestra_spawn_uniform(seed: vec2<u32>, ordinal: vec2<u32>, channel: u32) -> f32 {
    let term1 = aestra_u64_mul(ordinal, vec2<u32>(0x7F4A7C15u, 0x9E3779B9u));
    let term2 = aestra_u64_mul(vec2<u32>(channel + 1u, 0u), vec2<u32>(0x6659FD93u, 0xD6E8FEB8u));
    return aestra_unit01(aestra_splitmix64((seed ^ term1) ^ term2));
}
// The full per-particle launch velocity: the authored direction blended with the random unit vector to
// a spread cone, renormalized (sqrt/division only — no trig, so the CPU reference matches bit-for-bit),
// scaled by a per-particle random speed. Mirrors aestra_runtime::StatefulSimulation::launch_velocity.
fn spawn_launch_velocity(
    seed: vec2<u32>, ordinal: u32, speed_min: f32, speed_max: f32, direction: vec3<f32>, spread: f32
) -> vec3<f32> {
    let ord = vec2<u32>(ordinal, 0u);
    let random_unit = spawn_launch_direction(seed, ord);
    let mixed = direction + spread * random_unit;
    let length_squared = mixed.x * mixed.x + mixed.y * mixed.y + mixed.z * mixed.z;
    var dir = random_unit;
    if (length_squared > 1e-12) {
        dir = mixed / sqrt(length_squared);
    }
    let speed = speed_min + (speed_max - speed_min) * aestra_spawn_uniform(seed, ord, 0u);
    return dir * speed;
}
// A signed per-particle uniform in [-1, 1) for a channel. Mirrors the CPU reference's spawn_signed.
fn aestra_spawn_signed(seed: vec2<u32>, ordinal: vec2<u32>, channel: u32) -> f32 {
    return aestra_spawn_uniform(seed, ordinal, channel) * 2.0 - 1.0;
}
// The initial position sampled from the spawn shape (0 = point, 1 = sphere, 2 = box). Trig- and
// cbrt-free, so it matches aestra_runtime::StatefulSimulation::launch_position bit-for-bit.
fn spawn_launch_position(
    seed: vec2<u32>, ordinal: u32, shape_kind: u32, radius: f32, half_extents: vec3<f32>
) -> vec3<f32> {
    let ord = vec2<u32>(ordinal, 0u);
    if (shape_kind == 2u) {
        return vec3<f32>(
            aestra_spawn_signed(seed, ord, 2u) * half_extents.x,
            aestra_spawn_signed(seed, ord, 3u) * half_extents.y,
            aestra_spawn_signed(seed, ord, 4u) * half_extents.z);
    } else if (shape_kind == 1u) {
        let v = vec3<f32>(
            aestra_spawn_signed(seed, ord, 2u),
            aestra_spawn_signed(seed, ord, 3u),
            aestra_spawn_signed(seed, ord, 4u));
        let length_squared = v.x * v.x + v.y * v.y + v.z * v.z;
        var dir = vec3<f32>(0.0, 1.0, 0.0);
        if (length_squared > 1e-12) {
            dir = v / sqrt(length_squared);
        }
        let distance = radius * sqrt(aestra_spawn_uniform(seed, ord, 5u));
        return dir * distance;
    }
    return vec3<f32>(0.0, 0.0, 0.0);
}
fn aestra_turbulence_hash(seed: vec2<u32>, ordinal: vec2<u32>, channel: u32, cell: u32) -> vec2<u32> {
    let key = channel * 0x10000u + cell + 1u;
    let term1 = aestra_u64_mul(ordinal, vec2<u32>(0x7F4A7C15u, 0x9E3779B9u));
    let term2 = aestra_u64_mul(vec2<u32>(key, 0u), vec2<u32>(0x6659FD93u, 0xD6E8FEB8u));
    return aestra_splitmix64((seed ^ term1) ^ term2);
}
// One axis of value noise in [-1, 1): hash the integer cell of age*FREQ and the next, smoothstep-lerp.
fn aestra_turbulence_noise(seed: vec2<u32>, ordinal: u32, channel: u32, age: f32) -> f32 {
    let ord = vec2<u32>(ordinal, 0u);
    let t = age * 3.0;
    let cell_f = floor(t);
    let frac = t - cell_f;
    let cell = u32(cell_f);
    let a = aestra_unit_signed(aestra_turbulence_hash(seed, ord, channel, cell));
    let b = aestra_unit_signed(aestra_turbulence_hash(seed, ord, channel, cell + 1u));
    let weight = frac * frac * (3.0 - 2.0 * frac);
    return a + (b - a) * weight;
}
// The per-axis turbulence acceleration for a particle at `age`. Mirrors the CPU
// aestra_runtime::StatefulSimulation::turbulence_acceleration; depends only on (seed, ordinal, age).
fn spawn_turbulence(seed: vec2<u32>, ordinal: u32, age: f32, strength: f32) -> vec3<f32> {
    if (strength == 0.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    return vec3<f32>(
        strength * aestra_turbulence_noise(seed, ordinal, 10u, age),
        strength * aestra_turbulence_noise(seed, ordinal, 11u, age),
        strength * aestra_turbulence_noise(seed, ordinal, 12u, age));
}
"#;

/// A GPU atomic slot allocator for stateful particle death/reuse (hybrid roadmap M6): dead slots are
/// pushed onto a free list and reused by spawns. The including shader must declare the module-scope
/// bindings `free_list: array<u32>` and `free_count: atomic<u32>` (naga does not accept atomic
/// pointers as function parameters, so these operate on the module bindings directly — WGSL allows
/// referring to a global declared elsewhere in the module). `aestra_free_pop` returns a distinct free
/// slot (the caller must dispatch no more poppers than the current free count); `aestra_free_push`
/// returns a dead slot to the list. The specific slot a spawn receives is irrelevant — particles are
/// matched by their deterministic spawn ordinal, not by slot — so parallel allocation order does not
/// affect the simulation result.
pub const STATEFUL_FREE_LIST_WGSL: &str = r#"
fn aestra_free_pop() -> u32 {
    let top = atomicSub(&free_count, 1u);
    return free_list[top - 1u];
}
fn aestra_free_push(slot: u32) {
    let index = atomicAdd(&free_count, 1u);
    free_list[index] = slot;
}
"#;

/// Presentation extraction for the stateful GPU backend (hybrid roadmap M6): map one persistent
/// simulation-state slot to the 48-byte [`GpuParticle`] presentation ABI the renderer consumes, so
/// the stateful path feeds the same alive/compaction/render pipeline the analytic path uses. This is
/// the GPU counterpart of `aestra_runtime::StatefulSimulation::present`.
///
/// The persistent state slot is `AESTRA_STATE_STRIDE` (9) `f32`s — position xyz, velocity xyz, age,
/// lifetime, and the spawn ordinal stored as bits (the stable particle identity). The presentation
/// record is 12 words matching `GpuParticle`: `color.rgba`, `position.xyz`, `size`, `rotation`,
/// `normalized_age`, `packed_emitter_alive` (`emitter << 16 | alive`), `particle_index`. The spawn
/// ordinal is written verbatim into `particle_index`, the analytic path's stable per-particle index
/// (used for per-particle RNG and ribbon strand grouping). A dead slot (`lifetime <= 0` or
/// `age >= lifetime`) is emitted with `alive = 0`; the existing compaction pass drops it.
///
/// The including shader must declare module-scope bindings `state: array<f32>` (the persistent
/// buffer) and `present_out: array<f32>` (12 words per slot), matching the free-list convention.
/// `slot` indexes the per-emitter `state`; `out_slot` indexes the effect-wide `present_out`, so a
/// stateful emitter whose particles occupy `[slot_offset, slot_offset + capacity)` of the shared
/// buffer passes `out_slot = slot_offset + slot`.
///
/// `subtick` (seconds since the last fixed tick, in `[0, dt)`) is the presentation-interpolation term
/// (hybrid roadmap M8): the position is extrapolated by `velocity * subtick` and the age by `subtick`,
/// so stateful particles move smoothly between the 60 Hz ticks and stay coherent with the continuous
/// time analytic emitters evaluate at. Pass `0` for the raw tick state.
pub const STATEFUL_PRESENT_WGSL: &str = r#"
const AESTRA_STATE_STRIDE: u32 = 9u;
const AESTRA_PRESENT_STRIDE: u32 = 12u;

fn aestra_present_stateful(slot: u32, out_slot: u32, emitter_index: u32, subtick: f32) {
    let s = slot * AESTRA_STATE_STRIDE;
    let o = out_slot * AESTRA_PRESENT_STRIDE;
    let age = state[s + 6u];
    let lifetime = state[s + 7u];
    let alive = lifetime > 0.0 && age < lifetime;
    // Presentation interpolation: extrapolate position by velocity and age by the sub-tick time.
    var normalized_age = 0.0;
    if (lifetime > 0.0) {
        normalized_age = clamp((age + subtick) / lifetime, 0.0, 1.0);
    }
    present_out[o + 0u] = 1.0; // color.r
    present_out[o + 1u] = 1.0; // color.g
    present_out[o + 2u] = 1.0; // color.b
    present_out[o + 3u] = 1.0; // color.a
    present_out[o + 4u] = state[s + 0u] + state[s + 3u] * subtick; // position.x
    present_out[o + 5u] = state[s + 1u] + state[s + 4u] * subtick; // position.y
    present_out[o + 6u] = state[s + 2u] + state[s + 5u] * subtick; // position.z
    present_out[o + 7u] = 1.0; // size
    present_out[o + 8u] = 0.0; // rotation
    present_out[o + 9u] = normalized_age;
    present_out[o + 10u] = bitcast<f32>((emitter_index << 16u) | select(0u, 1u, alive)); // packed_emitter_alive
    present_out[o + 11u] = state[s + 8u]; // particle_index = spawn ordinal (bits copied verbatim)
}
"#;

/// Collision resolution for the stateful GPU backend (hybrid roadmap M10): after a particle is
/// integrated, each authored collider is applied in order, pushing the particle back onto the surface
/// and bouncing (restitution + friction) or killing it. This is the GPU counterpart of
/// `aestra_runtime::stateful`'s `resolve_colliders`; every operation is `+ - * /`, comparison, or
/// `sqrt` (all IEEE-correctly-rounded — no trig), so the two match bit-for-bit, and it reads only the
/// persistent position/velocity, so a checkpoint restore reproduces every bounce.
///
/// Colliders are packed into `params` starting at [`AESTRA_COLLIDER_COUNT_INDEX`]: one count word, then
/// up to [`MAX_COLLIDERS`](aestra_runtime::MAX_COLLIDERS) records of 10 words each —
/// `[kind, a.xyz, b.xyz, restitution, friction, kill]`. `kind` is `0` plane (`a` = unit normal,
/// `b.x` = plane distance), `1` sphere (`a` = center, `b.x` = radius), `2` box (`a` = min, `b` = max).
/// The including shader must declare the `params: array<u32>` binding.
pub const STATEFUL_COLLISION_WGSL: &str = r#"
const AESTRA_COLLIDER_COUNT_INDEX: u32 = 26u;
const AESTRA_COLLIDER_BASE: u32 = 27u;
const AESTRA_COLLIDER_STRIDE: u32 = 10u;
const AESTRA_MAX_COLLIDERS: u32 = 4u;

struct AestraCollision { position: vec3<f32>, velocity: vec3<f32>, killed: bool };

// Left-to-right 3-component dot, matching the CPU reference's summation order exactly (the `dot`
// builtin may contract to a fused multiply-add and round differently, which near a contact boundary
// could flip whether a particle collides and diverge from the CPU).
fn aestra_dot3(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

// One collider (packed at params[base .. base+10]) resolved against a particle's position/velocity.
fn aestra_collider_at(base: u32, position: vec3<f32>, velocity: vec3<f32>) -> AestraCollision {
    let kind = params[base + 0u];
    let a = vec3<f32>(bitcast<f32>(params[base + 1u]), bitcast<f32>(params[base + 2u]), bitcast<f32>(params[base + 3u]));
    let b = vec3<f32>(bitcast<f32>(params[base + 4u]), bitcast<f32>(params[base + 5u]), bitcast<f32>(params[base + 6u]));
    let restitution = bitcast<f32>(params[base + 7u]);
    let friction = bitcast<f32>(params[base + 8u]);
    let kill = params[base + 9u] != 0u;

    // Each shape reports (contact, outward unit normal, penetration depth >= 0).
    var contact = false;
    var normal = vec3<f32>(0.0, 1.0, 0.0);
    var penetration = 0.0;
    if (kind == 0u) {
        // Plane: half-space below dot(normal, p) = distance.
        let signed = aestra_dot3(a, position) - b.x;
        contact = signed < 0.0;
        normal = a;
        penetration = -signed;
    } else if (kind == 1u) {
        // Sphere: inside the radius.
        let delta = position - a;
        let len2 = aestra_dot3(delta, delta);
        let distance = sqrt(len2);
        contact = distance < b.x;
        if (len2 > 1e-12) { normal = delta / sqrt(len2); } else { normal = vec3<f32>(0.0, 1.0, 0.0); }
        penetration = b.x - distance;
    } else {
        // AABB: inside the box; exit along the axis/face of least penetration (strict < breaks ties
        // toward the earlier axis and toward min, matching the CPU reference exactly).
        contact = position.x > a.x && position.x < b.x
            && position.y > a.y && position.y < b.y
            && position.z > a.z && position.z < b.z;
        var best_pen = 3.4028235e38;
        var best_normal = vec3<f32>(0.0, 1.0, 0.0);
        let to_min_x = position.x - a.x;
        if (to_min_x < best_pen) { best_pen = to_min_x; best_normal = vec3<f32>(-1.0, 0.0, 0.0); }
        let to_max_x = b.x - position.x;
        if (to_max_x < best_pen) { best_pen = to_max_x; best_normal = vec3<f32>(1.0, 0.0, 0.0); }
        let to_min_y = position.y - a.y;
        if (to_min_y < best_pen) { best_pen = to_min_y; best_normal = vec3<f32>(0.0, -1.0, 0.0); }
        let to_max_y = b.y - position.y;
        if (to_max_y < best_pen) { best_pen = to_max_y; best_normal = vec3<f32>(0.0, 1.0, 0.0); }
        let to_min_z = position.z - a.z;
        if (to_min_z < best_pen) { best_pen = to_min_z; best_normal = vec3<f32>(0.0, 0.0, -1.0); }
        let to_max_z = b.z - position.z;
        if (to_max_z < best_pen) { best_pen = to_max_z; best_normal = vec3<f32>(0.0, 0.0, 1.0); }
        normal = best_normal;
        penetration = best_pen;
    }

    if (!contact) { return AestraCollision(position, velocity, false); }
    if (kill) { return AestraCollision(position, velocity, true); }

    // Push out along the contact normal, reflect the inbound normal velocity by restitution, damp the
    // tangential velocity by friction — the uniform response shared by every shape.
    let new_position = position + normal * penetration;
    let normal_speed = aestra_dot3(velocity, normal);
    let tangential = velocity - normal * normal_speed;
    var reflected = normal_speed;
    if (normal_speed < 0.0) { reflected = -restitution * normal_speed; }
    let new_velocity = tangential * (1.0 - friction) + normal * reflected;
    return AestraCollision(new_position, new_velocity, false);
}

// Applies every active collider to a particle in order, returning the resolved state and whether it was
// killed. Mirrors aestra_runtime::stateful::resolve_colliders.
fn aestra_resolve_colliders(position: vec3<f32>, velocity: vec3<f32>) -> AestraCollision {
    var pos = position;
    var vel = velocity;
    let count = min(params[AESTRA_COLLIDER_COUNT_INDEX], AESTRA_MAX_COLLIDERS);
    for (var i = 0u; i < count; i = i + 1u) {
        let base = AESTRA_COLLIDER_BASE + i * AESTRA_COLLIDER_STRIDE;
        let r = aestra_collider_at(base, pos, vel);
        if (r.killed) { return AestraCollision(pos, vel, true); }
        pos = r.position;
        vel = r.velocity;
    }
    return AestraCollision(pos, vel, false);
}
"#;

/// The module-scope bindings the unified stateful simulation module ([`stateful_simulation_wgsl`])
/// declares, shared across its `death_integrate` / `spawn` / `present` entry points. The persistent
/// state, the free list and its atomic count, the atomic spawn counter, the constant per-dispatch
/// params, the presentation output (the 48-byte `GpuParticle` buffer, written as raw words), and the
/// three compaction outputs `present` shares with the analytic path so both feed one render pipeline:
/// the compacted alive-slot list, and the atomic indirect draw commands and live counters.
pub const STATEFUL_SIMULATION_BINDINGS: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read_write> free_list: array<u32>;
@group(0) @binding(2) var<storage, read_write> free_count: atomic<u32>;
@group(0) @binding(3) var<storage, read_write> spawn_counter: atomic<u32>;
@group(0) @binding(4) var<storage, read> params: array<u32>;
@group(0) @binding(5) var<storage, read_write> present_out: array<f32>;
@group(0) @binding(6) var<storage, read_write> alive_indices: array<u32>;
@group(0) @binding(7) var<storage, read_write> indirect: array<atomic<u32>>;
@group(0) @binding(8) var<storage, read_write> counters: array<atomic<u32>>;
"#;

/// The three entry points of the unified stateful simulation module (hybrid roadmap M6), over the
/// [`STATEFUL_SIMULATION_BINDINGS`] layout. `death_integrate` advances each live slot by one fixed
/// tick (gravity, then linear drag) and frees the ones that died; `spawn` claims a free slot and a
/// fresh ordinal for each of this tick's new particles, sampling a per-particle speed and lifetime in
/// range and a launch direction on the authored spread cone; `present` extracts the live state into the
/// presentation buffer *and* compacts the live slots into `alive_indices` while bumping the indirect
/// draw count and live counter — the same compaction the analytic `simulate` performs, so the stateful
/// output draws through the identical render path. All dynamics mirror
/// `aestra_runtime::StatefulSimulation` bit-for-bit. `params` is [`STATEFUL_SIMULATION_PARAM_WORDS`]
/// `u32`s: `[capacity, spawn_per_tick, seed_lo, seed_hi, speed_min, speed_max, lifetime_min,
/// lifetime_max, dt, gx, gy, gz, dir_x, dir_y, dir_z, spread, drag, emitter_index, slot_offset,
/// turbulence, shape_kind, shape_radius, half_x, half_y, half_z, subtick]` (floats stored as bits;
/// `shape_kind` 0 = point, 1 = sphere, 2 = box; `subtick` is the presentation-interpolation time in
/// seconds since the last tick), followed by the collider block (hybrid roadmap M10): a collider-count
/// word at index 26, then up to four 10-word collider records from index 27 (see
/// [`STATEFUL_COLLISION_WGSL`]), then the spawn placement (the emitter transform,
/// `aestra_runtime::SpawnPlacement`): a flag word at 67 (`0` = identity, skipped), translation at
/// 68..71, the unit rotation quaternion `xyzw` at 71..75 and scale at 75..78.
pub const STATEFUL_SIMULATION_PARAM_WORDS: usize = 78;

/// Packs a spawn placement into its [`STATEFUL_SIMULATION_PARAM_WORDS`] slots (67..78); the identity
/// leaves the flag clear so the kernel skips it, exactly as the CPU reference does.
pub fn pack_spawn_placement(placement: &aestra_runtime::SpawnPlacement, words: &mut [u32]) {
    words[67] = u32::from(*placement != aestra_runtime::SpawnPlacement::IDENTITY);
    let values = placement
        .translation
        .iter()
        .chain(&placement.rotation)
        .chain(&placement.scale);
    for (word, value) in words[68..78].iter_mut().zip(values) {
        *word = value.to_bits();
    }
}

/// Spawn placement for the stateful GPU backend: the emitter transform packed at
/// [`STATEFUL_SIMULATION_PARAM_WORDS`]' 67..78 (see [`pack_spawn_placement`]), applied by `spawn` when
/// the flag word is set. The GPU counterpart of `aestra_runtime::SpawnPlacement`, operation for
/// operation. The including shader must declare the `params: array<u32>` binding.
pub const STATEFUL_PLACEMENT_WGSL: &str = r#"
const AESTRA_PLACEMENT_INDEX: u32 = 67u;

fn aestra_param_f32(index: u32) -> f32 {
    return bitcast<f32>(params[index]);
}

// Rotates by the unit placement quaternion: v + w t + q x t with t = 2 q x v, in the operation order of
// aestra_runtime::SpawnPlacement::vector.
fn aestra_place_vector(v: vec3<f32>) -> vec3<f32> {
    let x = aestra_param_f32(71u);
    let y = aestra_param_f32(72u);
    let z = aestra_param_f32(73u);
    let w = aestra_param_f32(74u);
    let tx = 2.0 * (y * v.z - z * v.y);
    let ty = 2.0 * (z * v.x - x * v.z);
    let tz = 2.0 * (x * v.y - y * v.x);
    return vec3<f32>(
        v.x + w * tx + (y * tz - z * ty),
        v.y + w * ty + (z * tx - x * tz),
        v.z + w * tz + (x * ty - y * tx));
}

// Places a shape-local spawn position: translation + rotation(scale * local). Mirrors
// aestra_runtime::SpawnPlacement::point.
fn aestra_place_point(local: vec3<f32>) -> vec3<f32> {
    let rotated = aestra_place_vector(vec3<f32>(
        local.x * aestra_param_f32(75u),
        local.y * aestra_param_f32(76u),
        local.z * aestra_param_f32(77u)));
    return vec3<f32>(
        aestra_param_f32(68u) + rotated.x,
        aestra_param_f32(69u) + rotated.y,
        aestra_param_f32(70u) + rotated.z);
}
"#;

pub const STATEFUL_SIMULATION_ENTRIES: &str = r#"
@compute @workgroup_size(64)
fn death_integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= params[0]) { return; }
    let base = slot * AESTRA_STATE_STRIDE;
    let lifetime = state[base + 7u];
    let age = state[base + 6u];
    if (lifetime > 0.0 && age < lifetime) {
        let dt = bitcast<f32>(params[8]);
        let drag = bitcast<f32>(params[16]);
        let seed = vec2<u32>(params[2], params[3]);
        let ordinal = bitcast<u32>(state[base + 8u]);
        // Acceleration = gravity + value-noise turbulence at the current age.
        let turbulence = spawn_turbulence(seed, ordinal, age, bitcast<f32>(params[19]));
        let ax = bitcast<f32>(params[9]) + turbulence.x;
        let ay = bitcast<f32>(params[10]) + turbulence.y;
        let az = bitcast<f32>(params[11]) + turbulence.z;
        // Semi-implicit Euler with linear drag: accelerate, then damp, then position.
        let vgx = state[base + 3u] + ax * dt;
        let vgy = state[base + 4u] + ay * dt;
        let vgz = state[base + 5u] + az * dt;
        let vx = vgx - drag * vgx * dt;
        let vy = vgy - drag * vgy * dt;
        let vz = vgz - drag * vgz * dt;
        let px = state[base + 0u] + vx * dt;
        let py = state[base + 1u] + vy * dt;
        let pz = state[base + 2u] + vz * dt;
        // Collision: resolve the authored colliders against the freshly integrated state (M10). A kill
        // forces the death condition below; a bounce rewrites position/velocity.
        let collision = aestra_resolve_colliders(vec3<f32>(px, py, pz), vec3<f32>(vx, vy, vz));
        state[base + 0u] = collision.position.x;
        state[base + 1u] = collision.position.y;
        state[base + 2u] = collision.position.z;
        state[base + 3u] = collision.velocity.x;
        state[base + 4u] = collision.velocity.y;
        state[base + 5u] = collision.velocity.z;
        var new_age = age + dt;
        if (collision.killed) { new_age = lifetime; }
        state[base + 6u] = new_age;
        if (new_age >= lifetime) {
            state[base + 7u] = 0.0; // mark the slot free
            aestra_free_push(slot);
        }
    }
}

@compute @workgroup_size(64)
fn spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params[1]) { return; }
    // Claim a free slot; if the free list is empty this tick, this spawn does not happen (matching
    // the CPU reference's room bound).
    let top = atomicSub(&free_count, 1u);
    if (top == 0u || top > params[0]) {
        atomicAdd(&free_count, 1u);
        return;
    }
    let slot = free_list[top - 1u];
    let ordinal = atomicAdd(&spawn_counter, 1u);
    let seed = vec2<u32>(params[2], params[3]);
    let direction = vec3<f32>(bitcast<f32>(params[12]), bitcast<f32>(params[13]), bitcast<f32>(params[14]));
    let velocity = spawn_launch_velocity(
        seed, ordinal, bitcast<f32>(params[4]), bitcast<f32>(params[5]), direction, bitcast<f32>(params[15]));
    let lifetime = bitcast<f32>(params[6])
        + (bitcast<f32>(params[7]) - bitcast<f32>(params[6])) * aestra_spawn_uniform(seed, vec2<u32>(ordinal, 0u), 1u);
    let half_extents = vec3<f32>(bitcast<f32>(params[22]), bitcast<f32>(params[23]), bitcast<f32>(params[24]));
    var position = spawn_launch_position(seed, ordinal, params[20], bitcast<f32>(params[21]), half_extents);
    var launch = velocity;
    if (params[AESTRA_PLACEMENT_INDEX] != 0u) {
        position = aestra_place_point(position);
        launch = aestra_place_vector(velocity);
    }
    let base = slot * AESTRA_STATE_STRIDE;
    state[base + 0u] = position.x;
    state[base + 1u] = position.y;
    state[base + 2u] = position.z;
    state[base + 3u] = launch.x;
    state[base + 4u] = launch.y;
    state[base + 5u] = launch.z;
    state[base + 6u] = 0.0;
    state[base + 7u] = lifetime;
    state[base + 8u] = bitcast<f32>(ordinal);
}

@compute @workgroup_size(64)
fn present(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= params[0]) { return; }
    let emitter_index = params[17];
    let slot_offset = params[18];
    // This emitter's particles occupy [slot_offset, slot_offset + capacity) of the effect-wide
    // presentation and alive buffers; state is the emitter's own buffer, indexed by the local slot.
    let out_slot = slot_offset + slot;
    aestra_present_stateful(slot, out_slot, emitter_index, bitcast<f32>(params[25]));
    // Compact the live slots exactly as the analytic simulate does: append the global particle index
    // to this emitter's region of alive_indices at an atomically-claimed index, and bump its indirect
    // instance count and the global live counter. Dead slots are written (alive = 0) but not compacted.
    let base = slot * AESTRA_STATE_STRIDE;
    let lifetime = state[base + 7u];
    let age = state[base + 6u];
    if (lifetime > 0.0 && age < lifetime) {
        let compact_index = atomicAdd(&indirect[emitter_index * 4u + 1u], 1u);
        alive_indices[slot_offset + compact_index] = out_slot;
        atomicAdd(&counters[0], 1u);
    }
}
"#;

/// The full stateful simulation shader module (hybrid roadmap M6): the shared bindings, the three
/// proven primitives (the emulated-u64 spawn RNG, the atomic free-list allocator, and presentation
/// extraction), and the three entry points, composed into one WGSL module the render backend builds
/// its `death_integrate` / `spawn` / `present` compute pipelines from. Every fragment here is
/// conformance-checked on real GPU compute in `aestra-bevy-render`.
pub fn stateful_simulation_wgsl() -> String {
    format!(
        "{STATEFUL_SIMULATION_BINDINGS}{STATEFUL_SPAWN_RNG_WGSL}{STATEFUL_FREE_LIST_WGSL}\
         {STATEFUL_PRESENT_WGSL}{STATEFUL_COLLISION_WGSL}{STATEFUL_PLACEMENT_WGSL}\
         {STATEFUL_SIMULATION_ENTRIES}"
    )
}

/// Words of [`FIELD_FOLLOW_WGSL`]'s `params`: `[capacity, dims.x, dims.y, dims.z, cell stride,
/// origin.x, origin.y, origin.z, cell_size, strength, dt, staggered]` (floats as bits).
pub const FIELD_FOLLOW_PARAM_WORDS: usize = 12;

/// Follow Field for stateful particles (fluid F2b): each live slot of the persistent state
/// (`AESTRA_STATE_STRIDE` = 9 floats: position, velocity, age, lifetime, ordinal) samples a vector
/// grid field — a domain's [`aestra_runtime::FieldLayout`] — trilinearly at its position (cell-centred,
/// clamped to the grid) and moves its velocity toward the sampled `xyz` by `min(strength × dt, 1)`.
/// A gather over particles: no atomics, so reruns reproduce the same bits. The stateful backend runs it
/// right after each tick's `death_integrate`/`spawn`, so the pull shapes the next tick's motion.
pub const FIELD_FOLLOW_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> field: array<f32>;
@group(0) @binding(2) var<storage, read> params: array<u32>;

const FOLLOW_STATE_STRIDE: u32 = 9u;

fn follow_field_value(cell: vec3<i32>) -> vec3<f32> {
    let dims = vec3<i32>(i32(params[1]), i32(params[2]), i32(params[3]));
    let c = clamp(cell, vec3<i32>(0), dims - vec3<i32>(1));
    let base = (u32((c.z * dims.y + c.y) * dims.x + c.x)) * params[4];
    return vec3<f32>(field[base], field[base + 1u], field[base + 2u]);
}

// One component of a staggered (MAC) field at grid coordinate `g` (cell centres at integers):
// component `c` lives on the cells' minimum faces, so its own grid is shifted by half a cell.
fn follow_staggered_component(g: vec3<f32>, component: u32) -> f32 {
    let last = vec3<f32>(f32(params[1]), f32(params[2]), f32(params[3])) - vec3<f32>(1.0);
    var shift = vec3<f32>(0.0);
    if (component == 0u) { shift.x = 0.5; }
    if (component == 1u) { shift.y = 0.5; }
    if (component == 2u) { shift.z = 0.5; }
    let q = clamp(g + shift, vec3<f32>(0.0), last);
    let base = floor(q);
    let t = q - base;
    let b = vec3<i32>(base);
    let c000 = follow_field_value(b);
    let c100 = follow_field_value(b + vec3<i32>(1, 0, 0));
    let c010 = follow_field_value(b + vec3<i32>(0, 1, 0));
    let c110 = follow_field_value(b + vec3<i32>(1, 1, 0));
    let c001 = follow_field_value(b + vec3<i32>(0, 0, 1));
    let c101 = follow_field_value(b + vec3<i32>(1, 0, 1));
    let c011 = follow_field_value(b + vec3<i32>(0, 1, 1));
    let c111 = follow_field_value(b + vec3<i32>(1, 1, 1));
    let x0 = mix(c000, c100, t.x);
    let x1 = mix(c010, c110, t.x);
    let x2 = mix(c001, c101, t.x);
    let x3 = mix(c011, c111, t.x);
    let value = mix(mix(x0, x1, t.y), mix(x2, x3, t.y), t.z);
    if (component == 0u) { return value.x; }
    if (component == 1u) { return value.y; }
    return value.z;
}

@compute @workgroup_size(64)
fn follow_field(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= params[0]) { return; }
    let s = slot * FOLLOW_STATE_STRIDE;
    let age = state[s + 6u];
    let lifetime = state[s + 7u];
    if (!(lifetime > 0.0 && age < lifetime)) { return; }
    let origin = vec3<f32>(bitcast<f32>(params[5]), bitcast<f32>(params[6]), bitcast<f32>(params[7]));
    let cell_size = bitcast<f32>(params[8]);
    let position = vec3<f32>(state[s], state[s + 1u], state[s + 2u]);
    let last = vec3<f32>(f32(params[1]), f32(params[2]), f32(params[3])) - vec3<f32>(1.0);
    var target_velocity: vec3<f32>;
    if (params[11] != 0u) {
        let g = (position - origin) / cell_size - vec3<f32>(0.5);
        target_velocity = vec3<f32>(
            follow_staggered_component(g, 0u),
            follow_staggered_component(g, 1u),
            follow_staggered_component(g, 2u),
        );
    } else {
        let g = clamp((position - origin) / cell_size - vec3<f32>(0.5), vec3<f32>(0.0), last);
        let base = floor(g);
        let t = g - base;
        let b = vec3<i32>(base);
        let x0 = mix(follow_field_value(b), follow_field_value(b + vec3<i32>(1, 0, 0)), t.x);
        let x1 = mix(follow_field_value(b + vec3<i32>(0, 1, 0)), follow_field_value(b + vec3<i32>(1, 1, 0)), t.x);
        let x2 = mix(follow_field_value(b + vec3<i32>(0, 0, 1)), follow_field_value(b + vec3<i32>(1, 0, 1)), t.x);
        let x3 = mix(follow_field_value(b + vec3<i32>(0, 1, 1)), follow_field_value(b + vec3<i32>(1, 1, 1)), t.x);
        target_velocity = mix(mix(x0, x1, t.y), mix(x2, x3, t.y), t.z);
    }
    let pull = min(bitcast<f32>(params[9]) * bitcast<f32>(params[10]), 1.0);
    let velocity = vec3<f32>(state[s + 3u], state[s + 4u], state[s + 5u]);
    let followed = velocity + (target_velocity - velocity) * pull;
    state[s + 3u] = followed.x;
    state[s + 4u] = followed.y;
    state[s + 5u] = followed.z;
}
"#;

/// The [`FIELD_FOLLOW_WGSL`] params for one emitter following `follow` with `capacity` slots.
pub fn field_follow_params(
    capacity: u32,
    follow: &aestra_runtime::CompiledFieldFollow,
    dt: f32,
) -> [u32; FIELD_FOLLOW_PARAM_WORDS] {
    let field = &follow.field;
    let stride = if field.components == 3 {
        4
    } else {
        field.components
    };
    [
        capacity,
        field.dims[0],
        field.dims[1],
        field.dims[2],
        stride,
        field.origin[0].to_bits(),
        field.origin[1].to_bits(),
        field.origin[2].to_bits(),
        field.cell_size.to_bits(),
        follow.strength.to_bits(),
        dt.to_bits(),
        u32::from(field.staggered),
    ]
}

/// The 2D-diffusion compute pass — the first staged-simulation validation workload (hybrid roadmap
/// M13). One explicit (Jacobi) diffusion step on a periodic grid: it reads the front grid buffer and
/// writes the back, and the staged executor ping-pongs them across iterations. Bindings match the
/// staged executor's convention — read resources, then write resources, then `params` — over the
/// `diffuse` pass of a [`aestra_runtime::StagedPlan`]. `params` is [`STAGED_DIFFUSION_PARAM_WORDS`]
/// `u32`s: `[width, height, rate_bits]`. Uses only `+ - *` (no trig), so it reproduces
/// `aestra_runtime::diffuse_2d_step` bit-for-bit, and diffusion is numerically damping so any residual
/// rounding decays rather than amplifies.
pub const STAGED_DIFFUSION_PARAM_WORDS: usize = 3;

pub const STAGED_DIFFUSION_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> grid_in: array<f32>;
@group(0) @binding(1) var<storage, read_write> grid_out: array<f32>;
@group(0) @binding(2) var<storage, read> params: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn diffuse(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = params[0];
    let height = params[1];
    let rate = bitcast<f32>(params[2]);
    let x = gid.x;
    let y = gid.y;
    if (x >= width || y >= height) { return; }
    // Periodic wrap at the edges — no boundary special-casing to diverge on. The unused select branch
    // may wrap in u32 (defined) but is never the chosen value.
    let left = select(x - 1u, width - 1u, x == 0u);
    let right = select(x + 1u, 0u, x == width - 1u);
    let up = select(y - 1u, height - 1u, y == 0u);
    let down = select(y + 1u, 0u, y == height - 1u);
    let center = grid_in[y * width + x];
    // Neighbour sum in the same left, right, up, down order as the CPU reference.
    let neighbours = grid_in[y * width + left]
        + grid_in[y * width + right]
        + grid_in[up * width + x]
        + grid_in[down * width + x];
    grid_out[y * width + x] = center + rate * (neighbours - 4.0 * center);
}
"#;

impl GpuEffectArtifact {
    /// Builds the full artifact including capacity-sized particle storage. Use this
    /// when persistent GPU particle buffers are first created or resized; the
    /// per-frame update path should prefer [`Self::dynamics_from_instance`], whose
    /// cost scales with emitter and renderer count rather than particle capacity.
    pub fn from_instance(instance: &EffectInstance) -> Result<Self, GpuArtifactError> {
        let dynamics = Self::dynamics_from_instance(instance)?;
        Ok(Self {
            particles: vec![GpuParticle::default(); dynamics.storage_records as usize],
            emitters: dynamics.emitters,
            renderers: dynamics.renderers,
            total_slots: dynamics.total_slots,
            bounds_half_extents: dynamics.bounds_half_extents,
            simulation_state: dynamics.simulation_state,
        })
    }

    /// Builds only the dynamic emitter/renderer inputs (plus slot count and bounds)
    /// without allocating the capacity-sized particle buffer.
    pub fn dynamics_from_instance(
        instance: &EffectInstance,
    ) -> Result<GpuEffectDynamics, GpuArtifactError> {
        for flipbook in &instance.effect().flipbooks {
            if flipbook.frames.len() > MAX_FLIPBOOK_FRAMES {
                return Err(GpuArtifactError::FlipbookFrameLimit {
                    name: flipbook.name.clone(),
                    actual: flipbook.frames.len(),
                    maximum: MAX_FLIPBOOK_FRAMES,
                });
            }
        }
        let parameters = instance.parameter_values();
        let mut slot_offset = 0_u32;
        let mut bounds_half_extents = Vec3::splat(0.01);
        let mut mesh_bounds = Vec::new();
        let mut ribbon_bounds = Vec::new();
        let mut emitters = Vec::with_capacity(instance.effect().emitters.len());
        let mut renderers = Vec::new();
        let mut history_offset = instance
            .effect()
            .emitters
            .iter()
            .try_fold(0u32, |total, e| total.checked_add(e.max_particles))
            .ok_or(GpuArtifactError::TrailLimit)?;
        for (emitter_index, emitter) in instance.effect().emitters.iter().enumerate() {
            let trails: Vec<_> = emitter
                .renderers
                .iter()
                .filter_map(|r| match r.kind {
                    RendererPlanKind::Trail {
                        sample_interval,
                        lifetime,
                        max_points,
                        max_trails,
                        sampling,
                        sample_distance,
                        curve_tolerance,
                        ..
                    } if emitter.enabled => Some((
                        sample_interval,
                        lifetime,
                        max_points,
                        if max_trails == 0 {
                            emitter.max_particles
                        } else {
                            max_trails
                        },
                        sampling,
                        sample_distance,
                        curve_tolerance,
                    )),
                    _ => None,
                })
                .collect();
            let trail_offset = history_offset;
            let (
                trail_interval,
                trail_lifetime,
                trail_points,
                trail_capacity,
                trail_sampling,
                trail_distance,
                trail_tolerance,
            ) = trails.first().copied().unwrap_or_default();
            if !trails.is_empty() {
                if trails.len() > 1
                    || emitter.max_particles > 256
                    || !(2..=64).contains(&trail_points)
                    || trail_capacity < emitter.max_particles
                    || trail_capacity > 1024
                    || !trail_distance.is_finite()
                    || trail_distance < 0.001
                    || !trail_tolerance.is_finite()
                    || trail_tolerance < 0.001
                    || emitter.renderers.iter().any(|renderer| matches!(renderer.kind,
                        RendererPlanKind::Trail { tile_length, .. } if !tile_length.is_finite() || tile_length < 0.001))
                {
                    return Err(GpuArtifactError::TrailLimit);
                }
                history_offset = history_offset
                    .checked_add(1 + trail_capacity * trail_points)
                    .ok_or(GpuArtifactError::TrailLimit)?;
                if history_offset > 1_048_576 {
                    return Err(GpuArtifactError::TrailLimit);
                }
            }
            let (spawn_rate, burst_count) =
                emission(&emitter.execution, parameters).ok_or_else(|| {
                    GpuArtifactError::MissingInstruction(emitter.name.clone(), "Emit")
                })?;
            let shape = shape(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "SampleShape")
            })?;
            let init = initialize(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Initialize")
            })?;
            let motion = motion(&emitter.execution, parameters).unwrap_or_default();
            let appearance = appearance(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Appearance")
            })?;
            let (shape_kind, shape_radius, shape_depth, shape_extent_z) = match shape {
                EmitterShape::Point => (0, 0.0, 0.0, 0.0),
                EmitterShape::Circle { radius } => (1, radius, 0.0, 0.0),
                EmitterShape::Ring { radius } => (2, radius, 0.0, 0.0),
                EmitterShape::Sphere { radius } => (3, radius, 0.0, 0.0),
                EmitterShape::Hemisphere { radius } => (4, radius, 0.0, 0.0),
                EmitterShape::Box { half_extents } => {
                    (5, half_extents[0], half_extents[1], half_extents[2])
                }
                EmitterShape::Cylinder { radius, depth } => (6, radius, depth, 0.0),
                EmitterShape::Cone { radius, depth } => (7, radius, depth, 0.0),
            };
            if emitter.enabled {
                renderers.extend(emitter.renderers.iter().map(|renderer| {
                    // Semantic material instances are bound by the concrete render adapter. The
                    // portable simulation artifact still supplies their sprite geometry and
                    // particle inputs, while legacy materials retain their existing packed data.
                    let material = instance.effect().material(renderer.material);
                    let (tint, particle_color, blend, softness, material_texture, uv) = material
                        .map_or(
                            (
                                [1.0; 4],
                                1,
                                BlendMode::Additive,
                                1.0,
                                None,
                                aestra_core::UvRect::FULL,
                            ),
                            |material| {
                                let (tint, particle_color) = match &material.color {
                                    MaterialColorPlan::ParticleColor => ([1.0; 4], 1),
                                    MaterialColorPlan::Value(value) => {
                                        (*value.resolve(parameters), 0)
                                    }
                                };
                                (
                                    tint,
                                    particle_color,
                                    material.blend,
                                    *material.softness.resolve(parameters),
                                    material.texture,
                                    material.uv,
                                )
                            },
                        );
                    let mut frames = [Vec4::new(0.0, 0.0, 1.0, 1.0); MAX_FLIPBOOK_FRAMES];
                    let (
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        texture,
                    ) = match &renderer.kind {
                        RendererPlanKind::Sprite => (0, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Ribbon { .. } => (3, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Trail {
                            max_points,
                            lifetime,
                            uv_mode,
                            tile_length,
                            end_cap,
                            ..
                        } => {
                            // Trail reuses otherwise unused flipbook lanes; the
                            // compact particle/aux ABI and bindings stay unchanged.
                            frames[0].x = *tile_length;
                            (
                                4,
                                *max_points,
                                trail_capacity,
                                u32::from(*uv_mode == aestra_core::TrailUvMode::Tile)
                                    | (u32::from(*end_cap == aestra_core::TrailEndCap::Rounded)
                                        << 1),
                                *lifetime,
                                material_texture,
                            )
                        }
                        RendererPlanKind::Mesh { .. } => (2, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Flipbook {
                            flipbook,
                            time_source,
                            playback,
                            random_start,
                        } => {
                            let flipbook = instance
                                .effect()
                                .flipbook(*flipbook)
                                .expect("compiler guarantees flipbook references");
                            for (target, frame) in frames.iter_mut().zip(&flipbook.frames) {
                                *target = Vec4::new(
                                    frame.min[0],
                                    frame.min[1],
                                    frame.max[0],
                                    frame.max[1],
                                );
                            }
                            let mut flags = 0;
                            if *time_source == FlipbookTimeSource::EffectTime {
                                flags |= 1;
                            }
                            if *random_start {
                                flags |= 2;
                            }
                            if flipbook.looping {
                                flags |= 4;
                            }
                            (
                                1,
                                flipbook.frames.len() as u32,
                                match playback {
                                    FlipbookPlaybackMode::Forward => 0,
                                    FlipbookPlaybackMode::Reverse => 1,
                                    FlipbookPlaybackMode::PingPong => 2,
                                },
                                flags,
                                flipbook.frame_rate,
                                Some(flipbook.texture),
                            )
                        }
                    };
                    GpuRenderer {
                        emitter_index: emitter_index as u32,
                        blend_mode: match blend {
                            BlendMode::Alpha => GpuBlend::Alpha as u32,
                            BlendMode::Additive => GpuBlend::Additive as u32,
                            BlendMode::Multiply => GpuBlend::Multiply as u32,
                        },
                        softness,
                        textured: u32::from(texture.is_some()),
                        uv_min: Vec2::from_array(uv.min),
                        uv_max: Vec2::from_array(uv.max),
                        tint: Vec4::from_array(tint),
                        particle_color,
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        attribute_flags: UVec3::new(
                            0,
                            match renderer.kind {
                                RendererPlanKind::Ribbon { width, .. }
                                | RendererPlanKind::Trail { width, .. } => width.to_bits(),
                                _ => 0,
                            },
                            if renderer_kind == 4 { trail_offset } else { 0 },
                        ),
                        frames,
                    }
                }));
            }
            let local_bounds = emitter_bounds(
                shape,
                init.lifetime,
                init.speed,
                maximum_absolute_vector_source(motion.gravity),
                maximum_absolute_scalar_source(motion.turbulence),
                maximum_absolute_curve(appearance.size)
                    * 0.5
                    * emitter
                        .renderers
                        .iter()
                        .map(|r| match r.kind {
                            RendererPlanKind::Ribbon { width, .. } => width,
                            _ => 1.0,
                        })
                        .fold(1.0_f32, f32::max),
            );
            bounds_half_extents = bounds_half_extents.max(transformed_emitter_bounds(
                local_bounds,
                emitter.transform.translation,
                emitter.transform.rotation,
                emitter.transform.scale,
            ));
            let rotation = Quat::from_array(emitter.transform.rotation).normalize();
            let scale = Vec3::from_array(emitter.transform.scale);
            let particle_bounds = mesh_bounds::MeshParticleBounds {
                position_half_extents: transformed_emitter_bounds(
                    emitter_bounds(
                        shape,
                        init.lifetime,
                        init.speed,
                        maximum_absolute_vector_source(motion.gravity),
                        maximum_absolute_scalar_source(motion.turbulence),
                        0.0,
                    ),
                    emitter.transform.translation,
                    emitter.transform.rotation,
                    emitter.transform.scale,
                ),
                linear_from_local: glam::Mat3::from_quat(rotation)
                    * glam::Mat3::from_diagonal(scale),
                maximum_size: maximum_absolute_curve(appearance.size),
            };
            let maximum_width = emitter
                .renderers
                .iter()
                .map(|r| match r.kind {
                    RendererPlanKind::Ribbon { width, .. } => width,
                    _ => 0.0,
                })
                .fold(0.0_f32, f32::max);
            let ribbon = ribbon_bounds::RibbonParticleBounds {
                position_half_extents: particle_bounds.position_half_extents,
                maximum_half_width: particle_bounds.maximum_size
                    * scale.abs().max_element()
                    * maximum_width
                    * 0.5,
            };
            // The aggregate bound is also used for initial viewport framing. A
            // camera-facing ribbon cannot use emitter-axis-scaled sprite padding.
            if maximum_width > 0.0
                && let Some(bounds) = ribbon.half_extents(glam::Mat3::IDENTITY)
            {
                bounds_half_extents = bounds_half_extents.max(bounds);
            }
            ribbon_bounds.push(ribbon);
            mesh_bounds.push(particle_bounds);
            let (spawn_inverse, spawn_inverse_total) = if emitter.enabled {
                build_spawn_inverse(spawn_rate, emitter.source_duration)
            } else {
                ([0.0; SPAWN_INVERSE_SAMPLES], 0.0)
            };
            let (spawn_rate, spawn_rate_source, spawn_rate_curve) = if emitter.enabled {
                pack_scalar_source(spawn_rate)?
            } else {
                (Vec2::ZERO, 0, GpuCurve::default())
            };
            let (drag, drag_source, drag_curve) = pack_scalar_source(motion.drag)?;
            let (turbulence, turbulence_source, turbulence_curve) =
                pack_scalar_source(motion.turbulence)?;
            let (gravity, gravity_source, gravity_max, gravity_curves) =
                pack_vector_source(motion.gravity)?;
            emitters.push(GpuEmitter {
                slot_offset,
                max_particles: emitter.max_particles,
                burst_count: if emitter.enabled { burst_count } else { 0 },
                shape_kind,
                start_time: emitter.start_time,
                duration: emitter.duration,
                source_offset: emitter.source_offset,
                source_duration: emitter.source_duration,
                spawn_rate,
                spawn_rate_source,
                seed_index: emitter.seed_index,
                spawn_rate_curve,
                shape_radius,
                shape_depth,
                shape_extent_z,
                spread_radians: init.spread_degrees.to_radians(),
                drag,
                drag_source,
                omitted_attributes: 0,
                drag_curve,
                direction: Vec3::from_array(init.direction).normalize_or_zero(),
                _direction_padding: 0.0,
                lifetime: Vec2::new(init.lifetime.min, init.lifetime.max),
                speed: Vec2::new(init.speed.min, init.speed.max),
                angular_velocity: Vec2::new(init.angular_velocity.min, init.angular_velocity.max),
                _range_padding: Vec2::ZERO,
                gravity,
                gravity_source,
                gravity_max,
                _gravity_max_padding: 0.0,
                gravity_curves,
                turbulence,
                turbulence_source,
                _turbulence_padding: emitter
                    .renderers
                    .iter()
                    .filter_map(|r| match r.kind {
                        RendererPlanKind::Ribbon { strand_count, .. } => Some(strand_count.max(1)),
                        RendererPlanKind::Trail { .. } => Some(1),
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0),
                turbulence_curve,
                translation: Vec3::from_array(emitter.transform.translation),
                max_scale: scale.max_element(),
                rotation: Vec4::from_array(rotation.to_array()),
                scale,
                stateful: u32::from(
                    emitter.enabled && emitter.simulation_class != SimulationClass::Analytic,
                ),
                size: pack_curve(appearance.size)?,
                opacity: pack_curve(appearance.opacity)?,
                color: pack_gradient(appearance.color)?,
                spawn_inverse,
                spawn_inverse_total,
                _spawn_inverse_padding: Vec3::ZERO,
                trail_offset,
                trail_points,
                trail_interval,
                trail_lifetime,
                trail_capacity,
                trail_sampling: match trail_sampling {
                    aestra_core::TrailSamplingMode::Time => 0,
                    aestra_core::TrailSamplingMode::Distance => 1,
                    aestra_core::TrailSamplingMode::Adaptive => 2,
                },
                trail_distance,
                trail_tolerance,
            });
            slot_offset = slot_offset.saturating_add(emitter.max_particles);
        }
        // Persistent simulation-state sizing (hybrid M4/M6): the combined slot capacity of the
        // enabled stateful/staged emitters. Every current effect is analytic, so this is empty.
        let stateful_records = instance
            .effect()
            .emitters
            .iter()
            .filter(|emitter| {
                emitter.enabled && emitter.simulation_class != SimulationClass::Analytic
            })
            .fold(0u32, |total, emitter| {
                total.saturating_add(emitter.max_particles)
            });
        let simulation_state = if stateful_records > 0 {
            GpuSimulationState {
                stride: SimulationStateLayout::for_class(SimulationClass::Stateful).stride_floats(),
                records: stateful_records,
            }
        } else {
            GpuSimulationState::default()
        };
        Ok(GpuEffectDynamics {
            mesh_bounds,
            ribbon_bounds,
            emitters,
            renderers,
            total_slots: slot_offset,
            storage_records: history_offset,
            bounds_half_extents,
            simulation_state,
        })
    }
}

pub fn fold_seed(seed: u64) -> u32 {
    seed as u32 ^ (seed >> 32) as u32
}

pub fn indirect_draw_commands(emitters: &[GpuEmitter]) -> Vec<u32> {
    emitters.iter().flat_map(|_| [6, 0, 0, 0]).collect()
}

/// Optional readback trailer: magic, context token, history epoch, simulation-time bits.
/// The original four-word draw commands (and their offsets) stay unchanged.
pub const PARTICLE_STATISTICS_MAGIC: u32 = 0xae57_a001;

pub fn indirect_draw_commands_with_statistics(emitters: &[GpuEmitter]) -> Vec<u32> {
    let mut words = indirect_draw_commands(emitters);
    words.extend([0; 4]);
    words
}

pub const fn indirect_draw_offset(emitter_index: u32) -> u64 {
    emitter_index as u64 * INDIRECT_DRAW_BYTES
}

fn emitter_bounds(
    shape: EmitterShape,
    lifetime: ScalarRange,
    speed: ScalarRange,
    gravity: [f32; 3],
    turbulence: f32,
    size: f32,
) -> Vec3 {
    let shape_extents = match shape {
        EmitterShape::Point => Vec3::ZERO,
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            Vec3::new(radius.abs(), radius.abs(), 0.0)
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            Vec3::splat(radius.abs())
        }
        EmitterShape::Box { half_extents } => Vec3::from_array(half_extents).abs(),
        EmitterShape::Cylinder { radius, depth } => {
            Vec3::new(radius.abs(), depth.abs() * 0.5, radius.abs())
        }
        EmitterShape::Cone { radius, depth } => Vec3::new(radius.abs(), depth.abs(), radius.abs()),
    };
    let lifetime = lifetime.min.abs().max(lifetime.max.abs());
    let speed = speed.min.abs().max(speed.max.abs());
    let travel = speed * lifetime;
    shape_extents
        + Vec3::new(
            travel + turbulence.abs() + gravity[0].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[1].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[2].abs() * lifetime * lifetime * 0.5 + size,
        )
}

fn transformed_emitter_bounds(
    local: Vec3,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
) -> Vec3 {
    let scaled = local * Vec3::from_array(scale).abs();
    let rotation = Quat::from_array(rotation).normalize();
    let rotated = (rotation * Vec3::X * scaled.x).abs()
        + (rotation * Vec3::Y * scaled.y).abs()
        + (rotation * Vec3::Z * scaled.z).abs();
    Vec3::from_array(translation).abs() + rotated
}

fn pack_curve(curve: &CompiledCurve) -> Result<GpuCurve, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = curve.first() {
        points.push(first);
        points.extend(
            curve
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_value)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "curve",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuCurve {
        count: points.len() as u32,
        _padding: Vec3::new(curve.interpolation() as u32 as f32, 0.0, 0.0),
        ..Default::default()
    };
    for (target, (time, value)) in packed.keys.iter_mut().zip(points) {
        *target = Vec2::new(time, value);
    }
    Ok(packed)
}

fn pack_gradient(gradient: &CompiledGradient) -> Result<GpuGradient, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = gradient.first() {
        points.push(first);
        points.extend(
            gradient
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_color)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "gradient",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuGradient {
        count: points.len() as u32,
        ..Default::default()
    };
    for (target, (time, color)) in packed.keys.iter_mut().zip(points) {
        target.time = time;
        target.color = Vec4::from_array(color);
    }
    Ok(packed)
}

#[derive(Clone, Copy)]
enum ResolvedGpuScalarSource<'a> {
    Constant(f32),
    RandomRange(ScalarRange),
    Curve(&'a CompiledCurve, PropertyEvaluationDomain),
}

#[derive(Clone, Copy)]
enum ResolvedGpuVectorSource<'a> {
    Constant([f32; 3]),
    RandomRange(Vec3Range),
    Curve(&'a CompiledVec3Curve, PropertyEvaluationDomain),
}

fn resolve_gpu_vector_source<'a>(
    source: &'a VectorSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuVectorSource<'a> {
    match source {
        VectorSource::Constant(value) => ResolvedGpuVectorSource::Constant(*value.resolve(values)),
        VectorSource::RandomRange(value) => {
            ResolvedGpuVectorSource::RandomRange(*value.resolve(values))
        }
        VectorSource::Curve { value, domain } => {
            ResolvedGpuVectorSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_curve(curve: &CompiledCurve) -> f32 {
    curve
        .first()
        .map_or(0.0, |(_, value)| value.abs())
        .max(curve.last_value().abs())
        .max(
            curve
                .segments()
                .iter()
                .map(|segment| segment.start_value.abs().max(segment.end_value.abs()))
                .fold(0.0, f32::max),
        )
}

fn maximum_absolute_vector_source(source: ResolvedGpuVectorSource<'_>) -> [f32; 3] {
    match source {
        ResolvedGpuVectorSource::Constant(value) => value.map(f32::abs),
        ResolvedGpuVectorSource::RandomRange(range) => {
            std::array::from_fn(|axis| range.min[axis].abs().max(range.max[axis].abs()))
        }
        ResolvedGpuVectorSource::Curve(curve, _) => {
            std::array::from_fn(|axis| maximum_absolute_curve(&curve.curves[axis]))
        }
    }
}

fn pack_vector_source(
    source: ResolvedGpuVectorSource<'_>,
) -> Result<(Vec3, u32, Vec3, [GpuCurve; 3]), GpuArtifactError> {
    match source {
        ResolvedGpuVectorSource::Constant(value) => Ok((
            Vec3::from_array(value),
            0,
            Vec3::from_array(value),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::RandomRange(range) => Ok((
            Vec3::from_array(range.min),
            1,
            Vec3::from_array(range.max),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::Curve(curve, domain) => Ok((
            Vec3::ZERO,
            match domain {
                PropertyEvaluationDomain::EmitterTime => 2,
                PropertyEvaluationDomain::ParticleLife => 3,
            },
            Vec3::ZERO,
            [
                pack_curve(&curve.curves[0])?,
                pack_curve(&curve.curves[1])?,
                pack_curve(&curve.curves[2])?,
            ],
        )),
    }
}

fn resolve_gpu_scalar_source<'a>(
    source: &'a ScalarSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuScalarSource<'a> {
    match source {
        ScalarSource::Constant(value) => ResolvedGpuScalarSource::Constant(*value.resolve(values)),
        ScalarSource::RandomRange(value) => {
            ResolvedGpuScalarSource::RandomRange(*value.resolve(values))
        }
        ScalarSource::Curve { value, domain } => {
            ResolvedGpuScalarSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_scalar_source(source: ResolvedGpuScalarSource<'_>) -> f32 {
    match source {
        ResolvedGpuScalarSource::Constant(value) => value.abs(),
        ResolvedGpuScalarSource::RandomRange(range) => range.min.abs().max(range.max.abs()),
        ResolvedGpuScalarSource::Curve(curve, _) => maximum_absolute_curve(curve),
    }
}

/// Builds the inverse-emission table for a curve-driven (emitter-time) spawn rate,
/// returning the table plus the total emission over `source_duration`. Other sources
/// return zeros and the shader keeps its analytic constant-rate path. Each entry is
/// the spawn time at an evenly spaced emission fraction, found by inverting the exact
/// cumulative-emission function the GPU evaluates — so the shader only needs a few
/// refinement steps to match the previous per-particle binary search.
fn build_spawn_inverse(
    source: ResolvedGpuScalarSource<'_>,
    source_duration: f32,
) -> ([f32; SPAWN_INVERSE_SAMPLES], f32) {
    let ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::EmitterTime) = source
    else {
        return ([0.0; SPAWN_INVERSE_SAMPLES], 0.0);
    };
    let duration = source_duration.max(f32::EPSILON);
    let total = (duration * curve.integral(1.0)).max(0.0);
    let mut table = [0.0; SPAWN_INVERSE_SAMPLES];
    if total <= 0.0 {
        return (table, 0.0);
    }
    let denominator = (SPAWN_INVERSE_SAMPLES - 1) as f32;
    for (index, entry) in table.iter_mut().enumerate() {
        let target = (index as f32 / denominator) * total;
        let mut low = 0.0;
        let mut high = duration;
        for _ in 0..24 {
            let middle = (low + high) * 0.5;
            let emitted = (duration * curve.integral(middle / duration)).max(0.0);
            if emitted < target {
                low = middle;
            } else {
                high = middle;
            }
        }
        *entry = (low + high) * 0.5;
    }
    (table, total)
}

fn pack_scalar_source(
    source: ResolvedGpuScalarSource<'_>,
) -> Result<(Vec2, u32, GpuCurve), GpuArtifactError> {
    match source {
        ResolvedGpuScalarSource::Constant(value) => {
            Ok((Vec2::splat(value), 0, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::RandomRange(range) => {
            Ok((Vec2::new(range.min, range.max), 1, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::EmitterTime) => {
            Ok((Vec2::ZERO, 2, pack_curve(curve)?))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::ParticleLife) => {
            Ok((Vec2::ZERO, 3, pack_curve(curve)?))
        }
    }
}

fn emission<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<(ResolvedGpuScalarSource<'a>, u32)> {
    plan.emitter_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } => {
                let spawn_rate = resolve_gpu_scalar_source(spawn_rate, values);
                Some((spawn_rate, *burst_count.resolve(values)))
            }
            _ => None,
        })
}

fn shape(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<EmitterShape> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::SampleShape { shape, .. } => Some(*shape.resolve(values)),
            _ => None,
        })
}

struct GpuInitialize {
    lifetime: ScalarRange,
    speed: ScalarRange,
    direction: [f32; 3],
    spread_degrees: f32,
    angular_velocity: ScalarRange,
}

fn initialize(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<GpuInitialize> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
                ..
            } => Some(GpuInitialize {
                lifetime: *lifetime.resolve(values),
                speed: *speed.resolve(values),
                direction: *direction.resolve(values),
                spread_degrees: *spread_degrees.resolve(values),
                angular_velocity: *angular_velocity.resolve(values),
            }),
            _ => None,
        })
}

struct GpuMotion<'a> {
    gravity: ResolvedGpuVectorSource<'a>,
    drag: ResolvedGpuScalarSource<'a>,
    turbulence: ResolvedGpuScalarSource<'a>,
}

impl Default for GpuMotion<'_> {
    fn default() -> Self {
        Self {
            gravity: ResolvedGpuVectorSource::Constant([0.0; 3]),
            drag: ResolvedGpuScalarSource::Constant(0.0),
            turbulence: ResolvedGpuScalarSource::Constant(0.0),
        }
    }
}

fn motion<'a>(plan: &'a ExecutionPlan, values: &'a [RuntimeValue]) -> Option<GpuMotion<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Motion {
                gravity,
                drag,
                turbulence,
                ..
            } => Some(GpuMotion {
                gravity: resolve_gpu_vector_source(gravity, values),
                drag: resolve_gpu_scalar_source(drag, values),
                turbulence: resolve_gpu_scalar_source(turbulence, values),
            }),
            _ => None,
        })
}

struct GpuAppearance<'a> {
    size: &'a CompiledCurve,
    opacity: &'a CompiledCurve,
    color: &'a CompiledGradient,
}

fn appearance<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<GpuAppearance<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Appearance {
                size,
                opacity,
                color,
                ..
            } => Some(GpuAppearance {
                size: size.resolve(values),
                opacity: opacity.resolve(values),
                color: color.resolve(values),
            }),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::EffectCompiler;
    use aestra_core::{
        Curve, CurveKey, EffectAsset, Emitter, MODULE_EMISSION, PropertyEvaluationDomain,
        PropertySource, PropertySourceValue, ScalarRange, Value,
    };
    use std::sync::Arc;

    #[test]
    fn curve_interpolation_is_packed_without_changing_the_gpu_layout() {
        for mode in [
            aestra_core::CurveInterpolation::Smooth,
            aestra_core::CurveInterpolation::Linear,
            aestra_core::CurveInterpolation::Step,
        ] {
            let mut source = Curve::new(vec![CurveKey::new(0.0, 2.0), CurveKey::new(1.0, 9.0)]);
            source.interpolation = mode;
            let packed = pack_curve(&CompiledCurve::compile(&source)).unwrap();
            assert_eq!(packed._padding, Vec3::new(mode as u32 as f32, 0.0, 0.0));
            assert_eq!(packed.count, 2);
            assert_eq!(packed.keys[1], Vec2::new(1.0, 9.0));
        }
    }

    #[test]
    fn artifact_capacity_matches_authored_bounds() {
        let mut effect = EffectAsset::new("GPU", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();

        assert_eq!(artifact.total_slots, 40);
        assert_eq!(artifact.emitters[0].slot_offset, 0);
        assert_eq!(artifact.emitters[1].slot_offset, 17);
        assert_eq!(artifact.particles.len(), 40);
    }

    #[test]
    fn mesh_bounds_enclose_evaluated_particles_and_geometry() {
        let mut effect = EffectAsset::new("Mesh bounds", 2.0);
        let mut disabled = Emitter::basic_sprite("Disabled", 2.0);
        disabled.enabled = false;
        let mut emitter = Emitter::basic_sprite("Moving mesh", 2.0);
        emitter.transform.translation = [100.0, 20.0, -30.0];
        emitter.transform.rotation =
            Quat::from_euler(glam::EulerRot::XYZ, 0.5, 0.9, -0.3).to_array();
        emitter.transform.scale = [2.0, 0.3, 4.0];
        effect.emitters.extend([disabled, emitter]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let mut instance = EffectInstance::new(compiled);
        let dynamics = GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();
        assert_eq!(dynamics.mesh_bounds.len(), 2);
        let source = dynamics.mesh_bounds[1];
        let bounds = source
            .half_extents(Vec3::new(-30.0, -4.0, -1.0), Vec3::new(10.0, 20.0, 8.0))
            .unwrap();
        let mut particles = Vec::new();
        let mut sampled = 0;
        for frame in 0..120 {
            instance.seek(frame as f32 / 60.0);
            instance.evaluate(&mut particles);
            for particle in &particles {
                assert_eq!(particle.emitter_index, 1);
                for vertex in [Vec3::new(-30.0, -4.0, -1.0), Vec3::new(10.0, 20.0, 8.0)] {
                    // Same emitter scale compensation as the native mesh vertex shader.
                    let vertex = Vec3::from_array(particle.position)
                        + source.linear_from_local
                            * (Quat::from_rotation_z(particle.rotation) * vertex)
                            * (particle.size / dynamics.emitters[1].max_scale);
                    assert!(
                        vertex.abs().cmple(bounds).all(),
                        "{vertex:?} outside {bounds:?}"
                    );
                    sampled += 1;
                }
            }
        }
        assert!(sampled > 0);
    }

    #[test]
    fn dynamics_match_the_full_artifact_without_allocating_particles() {
        let mut effect = EffectAsset::new("GPU", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let instance = EffectInstance::new(compiled);

        let artifact = GpuEffectArtifact::from_instance(&instance).unwrap();
        let dynamics = GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();

        // The dynamics builder must produce the same emitter/renderer inputs and
        // slot count as the full builder — it only omits the particle scratch, which
        // the full builder sizes from exactly this slot count.
        assert_eq!(dynamics.total_slots, artifact.total_slots);
        assert_eq!(dynamics.bounds_half_extents, artifact.bounds_half_extents);
        assert_eq!(dynamics.emitters.len(), artifact.emitters.len());
        assert_eq!(dynamics.renderers.len(), artifact.renderers.len());
        assert_eq!(artifact.particles.len(), dynamics.total_slots as usize);
        for (dynamic, full) in dynamics.emitters.iter().zip(&artifact.emitters) {
            assert_eq!(dynamic.slot_offset, full.slot_offset);
            assert_eq!(dynamic.max_particles, full.max_particles);
        }
    }

    #[test]
    fn indirect_draw_contract_is_isolated_per_emitter() {
        let emitters = [GpuEmitter::default(), GpuEmitter::default()];
        assert_eq!(
            indirect_draw_commands(&emitters),
            vec![6, 0, 0, 0, 6, 0, 0, 0]
        );
        assert_eq!(indirect_draw_offset(0), 0);
        assert_eq!(indirect_draw_offset(1), INDIRECT_DRAW_BYTES);
        let telemetry = indirect_draw_commands_with_statistics(&emitters);
        assert_eq!(telemetry.len() * size_of::<u32>(), 16 * emitters.len() + 16);
        assert_eq!(&telemetry[..8], indirect_draw_commands(&emitters));
        assert_eq!(&telemetry[8..], &[0; 4]);
    }

    #[test]
    fn seed_fold_matches_the_runtime_contract() {
        assert_eq!(fold_seed(0x1234_5678_9abc_def0), 0x8888_8888);
    }

    #[test]
    fn artifact_lowering_preserves_emitter_time_curves_without_bevy() {
        let mut effect = EffectAsset::new("GPU curve rate", 2.0);
        effect
            .emitters
            .push(Emitter::basic_sprite("Emitter", effect.duration));
        let emission = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_EMISSION)
            .unwrap();
        let source = PropertySource::Curve(PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![PropertySourceValue::new(
                source,
                Value::Curve(Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                    ScalarRange::new(2.0, 20.0),
                )),
            )],
        );

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let emitter = artifact.emitters[0];

        assert_eq!(emitter.spawn_rate_source, 2);
        assert_eq!(emitter.spawn_rate_curve.count, 2);
        assert_eq!(emitter.spawn_rate_curve.keys[0], Vec2::new(0.0, 2.0));
        assert_eq!(emitter.spawn_rate_curve.keys[1], Vec2::new(1.0, 20.0));
    }
}
