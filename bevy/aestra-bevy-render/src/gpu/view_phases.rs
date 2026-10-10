//! Per-view visibility, retained-phase lifecycle and transparent ordering.
//! Shared production code; MainEntity comes from the selected engine adapter.
use super::{draw_instance::GpuDrawInstance, extraction::MainEntity, sprite_culling};
use bevy::{
    core_pipeline::{
        core_2d::Transparent2d,
        core_3d::{Transparent3d, TransparentSortingInfo3d},
    },
    math::FloatOrd,
    prelude::*,
    render::{
        render_phase::{DrawFunctionId, PhaseItemExtraIndex, SortedPhaseItem, SortedRenderPhase},
        render_resource::CachedRenderPipelineId,
        view::RenderVisibleEntities,
    },
};

/// One renderer-local draw. Keep batching disabled: instances are GPU-owned.
pub(super) struct PhaseDraw {
    pub entity: (Entity, MainEntity),
    pub pipeline: CachedRenderPipelineId,
    pub draw_function: DrawFunctionId,
    pub indexed: bool,
}

impl PhaseDraw {
    pub(super) fn two_d(self, renderer_order: u32) -> Transparent2d {
        Transparent2d {
            sort_key: FloatOrd(renderer_order as f32),
            entity: self.entity,
            pipeline: self.pipeline,
            draw_function: self.draw_function,
            batch_range: 0..1,
            extracted_index: usize::MAX,
            extra_index: PhaseItemExtraIndex::None,
            indexed: self.indexed,
        }
    }

    pub(super) fn three_d(self, mesh_center: Vec3, renderer_order: u32) -> Transparent3d {
        Transparent3d {
            sorting_info: gpu_draw_sorting_info(mesh_center, renderer_order),
            entity: self.entity,
            pipeline: self.pipeline,
            draw_function: self.draw_function,
            distance: 0.0,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: self.indexed,
        }
    }
}

pub(super) fn reject_sampled_sprite<I: SortedPhaseItem>(
    phase: &mut SortedRenderPhase<I>,
    bounds: Option<&sprite_culling::Bounds>,
    view: Option<&sprite_culling::View>,
    render_entity: Entity,
    main_entity: MainEntity,
) -> bool {
    if bounds
        .zip(view)
        .is_some_and(|(bounds, view)| !view.visible(bounds))
    {
        // Sorted phases retain entries: a previously visible draw must be removed.
        phase.remove(render_entity, main_entity);
        true
    } else {
        false
    }
}

const RENDERER_ORDER_DEPTH_BIAS: f32 = 0.0001;

pub(super) fn visible_gpu_draws(
    visible_entities: &RenderVisibleEntities,
) -> impl Iterator<Item = (Entity, MainEntity)> + '_ {
    visible_entities
        .get::<GpuDrawInstance>()
        .into_iter()
        .flat_map(|class| class.iter_visible())
        .map(|(render_entity, main_entity)| (*render_entity, *main_entity))
}

/// Retained phases are shared with Bevy meshes: retire only our command entries.
/// Visibility is per view, so a draw visible elsewhere must still leave this view.
pub(super) fn retire_invisible_draws<I: SortedPhaseItem>(
    phase: &mut SortedRenderPhase<I>,
    visible_entities: &RenderVisibleEntities,
    commands: &[DrawFunctionId],
) {
    let visible = visible_gpu_draws(visible_entities).collect::<std::collections::HashSet<_>>();
    phase.items.retain(|entity, item| {
        !commands.contains(&item.draw_function()) || visible.contains(entity)
    });
}

