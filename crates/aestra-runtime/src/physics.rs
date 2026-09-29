//! The host's physics scene as collision proxies (host bindings HB10, the `EnginePhysicsQuery`
//! provider): each frame an adapter asks the host's physics engine for the colliders around an
//! effect and describes them as a few analytic primitives in world space — spheres, capsules,
//! oriented boxes, half-spaces. Stateful particles with a `Physics` collider collide with them on the
//! device, so moving rigid bodies push particles around without any read back.
//!
//! Engine-neutral: nothing here knows a physics crate. Adapters (`aestra-bevy-rapier`,
//! `aestra-bevy-avian`, or a game's own) produce a [`PhysicsScene`]; the GPU ABI is in `aestra-gpu`.

/// The most proxies one effect collides with; an adapter keeps the nearest ones.
pub const MAX_PHYSICS_PROXIES: usize = 64;

/// One of the host's colliders, as an analytic primitive in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhysicsProxy {
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    /// The points within `radius` of the segment `a`–`b`.
    Capsule {
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    },
    /// A box of `half_extents`, rotated by the unit quaternion `rotation` (`xyzw`) about `center`.
    Box {
        center: [f32; 3],
        rotation: [f32; 4],
        half_extents: [f32; 3],
    },
    /// The half-space `dot(normal, p) < distance` (unit `normal`): a ground, a wall.
    HalfSpace {
        normal: [f32; 3],
        distance: f32,
    },
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Rotates `v` by the unit quaternion `q`: `v + w·t + q×t` with `t = 2 q×v` (trig-free, as the
/// kernel does).
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let tx = 2.0 * (y * v[2] - z * v[1]);
    let ty = 2.0 * (z * v[0] - x * v[2]);
    let tz = 2.0 * (x * v[1] - y * v[0]);
    [
        v[0] + w * tx + (y * tz - z * ty),
        v[1] + w * ty + (z * tx - x * tz),
        v[2] + w * tz + (x * ty - y * tx),
    ]
}

/// `v / |v|`, or `fallback` for a vanishing `v`.
fn unit(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length_squared = dot(v, v);
    if length_squared > 1e-12 {
        let length = length_squared.sqrt();
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        fallback
    }
}

impl PhysicsProxy {
    /// The signed distance from `p` to the primitive (negative inside) and the outward unit normal
    /// there. The GPU's physics collider computes the same, operation for operation.
    pub fn distance_normal(&self, p: [f32; 3]) -> (f32, [f32; 3]) {
        match *self {
            Self::Sphere { center, radius } => {
                let delta = sub(p, center);
                (
                    dot(delta, delta).sqrt() - radius,
                    unit(delta, [0.0, 1.0, 0.0]),
                )
            }
            Self::Capsule { a, b, radius } => {
                let axis = sub(b, a);
                let length_squared = dot(axis, axis);
                let t = if length_squared > 1e-12 {
                    (dot(sub(p, a), axis) / length_squared).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let nearest = [a[0] + axis[0] * t, a[1] + axis[1] * t, a[2] + axis[2] * t];
                let delta = sub(p, nearest);
                (
                    dot(delta, delta).sqrt() - radius,
                    unit(delta, [0.0, 1.0, 0.0]),
                )
            }
            Self::Box {
                center,
                rotation,
                half_extents,
            } => {
                let inverse = [-rotation[0], -rotation[1], -rotation[2], rotation[3]];
                let local = rotate(inverse, sub(p, center));
                let q: [f32; 3] = std::array::from_fn(|i| local[i].abs() - half_extents[i]);
                let outside = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
                let outside_length = dot(outside, outside).sqrt();
                let inside = q[0].max(q[1].max(q[2])).min(0.0);
                let sign = |value: f32| if value < 0.0 { -1.0 } else { 1.0 };
                let local_normal = if outside_length > 0.0 {
                    [
                        outside[0] * sign(local[0]),
                        outside[1] * sign(local[1]),
                        outside[2] * sign(local[2]),
                    ]
                } else {
                    // Inside: out through the nearest face (ties toward the earlier axis).
                    let mut axis = 0;
                    if q[1] > q[axis] {
                        axis = 1;
                    }
                    if q[2] > q[axis] {
                        axis = 2;
                    }
                    let mut normal = [0.0; 3];
                    normal[axis] = sign(local[axis]);
                    normal
                };
                (
                    outside_length + inside,
                    rotate(rotation, unit(local_normal, [0.0, 1.0, 0.0])),
                )
            }
            Self::HalfSpace { normal, distance } => (dot(normal, p) - distance, normal),
        }
    }
}

/// The host's physics colliders around an effect, in world space: at most
/// [`MAX_PHYSICS_PROXIES`] proxies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysicsScene {
    pub proxies: Vec<PhysicsProxy>,
}

impl PhysicsScene {
    /// The distance from `p` to the nearest proxy and its normal there (the first on a tie); far and
    /// straight up with no proxy.
    pub fn distance_normal(&self, p: [f32; 3]) -> (f32, [f32; 3]) {
        let mut best = (1.0e30, [0.0, 1.0, 0.0]);
        for proxy in self.proxies.iter().take(MAX_PHYSICS_PROXIES) {
            let candidate = proxy.distance_normal(p);
            if candidate.0 < best.0 {
                best = candidate;
            }
        }
        best
    }
}
