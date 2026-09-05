//! Engine-independent reference for linear normal-map decoding and graph previews.

/// Decodes RGB normal data into a world-space unit vector; alpha is deliberately absent.
/// Non-positive strength and zero-length samples return the geometric normal.
pub fn evaluate_normal_map(
    sample: [f32; 3],
    strength: f32,
    flip_y: bool,
    normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
) -> [f32; 3] {
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn unit(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
        let length_squared = dot(v, v);
        if length_squared <= 1e-12 {
            fallback
        } else {
            v.map(|x| x / length_squared.sqrt())
        }
    }
    let n = unit(normal, [0.0, 0.0, 1.0]);
    if strength <= 0.0 {
        return n;
    }
    let axis = if n[1].abs() < 0.99 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = unit(
        std::array::from_fn(|i| tangent[i] - n[i] * dot(n, tangent)),
        unit(cross(axis, n), [1.0, 0.0, 0.0]),
    );
    let cross_nt = cross(n, t);
    let sign = if dot(cross_nt, bitangent) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let b = cross_nt.map(|x| x * sign);
    let decoded = sample.map(|x| x * 2.0 - 1.0);
    let local = [
        decoded[0] * strength,
        decoded[1] * strength * if flip_y { -1.0 } else { 1.0 },
        decoded[2],
    ];
    unit(
        std::array::from_fn(|i| t[i] * local[0] + b[i] * local[1] + n[i] * local[2]),
        n,
    )
}
