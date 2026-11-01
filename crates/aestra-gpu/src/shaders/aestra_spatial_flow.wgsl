const AESTRA_FLOW_BASE: u32 = 191u;
fn aestra_flow_scalar(q: vec3<f32>, channel: u32) -> f32 {
    return aestra_flow_potential(q, 1.0, vec3<f32>(0.0), 1.0,
        params[AESTRA_FLOW_BASE+4u]+channel*0x9e3779b9u,
        f32(params[AESTRA_FLOW_BASE+5u]), 2.0, 0.5, 0.0)*2.0-1.0;
}
fn aestra_flow_derivative(q: vec3<f32>, h: f32, channel: u32, axis: u32) -> f32 {
    var a=q; var b=q; a[axis]+=h; b[axis]-=h;
    return (aestra_flow_scalar(a,channel)-aestra_flow_scalar(b,channel))/(2.0*h);
}
fn aestra_flow_field(position: vec3<f32>) -> vec3<f32> {
    let scale=bitcast<f32>(params[AESTRA_FLOW_BASE+2u]);
    let strength=bitcast<f32>(params[AESTRA_FLOW_BASE+3u]);
    let evolution=vec3<f32>(bitcast<f32>(params[AESTRA_FLOW_BASE+6u]),bitcast<f32>(params[AESTRA_FLOW_BASE+7u]),bitcast<f32>(params[AESTRA_FLOW_BASE+8u]));
    let offset=vec3<f32>(bitcast<f32>(params[AESTRA_FLOW_BASE+9u]),bitcast<f32>(params[AESTRA_FLOW_BASE+10u]),bitcast<f32>(params[AESTRA_FLOW_BASE+11u]));
    let time=bitcast<f32>(params[AESTRA_FLOW_BASE+12u]);
    let q=(position+offset-evolution*time)*scale;
    if params[AESTRA_FLOW_BASE]==1u {
        return vec3<f32>(aestra_flow_scalar(q,0u),aestra_flow_scalar(q,1u),aestra_flow_scalar(q,2u))*strength;
    }
    let octaves=params[AESTRA_FLOW_BASE+5u];
    let highest=f32(1u<<(octaves-1u));
    let h=1.0/(128.0*highest);
    var weight=1.0; var weights=0.0; var weighted_frequency=0.0;
    for(var octave=0u;octave<octaves;octave+=1u) {
        weights+=weight; weighted_frequency+=weight*f32(1u<<octave); weight*=0.5;
    }
    let gain=strength/(7.5*weighted_frequency/weights);
    return vec3<f32>(
        aestra_flow_derivative(q,h,2u,1u)-aestra_flow_derivative(q,h,1u,2u),
        aestra_flow_derivative(q,h,0u,2u)-aestra_flow_derivative(q,h,2u,0u),
        aestra_flow_derivative(q,h,1u,0u)-aestra_flow_derivative(q,h,0u,1u))*gain;
}
fn aestra_spatial_acceleration(position: vec3<f32>) -> vec3<f32> {
    if params[AESTRA_FLOW_BASE]==0u || bitcast<f32>(params[AESTRA_FLOW_BASE+3u])==0.0 {return vec3<f32>(0.0);}
    if params[AESTRA_FLOW_BASE+1u]==0u {return aestra_flow_field(position);}
    // Same effect-to-world affine as the collision provider; invert its linear
    // part for accelerations, including nonuniform scale and mirrored transforms.
    let r0=vec4<f32>(bitcast<f32>(params[176u]),bitcast<f32>(params[177u]),bitcast<f32>(params[178u]),bitcast<f32>(params[179u]));
    let r1=vec4<f32>(bitcast<f32>(params[180u]),bitcast<f32>(params[181u]),bitcast<f32>(params[182u]),bitcast<f32>(params[183u]));
    let r2=vec4<f32>(bitcast<f32>(params[184u]),bitcast<f32>(params[185u]),bitcast<f32>(params[186u]),bitcast<f32>(params[187u]));
    let c0=vec3<f32>(r0.x,r1.x,r2.x); let c1=vec3<f32>(r0.y,r1.y,r2.y); let c2=vec3<f32>(r0.z,r1.z,r2.z);
    let i0=cross(c1,c2); let i1=cross(c2,c0); let i2=cross(c0,c1);
    let det=dot(c0,i0);
    if !(abs(det)>0.0) {return vec3<f32>(0.0);}
    let p=vec4<f32>(position,1.0);
    let a=aestra_flow_field(vec3<f32>(dot(r0,p),dot(r1,p),dot(r2,p)));
    return vec3<f32>(dot(i0,a),dot(i1,a),dot(i2,a))/det;
}