fn gpu_draw_sorting_info(mesh_center: Vec3, renderer_order: u32) -> TransparentSortingInfo3d {
    TransparentSortingInfo3d::Sorted {
        mesh_center,
        depth_bias: renderer_order as f32 * RENDERER_ORDER_DEPTH_BIAS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::CameraProjection,
        math::Affine3A,
        render::{
            render_phase::{CachedRenderPipelinePhaseItem, PhaseItem, ViewRangefinder3d},
            render_resource::TextureFormat,
            view::{ExtractedView, RenderVisibleEntitiesClass},
        },
    };
    use std::any::TypeId;

    fn draw(entity: (Entity, MainEntity)) -> PhaseDraw {
        PhaseDraw {
            entity,
            pipeline: CachedRenderPipelineId::INVALID,
            draw_function: DrawFunctionId(0),
            indexed: false,
        }
    }

    fn view() -> ExtractedView {
        use bevy::render::view::RetainedViewEntity;
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
            clip_from_view: PerspectiveProjection {
                fov: 1.0,
                aspect_ratio: 1.0,
                near: 0.1,
                ..default()
            }
            .get_clip_from_view(),
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: TextureFormat::Rgba16Float,
            viewport: UVec4::new(0, 0, 540, 540),
            color_grading: default(),
            invert_culling: false,
        }
    }

    #[test]
    fn phase_draw_preserves_identity_command_and_geometry_without_cpu_batching() {
        let mut world = World::new();
        let entity = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let rangefinder = view().rangefinder3d();
        for indexed in [false, true] {
            let input = || PhaseDraw {
                entity,
                draw_function: DrawFunctionId(7),
                indexed,
                ..draw(entity)
            };
            let two = input().two_d(12);
            let three = input().three_d(Vec3::new(0.0, 0.0, -8.0), 12);
            assert_eq!(two.entity(), entity.0);
            assert_eq!(three.entity(), entity.0);
            assert_eq!(two.main_entity(), entity.1);
            assert_eq!(three.main_entity(), entity.1);
            assert_eq!(two.draw_function(), DrawFunctionId(7));
            assert_eq!(three.draw_function(), DrawFunctionId(7));
            assert_eq!(two.cached_pipeline(), CachedRenderPipelineId::INVALID);
            assert_eq!(three.cached_pipeline(), CachedRenderPipelineId::INVALID);
            assert_eq!(two.batch_range(), &(0..1));
            assert_eq!(three.batch_range(), &(0..1));
            assert_eq!(two.extracted_index, usize::MAX);
            assert!(matches!(two.extra_index(), PhaseItemExtraIndex::None));
            assert!(matches!(three.extra_index(), PhaseItemExtraIndex::None));
            assert_eq!(two.indexed(), indexed);
            assert_eq!(three.indexed(), indexed);
            assert_eq!(two.sort_key(), FloatOrd(12.0));
            assert_eq!(three.distance, 0.0);
            assert_eq!(
                three.sorting_info.sort_distance(&rangefinder),
                -8.0 + 0.0012
            );
        }
    }

    #[test]
    fn retained_updates_replace_draws_and_removal_is_view_and_owner_specific() {
        let mut world = World::new();
        let render = world.spawn_empty().id();
        let first = (render, world.spawn_empty().id().into());
        let second = (render, world.spawn_empty().id().into());
        let mut left = SortedRenderPhase::<Transparent3d>::default();
        let mut right = SortedRenderPhase::<Transparent3d>::default();
        for phase in [&mut left, &mut right] {
            phase.add_retained(draw(first).three_d(Vec3::ZERO, 0));
            phase.add_retained(draw(second).three_d(Vec3::ZERO, 0));
        }
        left.add_retained(
            PhaseDraw {
                draw_function: DrawFunctionId(3),
                indexed: true,
                ..draw(first)
            }
            .three_d(Vec3::new(0.0, 0.0, -2.0), 1),
        );
        assert_eq!(left.items.len(), 2);
        assert_eq!(left.items[&first].draw_function(), DrawFunctionId(3));
        assert!(left.items[&first].indexed());
        assert_eq!(right.items[&first].draw_function(), DrawFunctionId(0));
        assert!(!right.items[&first].indexed());

        let bounds = sprite_culling::Bounds {
            half_extents: Vec3::ZERO,
            maximum_size: 0.01,
            minimum_pixels: 2.0,
            world_from_effect: Mat4::from_translation(Vec3::new(100.0, 0.0, -10.0)),
        };
        let culling = sprite_culling::prepare(&view(), None, false).unwrap();
        assert!(reject_sampled_sprite(
            &mut left,
            Some(&bounds),
            Some(&culling),
            first.0,
            first.1
        ));
        assert!(!left.items.contains_key(&first));
        assert!(left.items.contains_key(&second));
        assert_eq!(right.items.len(), 2);
        left.add_retained(draw(first).three_d(Vec3::ZERO, 0));
        assert_eq!(left.items.len(), 2);
    }

    #[test]
    fn real_phase_sorting_recalculates_for_camera_changes_and_renderer_order() {
        let mut world = World::new();
        let near = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let far = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let mut phase = SortedRenderPhase::<Transparent3d>::default();
        phase.add_retained(draw(near).three_d(Vec3::new(0.0, 0.0, -2.0), 0));
        phase.add_retained(draw(far).three_d(Vec3::new(0.0, 0.0, -8.0), 0));
        let mut camera = view();
        phase.recalculate_sort_keys(&camera);
        phase.sort();
        assert_eq!(phase.iter_entities().collect::<Vec<_>>(), [far.0, near.0]);
        camera.world_from_view = GlobalTransform::from(Transform::from_rotation(
            Quat::from_rotation_y(std::f32::consts::PI),
        ));
        phase.recalculate_sort_keys(&camera);
        phase.sort();
        assert_eq!(phase.iter_entities().collect::<Vec<_>>(), [near.0, far.0]);

        phase.add_retained(draw(near).three_d(Vec3::ZERO, 2));
        phase.add_retained(draw(far).three_d(Vec3::ZERO, 1));
        phase.recalculate_sort_keys(&camera);
        phase.sort();
        assert_eq!(phase.iter_entities().collect::<Vec<_>>(), [far.0, near.0]);
        let mut two_d = SortedRenderPhase::<Transparent2d>::default();
        two_d.add_retained(draw(near).two_d(2));
        two_d.add_retained(draw(far).two_d(1));
        two_d.recalculate_sort_keys(&camera);
        two_d.sort();
        assert_eq!(two_d.iter_entities().collect::<Vec<_>>(), [far.0, near.0]);
    }

    #[test]
    fn visibility_combines_cpu_and_gpu_lists_without_other_classes_or_removed_draws() {
        let mut world = World::new();
        let cpu = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let gpu = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let removed = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let mut visible = RenderVisibleEntities::default();
        assert_eq!(visible_gpu_draws(&visible).count(), 0);
        visible.classes.insert(
            TypeId::of::<Transform>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![removed],
                ..default()
            },
        );
        assert_eq!(visible_gpu_draws(&visible).count(), 0);
        let mut class = RenderVisibleEntitiesClass {
            entities_cpu_culling: vec![cpu],
            removed_entities: vec![removed],
            ..default()
        };
        class.entities_gpu_culling.insert(gpu.1, gpu.0);
        visible
            .classes
            .insert(TypeId::of::<GpuDrawInstance>(), class);
        let actual = visible_gpu_draws(&visible).collect::<Vec<_>>();
        assert_eq!(actual.len(), 2);
        assert!(actual.contains(&cpu));
        assert!(actual.contains(&gpu));
        assert!(!actual.contains(&removed));
    }

    #[test]
    fn visibility_retirement_preserves_other_commands_and_exact_owner_identity() {
        let mut world = World::new();
        let render = world.spawn_empty().id();
        let live = (render, world.spawn_empty().id().into());
        let stale_owner = (render, world.spawn_empty().id().into());
        let unrelated = (world.spawn_empty().id(), world.spawn_empty().id().into());
        let mut phase = SortedRenderPhase::<Transparent2d>::default();
        for entity in [live, stale_owner] {
            phase.add_retained(draw(entity).two_d(0));
        }
        phase.add_retained(
            PhaseDraw {
                draw_function: DrawFunctionId(1),
                ..draw(unrelated)
            }
            .two_d(0),
        );
        let mut visible = RenderVisibleEntities::default();
        visible.classes.insert(
            TypeId::of::<GpuDrawInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![live],
                ..default()
            },
        );
        retire_invisible_draws(&mut phase, &visible, &[DrawFunctionId(0)]);
        assert!(phase.items.contains_key(&live));
        assert!(!phase.items.contains_key(&stale_owner));
        assert!(phase.items.contains_key(&unrelated));
        visible.classes.clear();
        retire_invisible_draws(&mut phase, &visible, &[DrawFunctionId(0)]);
        assert_eq!(phase.items.len(), 1);
        assert!(phase.items.contains_key(&unrelated));
    }

    #[test]
    fn sampled_sprite_culling_removes_retained_draws_and_allows_reentry_in_both_phases() {
        use bevy::render::view::RetainedViewEntity;
        let mut world = World::new();
        let render_entity = world.spawn_empty().id();
        let main_entity = MainEntity::from(world.spawn_empty().id());
        let extracted = ExtractedView {
            retained_view_entity: RetainedViewEntity::new(main_entity, None, 0),
            clip_from_view: PerspectiveProjection {
                fov: 1.0,
                aspect_ratio: 1.0,
                near: 0.1,
                ..default()
            }
            .get_clip_from_view(),
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: TextureFormat::Rgba16Float,
            viewport: UVec4::new(0, 0, 540, 540),
            color_grading: default(),
            invert_culling: false,
        };
        let culling = super::sprite_culling::prepare(&extracted, None, false).unwrap();
        let mut bounds = super::sprite_culling::Bounds {
            half_extents: Vec3::ZERO,
            maximum_size: 0.01,
            minimum_pixels: 2.0,
            world_from_effect: Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0)),
        };
        let mut phase2d = SortedRenderPhase::<Transparent2d>::default();
        let mut phase3d = SortedRenderPhase::<Transparent3d>::default();
        for x in [0.0, 100.0, 0.0] {
            bounds.world_from_effect.w_axis.x = x;
            let rejected2d = reject_sampled_sprite(
                &mut phase2d,
                Some(&bounds),
                Some(&culling),
                render_entity,
                main_entity,
            );
            let rejected3d = reject_sampled_sprite(
                &mut phase3d,
                Some(&bounds),
                Some(&culling),
                render_entity,
                main_entity,
            );
            assert_eq!(rejected2d, x == 100.0);
            assert_eq!(rejected3d, rejected2d);
            if rejected2d {
                assert_eq!(phase2d.iter_entities().count(), 0);
                assert_eq!(phase3d.iter_entities().count(), 0);
            } else {
                phase2d.add_retained(draw((render_entity, main_entity)).two_d(0));
                phase3d.add_retained(draw((render_entity, main_entity)).three_d(Vec3::ZERO, 0));
                assert_eq!(phase2d.iter_entities().count(), 1);
                assert_eq!(phase3d.iter_entities().count(), 1);
            }
        }
        // Unsupported bounds/cameras must retain entries rather than accidentally remove them.
        bounds.world_from_effect.w_axis.x = 100.0;
        assert!(!reject_sampled_sprite(
            &mut phase2d,
            None,
            Some(&culling),
            render_entity,
            main_entity
        ));
        assert!(!reject_sampled_sprite(
            &mut phase3d,
            Some(&bounds),
            None,
            render_entity,
            main_entity
        ));
        assert_eq!(phase2d.iter_entities().count(), 1);
        assert_eq!(phase3d.iter_entities().count(), 1);
    }

    #[test]
    fn three_dimensional_draw_selection_is_specific_to_each_view() {
        let mut world = World::new();
        let render_a = world.spawn_empty().id();
        let render_b = world.spawn_empty().id();
        let main_a = MainEntity::from(world.spawn_empty().id());
        let main_b = MainEntity::from(world.spawn_empty().id());
        let mut first_view = RenderVisibleEntities::default();
        first_view.classes.insert(
            TypeId::of::<GpuDrawInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![(render_a, main_a)],
                ..default()
            },
        );
        let mut second_view = RenderVisibleEntities::default();
        second_view.classes.insert(
            TypeId::of::<GpuDrawInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![(render_b, main_b)],
                ..default()
            },
        );

        assert_eq!(
            visible_gpu_draws(&first_view).collect::<Vec<_>>(),
            vec![(render_a, main_a)]
        );
        assert_eq!(
            visible_gpu_draws(&second_view).collect::<Vec<_>>(),
            vec![(render_b, main_b)]
        );
    }

    #[test]
    fn three_dimensional_draw_sorting_uses_world_center_and_renderer_order() {
        let rangefinder = ViewRangefinder3d::from_world_from_view(&Affine3A::IDENTITY);
        let near = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -2.0), 0);
        let far = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -8.0), 0);
        let first_renderer = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -4.0), 0);
        let second_renderer = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -4.0), 1);

        assert!(far.sort_distance(&rangefinder) < near.sort_distance(&rangefinder));
        assert!(
            first_renderer.sort_distance(&rangefinder)
                < second_renderer.sort_distance(&rangefinder)
        );
    }
}
