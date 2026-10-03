@compute @workgroup_size(64)
fn merge_lights(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = dispatch.input.z;
    let runs = dispatch.input.w;
    if id.x >= width * runs { return; }
    let run = id.x / width;
    let lane = id.x % width;
    let pair = run / 2u;
    let other = run ^ 1u;
    let a = input_runs[id.x];
    var lower = 0u;
    var upper = select(0u, width, other < runs);
    // Rank in the other sorted run; exact ties use left-before-right.
    while lower < upper {
        let mid = lower + (upper - lower) / 2u;
        let b = input_runs[other * width + mid];
        let preceding = better(b, a) || ((run & 1u) != 0u && !better(a, b));
        if preceding { lower = mid + 1u; } else { upper = mid; }
    }
    let rank = lane + lower;
    if rank < dispatch.output.x { output_runs[pair * dispatch.output.x + rank] = a; }
    // Odd final run: initialize its missing right half too.
    if other >= runs && lane + width < dispatch.output.x {
        var empty: Light;
        output_runs[pair * dispatch.output.x + lane + width] = empty;
    }
}
@compute @workgroup_size(1)
fn finish_lights() {
    var lower = 0u;
    var upper = dispatch.input.z;
    while lower < upper {
        let mid = lower + (upper - lower) / 2u;
        if input_runs[mid].color_intensity.w > 0.0 { lower = mid + 1u; } else { upper = mid; }
    }
    atomicStore(&counters.selected, lower);
    atomicStore(&counters.dropped, atomicLoad(&counters.candidates) - lower);
}
