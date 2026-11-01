fn billow(uv: vec2<f32>, seed: f32, age: f32, world: vec3<f32>, core: f32, time: f32, flow: vec3<f32>, feather: f32, breakup: f32, depth: bool) -> f32 {
    let softness = clamp(feather, 0.0, 1.0);
    let structure = clamp(breakup, 0.0, 1.0);
    let spread = smoothstep(0.10, 0.75, age);
    let angle = (seed - 0.5) * mix(6.2831853, 1.2, softness) * spread;
    let centered = uv - vec2<f32>(0.5);
    let rotated = vec2<f32>(cos(angle) * centered.x - sin(angle) * centered.y,
        sin(angle) * centered.x + cos(angle) * centered.y);
    // Curve the strand's spine; avoid a stack of identically round puffs.
    let curled = rotated + vec2<f32>(0.09 * sin(rotated.y * 10.0 + seed * 13.0 + age * 0.8), 0.0);
    // Every sample in this effect shares one advected field. Particle seed/age
    // cannot shift its phase independently. Effect time freezes with playback;
    // zero flow recovers the previous world-anchored pattern.
    let advected = world - flow * time;
    var p = advected.xy * vec2<f32>(0.28, 0.38) + advected.z * vec2<f32>(0.17, 0.23);
    var spatial = advected * vec3<f32>(0.28, 0.38, 0.28);
    let weights = array<f32, 3>(0.80, 0.16, 0.04);
    var n = 0.0;
    var broad = 0.0;
    for (var octave = 0u; octave < 3u; octave += 1u) {
        var sample = 0.0;
        if depth {
            // All three world axes affect density independently. The projected
            // control repeats along one oblique axis; it remains available below.
            let i = floor(spatial);
            let f = fract(spatial);
            let u = f * f * (vec3<f32>(3.0) - 2.0 * f);
            var corners: array<f32, 8>;
            for (var corner = 0u; corner < 8u; corner += 1u) {
                let point = i + vec3<f32>(f32(corner % 2u), f32((corner / 2u) % 2u), f32(corner / 4u));
                var q = fract(point * 0.1031);
                q += vec3<f32>(dot(q, q.yzx + vec3<f32>(33.33)));
                corners[corner] = fract((q.x + q.y) * q.z);
            }
            sample = mix(
                mix(mix(corners[0], corners[1], u.x), mix(corners[2], corners[3], u.x), u.y),
                mix(mix(corners[4], corners[5], u.x), mix(corners[6], corners[7], u.x), u.y), u.z);
        } else {
            let i = floor(p);
            let f = fract(p);
            let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
            var corners: array<f32, 4>;
            for (var corner = 0u; corner < 4u; corner += 1u) {
                let point = i + vec2<f32>(f32(corner % 2u), f32(corner / 2u));
                var q = fract(vec3<f32>(point.x, point.y, point.x) * 0.1031);
                q += vec3<f32>(dot(q, q.yzx + vec3<f32>(33.33)));
                corners[corner] = fract((q.x + q.y) * q.z);
            }
            sample = mix(mix(corners[0], corners[1], u.x), mix(corners[2], corners[3], u.x), u.y);
        }
        if octave == 0u { broad = sample; }
        n += sample * weights[octave];
        // Domain distortion uses the existing octaves, rather than another noise stack.
        p = p * 2.03 + vec2<f32>(7.1 + n * 0.35, 7.1 - n * 0.3);
        spatial = spatial * 2.03 + vec3<f32>(7.1 + n * 0.35, 7.1 - n * 0.3, 7.1 + n * 0.2);
    }
    let anisotropy = mix(vec2<f32>(0.95, 0.60), vec2<f32>(0.75, 1.15), spread);
    let warped = curled * anisotropy + vec2<f32>(n - 0.5, 0.5 - n) * 0.16;
    // Expansion comes from the authored particle size, not a second widening
    // envelope. Most old samples dilute into thin fragments rather than fog.
    let cloud = smoothstep(0.60, 0.85, seed);
    let radius = length(warped) / mix(0.42, mix(0.28, 0.40, cloud), spread);
    // Contract the envelope unevenly using the shared coarse field. Never widen
    // the old footprint: thin paths must not become overlapping circular sheets.
    let normalized = radius * (1.0 + structure * (0.15 + 0.75 * (1.0 - broad)) * spread);
    // Feather the whole outer envelope, not just its last narrow rim. A modest
    // core compensation retains body while suppressing independent puff edges.
    // Zero softness retains the original envelope and rotation range.
    let edge = exp(-1.5 * normalized * normalized)
        * (1.0 - smoothstep(mix(0.72, 0.32, softness), 1.0, normalized))
        * mix(1.0, 1.16, softness);
    // Fresh deposits retain a soft core across low-noise cells; otherwise a
    // coherent field merely turns individual beads into long empty wake gaps.
    let density = smoothstep(0.14, 0.85, n);
    let fresh = mix(density, 0.35 + 0.65 * density, clamp(core, 0.0, 1.0));
    let original = mix(fresh, smoothstep(0.34, 0.85, n), spread);
    // Reuse detail already sampled above to break older smoke into wisps. No
    // independent per-particle noise phase or extra noise octaves are introduced.
    let filament = clamp(n + (n - broad) * 2.0, 0.0, 1.0);
    let broken = smoothstep(mix(0.17, 0.31, spread), 0.78, filament);
    let continuity = clamp(core, 0.0, 1.0) * (1.0 - spread);
    let erosion = mix(original, broken, structure * (1.0 - 0.8 * continuity));
    let tile = 1.0 - smoothstep(0.34, 0.50, max(abs(centered.x), abs(centered.y)));
    return edge * erosion * tile * (0.85 + 0.15 * seed);
}
