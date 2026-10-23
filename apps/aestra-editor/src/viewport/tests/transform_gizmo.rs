//! Original shipping TRS regressions plus transactional consumer edge cases on both engines.
use super::super::runtime_preview::compiled_effects_differ_only_in_live_edits;
use super::*;
use crate::test_support;
use aestra_core::EmitterId;

#[test]
fn preview_transform_gizmo_is_larger_and_easier_to_hit_than_the_bevy_default() {
    let defaults = TransformGizmoSettings::default();
    let preview = preview_transform_gizmo_settings();

    assert_eq!(
        preview.screen_scale_factor,
        PREVIEW_TRANSFORM_GIZMO_SCREEN_SCALE
    );
    assert_eq!(
        preview.axis_hit_distance,
        PREVIEW_TRANSFORM_GIZMO_HIT_DISTANCE
    );
    assert!(preview.screen_scale_factor > defaults.screen_scale_factor);
    assert!(preview.axis_hit_distance > defaults.axis_hit_distance);
}

#[test]
fn transform_snap_rounds_absolute_values_instead_of_drag_deltas() {
    let transform = EmitterTransform {
        translation: [0.695_576, 2.4, -3.6],
        ..default()
    };

    let snapped = snap_transform_absolute(
        TransformGizmoMode::Translate,
        Some(TransformGizmoAxis::X),
        transform,
        false,
    );

    assert_eq!(snapped.translation, [1.0, 2.4, -3.6]);
}

#[test]
fn precise_transform_snap_uses_the_finer_absolute_grid() {
    let transform = EmitterTransform {
        translation: [0.695_576, 0.0, 0.0],
        ..default()
    };

    let snapped = snap_transform_absolute(
        TransformGizmoMode::Translate,
        Some(TransformGizmoAxis::X),
        transform,
        true,
    );

    assert_eq!(snapped.translation, [0.7, 0.0, 0.0]);
}

#[test]
fn precision_drag_scales_motion_without_changing_unedited_components() {
    let original = EmitterTransform::default();
    let mut raw = original;
    raw.translation = [10.0, -5.0, 2.0];
    let precise = transform_with_drag_precision(
        TransformGizmoMode::Translate,
        original,
        original,
        raw,
        PREVIEW_TRANSFORM_PRECISION_FACTOR,
    );

    assert_eq!(precise.translation, [1.0, -0.5, 0.2]);
    assert_eq!(precise.rotation, original.rotation);
    assert_eq!(precise.scale, original.scale);
}

#[test]
fn precision_reanchor_keeps_modifier_transitions_continuous() {
    let raw_anchor = EmitterTransform {
        translation: [10.0, 0.0, 0.0],
        ..default()
    };
    let output_anchor = EmitterTransform {
        translation: [4.0, 0.0, 0.0],
        ..default()
    };
    let raw_current = EmitterTransform {
        translation: [20.0, 0.0, 0.0],
        ..default()
    };

    let precise = transform_with_drag_precision(
        TransformGizmoMode::Translate,
        raw_anchor,
        output_anchor,
        raw_current,
        PREVIEW_TRANSFORM_PRECISION_FACTOR,
    );

    assert_eq!(precise.translation, [5.0, 0.0, 0.0]);
}

#[test]
fn gizmo_value_readout_exposes_active_modifiers() {
    let current = EmitterTransform {
        translation: [2.123_456, 3.0, 4.0],
        ..default()
    };

    assert_eq!(
        transform_gizmo_value_text(
            TransformGizmoMode::Translate,
            Some(TransformGizmoAxis::X),
            current,
            false,
            true,
        ),
        "MOVE X  +2.1235  \u{00b7} PRECISE"
    );
}

#[test]
fn transform_gizmo_materials_render_after_transparent_particles() {
    let mut app = App::new();
    app.init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, configure_transform_gizmo_overlay_materials);
    let overlay_material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let scene_material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let overlay_root = app.world_mut().spawn_empty().id();
    let scene_root = app.world_mut().spawn_empty().id();
    app.world_mut().spawn((
        RenderLayers::layer(15),
        MeshMaterial3d(overlay_material.clone()),
        ChildOf(overlay_root),
    ));
    app.world_mut().spawn((
        RenderLayers::layer(0),
        MeshMaterial3d(scene_material.clone()),
        ChildOf(scene_root),
    ));

    app.update();

    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let overlay = materials.get(&overlay_material).unwrap();
    assert_eq!(overlay.alpha_mode, AlphaMode::Blend);
    assert_eq!(overlay.depth_bias, 10_000.0);
    let scene = materials.get(&scene_material).unwrap();
    assert_eq!(scene.alpha_mode, AlphaMode::Opaque);
    assert_eq!(scene.depth_bias, 0.0);
    assert!(
        app.world()
            .entity(overlay_root)
            .contains::<TransformGizmoVisualRoot>()
    );
    assert!(
        !app.world()
            .entity(scene_root)
            .contains::<TransformGizmoVisualRoot>()
    );
}

