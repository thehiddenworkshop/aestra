// Aestra's authored GPU source is WESL. Engine adapters consume the validated
// WGSL contract or register this portable source with their shader pipeline.

const MAX_CURVE_KEYS: u32 = 8u;
const SPAWN_INVERSE_SAMPLES: u32 = 32u;
const TAU: f32 = 6.283185307179586;

struct Curve {
    keys: array<vec2<f32>, 8>,
    count: u32,
    _padding: vec3<f32>,
}

struct GradientKey {
    color: vec4<f32>,
    time: f32,
    _padding: vec3<f32>,
}

struct Gradient {
    keys: array<GradientKey, 8>,
    count: u32,
    _padding: vec3<f32>,
}

struct Emitter {
    slot_offset: u32,
    max_particles: u32,
    burst_count: u32,
    shape_kind: u32,
    start_time: f32,
    duration: f32,
    source_offset: f32,
    source_duration: f32,
    spawn_rate: vec2<f32>,
    spawn_rate_source: u32,
    seed_index: u32,
    spawn_rate_curve: Curve,
    shape_radius: f32,
    shape_depth: f32,
    shape_extent_z: f32,
    spread_radians: f32,
    drag: vec2<f32>,
    drag_source: u32,
    omitted_attributes: u32,
    drag_curve: Curve,
    direction: vec3<f32>,
    _direction_padding: f32,
    lifetime: vec2<f32>,
    speed: vec2<f32>,
    angular_velocity: vec2<f32>,
    _range_padding: vec2<f32>,
    gravity: vec3<f32>,
    gravity_source: u32,
    gravity_max: vec3<f32>,
    _gravity_max_padding: f32,
    gravity_curves: array<Curve, 3>,
    turbulence: vec2<f32>,
    turbulence_source: u32,
    _turbulence_padding: u32,
    turbulence_curve: Curve,
    translation: vec3<f32>,
    max_scale: f32,
    rotation: vec4<f32>,
    scale: vec3<f32>,
    _transform_padding: f32,
    size: Curve,
    opacity: Curve,
    color: Gradient,
    spawn_inverse: array<f32, 32>,
    spawn_inverse_total: f32,
    _spawn_inverse_padding: vec3<f32>,
    trail_offset: u32,
    trail_points: u32,
    trail_interval: f32,
    trail_lifetime: f32,
}

struct Particle {
    color: vec4<f32>,
    position: vec3<f32>,
    size: f32,
    rotation: f32,
    normalized_age: f32,
    emitter_index: u32,
    alive: u32,
    particle_index: u32,
    _padding_0: u32,
    _padding_1: u32,
    _padding_2: u32,
}

struct Globals {
    time: f32,
    total_slots: u32,
    seed: u32,
    emitter_count: u32,
    duration: f32,
    continuous: u32,
    _padding: vec2<u32>,
    world_from_effect: mat4x4<f32>,
}

@group(0) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> alive_indices: array<u32>;
@group(0) @binding(3) var<storage, read_write> dead_indices: array<u32>;
@group(0) @binding(4) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> indirect: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read> globals: Globals;

fn hash01_seeded(index: u32, channel: u32, seed: u32) -> f32 {
    var value = (index * 0x9e3779b9u) ^ (channel * 0x85ebca6bu) ^ seed;
    value = value ^ (value >> 16u);
    value = value * 0x7feb352du;
    value = value ^ (value >> 15u);
    value = value * 0x846ca68bu;
    value = value ^ (value >> 16u);
    return f32(value) / 4294967295.0;
}

fn multiply_high(left: u32, right: u32) -> u32 {
    let left_low = left & 0xffffu;
    let left_high = left >> 16u;
    let right_low = right & 0xffffu;
    let right_high = right >> 16u;
    let low = left_low * right_low;
    let middle = left_high * right_low + (low >> 16u);
    let carry = (middle & 0xffffu) + left_low * right_high;
    return left_high * right_high + (middle >> 16u) + (carry >> 16u);
}

