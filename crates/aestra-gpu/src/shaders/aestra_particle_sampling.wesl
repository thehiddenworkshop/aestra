// N3 launch layouts. Mirrors aestra_runtime::particle_sampling. All loops bounded;
// the launch ordinal is identity, never a slot or workgroup invocation index.
fn aestra_sampling_hash(input: u32) -> u32 {
    var x=input; x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu; return x ^ (x >> 16u);
}
fn aestra_sampling_unit(x:u32) -> f32 {return f32(aestra_sampling_hash(x) >> 8u) * (1.0 / 16777216.0);}
fn aestra_sampling_fraction(x:u32) -> f32 {return f32(x >> 8u) * (1.0 / 16777216.0);}
// index, requested count, first destination ordinal.
fn aestra_sampling_sequence(ordinal:u32,count:u32) -> vec3<u32> {
    let n=max(count,1u); let i=ordinal % n; return vec3<u32>(i,n,ordinal-i);
}
fn aestra_sampling_key(seed:u32,effect_seed:u32,cohort:vec3<u32>,stream:u32) -> u32 {
    return seed ^ effect_seed ^ (cohort.z * 0x9e3779b9u) ^ (stream * 0x85ebca6bu);
}
fn aestra_sampling_coordinates(config:vec4<u32>,seed:u32,effect_seed:u32,cohort:vec3<u32>,stream:u32,mode:u32) -> vec3<f32> {
    let k=aestra_sampling_key(seed,effect_seed,cohort,stream); let i=cohort.x;
    let random=vec3<f32>(aestra_sampling_unit(k ^ (i*0x27d4eb2du) ^ 0x165667b1u),
        aestra_sampling_unit(k ^ (i*0x27d4eb2du) ^ (2u*0x165667b1u)),aestra_sampling_unit(k ^ (i*0x27d4eb2du) ^ (3u*0x165667b1u)));
    if mode != 2u {return random;}
    let n=f32(max(cohort.y,1u)); let jitter=bitcast<f32>(config.w);
    let u=min((f32(i)+0.5+(random.x-0.5)*jitter*0.98)/n,0.99999994);
    let v=fract(aestra_sampling_fraction(i*0x9e3779b9u+aestra_sampling_hash(k))+(random.y-0.5)*jitter/sqrt(n));
    let w=fract(aestra_sampling_fraction(i*0xbb67ae85u+aestra_sampling_hash(k ^ 0xa511e9b3u))+(random.z-0.5)*jitter/sqrt(n));
    return vec3<f32>(u,v,w);
}
fn aestra_sampling_rotate(v:vec3<f32>,k:u32) -> vec3<f32> {
    let a=aestra_sampling_unit(k ^ 0x1234567u);
    let b=aestra_sampling_unit(k ^ 0x7654321u)*6.283185307179586;
    let c=aestra_sampling_unit(k ^ 0xabcdefu)*6.283185307179586;
    let q=vec4<f32>(sqrt(1.0-a)*sin(b),sqrt(1.0-a)*cos(b),sqrt(a)*sin(c),sqrt(a)*cos(c));
    let t=2.0*cross(q.xyz,v);return (v+q.w*t)+cross(q.xyz,t);
}
fn aestra_sampled_direction(config:vec4<u32>,seed:u32,effect_seed:u32,cohort:vec3<u32>,mode:u32,axis:vec3<f32>,spread:f32) -> vec3<f32> {
    let uvw=aestra_sampling_coordinates(config,seed,effect_seed,cohort,1u,config.y);
    let d=aestra_velocity_direction(mode,axis,spread,uvw.x,uvw.y);
    if mode==3u {return aestra_sampling_rotate(d,aestra_sampling_key(seed,effect_seed,cohort,1u));}
    return d;
}
// Shape tags match analytic GpuEmitter, not the historical stateful shape tags.
fn aestra_sampled_position(config:vec4<u32>,seed:u32,effect_seed:u32,cohort:vec3<u32>,shape:u32,radius:f32,depth:f32,extent_z:f32) -> vec3<f32> {
    let uvw=aestra_sampling_coordinates(config,seed,effect_seed,cohort,2u,config.x);
    let u=uvw.x;let v=uvw.y;let w=uvw.z; let phi=v*6.283185307179586;
    if shape==1u {let r=radius*sqrt(u);return vec3<f32>(r*cos(phi),r*sin(phi),0.0);}
    if shape==2u {return vec3<f32>(radius*cos(phi),radius*sin(phi),0.0);}
    if shape==3u || shape==4u {
        var y=u;if shape==3u {y=1.0-2.0*u;}
        let r=sqrt(max(1.0-y*y,0.0)); var d=vec3<f32>(r*cos(phi),y,r*sin(phi));
        if shape==3u {d=aestra_sampling_rotate(d,aestra_sampling_key(seed,effect_seed,cohort,2u));}
        return d*(radius*pow(w,1.0/3.0));
    }
    if shape==5u {return (2.0*uvw-vec3<f32>(1.0))*vec3<f32>(radius,depth,extent_z);}
    if shape==6u {let r=radius*sqrt(u);return vec3<f32>(r*cos(phi),(w-0.5)*depth,r*sin(phi));}
    if shape==7u {let h=pow(w,1.0/3.0);let r=radius*h*sqrt(u);return vec3<f32>(r*cos(phi),h*depth,r*sin(phi));}
    return vec3<f32>(0.0);
}
