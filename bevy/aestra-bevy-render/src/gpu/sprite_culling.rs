//! Conservative per-view queue rejection for minimum-footprint analytic sprites.
//! Main-world visibility remains permissive so expanded edge footprints reach this check.
use bevy::{camera::MainPassResolutionOverride, prelude::*, render::view::ExtractedView};

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub half_extents: Vec3,
    pub maximum_size: f32,
    pub world_from_effect: Mat4,
    pub minimum_pixels: f32,
}

pub(super) struct View {
    clip_from_world: Mat4,
    pixel_padding: Vec2,
    pixel_ratio: f32,
}

/// Prepare camera data once per view, not once per draw or particle. None fails open.
pub(super) fn prepare(
    view: &ExtractedView,
    resolution: Option<&MainPassResolutionOverride>,
    jittered: bool,
) -> Option<View> {
    if jittered {
        return None;
    }
    let viewport = resolution.map_or(view.viewport.zw(), |resolution| resolution.0);
    let world_from_view = view.world_from_view.to_matrix();
    let matrix = view.clip_from_view * world_from_view.inverse();
    // Custom clip overrides can disagree with the unjittered matrix used for footprint sizing.
    if view
        .clip_from_world
        .is_some_and(|override_matrix| !override_matrix.abs_diff_eq(matrix, 1e-5))
    {
        return None;
    }
    View::new(view.clip_from_view, world_from_view, viewport)
}

impl View {
    fn new(projection: Mat4, world_from_view: Mat4, viewport: UVec2) -> Option<Self> {
        if viewport.min_element() == 0
            || !standard_projection(projection)
            || !rigid_camera(world_from_view)
        {
            return None;
        }
        let pixels =
            Vec2::new(projection.x_axis.x.abs(), projection.y_axis.y.abs()) * viewport.as_vec2();
        let pixel_ratio = pixels.max_element() / pixels.min_element();
        let clip_from_world = projection * world_from_view.inverse();
        (pixel_ratio.is_finite() && clip_from_world.is_finite()).then_some(Self {
            clip_from_world,
            pixel_padding: Vec2::splat(2.0) / viewport.as_vec2(),
            pixel_ratio,
        })
    }

    /// Unknown/unbounded inputs fail open. This is per draw/view, never per particle.
    pub(super) fn visible(&self, bounds: &Bounds) -> bool {
        if !bounds.minimum_pixels.is_finite()
            || bounds.minimum_pixels <= 0.0
            || !bounds.half_extents.is_finite()
            || bounds.half_extents.min_element() < 0.0
            || !bounds.maximum_size.is_finite()
            || bounds.maximum_size < 0.0
            || !uniform_affine(bounds.world_from_effect)
        {
            return true;
        }
        // The shader uses normalized camera axes. Standard perspective/orthographic projections
        // have a diagonal, constant-depth pixel Jacobian; account for anisotropic main-pass pixels.
        // A rotated square's corner radius is floor/sqrt(2) for isotropic pixels. This margin
        // bounds the WHOLE expanded quad (not only the procedural mask), conservatively added to
        // existing geometry bounds. One extra pixel covers edge/rounding and raster sample phase.
        let radius =
            bounds.minimum_pixels * std::f32::consts::FRAC_1_SQRT_2 * self.pixel_ratio + 1.0;
        let padding = radius * self.pixel_padding;
        if !padding.is_finite() {
            return true;
        }
        let clip_from_world = self.clip_from_world;
        // Transform POSITION bounds, then add a camera-independent sphere enclosing every
        // rotated billboard corner. Transforming an ordinary size-padded local AABB is not
        // sufficient: emitter max-scale and camera-facing geometry can exceed that AABB.
        let world = bounds.world_from_effect;
        let scale = world.x_axis.truncate().length();
        let half_extents = world.x_axis.truncate().abs() * bounds.half_extents.x
            + world.y_axis.truncate().abs() * bounds.half_extents.y
            + world.z_axis.truncate().abs() * bounds.half_extents.z
            + Vec3::splat(bounds.maximum_size * scale * std::f32::consts::FRAC_1_SQRT_2);
        let half_extents = half_extents * 1.0001 + Vec3::splat(0.001);
        let center = world.w_axis.truncate();
        if !clip_from_world.is_finite() || !half_extents.is_finite() {
            return true;
        }
        let mut outside = [true; 4];
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    let clip =
                        clip_from_world * (center + half_extents * Vec3::new(x, y, z)).extend(1.0);
                    // Bounds crossing the eye plane cannot use a positive-w side-plane test.
                    // Near/far rejection is intentionally left to raster clipping too.
                    if !clip.is_finite() || clip.w <= 1e-5 {
                        return true;
                    }
                    let limit = (Vec2::ONE + padding) * clip.w;
                    outside[0] &= clip.x < -limit.x;
                    outside[1] &= clip.x > limit.x;
                    outside[2] &= clip.y < -limit.y;
                    outside[3] &= clip.y > limit.y;
                }
            }
        }
        !outside.into_iter().any(|side| side)
    }
}