#[test]
fn transform_gizmo_scale_feedback_stretches_only_the_dragged_axis() {
    let original = Transform::from_scale(Vec3::new(2.0, 3.0, 4.0));
    let current = Transform::from_scale(Vec3::new(5.0, 3.0, 4.0));

    let visual = transform_gizmo_drag_visual(
        TransformGizmoMode::Scale,
        TransformGizmoSpace::World,
        Some(TransformGizmoAxis::X),
        original,
        current,
    );

    assert_eq!(visual.scale, Vec3::new(2.5, 1.0, 1.0));
    assert!(visual.rotation.is_none());
}

#[test]
fn transform_gizmo_rotation_feedback_uses_the_live_drag_delta() {
    let original = Transform::from_rotation(Quat::from_rotation_z(30.0_f32.to_radians()));
    let current = Transform::from_rotation(Quat::from_rotation_z(75.0_f32.to_radians()));

    let visual = transform_gizmo_drag_visual(
        TransformGizmoMode::Rotate,
        TransformGizmoSpace::World,
        Some(TransformGizmoAxis::Z),
        original,
        current,
    );

    let rotation = visual.rotation.unwrap();
    assert!(rotation.angle_between(Quat::from_rotation_z(45.0_f32.to_radians())) < 0.0001);
    assert_eq!(visual.scale, Vec3::ONE);
}

#[test]
fn empty_effect_disables_emitter_gizmo_but_keeps_clip_transforms() {
    let session = EditorSession::from_test_effect(aestra_core::EffectAsset::new("Empty", 4.0));
    assert!(selected_preview_transform(&session, None).is_none());
    let mut app = App::new();
    app.insert_resource(session)
        .init_resource::<ShapeGizmoState>()
        .init_resource::<EmitterTransformGizmoInteraction>()
        .add_systems(Update, sync_transform_gizmo_focus);
    let proxy = app
        .world_mut()
        .spawn((EmitterTransformGizmoProxy, TransformGizmoFocus))
        .id();
    app.update();
    assert!(app.world().get::<TransformGizmoFocus>(proxy).is_none());
    let mut clip = aestra_core::EffectClip::new(aestra_core::EffectId::new(), 0.0, 1.0);
    clip.transform.translation = [1.0, 2.0, 3.0];
    let id = clip.id;
    let mut session = app.world_mut().resource_mut::<EditorSession>();
    session.effect.effect_clips.push(clip);
    session.selection.select_effect_clip(id);
    let (target, transform) = selected_preview_transform(&session, None).unwrap();
    assert!(matches!(target, PreviewTransformTarget::EffectClip(selected) if selected == id));
    assert_eq!(transform.translation, [1.0, 2.0, 3.0]);
    app.update();
    assert!(app.world().get::<TransformGizmoFocus>(proxy).is_some());
}

#[test]
fn a_selected_fluid_source_is_moved_by_the_gizmo_without_restarting_the_fluid() {
    aestra_fluid::link();
    let registry = aestra_compiler::ExtensionRegistry::linked();
    let mut session = EditorSession::from_test_effect(aestra_fluid::fire_effect(&registry));
    let source = session.effect.simulation_stages[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_fluid::MODULE_DENSITY_SOURCE)
        .unwrap()
        .id;
    session.selection.primary = SemanticTarget::Module(source);
    let (target, transform) =
        selected_preview_transform(&session, Some(&registry.modules)).unwrap();
    assert_eq!(
        target,
        PreviewTransformTarget::ModuleHandle(source, "position")
    );
    assert_eq!(transform.translation, [0.0, 15.0, 0.0]);

    // Dragging edits the domain's source through the effect scope, as one live preview.
    let before = session.preview().unwrap().effect().clone();
    let moved = EmitterTransform {
        translation: [20.0, 10.0, 0.0],
        ..default()
    };
    let command = preview_transform_command(target, moved, &session).unwrap();
    assert!(matches!(
        &command,
        EffectCommand::SetModuleParameter { emitter, parameter, .. }
            if *emitter == EmitterId::EFFECT_SCOPE && parameter == "position"
    ));
    assert!(session.preview_interaction(EffectTransaction::single("Preview transform", command)));
    let after = session.preview().unwrap().effect();
    assert_ne!(
        before.extension_stages[0].block.constants,
        after.extension_stages[0].block.constants
    );
    assert!(
        compiled_effects_differ_only_in_live_edits(&before, after),
        "the player is swapped in place, so the running fluid is kept"
    );

    // A position a host object drives offers no handle.
    let module = session.effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.id == source)
        .unwrap();
    module
        .property_sources
        .insert("position".into(), aestra_core::PropertySource::HostBinding);
    assert!(!matches!(
        selected_preview_transform(&session, Some(&registry.modules)),
        Some((PreviewTransformTarget::ModuleHandle(..), _))
    ));
}