fn seed_for_cycle(cycle: u32) -> u32 {
    let multiplier_low = 0x7f4a7c15u;
    let multiplier_high = 0x9e3779b9u;
    let product_low = cycle * multiplier_low;
    let product_high = cycle * multiplier_high + multiply_high(cycle, multiplier_low);
    return globals.seed ^ product_low ^ product_high;
}

fn sample_range(range: vec2<f32>, random: f32) -> f32 {
    return mix(range.x, range.y, clamp(random, 0.0, 1.0));
}

fn rotate_by_quaternion(value: vec3<f32>, rotation: vec4<f32>) -> vec3<f32> {
    let q = normalize(rotation);
    let t = 2.0 * cross(q.xyz, value);
    return value + q.w * t + cross(q.xyz, t);
}

fn sample_curve(curve: Curve, time: f32) -> f32 {
    if curve.count == 0u {
        return 0.0;
    }
    let t = clamp(time, 0.0, 1.0);
    if t <= curve.keys[0].x {
        return curve.keys[0].y;
    }
    var index = 1u;
    loop {
        if index >= curve.count || index >= MAX_CURVE_KEYS {
            break;
        }
        let end = curve.keys[index];
        if t <= end.x {
            let start = curve.keys[index - 1u];
            let x = clamp((t - start.x) / max(end.x - start.x, 0.000000119), 0.0, 1.0);
            let smoothed = x * x * (3.0 - 2.0 * x);
            return mix(start.y, end.y, smoothed);
        }
        index += 1u;
    }
    return curve.keys[curve.count - 1u].y;
}

fn curve_integral(curve: Curve, time: f32) -> f32 {
    if curve.count == 0u {
        return 0.0;
    }
    let t = clamp(time, 0.0, 1.0);
    let first = curve.keys[0];
    var area = first.y * min(t, first.x);
    if t <= first.x {
        return area;
    }
    var index = 1u;
    loop {
        if index >= curve.count || index >= MAX_CURVE_KEYS {
            break;
        }
        let start = curve.keys[index - 1u];
        let end = curve.keys[index];
        if t > start.x {
            let span = max(end.x - start.x, 0.000000119);
            let x = clamp((min(t, end.x) - start.x) / span, 0.0, 1.0);
            let x3 = x * x * x;
            let x4 = x3 * x;
            area += span * (start.y * x + (end.y - start.y) * (x3 - 0.5 * x4));
        }
        if t <= end.x {
            return area;
        }
        index += 1u;
    }
    let last = curve.keys[curve.count - 1u];
    return area + last.y * max(t - last.x, 0.0);
}

fn resolved_spawn_rate(emitter: Emitter, seed: u32) -> f32 {
    if emitter.spawn_rate_source == 1u {
        return max(
            sample_range(
                emitter.spawn_rate,
                hash01_seeded(emitter.seed_index, 0x53504157u, seed),
            ),
            0.0,
        );
    }
    return max(emitter.spawn_rate.x, 0.0);
}

fn resolved_particle_scalar(
    value: vec2<f32>,
    source: u32,
    curve: Curve,
    particle_index: u32,
    normalized_particle_life: f32,
    normalized_emitter_time: f32,
    random_channel: u32,
    seed: u32,
) -> f32 {
    if source == 1u {
        return sample_range(value, hash01_seeded(particle_index, random_channel, seed));
    }
    if source == 2u {
        return sample_curve(curve, normalized_emitter_time);
    }
    if source == 3u {
        return sample_curve(curve, normalized_particle_life);
    }
    return value.x;
}

fn resolved_particle_vector(
    value: vec3<f32>,
    source: u32,
    maximum: vec3<f32>,
    curves: array<Curve, 3>,
    particle_index: u32,
    normalized_particle_life: f32,
    normalized_emitter_time: f32,
    random_channel: u32,
    seed: u32,
) -> vec3<f32> {
    if source == 1u {
        return vec3<f32>(
            mix(value.x, maximum.x, hash01_seeded(particle_index, random_channel, seed)),
            mix(value.y, maximum.y, hash01_seeded(particle_index, random_channel + 1u, seed)),
            mix(value.z, maximum.z, hash01_seeded(particle_index, random_channel + 2u, seed)),
        );
    }
    if source == 2u {
        return vec3<f32>(
            sample_curve(curves[0], normalized_emitter_time),
            sample_curve(curves[1], normalized_emitter_time),
            sample_curve(curves[2], normalized_emitter_time),
        );
    }
    if source == 3u {
        return vec3<f32>(
            sample_curve(curves[0], normalized_particle_life),
            sample_curve(curves[1], normalized_particle_life),
            sample_curve(curves[2], normalized_particle_life),
        );
    }
    return value;
}