fn standard_projection(matrix: Mat4) -> bool {
    if !matrix.is_finite() || matrix.x_axis.x == 0.0 || matrix.y_axis.y == 0.0 {
        return false;
    }
    // Standard off-center projections are allowed; skew/oblique/custom Jacobians are not.
    let diagonal_axes = matrix.x_axis.y == 0.0
        && matrix.x_axis.z == 0.0
        && matrix.x_axis.w == 0.0
        && matrix.y_axis.x == 0.0
        && matrix.y_axis.z == 0.0
        && matrix.y_axis.w == 0.0;
    let perspective = matrix.z_axis.w == -1.0
        && matrix.w_axis.w == 0.0
        && matrix.w_axis.x == 0.0
        && matrix.w_axis.y == 0.0;
    let orthographic = matrix.z_axis.w == 0.0
        && matrix.w_axis.w == 1.0
        && matrix.z_axis.x == 0.0
        && matrix.z_axis.y == 0.0;
    diagonal_axes && (perspective || orthographic)
}

fn uniform_affine(matrix: Mat4) -> bool {
    if !matrix.is_finite()
        || matrix.x_axis.w != 0.0
        || matrix.y_axis.w != 0.0
        || matrix.z_axis.w != 0.0
        || matrix.w_axis.w != 1.0
    {
        return false;
    }
    let axes = [
        matrix.x_axis.truncate(),
        matrix.y_axis.truncate(),
        matrix.z_axis.truncate(),
    ];
    let lengths = axes.map(|axis| axis.length());
    if !lengths
        .iter()
        .all(|length| length.is_finite() && *length > 1e-6)
    {
        return false;
    }
    // Fail open on anisotropic transforms: the billboard shader applies column lengths in
    // camera space, which need not fit a transformed world-space AABB under nonuniform scale.
    if lengths
        .iter()
        .any(|length| (*length / lengths[0] - 1.0).abs() > 1e-5)
    {
        return false;
    }
    let axes = [
        axes[0] / lengths[0],
        axes[1] / lengths[1],
        axes[2] / lengths[2],
    ];
    axes[0].dot(axes[1]).abs() < 1e-5
        && axes[0].dot(axes[2]).abs() < 1e-5
        && axes[1].dot(axes[2]).abs() < 1e-5
}