#[test]
fn selecting_shape_module_keeps_root_transform_gizmo_available() {
    let mut session = test_support::session_with_timing_slack();
    let shape_module = session
        .selected_layer()
        .unwrap()
        .module_by_type(aestra_core::MODULE_SHAPE)
        .unwrap()
        .id;
    session.selection.primary = SemanticTarget::Module(shape_module);
    let mut app = App::new();
    app.insert_resource(session)
        .init_resource::<ShapeGizmoState>()
        .init_resource::<EmitterTransformGizmoInteraction>()
        .add_systems(Update, sync_transform_gizmo_focus);
    let player = app.world_mut().spawn(EmitterTransformGizmoProxy).id();

    app.update();

    assert!(app.world().entity(player).contains::<TransformGizmoFocus>());
}

#[test]
fn emitter_gizmo_previews_then_commits_one_undoable_transform() {
    let session = test_support::session_with_timing_slack();
    let gizmo = TransformGizmoState {
        active: true,
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(gizmo)
        .insert_resource(preview_transform_gizmo_settings())
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EmitterTransformGizmoInteraction>()
        .add_systems(Update, update_emitter_transform_gizmo);
    app.world_mut().spawn((
        EmitterTransformGizmoProxy,
        Transform::from_xyz(4.0, 5.0, 6.0),
    ));

    app.update();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(
        session.selected_layer().unwrap().transform,
        EmitterTransform::default()
    );
    assert_eq!(
        session.preview().unwrap().effect().emitters[0]
            .transform
            .translation,
        [4.0, 5.0, 6.0],
        "drag preview must update the compiled effect before release"
    );

    app.world_mut().resource_mut::<TransformGizmoState>().active = false;
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(
        session.selected_layer().unwrap().transform.translation,
        [4.0, 5.0, 6.0]
    );

    app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .selected_layer()
            .unwrap()
            .transform,
        EmitterTransform::default()
    );
}

