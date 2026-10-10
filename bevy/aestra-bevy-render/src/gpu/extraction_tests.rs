//! Identical lifecycle contracts run through the real 0.19 and 0.20 schedules.
use super::super::{
    draw_instance::{
        GpuDrawInstance, GpuRenderMode, GpuSemanticMaterialBinding, SpriteCullBounds,
        WireframeGeometry,
    },
    mesh_inputs::MeshInputs,
    world_sdf::AestraWorldSdf,
};
use super::{MainEntity, render_entity, test_app};
use bevy::{
    asset::{Asset, uuid::Uuid},
    camera::primitives::Aabb,
    prelude::*,
    render::{Render, RenderApp, RenderSystems},
};
use std::sync::Arc;

fn handle<A: Asset>(id: u128) -> Handle<A> {
    Handle::Uuid(Uuid::from_u128(id), Default::default())
}

pub(super) fn draw(owner: Entity) -> GpuDrawInstance {
    let program = aestra_core::material::MaterialProgram::additive_sprite("Extracted smoke");
    let ir = aestra_compiler::MaterialCompiler.compile(&program).unwrap();
    let program = Arc::new(
        aestra_gpu::material::MaterialShaderCompiler
            .compile(
                &ir,
                &aestra_gpu::material::MaterialBackendCapabilities::portable_minimum(),
            )
            .unwrap(),
    );
    GpuDrawInstance {
        sort_range: UVec2::new(7, 19),
        renderer_kind: 2,
        owner,
        mesh: Some(handle(1)),
        wireframe_geometry: Some(Arc::new(WireframeGeometry {
            vertices: vec![[0.25; 14]; 3],
            indices: vec![0, 1, 2],
            inputs: MeshInputs {
                uv1: true,
                tangent: true,
            },
            deformation_ready: true,
        })),
        renderers: handle(2),
        particles: handle(3),
        alive: handle(4),
        aux: handle(5),
        indirect: handle(6),
        render_globals: handle(7),
        render_params: handle(8),
        texture: handle(9),
        fallback_texture: handle(10),
        renderer_order: 11,
        emitter_index: 12,
        indirect_offset: 256,
        trail_instances: Some(39),
        trail_owners: 21,
        blend: aestra_gpu::GpuBlend::Alpha,
        material: aestra_core::MaterialId::from_u128(42),
        semantic_material: Some(GpuSemanticMaterialBinding {
            program,
            render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
            shader: handle(13),
            multisampled_shader: handle(14),
            uniforms: Arc::from([1, 2, 3, 4]),
            textures: vec![handle(15), handle(16)],
            fallback_texture: handle(17),
        }),
        render_mode: GpuRenderMode::Wireframe,
        mesh_center: Vec3::splat(-100.0),
        sampled_sprite_cull: Some(SpriteCullBounds {
            half_extents: Vec3::new(2.0, 3.0, 4.0),
            maximum_size: 5.0,
            world_from_effect: Mat4::IDENTITY,
            minimum_pixels: 1.5,
        }),
    }
}

fn spawn(app: &mut App, owner: Entity) -> Entity {
    app.world_mut()
        .spawn((
            draw(owner),
            ViewVisibility::VISIBLE,
            GlobalTransform::from(Transform::from_xyz(10.0, 20.0, 30.0)),
            Aabb {
                center: Vec3::new(1.0, 2.0, 3.0).into(),
                half_extents: Vec3::ONE.into(),
            },
        ))
        .id()
}

fn extracted(app: &App, main: Entity) -> Option<&GpuDrawInstance> {
    app.sub_app(RenderApp).world().get(render_entity(app, main))
}

fn assert_payload(source: &GpuDrawInstance, extracted: &GpuDrawInstance) {
    macro_rules! same { ($($field:ident),* $(,)?) => { $(assert_eq!(source.$field, extracted.$field, stringify!($field));)* }; }
    same!(
        sort_range,
        renderer_kind,
        owner,
        mesh,
        renderers,
        particles,
        alive,
        aux,
        indirect,
        render_globals,
        render_params,
        texture,
        fallback_texture,
        renderer_order,
        emitter_index,
        indirect_offset,
        trail_instances,
        trail_owners,
        blend,
        material,
        render_mode
    );
    let original = source.wireframe_geometry.as_ref().unwrap();
    let copied = extracted.wireframe_geometry.as_ref().unwrap();
    assert!(Arc::ptr_eq(original, copied));
    assert_eq!(copied.vertices, vec![[0.25; 14]; 3]);
    assert_eq!(copied.indices, vec![0, 1, 2]);
    assert!(copied.inputs.uv1 && copied.inputs.tangent && copied.deformation_ready);
    let original = source.semantic_material.as_ref().unwrap();
    let copied = extracted.semantic_material.as_ref().unwrap();
    assert!(Arc::ptr_eq(&original.program, &copied.program));
    assert!(Arc::ptr_eq(&original.uniforms, &copied.uniforms));
    assert_eq!(copied.uniforms.as_ref(), [1, 2, 3, 4]);
    assert_eq!(original.render_state, copied.render_state);
    assert_eq!(original.shader, copied.shader);
    assert_eq!(original.multisampled_shader, copied.multisampled_shader);
    assert_eq!(original.textures, copied.textures);
    assert_eq!(original.fallback_texture, copied.fallback_texture);
    let original = source.sampled_sprite_cull.unwrap();
    let copied = extracted.sampled_sprite_cull.unwrap();
    assert_eq!(original.half_extents, copied.half_extents);
    assert_eq!(original.maximum_size, copied.maximum_size);
    assert_eq!(original.minimum_pixels, copied.minimum_pixels);
}

