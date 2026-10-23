//! Shared production ingestion with explicit backend, samples and unavailable observations.
use super::super::runtime_preview::configured_preview_player;
use super::*;
use crate::test_support;
use aestra_bevy_render::{ActiveBackend, EffectRuntimeStatus};
use aestra_runtime::{CompiledEffectProject, EffectProfile, ProfileValue};

pub(crate) struct Fixture {
    pub(crate) app: App,
    pub(crate) root: Option<Entity>,
}
impl Fixture {
    pub(crate) fn new(backend: Option<ActiveBackend>) -> Self {
        Self::from_session(test_support::session_with_timing_slack(), backend)
    }
    pub(crate) fn from_session(mut session: EditorSession, backend: Option<ActiveBackend>) -> Self {
        session.seek_time(1.0);
        let player = configured_preview_player(&session).unwrap();
        let compiled = player.effect().clone();
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ProfilerState>()
            .init_resource::<TimelineState>()
            .insert_resource(EditorPreviewProject {
                source_root: Some(compiled.clone()),
                project: Some(Arc::new(CompiledEffectProject {
                    root: compiled,
                    dependencies: default(),
                })),
                ..default()
            })
            .add_systems(Update, update_preview);
        let root = backend.map(|backend| {
            app.world_mut()
                .spawn((
                    PreviewPresentedEffect,
                    PreviewEffectInstancePath::default(),
                    player,
                    status(backend),
                ))
                .id()
        });
        Self { app, root }
    }
    pub(crate) fn tick(&mut self) {
        self.app.update();
    }
    pub(crate) fn profile(&self) -> &EffectProfile {
        self.app
            .world()
            .resource::<ProfilerState>()
            .current_profile()
            .unwrap()
    }
}
pub(crate) fn status(active: ActiveBackend) -> EffectRuntimeStatus {
    EffectRuntimeStatus {
        active,
        reason: "Explicit headless test".into(),
        compatibility: aestra_runtime::CompatibilityReport::compatible(
            aestra_runtime::CompatibilityTarget::NativeGpu,
        ),
    }
}

#[test]
fn headless_reference_fallback_measures_cpu_counts_without_gpu_estimates() {
    let mut f = Fixture::new(None);
    f.tick();
    assert!(f.profile().cpu_time_ns.value().is_some());
    assert!(matches!(f.profile().cpu_time_ns, ProfileValue::Measured(_)));
    assert!(f.profile().alive_particles.value().unwrap() > 0);
    assert_eq!(
        f.profile().gpu_simulation_time_ns,
        ProfileValue::Unavailable
    );
    // No extension stages means a known-zero stage workload, not a missing timer.
    assert_eq!(f.profile().gpu_stage_time_ns, ProfileValue::Measured(0));
    assert_eq!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        f.app.world().resource::<EditorSession>().samples.len()
    );
}

#[test]
fn native_gpu_without_observations_never_evaluates_reference_particles() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    f.tick();
    let p = f.profile();
    for metric in [p.cpu_time_ns, p.gpu_time_ns, p.gpu_simulation_time_ns] {
        assert_eq!(metric, ProfileValue::Unavailable);
    }
    assert_eq!(p.alive_particles, ProfileValue::Unavailable);
    // Project aggregation preserves its initial known-zero peak as an estimate
    // when the current count is unavailable; it is not a measured live count.
    assert_eq!(p.peak_particles, ProfileValue::Estimated(0));
    assert_eq!(p.submitted_instances, ProfileValue::Unavailable);
    assert!(f.app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        0
    );
}

#[test]
fn default_observations_remain_unknown_and_do_not_manufacture_measurements() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    f.app.world_mut().entity_mut(f.root.unwrap()).insert((
        aestra_bevy_render::gpu::GpuTrailStatistics::default(),
        aestra_bevy_render::gpu::GpuStageTiming::default(),
        aestra_bevy_render::gpu::GpuSimulationTiming::default(),
        aestra_bevy_render::gpu::GpuPreparationTiming::default(),
    ));
    f.tick();
    assert_eq!(f.profile().gpu_stage_time_ns, ProfileValue::Unavailable);
    assert_eq!(
        f.profile().gpu_simulation_time_ns,
        ProfileValue::Unavailable
    );
    // No trail renderer has a known-zero occupancy without a readback.
    assert_eq!(f.profile().occupied_trails, ProfileValue::Measured(0));
    assert_eq!(
        f.profile().gpu_trail_compaction_time_ns,
        ProfileValue::Unavailable
    );
}

#[test]
fn presented_reference_samples_are_reused_without_a_second_reference_evaluation() {
    let mut f = Fixture::new(Some(ActiveBackend::CpuReference));
    let root = f.root.unwrap();
    let mut reference = Vec::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .evaluate_preview(&mut reference);
    assert!(!reference.is_empty());
    f.app
        .world_mut()
        .get_mut::<PresentedEffect>(root)
        .unwrap()
        .restore_gpu_samples(reference.clone());
    f.tick();
    assert_eq!(f.profile().cpu_time_ns, ProfileValue::Unavailable);
    assert_eq!(f.app.world().resource::<EditorSession>().samples, reference);
    assert_eq!(
        f.profile().alive_particles.value(),
        Some(reference.len() as u32)
    );
}

#[test]
fn switching_to_native_gpu_clears_previous_reference_samples() {
    let mut f = Fixture::new(None);
    f.tick();
    assert!(!f.app.world().resource::<EditorSession>().samples.is_empty());
    let player = configured_preview_player(f.app.world().resource::<EditorSession>()).unwrap();
    f.root = Some(
        f.app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath::default(),
                player,
                status(ActiveBackend::Gpu),
            ))
            .id(),
    );
    f.tick();
    assert!(f.app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(f.profile().cpu_time_ns, ProfileValue::Unavailable);
    assert_eq!(f.profile().alive_particles, ProfileValue::Unavailable);
}