fn drag_app() -> (App, Entity) {
    let mut a = App::new();
    a.insert_resource(test_support::session_with_timing_slack())
        .insert_resource(TransformGizmoState {
            active: true,
            ..default()
        })
        .insert_resource(preview_transform_gizmo_settings())
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EmitterTransformGizmoInteraction>()
        .add_systems(Update, update_emitter_transform_gizmo);
    let e = a
        .world_mut()
        .spawn((EmitterTransformGizmoProxy, Transform::IDENTITY))
        .id();
    (a, e)
}
fn move_proxy(a: &mut App, e: Entity, x: f32) {
    a.world_mut()
        .entity_mut(e)
        .insert(Transform::from_xyz(x, 0.0, 0.0));
    a.update();
}
#[test]
fn escape_restores_emitter_preview_and_late_release_cannot_commit() {
    let (mut a, e) = drag_app();
    move_proxy(&mut a, e, 9.0);
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    a.update();
    assert!(
        !a.world()
            .resource::<EmitterTransformGizmoInteraction>()
            .is_active()
    );
    assert!(
        a.world()
            .resource::<EmitterTransformGizmoInteraction>()
            .cancelled
    );
    a.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    move_proxy(&mut a, e, 12.0);
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let s = a.world().resource::<EditorSession>();
    assert!(!s.can_undo());
    assert!(!s.dirty);
    assert_eq!(
        s.selected_layer().unwrap().transform,
        EmitterTransform::default()
    );
    assert_eq!(
        s.preview().unwrap().effect().emitters[0].transform,
        EmitterTransform::default()
    );
    assert_eq!(*a.world().get::<Transform>(e).unwrap(), Transform::IDENTITY);
    assert!(
        !a.world()
            .resource::<EmitterTransformGizmoInteraction>()
            .cancelled
    );
}
#[test]
fn changing_document_revision_cancels_without_overwriting_a_new_edit() {
    let (mut a, e) = drag_app();
    move_proxy(&mut a, e, 9.0);
    assert!(a.world_mut().resource_mut::<EditorSession>().execute(
        "Rename",
        EffectCommand::SetEffectName {
            name: "New name".into()
        },
        true
    ));
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let mut s = a.world_mut().resource_mut::<EditorSession>();
    assert_eq!(s.effect.name, "New name");
    assert_eq!(
        s.selected_layer().unwrap().transform,
        EmitterTransform::default()
    );
    s.undo();
    assert!(!s.can_undo());
}
#[test]
fn selection_change_cancels_without_transforming_the_new_emitter() {
    let (mut a, e) = drag_app();
    a.world_mut()
        .resource_mut::<EditorSession>()
        .duplicate_selected_layer();
    a.world_mut().resource_mut::<EditorSession>().dirty = false;
    let before = a.world().resource::<EditorSession>().effect.clone();
    let undo = a.world().resource::<EditorSession>().effect_undo_len();
    move_proxy(&mut a, e, 9.0);
    let other = before.emitters[0].id;
    a.world_mut()
        .resource_mut::<EditorSession>()
        .selection
        .primary = SemanticTarget::Emitter(other);
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let s = a.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert_eq!(s.effect_undo_len(), undo);
    assert!(!s.dirty);
}
#[test]
fn locked_effect_cancels_active_drag_without_creating_history() {
    let (mut a, e) = drag_app();
    move_proxy(&mut a, e, 4.0);
    let mut s = a.world_mut().resource_mut::<EditorSession>();
    let id = s.effect.id;
    s.locks.lock(SemanticTarget::Effect(id));
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let s = a.world().resource::<EditorSession>();
    assert!(!s.can_undo());
    assert_eq!(
        s.selected_layer().unwrap().transform,
        EmitterTransform::default()
    );
}
#[test]
fn pending_transaction_cancels_drag_and_keeps_candidate_unapplied() {
    let (mut a, e) = drag_app();
    move_proxy(&mut a, e, 4.0);
    assert!(
        a.world_mut()
            .resource_mut::<EditorSession>()
            .preview_transaction(EffectTransaction::single(
                "Review",
                EffectCommand::SetEffectName {
                    name: "Pending".into()
                },
            ))
    );
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let s = a.world().resource::<EditorSession>();
    assert!(s.pending_change.is_some());
    assert!(!s.can_undo());
    assert_ne!(s.effect.name, "Pending");
    assert_eq!(
        s.selected_layer().unwrap().transform,
        EmitterTransform::default()
    );
}
#[test]
fn no_motion_drag_has_no_undo_step_and_multiple_proxies_fail_closed() {
    let (mut a, e) = drag_app();
    a.update();
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    assert!(!a.world().resource::<EditorSession>().can_undo());
    a.world_mut().resource_mut::<TransformGizmoState>().active = true;
    a.world_mut().spawn((
        EmitterTransformGizmoProxy,
        Transform::from_xyz(5.0, 0.0, 0.0),
    ));
    move_proxy(&mut a, e, 8.0);
    assert!(
        !a.world()
            .resource::<EmitterTransformGizmoInteraction>()
            .is_active()
    );
    assert!(!a.world().resource::<EditorSession>().can_undo());
}
#[test]
fn right_modifiers_snap_and_reanchor_without_changing_authored_values_until_release() {
    let (mut a, e) = drag_app();
    a.world_mut().resource_mut::<TransformGizmoState>().axis = Some(TransformGizmoAxis::X);
    move_proxy(&mut a, e, 4.0);
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftRight);
    move_proxy(&mut a, e, 4.0);
    move_proxy(&mut a, e, 5.0);
    assert!((a.world().get::<Transform>(e).unwrap().translation.x - 4.1).abs() < 1e-5);
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    move_proxy(&mut a, e, 5.25);
    assert!((a.world().get::<Transform>(e).unwrap().translation.x - 4.1).abs() < 1e-5);
    assert_eq!(
        a.world()
            .resource::<EditorSession>()
            .selected_layer()
            .unwrap()
            .transform
            .translation,
        [0.0; 3]
    );
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let s = a.world().resource::<EditorSession>();
    assert_eq!(s.effect_undo_len(), 1);
    assert!((s.selected_layer().unwrap().transform.translation[0] - 4.1).abs() < 1e-5);
}
#[test]
fn proxy_sync_preserves_active_drag_and_updates_when_idle() {
    let (mut a, e) = drag_app();
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    let id = a
        .world()
        .resource::<EditorSession>()
        .selected_layer()
        .unwrap()
        .id;
    a.world_mut().resource_mut::<EditorSession>().execute(
        "Move",
        EffectCommand::SetEmitterTransform {
            id,
            transform: EmitterTransform {
                translation: [3.0, 2.0, 1.0],
                ..default()
            },
        },
        true,
    );
    a.world_mut()
        .run_system_cached(sync_emitter_transform_proxy)
        .unwrap();
    assert_eq!(
        a.world().get::<Transform>(e).unwrap().translation,
        Vec3::new(3.0, 2.0, 1.0)
    );
    a.world_mut().resource_mut::<TransformGizmoState>().active = true;
    a.world_mut()
        .entity_mut(e)
        .insert(Transform::from_xyz(8.0, 0.0, 0.0));
    a.world_mut()
        .run_system_cached(sync_emitter_transform_proxy)
        .unwrap();
    assert_eq!(a.world().get::<Transform>(e).unwrap().translation.x, 8.0);
}
#[test]
fn every_document_modal_removes_focus_and_idle_proxy_recovers() {
    for protection in crate::persistence::DocumentProtectionState::blocking_states_for_test() {
        let (mut a, e) = drag_app();
        a.init_resource::<ShapeGizmoState>()
            .insert_resource(protection);
        a.world_mut().entity_mut(e).insert(TransformGizmoFocus);
        a.world_mut()
            .run_system_cached(sync_transform_gizmo_focus)
            .unwrap();
        assert!(a.world().get::<TransformGizmoFocus>(e).is_none());
        a.world_mut()
            .remove_resource::<crate::persistence::DocumentProtectionState>();
        a.world_mut()
            .run_system_cached(sync_transform_gizmo_focus)
            .unwrap();
        assert!(a.world().get::<TransformGizmoFocus>(e).is_some());
    }
}
#[test]
fn hovered_host_pose_and_shape_handle_own_focus_before_trs() {
    let (mut a, e) = drag_app();
    a.init_resource::<ShapeGizmoState>()
        .init_resource::<host_motion::HostMotionViewport>();
    a.world_mut()
        .run_system_cached(sync_transform_gizmo_focus)
        .unwrap();
    assert!(a.world().get::<TransformGizmoFocus>(e).is_some());
    a.world_mut()
        .resource_mut::<host_motion::HostMotionViewport>()
        .hovered = Some(1);
    a.world_mut()
        .run_system_cached(sync_transform_gizmo_focus)
        .unwrap();
    assert!(a.world().get::<TransformGizmoFocus>(e).is_none());
    a.world_mut()
        .resource_mut::<host_motion::HostMotionViewport>()
        .hovered = None;
    a.world_mut().resource_mut::<ShapeGizmoState>().hovered = Some(ShapeGizmoHandle::Radius);
    a.world_mut()
        .run_system_cached(sync_transform_gizmo_focus)
        .unwrap();
    assert!(a.world().get::<TransformGizmoFocus>(e).is_none());
}
#[test]
fn effect_clip_drag_commits_only_its_transform_and_undo_restores_it() {
    let (mut a, e) = drag_app();
    let clip = aestra_core::EffectClip::new(aestra_core::EffectId::new(), 0.0, 1.0);
    let clip_id = clip.id;
    let mut s = a.world_mut().resource_mut::<EditorSession>();
    s.effect.effect_clips.push(clip);
    s.selection.select_effect_clip(clip_id);
    s.restore_interaction_preview();
    move_proxy(&mut a, e, 7.0);
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.update();
    let mut s = a.world_mut().resource_mut::<EditorSession>();
    assert_eq!(
        s.effect.effect_clips[0].transform.translation,
        [7.0, 0.0, 0.0]
    );
    assert_eq!(s.effect.emitters[0].transform, EmitterTransform::default());
    assert_eq!(s.effect_undo_len(), 1);
    s.undo();
    assert_eq!(
        s.effect.effect_clips[0].transform,
        EmitterTransform::default()
    );
}
#[test]
fn overlay_offset_and_trs_conversion_remain_bounded() {
    assert_eq!(overlay_offset(900.0, 500.0, 200.0), 300.0);
    assert_eq!(overlay_offset(0.0, 20.0, 200.0), 8.0);
    let t = Transform {
        translation: Vec3::new(1.0, 2.0, 3.0),
        rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 2.0),
        scale: Vec3::new(0.0, -2.0, 3.0),
    };
    let converted = emitter_transform_from_bevy(&t);
    assert_eq!(converted.rotation, Quat::IDENTITY.to_array());
    assert_eq!(converted.scale, [0.001, 0.001, 3.0]);
    assert_eq!(
        bevy_transform_from_emitter(converted).translation,
        t.translation
    );
}