#[derive(Resource, Default)]
struct PreparedCount(usize);

#[test]
fn actual_payload_and_shared_allocations_arrive_before_render_preparation() {
    let mut app = test_app();
    app.sub_app_mut(RenderApp)
        .init_resource::<PreparedCount>()
        .add_systems(
            Render,
            (|draws: Query<&GpuDrawInstance>, mut count: ResMut<PreparedCount>| {
                count.0 = draws.iter().count();
            })
            .in_set(RenderSystems::Prepare),
        );
    let owner = app.world_mut().spawn_empty().id();
    let entity = spawn(&mut app, owner);
    app.update();
    let copied = extracted(&app, entity).unwrap();
    assert_payload(app.world().get(entity).unwrap(), copied);
    assert_eq!(copied.mesh_center, Vec3::new(11.0, 22.0, 33.0));
    assert_eq!(
        copied.sampled_sprite_cull.unwrap().world_from_effect,
        Mat4::from_translation(Vec3::new(10.0, 20.0, 30.0))
    );
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .get::<MainEntity>(render_entity(&app, entity))
            .unwrap()
            .id(),
        entity
    );
    assert_eq!(
        app.sub_app(RenderApp).world().resource::<PreparedCount>().0,
        1
    );
    // Extraction updates a clone, not the main-world payload.
    assert_eq!(
        app.world()
            .get::<GpuDrawInstance>(entity)
            .unwrap()
            .mesh_center,
        Vec3::splat(-100.0)
    );
}

#[test]
fn hidden_draw_is_removed_then_restored_with_current_transform_and_bounds() {
    let mut app = test_app();
    let owner = app.world_mut().spawn_empty().id();
    let entity = spawn(&mut app, owner);
    app.update();
    let mapped = render_entity(&app, entity);
    assert!(extracted(&app, entity).is_some());
    app.world_mut()
        .entity_mut(entity)
        .insert(ViewVisibility::HIDDEN);
    app.update();
    assert!(extracted(&app, entity).is_none());
    app.world_mut().entity_mut(entity).insert((
        ViewVisibility::VISIBLE,
        GlobalTransform::from(Transform::from_xyz(4.0, 5.0, 6.0).with_scale(Vec3::splat(2.0))),
        Aabb {
            center: Vec3::new(3.0, 2.0, 1.0).into(),
            half_extents: Vec3::ONE.into(),
        },
    ));
    app.update();
    let copied = extracted(&app, entity).unwrap();
    assert_eq!(render_entity(&app, entity), mapped);
    assert_eq!(copied.mesh_center, Vec3::new(10.0, 9.0, 8.0));
    assert_eq!(
        copied.sampled_sprite_cull.unwrap().world_from_effect,
        Mat4::from(app.world().get::<GlobalTransform>(entity).unwrap().affine())
    );
}

#[test]
fn component_removal_and_reinsertion_do_not_cross_effect_owners() {
    let mut app = test_app();
    let owner = app.world_mut().spawn_empty().id();
    let other_owner = app.world_mut().spawn_empty().id();
    let first = spawn(&mut app, owner);
    let second = spawn(&mut app, other_owner);
    app.update();
    app.world_mut()
        .entity_mut(first)
        .remove::<GpuDrawInstance>();
    app.update();
    assert!(extracted(&app, first).is_none());
    assert_eq!(extracted(&app, second).unwrap().owner, other_owner);
    let mut replacement = draw(owner);
    replacement.render_mode = GpuRenderMode::Rendered;
    replacement.renderer_order = 99;
    replacement.mesh = None;
    replacement.wireframe_geometry = None;
    replacement.semantic_material = None;
    replacement.sampled_sprite_cull = None;
    replacement.trail_instances = None;
    app.world_mut().entity_mut(first).insert(replacement);
    app.update();
    let copied = extracted(&app, first).unwrap();
    assert_eq!(copied.render_mode, GpuRenderMode::Rendered);
    assert_eq!(copied.renderer_order, 99);
    assert!(
        copied.mesh.is_none()
            && copied.wireframe_geometry.is_none()
            && copied.semantic_material.is_none()
            && copied.sampled_sprite_cull.is_none()
            && copied.trail_instances.is_none()
    );
    assert_eq!(extracted(&app, second).unwrap().renderer_order, 11);
}

