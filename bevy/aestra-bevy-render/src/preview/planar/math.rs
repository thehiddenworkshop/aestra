use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};

pub(super) fn reflected_pose(source: &GlobalTransform, height: f32) -> Transform {
    let reflect = |v: Vec3| Vec3::new(v.x, -v.y, v.z);
    let p = source.translation();
    Transform::from_translation(Vec3::new(p.x, 2.0 * height - p.y, p.z))
        .looking_to(reflect(*source.forward()), -reflect(*source.up()))
}

/// Conservative infinite-plane test; finite receiver bounds are left to normal
/// visibility. A view wholly above the horizon cannot contain this water plane.
pub(super) fn sees_plane(source: &GlobalTransform, base: &PerspectiveProjection) -> bool {
    let inverse = base.get_clip_from_view().inverse();
    [-1.0, 1.0].into_iter().any(|x| {
        [-1.0, 1.0].into_iter().any(|y| {
            let ray = inverse * Vec4::new(x, y, 1.0, 1.0);
            source.affine().transform_vector3(ray.truncate()).y < 0.0
        })
    })
}

/// Reverse-Z oblique near plane: z=w on the water and z=0 at the opposite
/// original far corner. X/Y/W projection is unchanged, preserving sampling.
pub(super) fn oblique(projection: Mat4, plane: Vec4) -> Mat4 {
    let q = projection.inverse() * Vec4::new(plane.x.signum(), plane.y.signum(), 0.0, 1.0);
    let denominator = plane.dot(q);
    if !denominator.is_finite() || denominator.abs() < 1e-6 {
        return projection;
    }
    let rows = projection.transpose();
    let z = rows.w_axis - plane / denominator;
    Mat4::from_cols(rows.x_axis, rows.y_axis, z, rows.w_axis).transpose()
}

#[derive(Clone, Debug)]
pub(super) struct MirrorProjection {
    pub base: PerspectiveProjection,
    pub plane: Vec4,
}
impl CameraProjection for MirrorProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        oblique(self.base.get_clip_from_view(), self.plane)
    }
    fn get_clip_from_view_for_sub(&self, sub: &SubCameraView) -> Mat4 {
        oblique(self.base.get_clip_from_view_for_sub(sub), self.plane)
    }
    fn update(&mut self, width: f32, height: f32) {
        self.base.update(width, height);
    }
    fn far(&self) -> f32 {
        self.base.far
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        self.base.get_frustum_corners(near, far)
    }
}

pub(super) fn extent(source: UVec2, scale: f32, budget: u64) -> Option<(UVec2, u64)> {
    if source.min_element() == 0 || !scale.is_finite() || scale <= 0.0 || scale > 1.0 || budget < 36
    {
        return None;
    }
    // Capture HDR + two Bevy HDR intermediates + depth + a filtered HDR chain.
    // 44 bytes/pixel conservatively bounds even a thin, nearly 2x mip chain.
    let available = (budget as f64 / (44.0 * source.x as f64 * source.y as f64)).sqrt();
    let scale = f64::from(scale).min(available);
    let size = UVec2::new(
        (source.x as f64 * scale).floor().max(1.0) as u32,
        (source.y as f64 * scale).floor().max(1.0) as u32,
    );
    let bytes = target_bytes(size)?;
    (bytes <= budget).then_some((size, bytes))
}

pub(super) fn target_bytes(mut size: UVec2) -> Option<u64> {
    let mut bytes = u64::from(size.x)
        .checked_mul(u64::from(size.y))?
        .checked_mul(28)?;
    loop {
        bytes = bytes.checked_add(
            u64::from(size.x)
                .checked_mul(u64::from(size.y))?
                .checked_mul(8)?,
        )?;
        if size == UVec2::ONE {
            break;
        }
        size = (size / 2).max(UVec2::ONE);
    }
    Some(bytes)
}