#[test]
fn live_value_label_tracks_cursor_modifiers_and_hides_after_release() {
    use bevy::{math::Affine2, window::WindowResolution};
    let (mut a, e) = drag_app();
    move_proxy(&mut a, e, 2.25);
    a.world_mut().resource_mut::<TransformGizmoState>().axis = Some(TransformGizmoAxis::X);
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    let mut window = Window {
        resolution: WindowResolution::new(800, 600),
        ..default()
    };
    window.set_cursor_position(Some(Vec2::new(490.0, 390.0)));
    a.world_mut().spawn((window, PrimaryWindow));
    a.world_mut().spawn((
        PreviewCanvas,
        ComputedNode {
            size: Vec2::new(400.0, 300.0),
            ..default()
        },
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(300.0, 250.0))),
    ));
    let label = a
        .world_mut()
        .spawn((
            TransformGizmoValueLabel,
            Text::new("stale"),
            Node::default(),
            Visibility::Hidden,
        ))
        .id();
    a.world_mut()
        .run_system_cached(update_transform_gizmo_value_label)
        .unwrap();
    assert_eq!(
        a.world().get::<Text>(label).unwrap().0,
        "MOVE X  +2.2500  · PRECISE"
    );
    let node = a.world().get::<Node>(label).unwrap();
    assert_eq!(node.left, Val::Px(200.0));
    assert_eq!(node.top, Val::Px(268.0));
    assert_eq!(
        *a.world().get::<Visibility>(label).unwrap(),
        Visibility::Inherited
    );
    a.world_mut().resource_mut::<TransformGizmoState>().active = false;
    a.world_mut()
        .run_system_cached(update_transform_gizmo_value_label)
        .unwrap();
    assert_eq!(
        *a.world().get::<Visibility>(label).unwrap(),
        Visibility::Hidden
    );
}

