//! Real Profiler composition, synchronization, native activation and installed schedule.
use super::super::{
    control_input, controls,
    plugin::{EditorProfilerPlugin, ProfilerSet},
};
use super::*;
use crate::feathers::{action_activation::PendingFeathersActivation, scroll::PersistedScroll};
use crate::test_support;
use aestra_runtime::ProjectInstanceProfile;

#[test]
fn project_ingestion_tracks_active_paths_and_rebuilds_only_on_structure_changes() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let root = ProjectInstanceProfile {
        path: Vec::new(),
        effect: compiled.source,
        name: compiled.name.clone(),
        profile: EffectProfile::from_compiled(compiled),
    };
    let mut child = root.clone();
    child.path.push(aestra_core::EffectClipId::new());
    child.profile.trail_capacity = ProfileValue::Measured(4);
    child
        .profile
        .record_trail_usage(Some(aestra_runtime::TrailUsage {
            occupied: 3,
            retired: 1,
            truncated: 1,
            evictions: 2,
        }));
    let mut state = ProfilerState::default();
    assert!(
        state
            .ingest_project(vec![root.clone(), child.clone()])
            .profile_rebuilt()
    );
    let profile = state.profile.as_ref().unwrap();
    assert_eq!(
        profile.particle_capacity.value(),
        Some(2 * compiled.max_particles as u32)
    );
    assert_eq!(profile.occupied_trails, ProfileValue::Measured(3));
    assert_eq!(profile.truncated_trails, ProfileValue::Measured(1));
    assert!(
        !state
            .ingest_project(vec![child, root.clone()])
            .profile_rebuilt()
    );
    for locale in ["en-US", "fr-FR"] {
        let localizer = Localizer::new(locale).unwrap();
        let text = project_profile_text(&state, &localizer);
        assert!(text.contains(&localizer.text("profiler-metric-live-particles")));
        assert!(text.contains(&localizer.text("profiler-metric-gpu-time")));
        assert!(text.contains(&localizer.text("profiler-metric-gpu-trail-compaction")));
        assert!(text.contains(&localizer.text("profiler-metric-gpu-trail-culling")));
        assert!(text.contains(&state.project.instances[1].path[0].to_string()));
    }
    assert!(state.ingest_project(vec![root]).profile_rebuilt());
    assert_eq!(
        state.profile.as_ref().unwrap().occupied_trails,
        ProfileValue::Measured(0)
    );
    state.ingest_project(Vec::new());
    assert_eq!(
        state.profile.as_ref().unwrap().particle_capacity,
        ProfileValue::Measured(0)
    );
}

#[test]
fn point_budget_warning_is_actionable_and_clears_with_telemetry() {
    let session = test_support::session_with_timing_slack();
    let mut profile = EffectProfile::from_compiled(session.preview().unwrap().effect());
    for locale in ["en-US", "fr-FR"] {
        let localizer = Localizer::new(locale).unwrap();
        profile.record_trail_usage(Some(aestra_runtime::TrailUsage {
            truncated: 2,
            ..default()
        }));
        let (message, color) = profiler_metric_source(
            &profile,
            ProfilerMetric::TruncatedTrails,
            ProfileValueSource::Measured,
            &localizer,
        );
        assert_eq!(message, localizer.text("profiler-trail-point-warning"));
        assert_eq!(color, Color::srgb(1.0, 0.65, 0.25));
        for usage in [Some(aestra_runtime::TrailUsage::default()), None] {
            profile.record_trail_usage(usage);
            let (_, source) = profiler_metric_display(&profile, ProfilerMetric::TruncatedTrails);
            let (message, _) = profiler_metric_source(
                &profile,
                ProfilerMetric::TruncatedTrails,
                source,
                &localizer,
            );
            assert_eq!(message, profile_source_label(source, &localizer));
        }
    }
}
use bevy::ecs::schedule::IntoSystemSet;
use bevy::{
    asset::AssetPlugin,
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ScrollArea, Scrollbar},
};

