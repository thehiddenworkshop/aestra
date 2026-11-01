let d = f-vec3<f32>(corner_offset);
var corner_value = 0.0;
    if @DIMENSIONS@ == 2u {
        let g = array<vec2<f32>,8>(vec2<f32>(1.0,0.0),vec2<f32>(-1.0,0.0),vec2<f32>(0.0,1.0),vec2<f32>(0.0,-1.0),
            vec2<f32>(0.70710677,0.70710677),vec2<f32>(-0.70710677,0.70710677),vec2<f32>(0.70710677,-0.70710677),vec2<f32>(-0.70710677,-0.70710677));
        corner_value = dot(g[h&7u],d.xy);
    } else {
        let direction = h&15u;
        let gu = select(d.y,d.x,direction<8u);
        let gv = select(select(d.z,d.x,direction==12u || direction==14u),d.y,direction<4u);
        corner_value = select(-gu,gu,(direction&1u)==0u)+select(-gv,gv,(direction&2u)==0u);
    }
