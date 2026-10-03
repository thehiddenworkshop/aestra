struct Light {
    position_range: vec4<f32>, color_intensity: vec4<f32>,
    radius: f32, priority: u32, source_token: u32, particle_index: u32,
}
struct Dispatch { input: vec4<u32>, output: vec4<u32> }
struct Counters { requested: atomic<u32>, candidates: atomic<u32>, selected: atomic<u32>, dropped: atomic<u32> }
fn better(a: Light, b: Light) -> bool {
    let av = a.color_intensity.w > 0.0;
    let bv = b.color_intensity.w > 0.0;
    if av != bv { return av; }
    if !av { return false; }
    if a.priority != b.priority { return a.priority > b.priority; }
    if a.color_intensity.w != b.color_intensity.w { return a.color_intensity.w > b.color_intensity.w; }
    if a.source_token != b.source_token { return a.source_token < b.source_token; }
    return a.particle_index < b.particle_index;
}
