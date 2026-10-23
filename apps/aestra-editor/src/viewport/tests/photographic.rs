//! Actual profile/overlay consumers and change tracking without a native render pipeline.
use super::*;
use aestra_bevy_render::preview::{DisplayTransform, PhotographicPreview};
use bevy::{
    camera::Hdr,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    render::{render_resource::BlendState, view::ColorGrading},
};

#[derive(Resource, Default, Debug, PartialEq, Eq)]
struct Changes {
    profiles: usize,
    overlays: usize,
    grading: usize,
    cameras: usize,
    bloom: usize,
}
fn count_changes(
    mut changes: ResMut<Changes>,
    profiles: Query<Ref<AppliedPhotographicResponse>>,
    overlays: Query<Ref<AppliedOverlayComposition>>,
    grading: Query<Ref<ColorGrading>>,
    cameras: Query<Ref<Camera>>,
    bloom: Query<Ref<Bloom>>,
) {
    changes.profiles += profiles.iter().filter(|value| value.is_changed()).count();
    changes.overlays += overlays.iter().filter(|value| value.is_changed()).count();
    changes.grading += grading.iter().filter(|value| value.is_changed()).count();
    changes.cameras += cameras.iter().filter(|value| value.is_changed()).count();
    changes.bloom += bloom.iter().filter(|value| value.is_changed()).count();
}