fn emitted_until(emitter: Emitter, time: f32, seed: u32) -> f32 {
    if emitter.spawn_rate_source == 2u {
        return emitter.source_duration
            * max(curve_integral(emitter.spawn_rate_curve, time / max(emitter.source_duration, 0.000000119)), 0.0);
    }
    return time * resolved_spawn_rate(emitter, seed);
}

fn curve_spawn_time(emitter: Emitter, target_emission: f32, seed: u32) -> f32 {
    if target_emission <= 0.0 {
        return 0.0;
    }
    // Seed a tight bracket from the precomputed inverse-emission table (built on the
    // CPU by inverting the same cumulative-emission function), then refine. This
    // replaces the old full-range 12-iteration binary search with a short refinement
    // inside a narrow bracket, cutting the per-particle `emitted_until` evaluations.
    var low = 0.0;
    var high = emitter.source_duration;
    if emitter.spawn_inverse_total > 0.0 {
        let last = f32(SPAWN_INVERSE_SAMPLES - 1u);
        let position = clamp(target_emission / emitter.spawn_inverse_total, 0.0, 1.0) * last;
        let lower = u32(floor(position));
        let upper = min(lower + 1u, SPAWN_INVERSE_SAMPLES - 1u);
        low = emitter.spawn_inverse[lower];
        high = emitter.spawn_inverse[upper];
    }
    // Eight refinements inside the table bracket exceed the old full-range 12-iteration
    // accuracy (bracket width ~= source_duration / (SPAWN_INVERSE_SAMPLES - 1)), while
    // evaluating `emitted_until` a third fewer times.
    var iteration = 0u;
    loop {
        if iteration >= 8u {
            break;
        }
        let middle = (low + high) * 0.5;
        if emitted_until(emitter, middle, seed) < target_emission {
            low = middle;
        } else {
            high = middle;
        }
        iteration += 1u;
    }
    return (low + high) * 0.5;
}

fn sample_gradient(gradient: Gradient, time: f32) -> vec4<f32> {
    if gradient.count == 0u {
        return vec4<f32>(1.0);
    }
    let t = clamp(time, 0.0, 1.0);
    if t <= gradient.keys[0].time {
        return gradient.keys[0].color;
    }
    var index = 1u;
    loop {
        if index >= gradient.count || index >= MAX_CURVE_KEYS {
            break;
        }
        let end = gradient.keys[index];
        if t <= end.time {
            let start = gradient.keys[index - 1u];
            let x = clamp((t - start.time) / max(end.time - start.time, 0.000000119), 0.0, 1.0);
            return mix(start.color, end.color, x);
        }
        index += 1u;
    }
    return gradient.keys[gradient.count - 1u].color;
}

fn dead_particle(emitter_index: u32) -> Particle {
    return Particle(
        vec4<f32>(0.0),
        vec3<f32>(0.0),
        0.0,
        0.0,
        0.0,
        emitter_index,
        0u,
        0u,
        0u,
        0u,
        0u,
    );
}

fn append_dead(slot: u32) {
    let compact_index = atomicAdd(&counters[1], 1u);
    if compact_index < globals.total_slots {
        dead_indices[compact_index] = slot;
    }
}

@compute @workgroup_size(1)
fn reset() {
    atomicStore(&counters[0], 0u);
    atomicStore(&counters[1], 0u);
    var emitter_index = 0u;
    loop {
        if emitter_index >= globals.emitter_count {
            break;
        }
        let command = emitter_index * 4u;
        atomicStore(&indirect[command], 6u);
        atomicStore(&indirect[command + 1u], 0u);
        atomicStore(&indirect[command + 2u], 0u);
        atomicStore(&indirect[command + 3u], 0u);
        emitter_index += 1u;
    }
}

