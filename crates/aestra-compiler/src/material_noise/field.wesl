fn @ENTRY@(position: @VECTOR@, scale: f32, offset: @VECTOR@, amplitude: f32, seed: @SEED_TYPE@, @FRACTAL_PARAMETERS@footprint: f32) -> f32 {
    let frequency = clamp(scale,0.0,4096.0);
    var p = @POSITION@;
    var sample_footprint = max(footprint,0.0)*frequency;
    var weight = 1.0;
    var total = 0.0;
    var weights = 0.0;
    let count = @OCTAVES@;
    let seed_word = @SEED_WORD@;
    for (var octave=0u; octave<count; octave+=1u) {
        let visible = 1.0-smoothstep(0.25,0.75,sample_footprint);
        var value = 0.5;
        if visible>0.0 {
            let cell = floor(p);
            let f = p-cell;
            let u = f*f*f*(f*(f*6.0-vec3<f32>(15.0))+vec3<f32>(10.0));
            var sum = 0.0;
            for (var corner=0u; corner < @CORNERS@; corner+=1u) {
                let corner_offset = vec3<u32>(corner&1u,(corner>>1u)&1u,(corner>>2u)&1u);
                let c = cell+vec3<f32>(corner_offset);
                let wrapped = vec3<u32>(c-floor(c/65536.0)*65536.0);
                var h = (seed_word+octave*0x9e3779b9u) ^ (wrapped.x*0x9e3779b9u) ^ (wrapped.y*0x85ebca6bu) ^ (wrapped.z*0xc2b2ae35u);
                h = (h^(h>>16u))*0x7feb352du;
                h = (h^(h>>15u))*0x846ca68bu;
                h = h^(h>>16u);
                @SAMPLE@
                var corner_weight = 1.0;
                for (var axis=0u; axis<@DIMENSIONS@; axis+=1u) {
                    corner_weight *= select(1.0-u[axis],u[axis],corner_offset[axis]!=0u);
                }
                sum += corner_value*corner_weight;
            }
            value = @RESULT@;
        }
        total += (0.5+(value-0.5)*visible)*weight;
        weights += weight;
        p *= @LACUNARITY@;
        sample_footprint *= @LACUNARITY@;
        weight *= @GAIN@;
    }
    return total/weights*max(amplitude,0.0);
}