struct Fixture {
    app: App,
    effect: Entity,
    overlay: Entity,
    ui: Entity,
}
impl Fixture {
    fn new(enabled: bool) -> Self {
        let mut app = App::new();
        let mut settings = EditorSettings::default();
        settings.preview.photographic_enabled = enabled;
        app.insert_resource(settings)
            .init_resource::<Changes>()
            .add_systems(
                Update,
                (
                    sync_preview_photographic_response,
                    sync_photographic_overlay_composition,
                )
                    .chain(),
            )
            .add_systems(Last, count_changes);
        let effect = app
            .world_mut()
            .spawn((Camera3d::default(), PreviewRenderCamera))
            .id();
        let overlay = app
            .world_mut()
            .spawn((Camera3d::default(), RenderLayers::layer(15)))
            .id();
        let ui = app.world_mut().spawn((Camera2d, IsDefaultUiCamera)).id();
        Self {
            app,
            effect,
            overlay,
            ui,
        }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn enabled(&mut self, enabled: bool) {
        self.app
            .world_mut()
            .resource_mut::<EditorSettings>()
            .preview
            .photographic_enabled = enabled;
    }
    fn profile(&mut self, profile: PhotographicPreview) {
        self.app
            .world_mut()
            .resource_mut::<EditorSettings>()
            .preview
            .photographic = profile;
    }
    fn reset_changes(&mut self) {
        *self.app.world_mut().resource_mut::<Changes>() = Changes::default();
    }
    fn unchanged(&self) {
        assert_eq!(*self.app.world().resource::<Changes>(), Changes::default());
    }
    fn applied(&self, entity: Entity, expected: PhotographicPreview) {
        let world = self.app.world();
        assert!(world.get::<Hdr>(entity).is_some());
        assert_eq!(
            world.get::<ColorGrading>(entity).unwrap().global.exposure,
            expected.exposure_stops
        );
        assert_eq!(
            *world.get::<Tonemapping>(entity).unwrap(),
            match expected.tonemapping {
                DisplayTransform::Tony => Tonemapping::TonyMcMapface,
                DisplayTransform::Aces => Tonemapping::AcesFitted,
                DisplayTransform::Reinhard => Tonemapping::Reinhard,
            }
        );
        assert_eq!(
            *world.get::<DebandDither>(entity).unwrap(),
            DebandDither::Disabled
        );
        assert_eq!(
            world.get::<Bloom>(entity).map(|bloom| bloom.intensity),
            (expected.bloom_intensity > 0.0).then_some(expected.bloom_intensity)
        );
        assert_eq!(
            world.get::<AppliedPhotographicResponse>(entity).unwrap().0,
            Some(expected)
        );
    }
    fn composed(&self, entity: Entity) {
        let camera = self.app.world().get::<Camera>(entity).unwrap();
        assert!(
            matches!(camera.clear_color, ClearColorConfig::Custom(color) if color == Color::NONE)
        );
        assert!(
            matches!(camera.output_mode, bevy::camera::CameraOutputMode::Write {
            blend_state: Some(blend), clear_color: ClearColorConfig::None,
        } if blend == BlendState::ALPHA_BLENDING)
        );
        assert!(
            self.app
                .world()
                .get::<AppliedOverlayComposition>(entity)
                .unwrap()
                .0
        );
    }
}

#[test]
fn fresh_disabled_cameras_keep_custom_baselines_and_only_gain_cache_markers() {
    let mut f = Fixture::new(false);
    let mut grading = ColorGrading::default();
    grading.global.exposure = 1.75;
    f.app.world_mut().entity_mut(f.effect).insert((
        Hdr,
        grading,
        Tonemapping::Reinhard,
        DebandDither::Disabled,
        Bloom {
            intensity: 0.23,
            ..Bloom::NATURAL
        },
    ));
    for entity in [f.overlay, f.ui] {
        let mut camera = f.app.world_mut().get_mut::<Camera>(entity).unwrap();
        camera.order = 17;
        camera.clear_color = ClearColorConfig::Custom(Color::srgb(0.2, 0.3, 0.4));
        camera.output_mode = bevy::camera::CameraOutputMode::Skip;
    }
    f.tick();
    assert!(f.app.world().get::<Hdr>(f.effect).is_some());
    assert_eq!(
        f.app
            .world()
            .get::<ColorGrading>(f.effect)
            .unwrap()
            .global
            .exposure,
        1.75
    );
    assert_eq!(
        *f.app.world().get::<Tonemapping>(f.effect).unwrap(),
        Tonemapping::Reinhard
    );
    assert_eq!(
        f.app.world().get::<Bloom>(f.effect).unwrap().intensity,
        0.23
    );
    assert!(
        f.app
            .world()
            .get::<AppliedPhotographicResponse>(f.effect)
            .unwrap()
            .0
            .is_none()
    );
    for entity in [f.overlay, f.ui] {
        let camera = f.app.world().get::<Camera>(entity).unwrap();
        assert_eq!(camera.order, 17);
        assert!(matches!(
            camera.output_mode,
            bevy::camera::CameraOutputMode::Skip
        ));
        assert!(
            matches!(camera.clear_color, ClearColorConfig::Custom(color) if color == Color::srgb(0.2, 0.3, 0.4))
        );
        assert!(
            !f.app
                .world()
                .get::<AppliedOverlayComposition>(entity)
                .unwrap()
                .0
        );
    }
}

#[test]
fn enabled_profile_reaches_only_tagged_effect_cameras_and_overlays_stay_ldr() {
    let mut f = Fixture::new(true);
    let profile = PhotographicPreview {
        exposure_stops: 2.0,
        bloom_intensity: 0.4,
        ..default()
    };
    f.profile(profile);
    f.tick();
    f.applied(f.effect, profile);
    for entity in [f.overlay, f.ui] {
        f.composed(entity);
        assert!(f.app.world().get::<Hdr>(entity).is_none());
        assert!(f.app.world().get::<Bloom>(entity).is_none());
        assert!(f.app.world().get::<ColorGrading>(entity).is_none());
    }
}

#[test]
fn live_edits_apply_each_display_transform_exposure_and_bloom_value() {
    let mut f = Fixture::new(true);
    for (tonemapping, exposure_stops, bloom_intensity) in [
        (DisplayTransform::Tony, -2.0, 0.15),
        (DisplayTransform::Aces, 1.25, 0.45),
        (DisplayTransform::Reinhard, 4.0, 0.8),
    ] {
        let profile = PhotographicPreview {
            tonemapping,
            exposure_stops,
            bloom_intensity,
        };
        f.profile(profile);
        f.tick();
        f.applied(f.effect, profile);
        f.composed(f.overlay);
        f.composed(f.ui);
    }
}

#[test]
fn zero_bloom_removes_only_bloom_and_later_positive_values_recreate_it() {
    let mut f = Fixture::new(true);
    for intensity in [0.5, 0.0, 0.2, 0.0] {
        let profile = PhotographicPreview {
            bloom_intensity: intensity,
            ..default()
        };
        f.profile(profile);
        f.tick();
        f.applied(f.effect, profile);
    }
}

#[test]
fn invalid_profile_values_are_normalized_without_mutating_the_settings_resource() {
    let mut f = Fixture::new(true);
    for raw in [
        PhotographicPreview {
            exposure_stops: f32::NAN,
            bloom_intensity: f32::INFINITY,
            ..default()
        },
        PhotographicPreview {
            exposure_stops: -100.0,
            bloom_intensity: 4.0,
            ..default()
        },
        PhotographicPreview {
            exposure_stops: 100.0,
            bloom_intensity: -1.0,
            ..default()
        },
    ] {
        f.profile(raw);
        f.tick();
        f.applied(f.effect, raw.normalized());
        let settings = f.app.world().resource::<EditorSettings>();
        assert_eq!(
            settings.preview.photographic.exposure_stops.to_bits(),
            raw.exposure_stops.to_bits()
        );
        assert_eq!(
            settings.preview.photographic.bloom_intensity.to_bits(),
            raw.bloom_intensity.to_bits()
        );
    }
}

#[test]
fn disabling_restores_legacy_defaults_rather_than_an_arbitrary_prior_profile() {
    let mut f = Fixture::new(true);
    f.profile(PhotographicPreview {
        exposure_stops: 3.0,
        ..default()
    });
    f.tick();
    f.enabled(false);
    f.tick();
    assert!(f.app.world().get::<Hdr>(f.effect).is_none());
    assert!(f.app.world().get::<Bloom>(f.effect).is_none());
    assert_eq!(
        f.app
            .world()
            .get::<ColorGrading>(f.effect)
            .unwrap()
            .global
            .exposure,
        0.0
    );
    assert_eq!(
        *f.app.world().get::<Tonemapping>(f.effect).unwrap(),
        Tonemapping::default()
    );
    assert_eq!(
        *f.app.world().get::<DebandDither>(f.effect).unwrap(),
        DebandDither::Enabled
    );
    assert!(
        f.app
            .world()
            .get::<AppliedPhotographicResponse>(f.effect)
            .unwrap()
            .0
            .is_none()
    );
    for entity in [f.overlay, f.ui] {
        let camera = f.app.world().get::<Camera>(entity).unwrap();
        assert!(matches!(camera.clear_color, ClearColorConfig::None));
        assert!(matches!(
            camera.output_mode,
            bevy::camera::CameraOutputMode::Write {
                blend_state: None,
                ..
            }
        ));
        assert!(
            !f.app
                .world()
                .get::<AppliedOverlayComposition>(entity)
                .unwrap()
                .0
        );
    }
}

#[test]
fn idle_enabled_updates_do_not_reinsert_components_or_rewrite_cameras() {
    let mut f = Fixture::new(true);
    f.tick();
    f.reset_changes();
    for _ in 0..3 {
        f.tick();
    }
    f.unchanged();
}

#[test]
fn idle_disabled_updates_and_unrelated_settings_changes_do_not_touch_cameras() {
    let mut f = Fixture::new(false);
    f.tick();
    f.reset_changes();
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .autosave_interval_seconds = 55;
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .photographic
        .exposure_stops = 2.5;
    for _ in 0..3 {
        f.tick();
    }
    f.unchanged();
}

#[test]
fn equivalent_normalized_profiles_share_the_existing_cache_without_reapplication() {
    let mut f = Fixture::new(true);
    f.profile(PhotographicPreview {
        exposure_stops: 100.0,
        bloom_intensity: 5.0,
        ..default()
    });
    f.tick();
    f.reset_changes();
    f.profile(PhotographicPreview {
        exposure_stops: 200.0,
        bloom_intensity: 10.0,
        ..default()
    });
    f.tick();
    f.unchanged();
    f.applied(
        f.effect,
        PhotographicPreview {
            exposure_stops: 8.0,
            bloom_intensity: 1.0,
            ..default()
        },
    );
}

#[test]
fn newly_created_effect_camera_receives_the_enabled_profile_without_refreshing_the_old_one() {
    let mut f = Fixture::new(true);
    let profile = PhotographicPreview {
        exposure_stops: 2.0,
        ..default()
    };
    f.profile(profile);
    f.tick();
    f.reset_changes();
    let recreated = f
        .app
        .world_mut()
        .spawn((Camera3d::default(), PreviewRenderCamera))
        .id();
    f.tick();
    f.applied(recreated, profile);
    f.applied(f.effect, profile);
    assert_eq!(f.app.world().resource::<Changes>().profiles, 1);
    assert_eq!(f.app.world().resource::<Changes>().overlays, 0);
}

#[test]
fn recreated_ui_and_gizmo_cameras_receive_transparent_composition_while_enabled() {
    let mut f = Fixture::new(true);
    f.tick();
    f.app.world_mut().despawn(f.overlay);
    f.app.world_mut().despawn(f.ui);
    f.reset_changes();
    f.overlay = f
        .app
        .world_mut()
        .spawn((Camera3d::default(), RenderLayers::layer(15)))
        .id();
    f.ui = f.app.world_mut().spawn((Camera2d, IsDefaultUiCamera)).id();
    f.tick();
    f.composed(f.overlay);
    f.composed(f.ui);
    assert_eq!(f.app.world().resource::<Changes>().profiles, 0);
    assert_eq!(f.app.world().resource::<Changes>().overlays, 2);
}

#[test]
fn exact_gizmo_layers_and_default_ui_marker_define_overlay_ownership() {
    let mut f = Fixture::new(true);
    let mixed = f
        .app
        .world_mut()
        .spawn((Camera3d::default(), RenderLayers::from_layers(&[15, 16])))
        .id();
    let ui_mixed = f
        .app
        .world_mut()
        .spawn((
            Camera2d,
            IsDefaultUiCamera,
            RenderLayers::from_layers(&[15, 16]),
        ))
        .id();
    let layer_2d = f
        .app
        .world_mut()
        .spawn((Camera2d, RenderLayers::layer(15)))
        .id();
    f.tick();
    assert!(
        f.app
            .world()
            .get::<AppliedOverlayComposition>(mixed)
            .is_none()
    );
    f.composed(ui_mixed);
    f.composed(layer_2d);
}

#[test]
fn effect_camera_marker_takes_priority_over_overlay_markers() {
    let mut f = Fixture::new(true);
    f.app
        .world_mut()
        .entity_mut(f.effect)
        .insert((IsDefaultUiCamera, RenderLayers::layer(15)));
    let color = Color::srgb(0.13, 0.27, 0.41);
    f.app
        .world_mut()
        .get_mut::<Camera>(f.effect)
        .unwrap()
        .clear_color = ClearColorConfig::Custom(color);
    f.tick();
    f.applied(f.effect, PhotographicPreview::default());
    assert!(
        f.app
            .world()
            .get::<AppliedOverlayComposition>(f.effect)
            .is_none()
    );
    let after = f.app.world().get::<Camera>(f.effect).unwrap().clear_color;
    assert!(matches!(after, ClearColorConfig::Custom(actual) if actual == color));
}

#[test]
fn unrelated_custom_camera_keeps_its_own_hdr_grading_bloom_and_output() {
    let mut f = Fixture::new(true);
    let mut grading = ColorGrading::default();
    grading.global.exposure = -1.5;
    let other = f
        .app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Hdr,
            grading,
            Tonemapping::Reinhard,
            Bloom {
                intensity: 0.17,
                ..Bloom::NATURAL
            },
            Camera {
                order: 42,
                output_mode: bevy::camera::CameraOutputMode::Skip,
                ..default()
            },
        ))
        .id();
    for enabled in [true, false, true] {
        f.enabled(enabled);
        f.tick();
        assert!(f.app.world().get::<Hdr>(other).is_some());
        assert_eq!(
            f.app
                .world()
                .get::<ColorGrading>(other)
                .unwrap()
                .global
                .exposure,
            -1.5
        );
        assert_eq!(f.app.world().get::<Bloom>(other).unwrap().intensity, 0.17);
        assert_eq!(
            *f.app.world().get::<Tonemapping>(other).unwrap(),
            Tonemapping::Reinhard
        );
        let camera = f.app.world().get::<Camera>(other).unwrap();
        assert_eq!(camera.order, 42);
        assert!(matches!(
            camera.output_mode,
            bevy::camera::CameraOutputMode::Skip
        ));
        assert!(
            f.app
                .world()
                .get::<AppliedPhotographicResponse>(other)
                .is_none()
        );
        assert!(
            f.app
                .world()
                .get::<AppliedOverlayComposition>(other)
                .is_none()
        );
    }
}