@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let slot = global_id.x;
    if slot >= globals.total_slots {
        return;
    }

    var emitter_index = 0u;
    loop {
        if emitter_index >= globals.emitter_count {
            particles[slot] = dead_particle(0u);
            append_dead(slot);
            return;
        }
        let candidate = emitters[emitter_index];
        if slot >= candidate.slot_offset && slot < candidate.slot_offset + candidate.max_particles {
            break;
        }
        emitter_index += 1u;
    }

    let emitter = emitters[emitter_index];
    var particle_index = slot - emitter.slot_offset;
    var particle_cycle = 0u;
    var particle_seed = globals.seed;
    var cycle_start = 0.0;
    if globals.continuous != 0u && globals.duration > 0.0 {
        let current_cycle = u32(floor(globals.time / globals.duration));
        let phase = globals.time - f32(current_cycle) * globals.duration;
        let cycle_source_end = emitter.source_offset + emitter.duration;
        let oldest_time = max(
            globals.time - globals.duration - max(emitter.lifetime.x, emitter.lifetime.y),
            0.0,
        );
        let oldest_cycle = u32(floor(oldest_time / globals.duration));

        // Emission in the current, possibly partial, cycle.
        let current_seed = seed_for_cycle(current_cycle);
        var current_emit = 0u;
        let phase_region_time = phase - emitter.start_time;
        if phase_region_time >= 0.0 {
            let phase_local_time = min(emitter.source_offset + phase_region_time, cycle_source_end);
            current_emit = emitter.burst_count
                + u32(max(floor(emitted_until(emitter, phase_local_time, current_seed)), 0.0));
        }

        var found_cycle = false;
        if particle_index < current_emit {
            // The slot's particle belongs to the current cycle.
            particle_cycle = current_cycle;
            particle_seed = current_seed;
            found_cycle = true;
        } else {
            let remaining = particle_index - current_emit;
            if emitter.spawn_rate_source == 1u {
                // Random per-cycle spawn rate: each cycle emits a different count,
                // so the owning cycle must be found by walking backward.
                var remaining_index = remaining;
                var candidate_cycle = current_cycle;
                loop {
                    if candidate_cycle == oldest_cycle {
                        break;
                    }
                    candidate_cycle -= 1u;
                    let candidate_seed = seed_for_cycle(candidate_cycle);
                    let emitted_in_cycle = emitter.burst_count
                        + u32(max(
                            floor(emitted_until(emitter, cycle_source_end, candidate_seed)),
                            0.0,
                        ));
                    if remaining_index < emitted_in_cycle {
                        particle_cycle = candidate_cycle;
                        particle_index = remaining_index;
                        particle_seed = candidate_seed;
                        found_cycle = true;
                        break;
                    }
                    remaining_index -= emitted_in_cycle;
                }
            } else {
                // Constant per-cycle emission (constant rate or emitter-time curve):
                // every complete cycle emits the same integer count, so the owning
                // cycle is found in O(1) instead of walking lifetime/duration cycles.
                // Integer division reproduces the iterative subtraction exactly.
                let full_emit = emitter.burst_count
                    + u32(max(
                        floor(emitted_until(emitter, cycle_source_end, current_seed)),
                        0.0,
                    ));
                let complete_cycles = current_cycle - oldest_cycle;
                if full_emit > 0u {
                    let skip = remaining / full_emit;
                    if skip < complete_cycles {
                        particle_cycle = current_cycle - 1u - skip;
                        particle_index = remaining % full_emit;
                        particle_seed = seed_for_cycle(particle_cycle);
                        found_cycle = true;
                    }
                }
            }
        }
        if !found_cycle {
            particles[slot] = dead_particle(emitter_index);
            append_dead(slot);
            return;
        }
        cycle_start = f32(particle_cycle) * globals.duration;
    }
    let region_time = globals.time - cycle_start - emitter.start_time;
    if region_time < 0.0 {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }
    let local_time = emitter.source_offset + region_time;
    let source_end = emitter.source_offset + emitter.duration;
    let emission_time = min(local_time, source_end);

    let emitted = emitter.burst_count
        + u32(max(
            floor(emitted_until(emitter, emission_time, particle_seed)),
            0.0,
        ));
    if particle_index >= min(emitted, emitter.max_particles) {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }

    var spawn_time = 0.0;
    if particle_index >= emitter.burst_count {
        let particle_target = f32(particle_index - emitter.burst_count);
        if emitted_until(emitter, emitter.source_duration, particle_seed) < particle_target {
            particles[slot] = dead_particle(emitter_index);
            append_dead(slot);
            return;
        }
        if emitter.spawn_rate_source == 2u {
            spawn_time = curve_spawn_time(emitter, particle_target, particle_seed);
        } else {
            let rate = resolved_spawn_rate(emitter, particle_seed);
            if rate <= 0.0 {
                particles[slot] = dead_particle(emitter_index);
                append_dead(slot);
                return;
            }
            spawn_time = particle_target / rate;
        }
    }
    if spawn_time < emitter.source_offset || spawn_time >= source_end {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }
    let age = local_time - spawn_time;
    let lifetime = sample_range(emitter.lifetime, hash01_seeded(particle_index, 0u, particle_seed));
    if age < 0.0 || age >= lifetime || lifetime <= 0.0 {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }

    let normalized_age = age / lifetime;
    var position = vec3<f32>(0.0);
    if (emitter.omitted_attributes & 1u) == 0u {
        let forward = normalize(emitter.direction);
        let half_angle = min(abs(emitter.spread_radians) * 0.5, 3.141592653589793);
        let cos_theta = 1.0 - hash01_seeded(particle_index, 1u, particle_seed) * (1.0 - cos(half_angle));
        let sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
        let direction_angle = hash01_seeded(particle_index, 11u, particle_seed) * TAU;
        var helper = vec3<f32>(0.0, 1.0, 0.0);
        if abs(forward.y) >= 0.999 {
            helper = vec3<f32>(1.0, 0.0, 0.0);
        }
        let tangent = normalize(cross(helper, forward));
        let bitangent = cross(forward, tangent);
        let direction = forward * cos_theta
            + tangent * (sin_theta * cos(direction_angle))
            + bitangent * (sin_theta * sin(direction_angle));
        let speed = sample_range(emitter.speed, hash01_seeded(particle_index, 2u, particle_seed));
        let shape_angle = hash01_seeded(particle_index, 5u, particle_seed) * TAU;
        var origin = vec3<f32>(0.0);
        if emitter.shape_kind == 1u {
            let radius = emitter.shape_radius * sqrt(hash01_seeded(particle_index, 6u, particle_seed));
            origin = vec3<f32>(cos(shape_angle) * radius, sin(shape_angle) * radius, 0.0);
        } else if emitter.shape_kind == 2u {
            origin = vec3<f32>(
                cos(shape_angle) * emitter.shape_radius,
                sin(shape_angle) * emitter.shape_radius,
                0.0,
            );
        } else if emitter.shape_kind == 3u {
            let y = hash01_seeded(particle_index, 6u, particle_seed) * 2.0 - 1.0;
            let radial = sqrt(max(1.0 - y * y, 0.0));
            let radius = emitter.shape_radius * pow(hash01_seeded(particle_index, 8u, particle_seed), 0.3333333333333333);
            origin = vec3<f32>(cos(shape_angle) * radial, y, sin(shape_angle) * radial) * radius;
        } else if emitter.shape_kind == 4u {
            let y = hash01_seeded(particle_index, 6u, particle_seed);
            let radial = sqrt(max(1.0 - y * y, 0.0));
            let radius = emitter.shape_radius * pow(hash01_seeded(particle_index, 8u, particle_seed), 0.3333333333333333);
            origin = vec3<f32>(cos(shape_angle) * radial, y, sin(shape_angle) * radial) * radius;
        } else if emitter.shape_kind == 5u {
            origin = vec3<f32>(
                (hash01_seeded(particle_index, 5u, particle_seed) * 2.0 - 1.0) * emitter.shape_radius,
                (hash01_seeded(particle_index, 6u, particle_seed) * 2.0 - 1.0) * emitter.shape_depth,
                (hash01_seeded(particle_index, 7u, particle_seed) * 2.0 - 1.0) * emitter.shape_extent_z,
            );
        } else if emitter.shape_kind == 6u {
            let radius = emitter.shape_radius * sqrt(hash01_seeded(particle_index, 6u, particle_seed));
            origin = vec3<f32>(
                cos(shape_angle) * radius,
                (hash01_seeded(particle_index, 7u, particle_seed) - 0.5) * emitter.shape_depth,
                sin(shape_angle) * radius,
            );
        } else if emitter.shape_kind == 7u {
            let y = hash01_seeded(particle_index, 6u, particle_seed) * emitter.shape_depth;
            let radius = emitter.shape_radius * (y / max(emitter.shape_depth, 0.001))
                * sqrt(hash01_seeded(particle_index, 7u, particle_seed));
            origin = vec3<f32>(cos(shape_angle) * radius, y, sin(shape_angle) * radius);
        }

        let normalized_emitter_time = local_time / max(emitter.source_duration, 0.000000119);
        let drag = resolved_particle_scalar(
            emitter.drag,
            emitter.drag_source,
            emitter.drag_curve,
            particle_index,
            normalized_age,
            normalized_emitter_time,
            12u,
            particle_seed,
        );
        let damping = exp(-max(drag, 0.0) * age);
        var travel = speed * age;
        if abs(drag) >= 0.0001 {
            travel = speed * (1.0 - damping) / max(drag, 0.0001);
        }
        let turbulence_strength = resolved_particle_scalar(
            emitter.turbulence,
            emitter.turbulence_source,
            emitter.turbulence_curve,
            particle_index,
            normalized_age,
            normalized_emitter_time,
            13u,
            particle_seed,
        );
        let gravity = resolved_particle_vector(
            emitter.gravity,
            emitter.gravity_source,
            emitter.gravity_max,
            emitter.gravity_curves,
            particle_index,
            normalized_age,
            normalized_emitter_time,
            14u,
            particle_seed,
        );
        let turbulence = turbulence_strength * vec3<f32>(
            sin(age * 7.0 + hash01_seeded(particle_index, 3u, particle_seed) * TAU),
            sin(age * 6.3 + hash01_seeded(particle_index, 8u, particle_seed) * TAU),
            sin(age * 7.7 + hash01_seeded(particle_index, 10u, particle_seed) * TAU),
        );
        let local_position = origin + direction * travel + gravity * age * age * 0.5 + turbulence;
        position = emitter.translation
            + rotate_by_quaternion(local_position * emitter.scale, emitter.rotation);
    }
    // Missing presentation fields receive deterministic defaults, never stale data.
    var color = vec4<f32>(1.0);
    if (emitter.omitted_attributes & 24u) != 24u {
        let gradient = sample_gradient(emitter.color, normalized_age);
        if (emitter.omitted_attributes & 8u) == 0u {
            color = vec4<f32>(gradient.rgb, color.a);
        }
        if (emitter.omitted_attributes & 16u) == 0u {
            color.a = gradient.a * sample_curve(emitter.opacity, normalized_age);
        }
    }
    var rotation = 0.0;
    if (emitter.omitted_attributes & 4u) == 0u {
        rotation = sample_range(
            emitter.angular_velocity,
            hash01_seeded(particle_index, 4u, particle_seed),
        ) * age;
    }
    var size = 0.0;
    if (emitter.omitted_attributes & 2u) == 0u {
        size = sample_curve(emitter.size, normalized_age) * emitter.max_scale;
    }

    particles[slot] = Particle(
        color,
        position,
        size,
        rotation,
        select(normalized_age, 0.0, (emitter.omitted_attributes & 32u) != 0u),
        emitter_index,
        1u,
        particle_index + particle_cycle * max(emitter.max_particles, 1u),
        0u,
        0u,
        0u,
    );
    atomicAdd(&counters[0], 1u);
    let command = emitter_index * 4u;
    let compact_index = atomicAdd(&indirect[command + 1u], 1u);
    alive_indices[emitter.slot_offset + compact_index] = slot;
}