fn profile() -> EffectProfile {
    let session = test_support::session_with_timing_slack();
    let mut p = EffectProfile::from_compiled(session.preview().unwrap().effect());
    p.cpu_time_ns = ProfileValue::Measured(1_250_000);
    p.gpu_time_ns = ProfileValue::Measured(99_000_000);
    p.gpu_simulation_time_ns = ProfileValue::Estimated(7_500);
    p.alive_particles = ProfileValue::Measured(3);
    p.peak_particles = ProfileValue::Measured(9);
    p.truncated_trails = ProfileValue::Measured(2);
    p.emitters[0].alive_particles = 2;
    p.emitters[0].peak_particles = 7;
    p
}
fn state() -> ProfilerState {
    ProfilerState {
        profile: Some(profile()),
        cpu_history_ns: VecDeque::from([10, 20, 40]),
        ..default()
    }
}
fn build(mut c: Commands, s: Res<EditorSession>, p: Res<ProfilerState>, l: Res<Localizer>) {
    c.spawn(Node::default())
        .with_children(|root| spawn_profiler_workspace(root, &s, &p, &l));
}
fn app(locale: &str, state: ProfilerState, compose: bool) -> App {
    let mut a = App::new();
    a.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
    ))
    .insert_resource(test_support::session_with_timing_slack())
    .insert_resource(Localizer::new(locale).unwrap())
    .insert_resource(state)
    .add_plugins(EditorProfilerPlugin);
    if compose {
        a.add_systems(Startup, build);
    }
    a.update();
    a
}
fn texts(a: &mut App) -> Vec<String> {
    a.world_mut()
        .query::<&Text>()
        .iter(a.world())
        .map(|t| t.0.clone())
        .collect()
}
fn marker_count<T: Component>(a: &mut App) -> usize {
    a.world_mut()
        .query_filtered::<Entity, With<T>>()
        .iter(a.world())
        .count()
}
fn button(a: &mut App) -> Entity {
    a.world_mut()
        .spawn((
            control_input::NativeButton,
            FeathersActionButton,
            ProfilerAction::ResetPeaks,
        ))
        .id()
}
fn value_entity(a: &mut App, metric: ProfilerMetric, part: ProfilerMetricPart) -> Entity {
    a.world_mut()
        .query::<(Entity, &ProfilerMetricText)>()
        .iter(a.world())
        .find(|(_, m)| {
            std::mem::discriminant(&m.metric) == std::mem::discriminant(&metric)
                && std::mem::discriminant(&m.part) == std::mem::discriminant(&part)
        })
        .unwrap()
        .0
}
fn clean(a: &App, before: &aestra_core::EffectAsset, revision: u64, undo: usize, redo: usize) {
    let session = a.world().resource::<EditorSession>();
    assert_eq!(&session.effect, before);
    assert_eq!(session.document_revision(), revision);
    assert_eq!(session.effect_undo_len(), undo);
    assert_eq!(session.effect_redo_len(), redo);
    assert!(!session.dirty);
}
#[test]
fn duration_boundaries_and_unknown_preserve_provenance() {
    for (n, expected) in [
        (0, "0 ns"),
        (999, "999 ns"),
        (1000, "1.0 µs"),
        (999999, "1000.0 µs"),
        (1000000, "1.000 ms"),
    ] {
        assert_eq!(
            format_profile_duration(ProfileValue::Measured(n)),
            (expected.into(), ProfileValueSource::Measured)
        );
        assert_eq!(
            format_profile_duration(ProfileValue::Estimated(n)),
            (expected.into(), ProfileValueSource::Estimated)
        );
    }
    assert_eq!(
        format_profile_duration(ProfileValue::Unavailable),
        ("—".into(), ProfileValueSource::Unavailable)
    );
}
#[test]
fn memory_boundaries_and_counts_do_not_invent_measurements() {
    for (n, expected) in [(0, "0.0 KB"), (1024, "1.0 KB"), (1048576, "1.00 MB")] {
        assert_eq!(
            format_profile_memory(ProfileValue::Estimated(n)),
            (expected.into(), ProfileValueSource::Estimated)
        );
    }
    assert_eq!(
        format_profile_memory(ProfileValue::Unavailable),
        ("—".into(), ProfileValueSource::Unavailable)
    );
    assert_eq!(
        format_profile_count(ProfileValue::Measured(0)),
        ("0".into(), ProfileValueSource::Measured)
    );
    assert_eq!(
        format_profile_count(ProfileValue::<u32>::Unavailable),
        ("—".into(), ProfileValueSource::Unavailable)
    );
}
#[test]
fn all_twenty_metric_cards_keep_their_original_field_mapping() {
    let mut p = profile();
    p.gpu_trail_compaction_time_ns = ProfileValue::Measured(11);
    p.gpu_trail_culling_time_ns = ProfileValue::Measured(12);
    p.gpu_stage_time_ns = ProfileValue::Measured(13);
    p.submitted_instances = ProfileValue::Measured(14);
    p.submitted_vertices = ProfileValue::Measured(15);
    p.submitted_primitives = ProfileValue::Measured(16);
    p.particle_capacity = ProfileValue::Measured(17);
    p.trail_capacity = ProfileValue::Measured(18);
    p.occupied_trails = ProfileValue::Measured(19);
    p.retired_trails = ProfileValue::Measured(20);
    p.trail_evictions = ProfileValue::Measured(21);
    p.emitter_count = ProfileValue::Measured(22);
    p.draw_calls = ProfileValue::Measured(23);
    p.dispatch_count = ProfileValue::Measured(24);
    p.buffer_memory_bytes = ProfileValue::Measured(1024);
    let expected = [
        (ProfilerMetric::CpuTime, "1.250 ms"),
        (ProfilerMetric::GpuTime, "7.5 µs"),
        (ProfilerMetric::GpuTrailCompaction, "11 ns"),
        (ProfilerMetric::GpuTrailCulling, "12 ns"),
        (ProfilerMetric::GpuStages, "13 ns"),
        (ProfilerMetric::AliveParticles, "3"),
        (ProfilerMetric::SubmittedInstances, "14"),
        (ProfilerMetric::SubmittedVertices, "15"),
        (ProfilerMetric::SubmittedPrimitives, "16"),
        (ProfilerMetric::PeakParticles, "9"),
        (ProfilerMetric::ParticleCapacity, "17"),
        (ProfilerMetric::TrailCapacity, "18"),
        (ProfilerMetric::OccupiedTrails, "19"),
        (ProfilerMetric::RetiredTrails, "20"),
        (ProfilerMetric::TrailEvictions, "21"),
        (ProfilerMetric::TruncatedTrails, "2"),
        (ProfilerMetric::Emitters, "22"),
        (ProfilerMetric::DrawCalls, "23"),
        (ProfilerMetric::Dispatches, "24"),
        (ProfilerMetric::BufferMemory, "1.0 KB"),
    ];
    for (m, text) in expected {
        assert_eq!(profiler_metric_display(&p, m).0, text);
        for locale in ["en-US", "fr-FR"] {
            let l = Localizer::new(locale).unwrap();
            assert_ne!(
                l.text(profiler_metric_message(m)),
                profiler_metric_message(m)
            );
        }
    }
    assert_eq!(
        profiler_metric_display(&p, ProfilerMetric::GpuTime).1,
        ProfileValueSource::Estimated
    );
}
#[test]
fn source_colors_and_localized_labels_distinguish_all_provenances() {
    for locale in ["en-US", "fr-FR"] {
        let l = Localizer::new(locale).unwrap();
        for (s, key, color) in [
            (
                ProfileValueSource::Measured,
                "profiler-source-measured",
                Color::srgb(0.35, 0.88, 0.57),
            ),
            (
                ProfileValueSource::Estimated,
                "profiler-source-estimated",
                Color::srgb(1.0, 0.74, 0.30),
            ),
            (
                ProfileValueSource::Unavailable,
                "profiler-source-unavailable",
                theme::TEXT_FAINT,
            ),
        ] {
            assert_eq!(profile_source_label(s, &l), l.text(key));
            assert_eq!(profile_source_color(s), color);
        }
    }
}
#[test]
fn history_summary_is_localized_and_safe_at_u64_max() {
    for locale in ["en-US", "fr-FR"] {
        let l = Localizer::new(locale).unwrap();
        assert_eq!(
            profiler_history_summary(&VecDeque::new(), &l),
            l.text("profiler-history-collecting")
        );
        let history = VecDeque::from([u64::MAX, u64::MAX]);
        let text = profiler_history_summary(&history, &l);
        assert!(text.contains(&format_profile_duration(ProfileValue::Measured(u64::MAX)).0));
    }
}
#[test]
fn waiting_workspace_has_native_reset_but_no_invented_metrics() {
    for locale in ["en-US", "fr-FR"] {
        let mut a = app(locale, ProfilerState::default(), true);
        let l = a.world().resource::<Localizer>();
        let expected = [
            l.text("profiler-waiting"),
            l.text("profiler-waiting-description"),
        ];
        let all = texts(&mut a);
        for text in expected {
            assert!(all.contains(&text));
        }
        assert_eq!(marker_count::<ProfilerMetricText>(&mut a), 0);
        assert_eq!(marker_count::<ScrollArea>(&mut a), 0);
        assert_eq!(marker_count::<ProfilerAction>(&mut a), 1);
    }
}
#[test]
fn populated_workspace_constructs_real_cards_scroll_history_and_emitters() {
    let mut a = app("en-US", state(), true);
    assert_eq!(marker_count::<ProfilerMetricText>(&mut a), 40);
    assert_eq!(
        marker_count::<ProfilerHistoryBar>(&mut a),
        PROFILER_HISTORY_SAMPLES
    );
    assert_eq!(marker_count::<ProfilerHistorySummary>(&mut a), 1);
    assert_eq!(marker_count::<ProjectProfileText>(&mut a), 1);
    assert_eq!(
        marker_count::<ProfilerEmitterValue>(&mut a),
        profile().emitters.len()
    );
    assert_eq!(marker_count::<ScrollArea>(&mut a), 1);
    assert_eq!(marker_count::<Scrollbar>(&mut a), 1);
    let w = a.world_mut();
    assert!(
        w.query::<&PersistedScroll>()
            .iter(w)
            .any(|p| p.0 == ScrollMemoryKey::Profiler)
    );
    let values = texts(&mut a);
    assert!(values.contains(&"1.250 ms".into()));
    assert!(values.contains(&"7.5 µs".into()));
    assert!(!values.contains(&"99.000 ms".into()));
}
#[test]
fn reset_control_has_accessible_label_native_button_and_nonpicking_text() {
    for locale in ["en-US", "fr-FR"] {
        let mut a = app(locale, state(), true);
        let expected = a
            .world()
            .resource::<Localizer>()
            .text("profiler-reset-peaks");
        let w = a.world_mut();
        let (e, label, node, children) = w
            .query_filtered::<(Entity, &AccessibleLabel, &Node, &Children), With<ProfilerAction>>()
            .single(w)
            .unwrap();
        assert_eq!(label.0, expected);
        assert_eq!(node.height, Val::Px(24.0));
        assert!(w.get::<control_input::NativeButton>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_some());
        let child = children[0];
        assert_eq!(w.get::<Text>(child).unwrap().0, expected);
        assert!(w.get::<ThemedText>(child).is_some());
        assert_eq!(*w.get::<Pickable>(child).unwrap(), Pickable::IGNORE);
    }
}
#[test]
fn availability_is_driven_by_original_gpu_and_geometry_unknown_fields() {
    let mut s = state();
    let p = s.profile.as_mut().unwrap();
    p.gpu_simulation_time_ns = ProfileValue::Measured(0);
    p.gpu_trail_compaction_time_ns = ProfileValue::Measured(0);
    p.gpu_trail_culling_time_ns = ProfileValue::Measured(0);
    p.gpu_stage_time_ns = ProfileValue::Measured(0);
    p.submitted_vertices = ProfileValue::Measured(0);
    let mut known = app("en-US", s, true);
    let description = known
        .world()
        .resource::<Localizer>()
        .text("profiler-unavailable-description");
    assert!(!texts(&mut known).contains(&description));
    let mut unknown = app("en-US", state(), true);
    assert!(texts(&mut unknown).contains(&description));
}
#[test]
fn history_bars_align_right_normalize_and_highlight_latest() {
    let mut a = app("en-US", state(), true);
    let w = a.world_mut();
    let bars = w
        .query::<(&ProfilerHistoryBar, &Node, &BackgroundColor)>()
        .iter(w)
        .map(|(i, n, c)| (i.0, n.height, c.0))
        .collect::<Vec<_>>();
    assert_eq!(bars[0], (0, Val::Px(1.0), theme::ACCENT_DIM));
    for (i, h) in [(93, 20.0), (94, 38.0), (95, 74.0)] {
        let (_, height, color) = bars.iter().find(|b| b.0 == i).unwrap();
        assert_eq!(*height, Val::Px(h));
        assert_eq!(
            *color,
            if i == 95 {
                theme::ACCENT
            } else {
                theme::ACCENT_DIM
            }
        );
    }
}
#[test]
fn zero_history_and_clear_update_existing_bar_entities_safely() {
    let mut a = app("en-US", state(), true);
    a.world_mut().resource_mut::<ProfilerState>().cpu_history_ns = VecDeque::from([0, 0]);
    a.update();
    let w = a.world_mut();
    assert!(
        w.query::<(&ProfilerHistoryBar, &Node)>()
            .iter(w)
            .filter(|(i, _)| i.0 >= 94)
            .all(|(_, n)| n.height == Val::Px(2.0))
    );
    a.world_mut()
        .resource_mut::<ProfilerState>()
        .cpu_history_ns
        .clear();
    a.update();
    let w = a.world_mut();
    assert!(
        w.query::<(&ProfilerHistoryBar, &Node, &BackgroundColor)>()
            .iter(w)
            .all(|(_, n, c)| n.height == Val::Px(1.0) && c.0 == theme::ACCENT_DIM)
    );
}
#[test]
fn metric_and_warning_updates_reuse_existing_entities() {
    let mut a = app("en-US", state(), true);
    let value = value_entity(&mut a, ProfilerMetric::GpuTime, ProfilerMetricPart::Value);
    let source = value_entity(
        &mut a,
        ProfilerMetric::TruncatedTrails,
        ProfilerMetricPart::Source,
    );
    let old_count = a.world().entities().len();
    {
        let mut s = a.world_mut().resource_mut::<ProfilerState>();
        let p = s.profile.as_mut().unwrap();
        p.gpu_simulation_time_ns = ProfileValue::Unavailable;
        p.truncated_trails = ProfileValue::Measured(0);
    }
    a.update();
    assert_eq!(a.world().get::<Text>(value).unwrap().0, "—");
    assert_eq!(
        a.world().get::<Text>(source).unwrap().0,
        a.world()
            .resource::<Localizer>()
            .text("profiler-source-measured")
    );
    assert_eq!(
        a.world().get::<TextColor>(source).unwrap().0,
        profile_source_color(ProfileValueSource::Measured)
    );
    assert_eq!(a.world().entities().len(), old_count);
}
#[test]
fn idle_sync_preserves_text_and_entity_identity_until_state_changes() {
    let mut a = app("en-US", state(), true);
    let value = value_entity(&mut a, ProfilerMetric::CpuTime, ProfilerMetricPart::Value);
    a.world_mut().get_mut::<Text>(value).unwrap().0 = "idle sentinel".into();
    a.update();
    assert_eq!(a.world().get::<Text>(value).unwrap().0, "idle sentinel");
    a.world_mut()
        .resource_mut::<ProfilerState>()
        .cpu_history_ns
        .push_back(4);
    a.update();
    assert_eq!(a.world().get::<Text>(value).unwrap().0, "1.250 ms");
}
#[test]
fn locale_change_updates_existing_source_summary_emitter_and_project_text() {
    let mut a = app("en-US", state(), true);
    let source = value_entity(&mut a, ProfilerMetric::GpuTime, ProfilerMetricPart::Source);
    *a.world_mut().resource_mut::<Localizer>() = Localizer::new("fr-FR").unwrap();
    a.update();
    let l = a.world().resource::<Localizer>();
    assert_eq!(
        a.world().get::<Text>(source).unwrap().0,
        l.text("profiler-source-estimated")
    );
    let expected = profiler_emitter_value(&profile().emitters[0], l);
    let summary = profiler_history_summary(&VecDeque::from([10, 20, 40]), l);
    let all = texts(&mut a);
    assert!(all.contains(&expected));
    assert!(all.contains(&summary));
}
#[test]
fn plugin_initializes_missing_state_and_preserves_preinstalled_profile() {
    let mut a = App::new();
    a.add_plugins(EditorProfilerPlugin);
    assert!(a.world().resource::<ProfilerState>().profile.is_none());
    let mut b = App::new();
    b.insert_resource(state()).add_plugins(EditorProfilerPlugin);
    assert_eq!(
        b.world().resource::<ProfilerState>().cpu_history_ns.len(),
        3
    );
}
#[test]
fn installed_schedule_keeps_three_consumers_in_original_sets() {
    let mut a = app("en-US", state(), false);
    a.world_mut().schedule_scope(Update, |world, schedule| {
        schedule.initialize(world).unwrap();
        for (set, count) in [(ProfilerSet::Actions, 1), (ProfilerSet::Sync, 2)] {
            assert_eq!(
                schedule.graph().systems_in_set(set.intern()).unwrap().len(),
                count
            );
        }
        for system in [
            controls::handle_profiler_actions.into_system_set().intern(),
            update_profiler_labels.into_system_set().intern(),
            update_project_profile_text.into_system_set().intern(),
        ] {
            assert_eq!(schedule.graph().systems_in_set(system).unwrap().len(), 1);
        }
    });
}
#[test]
fn native_activate_resets_peaks_history_and_localized_status_without_editing() {
    for locale in ["en-US", "fr-FR"] {
        let mut a = app(locale, state(), true);
        let session = a.world().resource::<EditorSession>();
        let (before, rev, undo, redo) = (
            session.effect.clone(),
            session.document_revision(),
            session.effect_undo_len(),
            session.effect_redo_len(),
        );
        let e = a
            .world_mut()
            .query_filtered::<Entity, With<ProfilerAction>>()
            .single(a.world())
            .unwrap();
        a.world_mut().trigger(Activate { entity: e });
        a.update();
        let s = a.world().resource::<ProfilerState>();
        assert!(s.cpu_history_ns.is_empty());
        let p = s.profile.as_ref().unwrap();
        assert_eq!(p.peak_particles, ProfileValue::Measured(3));
        assert_eq!(p.emitters[0].peak_particles, 2);
        assert!(a.world().get::<PendingFeathersActivation>(e).is_none());
        assert!(!control_input::is_pressed(a.world(), e));
        assert_eq!(
            a.world().resource::<EditorSession>().status,
            a.world()
                .resource::<Localizer>()
                .text("profiler-reset-status")
        );
        clean(&a, &before, rev, undo, redo);
        a.world_mut()
            .resource_mut::<ProfilerState>()
            .cpu_history_ns
            .push_back(77);
        a.update();
        assert_eq!(
            a.world().resource::<ProfilerState>().cpu_history_ns,
            VecDeque::from([77])
        );
    }
}
#[derive(Resource)]
struct Enabled(bool);
#[test]
fn invalid_pending_transaction_displays_last_valid_without_applying_it() {
    for locale in ["en-US", "fr-FR"] {
        let mut a = app(locale, state(), false);
        let mut session = a.world_mut().resource_mut::<EditorSession>();
        let before = session.effect.clone();
        let emitter = session.effect.emitters[0].id;
        let module = session.effect.emitters[0]
            .module_by_type(aestra_core::MODULE_EMISSION)
            .unwrap()
            .id;
        assert!(
            session.preview_transaction(aestra_authoring::EffectTransaction::single(
                "Invalid candidate",
                aestra_authoring::EffectCommand::RemoveModule { emitter, module },
            ))
        );
        assert!(!session.pending_change.as_ref().unwrap().can_apply);
        a.world_mut().run_system_cached(build).unwrap();
        a.world_mut().flush();
        a.update();
        let expected = a
            .world()
            .resource::<Localizer>()
            .text("profiler-status-last-valid");
        assert!(texts(&mut a).contains(&expected));
        let session = a.world().resource::<EditorSession>();
        assert_eq!(session.effect, before);
        assert!(!session.pending_change.as_ref().unwrap().can_apply);
        assert!(!session.dirty);
    }
}
#[test]
fn project_text_syncs_nested_paths_and_clears_without_rebuilding() {
    let mut a = app("en-US", state(), true);
    let compiled = a
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    let child = ProjectInstanceProfile {
        path: vec![aestra_core::EffectClipId::new()],
        effect: compiled.source,
        name: "Nested smoke".into(),
        profile: EffectProfile::from_compiled(&compiled),
    };
    let path = child.path[0].to_string();
    a.world_mut()
        .resource_mut::<ProfilerState>()
        .ingest_project(vec![child]);
    a.update();
    let entity = a
        .world_mut()
        .query_filtered::<Entity, With<ProjectProfileText>>()
        .single(a.world())
        .unwrap();
    assert!(a.world().get::<Text>(entity).unwrap().0.contains(&path));
    *a.world_mut().resource_mut::<Localizer>() = Localizer::new("fr-FR").unwrap();
    a.update();
    let expected = project_profile_text(
        a.world().resource::<ProfilerState>(),
        a.world().resource::<Localizer>(),
    );
    assert_eq!(a.world().get::<Text>(entity).unwrap().0, expected);
    a.world_mut()
        .resource_mut::<ProfilerState>()
        .ingest_project(Vec::new());
    a.update();
    assert_eq!(a.world().get::<Text>(entity).unwrap().0, "");
}
#[test]
fn action_set_gate_defers_then_consumes_pending_activation_once() {
    let mut a = app("en-US", state(), false);
    a.insert_resource(Enabled(false)).configure_sets(
        Update,
        ProfilerSet::Actions.run_if(|gate: Res<Enabled>| gate.0),
    );
    let e = button(&mut a);
    a.world_mut().trigger(Activate { entity: e });
    a.update();
    assert_eq!(
        a.world().resource::<ProfilerState>().cpu_history_ns.len(),
        3
    );
    assert!(a.world().get::<PendingFeathersActivation>(e).is_some());
    a.world_mut().resource_mut::<Enabled>().0 = true;
    a.update();
    assert!(
        a.world()
            .resource::<ProfilerState>()
            .cpu_history_ns
            .is_empty()
    );
    assert!(a.world().get::<PendingFeathersActivation>(e).is_none());
}
#[test]
fn ungated_physical_press_and_unrelated_controls_cannot_reset_profiler() {
    let mut a = app("en-US", state(), false);
    let e = button(&mut a);
    control_input::set_pressed(a.world_mut(), e, true);
    a.update();
    assert_eq!(
        a.world().resource::<ProfilerState>().cpu_history_ns.len(),
        3
    );
    let unrelated = a.world_mut().spawn(FeathersActionButton).id();
    let not_feathers = a.world_mut().spawn(ProfilerAction::ResetPeaks).id();
    for entity in [unrelated, not_feathers] {
        a.world_mut().trigger(Activate { entity });
        a.world_mut().flush();
        assert!(a.world().get::<PendingFeathersActivation>(entity).is_none());
    }
    a.update();
    assert_eq!(
        a.world().resource::<ProfilerState>().cpu_history_ns.len(),
        3
    );
}
