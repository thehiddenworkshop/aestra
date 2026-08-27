// Aestra's authored GPU source is WESL. Bevy resolves it to the shader module
// consumed by wgpu; no generated WGSL source is stored by the project.

const MAX_CURVE_KEYS: u32 = 8u;
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
    spawn_rate: f32,
    shape_radius: f32,
    shape_depth: f32,
    shape_extent_z: f32,
    spread_radians: f32,
    drag: f32,
    direction: vec3<f32>,
    _direction_padding: f32,
    lifetime: vec2<f32>,
    speed: vec2<f32>,
    angular_velocity: vec2<f32>,
    _range_padding: vec2<f32>,
    gravity: vec3<f32>,
    turbulence: f32,
    translation: vec3<f32>,
    max_scale: f32,
    rotation: vec4<f32>,
    scale: vec3<f32>,
    _transform_padding: f32,
    size: Curve,
    opacity: Curve,
    color: Gradient,
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
}

@group(0) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> alive_indices: array<u32>;
@group(0) @binding(3) var<storage, read_write> dead_indices: array<u32>;
@group(0) @binding(4) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> indirect: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read> globals: Globals;

fn hash01(index: u32, channel: u32) -> f32 {
    var value = (index * 0x9e3779b9u) ^ (channel * 0x85ebca6bu) ^ globals.seed;
    value = value ^ (value >> 16u);
    value = value * 0x7feb352du;
    value = value ^ (value >> 15u);
    value = value * 0x846ca68bu;
    value = value ^ (value >> 16u);
    return f32(value) / 4294967295.0;
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
    atomicStore(&indirect[0], 6u);
    atomicStore(&indirect[1], 0u);
    atomicStore(&indirect[2], 0u);
    atomicStore(&indirect[3], 0u);
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
    let particle_index = slot - emitter.slot_offset;
    let local_time = globals.time - emitter.start_time;
    if local_time < 0.0 || local_time > emitter.duration {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }

    let emitted = emitter.burst_count + u32(max(floor(local_time * emitter.spawn_rate), 0.0));
    if particle_index >= min(emitted, emitter.max_particles) {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }

    var spawn_time = 0.0;
    if particle_index >= emitter.burst_count {
        if emitter.spawn_rate <= 0.0 {
            particles[slot] = dead_particle(emitter_index);
            append_dead(slot);
            return;
        }
        spawn_time = f32(particle_index - emitter.burst_count) / emitter.spawn_rate;
    }
    let age = local_time - spawn_time;
    let lifetime = sample_range(emitter.lifetime, hash01(particle_index, 0u));
    if age < 0.0 || age >= lifetime || lifetime <= 0.0 {
        particles[slot] = dead_particle(emitter_index);
        append_dead(slot);
        return;
    }

    let normalized_age = age / lifetime;
    let forward = normalize(emitter.direction);
    let half_angle = min(abs(emitter.spread_radians) * 0.5, 3.141592653589793);
    let cos_theta = 1.0 - hash01(particle_index, 1u) * (1.0 - cos(half_angle));
    let sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    let direction_angle = hash01(particle_index, 11u) * TAU;
    var helper = vec3<f32>(0.0, 1.0, 0.0);
    if abs(forward.y) >= 0.999 {
        helper = vec3<f32>(1.0, 0.0, 0.0);
    }
    let tangent = normalize(cross(helper, forward));
    let bitangent = cross(forward, tangent);
    let direction = forward * cos_theta
        + tangent * (sin_theta * cos(direction_angle))
        + bitangent * (sin_theta * sin(direction_angle));
    let speed = sample_range(emitter.speed, hash01(particle_index, 2u));
    let shape_angle = hash01(particle_index, 5u) * TAU;
    var origin = vec3<f32>(0.0);
    if emitter.shape_kind == 1u {
        let radius = emitter.shape_radius * sqrt(hash01(particle_index, 6u));
        origin = vec3<f32>(cos(shape_angle) * radius, sin(shape_angle) * radius, 0.0);
    } else if emitter.shape_kind == 2u {
        origin = vec3<f32>(
            cos(shape_angle) * emitter.shape_radius,
            sin(shape_angle) * emitter.shape_radius,
            0.0,
        );
    } else if emitter.shape_kind == 3u {
        let y = hash01(particle_index, 6u) * 2.0 - 1.0;
        let radial = sqrt(max(1.0 - y * y, 0.0));
        let radius = emitter.shape_radius * pow(hash01(particle_index, 8u), 0.3333333333333333);
        origin = vec3<f32>(cos(shape_angle) * radial, y, sin(shape_angle) * radial) * radius;
    } else if emitter.shape_kind == 4u {
        let y = hash01(particle_index, 6u);
        let radial = sqrt(max(1.0 - y * y, 0.0));
        let radius = emitter.shape_radius * pow(hash01(particle_index, 8u), 0.3333333333333333);
        origin = vec3<f32>(cos(shape_angle) * radial, y, sin(shape_angle) * radial) * radius;
    } else if emitter.shape_kind == 5u {
        origin = vec3<f32>(
            (hash01(particle_index, 5u) * 2.0 - 1.0) * emitter.shape_radius,
            (hash01(particle_index, 6u) * 2.0 - 1.0) * emitter.shape_depth,
            (hash01(particle_index, 7u) * 2.0 - 1.0) * emitter.shape_extent_z,
        );
    } else if emitter.shape_kind == 6u {
        let radius = emitter.shape_radius * sqrt(hash01(particle_index, 6u));
        origin = vec3<f32>(
            cos(shape_angle) * radius,
            (hash01(particle_index, 7u) - 0.5) * emitter.shape_depth,
            sin(shape_angle) * radius,
        );
    } else if emitter.shape_kind == 7u {
        let y = hash01(particle_index, 6u) * emitter.shape_depth;
        let radius = emitter.shape_radius * (y / max(emitter.shape_depth, 0.001))
            * sqrt(hash01(particle_index, 7u));
        origin = vec3<f32>(cos(shape_angle) * radius, y, sin(shape_angle) * radius);
    }

    let damping = exp(-max(emitter.drag, 0.0) * age);
    var travel = speed * age;
    if abs(emitter.drag) >= 0.0001 {
        travel = speed * (1.0 - damping) / max(emitter.drag, 0.0001);
    }
    let turbulence = emitter.turbulence * vec3<f32>(
        sin(age * 7.0 + hash01(particle_index, 3u) * TAU),
        sin(age * 6.3 + hash01(particle_index, 8u) * TAU),
        sin(age * 7.7 + hash01(particle_index, 10u) * TAU),
    );
    let local_position = origin + direction * travel + emitter.gravity * age * age * 0.5 + turbulence;
    let position = emitter.translation
        + rotate_by_quaternion(local_position * emitter.scale, emitter.rotation);
    var color = sample_gradient(emitter.color, normalized_age);
    color.a *= sample_curve(emitter.opacity, normalized_age);
    let rotation = sample_range(emitter.angular_velocity, hash01(particle_index, 4u)) * age;

    particles[slot] = Particle(
        color,
        position,
        sample_curve(emitter.size, normalized_age) * emitter.max_scale,
        rotation,
        normalized_age,
        emitter_index,
        1u,
        particle_index,
        0u,
        0u,
        0u,
    );
    let compact_index = atomicAdd(&counters[0], 1u);
    alive_indices[compact_index] = slot;
    atomicAdd(&indirect[1], 1u);
}
