// Reusable emission modulation: a stationary world-space field, independently
// seeded per strand. No wall clock, texture, moving UV phase or particle births.
fn emission_grain(world: vec3<f32>, seed: f32, age: f32, amount: f32, frequency: f32, cooling: f32) -> vec4<f32> {
    let p = world * clamp(frequency, 0.01, 16.0) + seed * vec3<f32>(19.1, 31.7, 47.3);
    var noise: array<f32, 2>;
    for (var octave = 0u; octave < 2u; octave += 1u) {
        let position = p * select(0.27, 1.0, octave == 1u);
        let cell = floor(position);
        let f = fract(position);
        let u = f * f * (vec3<f32>(3.0) - 2.0 * f);
        var corners: array<f32, 8>;
        for (var i = 0u; i < 8u; i += 1u) {
            var q = fract((cell + vec3<f32>(f32(i % 2u), f32((i / 2u) % 2u), f32(i / 4u))) * 0.1031);
            q += vec3<f32>(dot(q, q.yzx + vec3<f32>(33.33)));
            corners[i] = fract((q.x + q.y) * q.z);
        }
        noise[octave] = mix(mix(mix(corners[0], corners[1], u.x), mix(corners[2], corners[3], u.x), u.y),
            mix(mix(corners[4], corners[5], u.x), mix(corners[6], corners[7], u.x), u.y), u.z);
    }
    let broad = noise[0];
    let fine = noise[1];
    // Suppress unresolved grain at a distance instead of shimmering bright pixels.
    let footprint = max(length(dpdx(p)), length(dpdy(p)));
    let resolved = 1.0 - smoothstep(0.35, 1.5, footprint);
    let variation = mix(0.45, 1.5, smoothstep(0.15, 0.85, fine)) * mix(0.55, 1.25, broad);
    let grain = mix(0.85, variation, resolved) * mix(0.8, 1.2, seed);
    let intensity = mix(1.0, grain, clamp(amount, 0.0, 1.0));
    // Age is supplied by the graph: a trail uses its recorded sample age, while
    // sprites use their live age. This does not invent a second simulation clock.
    let cooled = smoothstep(0.25, 0.95, age) * clamp(cooling, 0.0, 1.0);
    return vec4<f32>(vec3<f32>(1.0, 1.0 - 0.35 * cooled, 1.0 - 0.7 * cooled) * intensity, 1.0);
}