#[test]
fn removing_application_markers_allows_explicit_profile_and_overlay_reapplication() {
    let mut f = Fixture::new(true);
    f.tick();
    f.app
        .world_mut()
        .entity_mut(f.effect)
        .remove::<(AppliedPhotographicResponse, Hdr, ColorGrading)>();
    f.app
        .world_mut()
        .entity_mut(f.overlay)
        .remove::<AppliedOverlayComposition>();
    f.app
        .world_mut()
        .get_mut::<Camera>(f.overlay)
        .unwrap()
        .output_mode = bevy::camera::CameraOutputMode::Skip;
    f.tick();
    f.applied(f.effect, PhotographicPreview::default());
    f.composed(f.overlay);
}

#[test]
fn photographic_composition_cooperates_with_startup_and_delayed_viewport_activation() {
    use super::super::{lifecycle, shortcuts::PreviewCanvas};
    use bevy::{
        math::Affine2,
        window::{PrimaryWindow, WindowResolution},
    };
    let mut f = Fixture::new(true);
    f.app
        .add_systems(
            PostStartup,
            lifecycle::configure_transform_gizmo_overlay_camera,
        )
        .add_systems(PostUpdate, lifecycle::sync_preview_camera_viewport);
    let window = f
        .app
        .world_mut()
        .spawn((
            Window {
                resolution: WindowResolution::new(800, 600),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    f.app.world_mut().spawn((
        PreviewCanvas,
        ComputedNode {
            size: Vec2::new(400.0, 300.0),
            ..default()
        },
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(200.0, 150.0))),
    ));
    for (size, expected_active) in [
        (UVec2::new(800, 600), false),
        (UVec2::new(800, 600), true),
        (UVec2::ZERO, false),
        (UVec2::new(800, 600), false),
        (UVec2::new(800, 600), true),
    ] {
        f.app
            .world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(size.x, size.y);
        f.tick();
        f.applied(f.effect, PhotographicPreview::default());
        f.composed(f.overlay);
        f.composed(f.ui);
        for entity in [f.effect, f.overlay] {
            let camera = f.app.world().get::<Camera>(entity).unwrap();
            assert_eq!(camera.is_active, expected_active);
            let viewport = camera.viewport.as_ref().unwrap();
            assert_eq!(viewport.physical_position, UVec2::ZERO);
            assert_eq!(viewport.physical_size, UVec2::new(400, 300));
        }
    }
}

#[test]
fn profile_toggles_leave_the_effect_revisions_history_and_status_untouched() {
    let mut f = Fixture::new(false);
    let session = crate::test_support::session_with_timing_slack();
    let effect = session.effect.clone();
    let revision = session.document_revision();
    let ui_revision = session.ui_revision;
    let status = session.status.clone();
    let history = (session.effect_undo_len(), session.effect_redo_len());
    f.app.insert_resource(session);
    for enabled in [false, true, false, true] {
        f.enabled(enabled);
        f.tick();
        let session = f.app.world().resource::<crate::EditorSession>();
        assert_eq!(session.effect, effect);
        assert_eq!(session.document_revision(), revision);
        assert_eq!(session.ui_revision, ui_revision);
        assert_eq!(session.status, status);
        assert_eq!(
            (session.effect_undo_len(), session.effect_redo_len()),
            history
        );
        assert!(!session.dirty);
    }
}

#[test]
fn profile_consumers_remain_safe_with_no_camera_entities() {
    let mut app = App::new();
    app.init_resource::<EditorSettings>().add_systems(
        Update,
        (
            sync_preview_photographic_response,
            sync_photographic_overlay_composition,
        )
            .chain(),
    );
    app.update();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .photographic_enabled = true;
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&AppliedPhotographicResponse>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&AppliedOverlayComposition>()
            .iter(app.world())
            .count(),
        0
    );
}