#[test]
fn despawn_and_recycled_entity_index_cannot_retain_old_draw_data() {
    let mut app = test_app();
    let owner = app.world_mut().spawn_empty().id();
    let entity = spawn(&mut app, owner);
    app.update();
    let mapped = render_entity(&app, entity);
    let payload = app.world().get::<GpuDrawInstance>(entity).unwrap().clone();
    app.world_mut().despawn(entity);
    // Bevy batches freed IDs; do not assume the very next spawn reuses an index.
    // Publish a batch and find the recycled index before the next extraction.
    let temporary: Vec<_> = (0..256)
        .map(|_| app.world_mut().spawn_empty().id())
        .collect();
    for temporary in temporary {
        app.world_mut().despawn(temporary);
    }
    let replacement = (0..512)
        .map(|_| app.world_mut().spawn_empty().id())
        .find(|candidate| candidate.index() == entity.index())
        .expect("the despawned index should be recycled within this bounded batch");
    assert_eq!(replacement.index(), entity.index());
    assert_ne!(replacement, entity);
    app.world_mut().entity_mut(replacement).insert((
        payload,
        ViewVisibility::VISIBLE,
        GlobalTransform::default(),
        Aabb::default(),
    ));
    app.world_mut()
        .get_mut::<GpuDrawInstance>(replacement)
        .unwrap()
        .renderer_order = 123;
    app.update();
    assert!(app.sub_app(RenderApp).world().get_entity(mapped).is_err());
    assert_eq!(extracted(&app, replacement).unwrap().renderer_order, 123);
    app.world_mut().despawn(replacement);
    app.update();
    let world = app.sub_app_mut(RenderApp).world_mut();
    assert_eq!(world.query::<&GpuDrawInstance>().iter(world).count(), 0);
}

fn sdf(value: f32) -> aestra_runtime::SdfVolume {
    aestra_runtime::SdfVolume {
        dims: [2; 3],
        origin: [1.0, 2.0, 3.0],
        voxel_size: 0.5,
        distances: vec![value; 8],
    }
}

#[test]
fn optional_sdf_updates_share_words_and_clear_publishes_absence() {
    let mut app = test_app();
    app.update();
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .contains_resource::<AestraWorldSdf>()
    );
    app.insert_resource(AestraWorldSdf::new(&sdf(2.0)));
    app.update();
    for value in [3.0, 4.0] {
        app.world_mut()
            .resource_mut::<AestraWorldSdf>()
            .set(&sdf(value));
        app.update();
        let original = app.world().resource::<AestraWorldSdf>();
        let copied = app.sub_app(RenderApp).world().resource::<AestraWorldSdf>();
        assert_eq!(original.revision(), copied.revision());
        assert!(Arc::ptr_eq(
            &original.packed().unwrap().words,
            &copied.packed().unwrap().words
        ));
        assert_eq!(copied.packed().unwrap().words[8], value.to_bits());
    }
    app.world_mut().resource_mut::<AestraWorldSdf>().clear();
    app.update();
    let copied = app.sub_app(RenderApp).world().resource::<AestraWorldSdf>();
    assert_eq!(copied.revision(), 4);
    assert_eq!(copied.packed().unwrap().revision, 4);
    assert_eq!(copied.packed().unwrap().words.as_ref(), [0; 8]);
}

#[test]
fn removing_sdf_resource_cleans_render_copy_and_reinsertion_is_extracted() {
    let mut app = test_app();
    app.insert_resource(AestraWorldSdf::new(&sdf(2.0)));
    app.update();
    app.world_mut().remove_resource::<AestraWorldSdf>();
    app.update();
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .contains_resource::<AestraWorldSdf>()
    );
    app.insert_resource(AestraWorldSdf::new(&sdf(9.0)));
    app.update();
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<AestraWorldSdf>()
            .packed()
            .unwrap()
            .words[8],
        9.0_f32.to_bits()
    );
    // Recover if only the render copy is absent and the source has not changed.
    app.sub_app_mut(RenderApp)
        .world_mut()
        .remove_resource::<AestraWorldSdf>();
    app.update();
    assert!(
        app.sub_app(RenderApp)
            .world()
            .contains_resource::<AestraWorldSdf>()
    );
}