fn rigid_camera(matrix: Mat4) -> bool {
    uniform_affine(matrix) && (matrix.x_axis.truncate().length() - 1.0).abs() < 1e-5
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::{render_resource::TextureFormat, view::RetainedViewEntity};

    fn extracted(projection: Mat4) -> ExtractedView {
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
            clip_from_view: projection,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: TextureFormat::Rgba16Float,
            viewport: UVec4::new(0, 0, 960, 540),
            color_grading: Default::default(),
            invert_culling: false,
        }
    }

    fn bounds(position: Vec3, floor: f32) -> Bounds {
        Bounds {
            half_extents: Vec3::ZERO,
            maximum_size: 0.001,
            world_from_effect: Mat4::from_translation(position),
            minimum_pixels: floor,
        }
    }

    fn projections() -> [Mat4; 2] {
        [
            Mat4::perspective_infinite_reverse_rh(1.0, 960.0 / 540.0, 0.1),
            Mat4::orthographic_rh(-16.0, 16.0, -9.0, 9.0, 0.1, 100.0),
        ]
    }

    fn position_at_ndc(projection: Mat4, ndc: Vec2) -> Vec3 {
        let depth = 10.0;
        let w = if projection.z_axis.w == -1.0 {
            depth
        } else {
            1.0
        };
        Vec3::new(
            ndc.x * w / projection.x_axis.x,
            ndc.y * w / projection.y_axis.y,
            -depth,
        )
    }

    #[test]
    fn four_edges_keep_expanded_quads_but_reject_fully_offscreen_draws() {
        for projection in projections() {
            let view = prepare(&extracted(projection), None, false).unwrap();
            for floor in [2.0, 4.0, 8.0] {
                for edge in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                    let viewport = Vec2::new(960.0, 540.0);
                    // Center a quarter physical pixel outside the viewport. Authored quads
                    // do not overlap it, but their minimum-footprint replacement does.
                    let near = edge * (Vec2::ONE + Vec2::splat(0.5) / viewport);
                    assert!(view.visible(&bounds(position_at_ndc(projection, near), floor)));
                    assert!(!view.visible(&bounds(position_at_ndc(projection, edge * 1.2), floor)));
                }
            }
        }
    }

    #[test]
    fn physical_main_pass_resolution_and_each_view_control_the_margin() {
        let projection = projections()[0];
        let mut extracted = extracted(projection);
        let draw = bounds(position_at_ndc(projection, Vec2::new(1.02, 0.0)), 4.0);
        assert!(!prepare(&extracted, None, false).unwrap().visible(&draw));
        let small = MainPassResolutionOverride(UVec2::new(96, 54));
        assert!(
            prepare(&extracted, Some(&small), false)
                .unwrap()
                .visible(&draw)
        );
        extracted.viewport.x = 430;
        extracted.viewport.y = 120;
        assert!(!prepare(&extracted, None, false).unwrap().visible(&draw));
        extracted.world_from_view = GlobalTransform::from_translation(Vec3::new(1.0, 0.0, 0.0));
        assert!(prepare(&extracted, None, false).unwrap().visible(&draw));
    }

    #[test]
    fn ordinary_rotated_billboards_are_bounded_independently_of_effect_rotation() {
        let projection = projections()[1];
        let view = prepare(&extracted(projection), None, false).unwrap();
        for angle in [0.0, 0.4, 1.2, 2.3] {
            for scale in [1.0, 3.0, -3.0] {
                let mut draw = bounds(Vec3::ZERO, 2.0);
                draw.maximum_size = 4.0;
                draw.world_from_effect = Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    Quat::from_euler(EulerRot::XYZ, angle, angle, angle),
                    Vec3::new(16.0 + 4.0 * scale.abs() * 0.6, 0.0, -10.0),
                );
                assert!(
                    view.visible(&draw),
                    "a rotated corner can overlap the viewport"
                );
                draw.world_from_effect.w_axis.x += 20.0;
                assert!(!view.visible(&draw));
            }
        }
    }

    #[test]
    fn invalid_bounds_and_unbounded_transforms_fail_open() {
        let view = prepare(&extracted(projections()[0]), None, false).unwrap();
        let offscreen = bounds(Vec3::new(100.0, 0.0, -10.0), 2.0);
        assert!(!view.visible(&offscreen));
        for value in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(view.visible(&Bounds {
                half_extents: Vec3::splat(value),
                ..offscreen
            }));
            assert!(view.visible(&Bounds {
                maximum_size: value,
                ..offscreen
            }));
            assert!(view.visible(&Bounds {
                minimum_pixels: value,
                ..offscreen
            }));
        }
        assert!(view.visible(&Bounds {
            minimum_pixels: 0.0,
            ..offscreen
        }));
        for scale in [Vec3::ZERO, Vec3::new(1.0, 2.0, 1.0), Vec3::splat(f32::NAN)] {
            assert!(view.visible(&Bounds {
                world_from_effect: Mat4::from_scale_rotation_translation(
                    scale,
                    Quat::IDENTITY,
                    Vec3::new(100.0, 0.0, -10.0)
                ),
                ..offscreen
            }));
        }
        let mut shear = offscreen;
        shear.world_from_effect.x_axis.y = 0.4;
        assert!(view.visible(&shear));
        assert!(view.visible(&Bounds {
            half_extents: Vec3::splat(20.0),
            ..offscreen
        }));
    }

    #[test]
    fn unsupported_camera_jitter_and_custom_projection_fail_open() {
        let mut extracted = extracted(projections()[0]);
        assert!(prepare(&extracted, None, true).is_none());
        assert!(
            prepare(
                &extracted,
                Some(&MainPassResolutionOverride(UVec2::ZERO)),
                false
            )
            .is_none()
        );
        extracted.clip_from_world = Some(Mat4::IDENTITY);
        assert!(prepare(&extracted, None, false).is_none());
        extracted.clip_from_world = Some(extracted.clip_from_view);
        assert!(prepare(&extracted, None, false).is_some());
        extracted.clip_from_world = None;
        extracted.world_from_view = GlobalTransform::from(Transform::from_scale(Vec3::splat(2.0)));
        assert!(prepare(&extracted, None, false).is_none());
        extracted.world_from_view = GlobalTransform::IDENTITY;
        extracted.clip_from_view.x_axis.y = 0.3;
        assert!(prepare(&extracted, None, false).is_none());
        extracted.clip_from_view = Mat4::from_cols_array(&[f32::NAN; 16]);
        assert!(prepare(&extracted, None, false).is_none());
    }

    #[test]
    fn visible_expanded_quad_corners_never_get_rejected() {
        // Independent shader-geometry oracle: actual camera-facing quad corners, including
        // rotations, pixel anisotropy, different depths, scales and authored sizes.
        for projection in projections() {
            for viewport in [UVec2::new(960, 540), UVec2::new(240, 540)] {
                let view = View::new(projection, Mat4::IDENTITY, viewport).unwrap();
                let pixel_axes = Vec2::new(projection.x_axis.x.abs(), projection.y_axis.y.abs())
                    * viewport.as_vec2()
                    * 0.5;
                for floor in [2.0, 4.0, 8.0] {
                    for size in [0.0001, 0.1, 4.0] {
                        for scale in [1.0_f32, 3.0, -3.0] {
                            for angle in [0.0_f32, 0.7, 1.4, 2.2] {
                                let (sine, cosine) = angle.sin_cos();
                                let axis_x = Vec2::new(cosine, sine) * size * scale.abs();
                                let axis_y = Vec2::new(-sine, cosine) * size * scale.abs();
                                let w = if projection.z_axis.w == -1.0 {
                                    10.0
                                } else {
                                    1.0
                                };
                                let projected = (pixel_axes * axis_x / w)
                                    .length()
                                    .min((pixel_axes * axis_y / w).length());
                                let expansion = (floor / projected).max(1.0);
                                for edge in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                                    for distance in [0.0005, 0.005, 0.02, 0.1] {
                                        let center =
                                            position_at_ndc(projection, edge * (1.0 + distance));
                                        let mut draw = bounds(center, floor);
                                        draw.maximum_size = size;
                                        draw.world_from_effect =
                                            Mat4::from_scale_rotation_translation(
                                                Vec3::splat(scale),
                                                Quat::from_rotation_y(angle),
                                                center,
                                            );
                                        for x in [-0.5, 0.5] {
                                            for y in [-0.5, 0.5] {
                                                let offset = (axis_x * x + axis_y * y) * expansion;
                                                let clip = projection
                                                    * (center + offset.extend(0.0)).extend(1.0);
                                                if clip.x.abs() <= clip.w && clip.y.abs() <= clip.w
                                                {
                                                    assert!(
                                                        view.visible(&draw),
                                                        "visible corner rejected: {floor}/{size}/{scale}/{angle}/{edge}/{distance}"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