#[test]
fn unowned_or_mismatched_presentations_cannot_replace_desired_instance_observations() {
    let mut f = Fixture::new(None);
    let player = configured_preview_player(f.app.world().resource::<EditorSession>()).unwrap();
    f.app.world_mut().spawn((
        PreviewEffectInstancePath::default(),
        player.clone(),
        status(ActiveBackend::Gpu),
    ));
    let mut wrong = player.effect().as_ref().clone();
    wrong.name = "Other artifact".into();
    f.app.world_mut().spawn((
        PreviewPresentedEffect,
        PreviewEffectInstancePath::default(),
        PresentedEffect::new(Arc::new(wrong)),
        status(ActiveBackend::Gpu),
    ));
    f.tick();
    assert!(matches!(f.profile().cpu_time_ns, ProfileValue::Measured(_)));
    assert!(!f.app.world().resource::<EditorSession>().samples.is_empty());
}

#[test]
fn repeated_ingestion_only_rebuilds_ui_when_project_structure_changes() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    let revision = f.app.world().resource::<EditorSession>().ui_revision;
    f.tick();
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    f.tick();
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .project = None;
    f.tick();
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision + 2
    );
    assert_eq!(f.profile().particle_capacity.value(), Some(0));
}

#[test]
fn ingestion_preserves_authored_history_clock_and_seed() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    let session = f.app.world().resource::<EditorSession>();
    let before = (
        session.effect.clone(),
        session.document_revision(),
        session.effect_undo_len(),
        session.effect_redo_len(),
        session.simulation_time(),
        session.preview_seed,
        session.dirty,
    );
    f.tick();
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(
        (
            session.effect.clone(),
            session.document_revision(),
            session.effect_undo_len(),
            session.effect_redo_len(),
            session.simulation_time(),
            session.preview_seed,
            session.dirty
        ),
        before
    );
}

#[test]
fn nested_reference_profiles_aggregate_but_only_root_samples_enter_the_editor_session() {
    let directory = tempfile::tempdir().unwrap();
    let mut child = aestra_core::EffectAsset::new("Child", 2.0);
    child
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Child emitter", 2.0));
    child
        .save_ron(directory.path().join("child.aestra.ron"))
        .unwrap();
    let mut effect = test_support::effect_with_timing_slack();
    effect
        .effect_clips
        .push(aestra_core::EffectClip::new(child.id, 0.0, 2.0));
    let mut f = Fixture::from_session(EditorSession::from_test_effect(effect), None);
    let catalog = crate::ProjectEffectCatalog::scan(directory.path());
    let project = catalog
        .compile_project(&f.app.world().resource::<EditorSession>().effect)
        .unwrap();
    let expected_capacity =
        project.root.max_particles + project.effect(child.id).unwrap().max_particles;
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .project = Some(Arc::new(project));
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    let root_count = session.samples.len();
    assert!(root_count > 0);
    assert_eq!(
        f.profile().particle_capacity,
        ProfileValue::Measured(expected_capacity as u32)
    );
    assert!(f.profile().alive_particles.value().unwrap() as usize > root_count);
    assert_eq!(
        f.profile().alive_particles.value().unwrap() as usize,
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count
    );
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .project = None;
    f.tick();
    assert!(f.app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        0
    );
}

#[test]
fn gpu_preview_uses_observations_without_cpu_reference_evaluation() {
    let mut session = test_support::session_with_timing_slack();
    session.seek_time(0.5);
    let mut player = configured_preview_player(&session).unwrap();
    let root = player.effect().clone();
    let mut reference = Vec::new();
    session.evaluate_preview(&mut reference);
    assert!(!reference.is_empty());
    let particle_count = reference.len();
    player.restore_gpu_samples(reference);
    let mut app = App::new();
    app.insert_resource(session)
        .init_resource::<ProfilerState>()
        .init_resource::<TimelineState>()
        .insert_resource(EditorPreviewProject {
            source_root: Some(root.clone()),
            project: Some(Arc::new(CompiledEffectProject {
                root,
                dependencies: default(),
            })),
            ..default()
        })
        .add_systems(Update, update_preview);
    let player_entity = app
        .world_mut()
        .spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath::default(),
            player,
            EffectRuntimeStatus {
                active: ActiveBackend::Gpu,
                reason: "Native test".into(),
                compatibility: aestra_runtime::CompatibilityReport::compatible(
                    aestra_runtime::CompatibilityTarget::NativeGpu,
                ),
            },
        ))
        .id();
    app.update();
    assert!(app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(
        app.world()
            .resource::<ProfilerState>()
            .current_profile()
            .unwrap()
            .cpu_time_ns,
        aestra_runtime::ProfileValue::Unavailable
    );
    assert_eq!(
        app.world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        0
    );
    app.world_mut()
        .get_mut::<EffectRuntimeStatus>(player_entity)
        .unwrap()
        .active = ActiveBackend::GpuReadback;
    app.update();
    let profile = app
        .world()
        .resource::<ProfilerState>()
        .current_profile()
        .unwrap();
    assert_eq!(
        profile.cpu_time_ns,
        aestra_runtime::ProfileValue::Unavailable
    );
    assert_eq!(
        profile.alive_particles,
        aestra_runtime::ProfileValue::Measured(particle_count as u32)
    );
    assert_eq!(
        app.world().resource::<EditorSession>().samples.len(),
        particle_count
    );
}