#[test]
fn drag_feedback_updates_only_tagged_visual_roots_and_their_children() {
    let (mut a, e) = drag_app();
    a.world_mut().resource_mut::<TransformGizmoSettings>().mode = TransformGizmoMode::Scale;
    a.world_mut().resource_mut::<TransformGizmoState>().axis = Some(TransformGizmoAxis::X);
    a.update();
    a.world_mut()
        .entity_mut(e)
        .insert(Transform::from_scale(Vec3::new(2.5, 1.0, 1.0)));
    a.update();
    let root = a
        .world_mut()
        .spawn((
            TransformGizmoVisualRoot,
            Transform::from_xyz(3.0, 0.0, 0.0),
            GlobalTransform::IDENTITY,
        ))
        .id();
    let child = a
        .world_mut()
        .spawn((
            Mesh3d::default(),
            ChildOf(root),
            Transform::from_xyz(2.0, 0.0, 0.0),
            GlobalTransform::IDENTITY,
        ))
        .id();
    let other = a
        .world_mut()
        .spawn((
            Mesh3d::default(),
            Transform::from_xyz(99.0, 0.0, 0.0),
            GlobalTransform::IDENTITY,
        ))
        .id();
    a.world_mut()
        .run_system_cached(apply_transform_gizmo_drag_feedback)
        .unwrap();
    assert_eq!(
        a.world().get::<Transform>(root).unwrap().scale,
        Vec3::new(2.5, 1.0, 1.0)
    );
    assert_eq!(
        a.world()
            .get::<GlobalTransform>(child)
            .unwrap()
            .translation(),
        Vec3::new(8.0, 0.0, 0.0)
    );
    assert_eq!(
        *a.world().get::<GlobalTransform>(other).unwrap(),
        GlobalTransform::IDENTITY
    );
}
