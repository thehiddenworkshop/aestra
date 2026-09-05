//! Runtime profile ingestion, presentation, and Profiler workspace actions.

use crate::feathers::panel::{
    spawn_panel_empty_state, spawn_panel_label_value, spawn_panel_section,
};
use crate::*;
use aestra_runtime::{
    CompiledEffect, EffectProfile, ParticleSample, ProfileValue, ProfileValueSource,
};
use bevy::ui_widgets::Activate;
use fluent_bundle::FluentArgs;
use std::{collections::VecDeque, time::Duration};

const PROFILER_HISTORY_SAMPLES: usize = 96;

pub(crate) struct EditorProfilerPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProfilerSet {
    Actions,
    Sync,
}

impl Plugin for EditorProfilerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfilerState>()
            .add_observer(queue_profiler_action_activation)
            .add_systems(
                Update,
                (
                    handle_profiler_actions.in_set(ProfilerSet::Actions),
                    update_profiler_labels.in_set(ProfilerSet::Sync),
                ),
            );
    }
}

/// One measured preview frame supplied to the Profiler without transferring ownership.
///
/// The producer owns simulation and timing. The Profiler exclusively owns aggregation,
/// compiler estimates, peak tracking, and bounded history.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProfilerFrameSample<'a> {
    effect: &'a CompiledEffect,
    particles: &'a [ParticleSample],
    cpu_time: Duration,
    trails: Option<aestra_runtime::TrailUsage>,
}

impl<'a> ProfilerFrameSample<'a> {
    pub(crate) const fn new(
        effect: &'a CompiledEffect,
        particles: &'a [ParticleSample],
        cpu_time: Duration,
    ) -> Self {
        Self {
            effect,
            particles,
            cpu_time,
            trails: None,
        }
    }

    pub(crate) fn with_trails(mut self, usage: Option<aestra_runtime::TrailUsage>) -> Self {
        self.trails = usage;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfilerIngestOutcome {
    Updated,
    ProfileRebuilt,
}

impl ProfilerIngestOutcome {
    pub(crate) const fn profile_rebuilt(self) -> bool {
        matches!(self, Self::ProfileRebuilt)
    }
}

#[derive(Resource, Default)]
pub(crate) struct ProfilerState {
    profile: Option<EffectProfile>,
    cpu_history_ns: VecDeque<u64>,
}

impl ProfilerState {
    pub(crate) fn ingest(&mut self, sample: ProfilerFrameSample<'_>) -> ProfilerIngestOutcome {
        let rebuilt = self
            .profile
            .as_ref()
            .is_none_or(|profile| !profile.matches_compiled(sample.effect));
        if rebuilt {
            self.profile = Some(EffectProfile::from_compiled(sample.effect));
            self.cpu_history_ns.clear();
        }
        let profile = self.profile.as_mut().expect("profile was initialized");
        profile.record_cpu_frame(sample.cpu_time, sample.particles);
        profile.record_submitted_frame(sample.effect, sample.particles);
        profile.record_trail_usage(sample.trails);
        self.cpu_history_ns
            .push_back(sample.cpu_time.as_nanos().min(u128::from(u64::MAX)) as u64);
        while self.cpu_history_ns.len() > PROFILER_HISTORY_SAMPLES {
            self.cpu_history_ns.pop_front();
        }
        if rebuilt {
            ProfilerIngestOutcome::ProfileRebuilt
        } else {
            ProfilerIngestOutcome::Updated
        }
    }

    fn reset_peaks(&mut self) {
        if let Some(profile) = &mut self.profile {
            profile.reset_peaks();
        }
        self.cpu_history_ns.clear();
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ProfilerAction {
    ResetPeaks,
}

#[derive(Debug, Clone, Copy)]
enum ProfilerMetric {
    CpuTime,
    GpuTime,
    AliveParticles,
    SubmittedInstances,
    PeakParticles,
    ParticleCapacity,
    TrailCapacity,
    OccupiedTrails,
    RetiredTrails,
    TrailEvictions,
    Emitters,
    DrawCalls,
    Dispatches,
    BufferMemory,
}

#[derive(Debug, Clone, Copy)]
enum ProfilerMetricPart {
    Value,
    Source,
}

#[derive(Component)]
struct ProfilerMetricText {
    metric: ProfilerMetric,
    part: ProfilerMetricPart,
}

#[derive(Component)]
struct ProfilerEmitterValue(usize);

#[derive(Component)]
struct ProfilerHistoryBar(usize);

#[derive(Component)]
struct ProfilerHistorySummary;

fn queue_profiler_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ProfilerAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_profiler_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &ProfilerAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut profiler: ResMut<ProfilerState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    for (entity, interaction, action, pending) in &mut actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        match action {
            ProfilerAction::ResetPeaks => {
                profiler.reset_peaks();
                session.status = localizer.text("profiler-reset-status");
            }
        }
    }
}

pub(crate) fn spawn_profiler_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &ProfilerState,
    localizer: &Localizer,
) {
    let status = if session
        .pending_change
        .as_ref()
        .is_some_and(|pending| !pending.can_apply)
    {
        "profiler-status-last-valid"
    } else {
        "profiler-status-live"
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(9.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new(localizer.text("profiler-effect-profile")),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.35, 0.88, 0.57)),
                    ));
                    header.spawn((
                        Text::new(localizer.text(status)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    spawn_profiler_reset_button(header, localizer);
                });

            let Some(profile) = &state.profile else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("profiler-waiting"),
                    &localizer.text("profiler-waiting-description"),
                    theme::TEXT_MUTED,
                );
                return;
            };

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Profiler,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(9.0),
                            ..default()
                        },
                        |content| {
                            spawn_profiler_metric_grid(content, profile, localizer);
                            spawn_profiler_history(content, state, localizer);
                            spawn_profiler_emitters(content, profile, localizer);
                            spawn_profiler_availability(content, profile, localizer);
                        },
                    );
                });
        });
}

fn spawn_profiler_reset_button(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    let label = localizer.text("profiler-reset-peaks");
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            ProfilerAction::ResetPeaks,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            Node {
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_profiler_metric_grid(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|grid| {
            for metric in [
                ProfilerMetric::CpuTime,
                ProfilerMetric::GpuTime,
                ProfilerMetric::AliveParticles,
                ProfilerMetric::SubmittedInstances,
                ProfilerMetric::PeakParticles,
                ProfilerMetric::ParticleCapacity,
                ProfilerMetric::TrailCapacity,
                ProfilerMetric::OccupiedTrails,
                ProfilerMetric::RetiredTrails,
                ProfilerMetric::TrailEvictions,
                ProfilerMetric::Emitters,
                ProfilerMetric::DrawCalls,
                ProfilerMetric::Dispatches,
                ProfilerMetric::BufferMemory,
            ] {
                spawn_profiler_metric_card(grid, profile, metric, localizer);
            }
        });
}

fn spawn_profiler_metric_card(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    metric: ProfilerMetric,
    localizer: &Localizer,
) {
    let (value, source) = profiler_metric_display(profile, metric);
    parent
        .spawn((
            Node {
                width: Val::Px(132.0),
                min_height: Val::Px(70.0),
                flex_grow: 1.0,
                padding: UiRect::all(Val::Px(9.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                ProfilerMetricText {
                    metric,
                    part: ProfilerMetricPart::Value,
                },
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
            card.spawn((
                Text::new(localizer.text(profiler_metric_message(metric))),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            card.spawn((
                ProfilerMetricText {
                    metric,
                    part: ProfilerMetricPart::Source,
                },
                Text::new(profile_source_label(source, localizer)),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(profile_source_color(source)),
            ));
        });
}

fn spawn_profiler_history(
    parent: &mut ChildSpawnerCommands,
    state: &ProfilerState,
    localizer: &Localizer,
) {
    spawn_panel_section(parent, &localizer.text("profiler-cpu-history"), |section| {
        section.spawn((
            ProfilerHistorySummary,
            Text::new(profiler_history_summary(&state.cpu_history_ns, localizer)),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
        ));
        section
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(82.0),
                align_items: AlignItems::End,
                column_gap: Val::Px(1.0),
                padding: UiRect::top(Val::Px(7.0)),
                ..default()
            })
            .with_children(|graph| {
                for index in 0..PROFILER_HISTORY_SAMPLES {
                    graph.spawn((
                        ProfilerHistoryBar(index),
                        Node {
                            height: Val::Px(1.0),
                            min_width: Val::Px(1.0),
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT_DIM),
                    ));
                }
            });
    });
}

fn spawn_profiler_emitters(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
    spawn_panel_section(parent, &localizer.text("profiler-emitters"), |section| {
        for (index, emitter) in profile.emitters.iter().enumerate() {
            section
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(35.0),
                        padding: UiRect::horizontal(Val::Px(7.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(format!("E{index:02}")),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                        Node {
                            width: Val::Px(38.0),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Text::new(&emitter.name),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    row.spawn((
                        ProfilerEmitterValue(index),
                        Text::new(profiler_emitter_value(emitter, localizer)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                });
        }
    });
}

fn spawn_profiler_availability(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
    spawn_panel_section(
        parent,
        &localizer.text("profiler-measurement-availability"),
        |section| {
            spawn_panel_label_value(
                section,
                &localizer.text("profiler-source-measured"),
                &localizer.text("profiler-measured-description"),
            );
            spawn_panel_label_value(
                section,
                &localizer.text("profiler-source-estimated"),
                &localizer.text("profiler-estimated-description"),
            );
            if profile.gpu_time_ns.source() == ProfileValueSource::Unavailable {
                spawn_panel_label_value(
                    section,
                    &localizer.text("profiler-source-unavailable"),
                    &localizer.text("profiler-unavailable-description"),
                );
            }
        },
    );
}

fn profiler_metric_message(metric: ProfilerMetric) -> &'static str {
    match metric {
        ProfilerMetric::CpuTime => "profiler-metric-cpu-update",
        ProfilerMetric::GpuTime => "profiler-metric-gpu-time",
        ProfilerMetric::AliveParticles => "profiler-metric-live-particles",
        ProfilerMetric::SubmittedInstances => "profiler-metric-submitted-instances",
        ProfilerMetric::PeakParticles => "profiler-metric-peak-particles",
        ProfilerMetric::ParticleCapacity => "profiler-metric-capacity",
        ProfilerMetric::TrailCapacity => "profiler-metric-trail-capacity",
        ProfilerMetric::OccupiedTrails => "profiler-metric-trails-occupied",
        ProfilerMetric::RetiredTrails => "profiler-metric-trails-retired",
        ProfilerMetric::TrailEvictions => "profiler-metric-trail-evictions",
        ProfilerMetric::Emitters => "profiler-metric-emitters",
        ProfilerMetric::DrawCalls => "profiler-metric-draw-calls",
        ProfilerMetric::Dispatches => "profiler-metric-dispatches",
        ProfilerMetric::BufferMemory => "profiler-metric-buffer-memory",
    }
}

fn profiler_metric_display(
    profile: &EffectProfile,
    metric: ProfilerMetric,
) -> (String, ProfileValueSource) {
    match metric {
        ProfilerMetric::CpuTime => format_profile_duration(profile.cpu_time_ns),
        ProfilerMetric::GpuTime => format_profile_duration(profile.gpu_time_ns),
        ProfilerMetric::AliveParticles => format_profile_count(profile.alive_particles),
        ProfilerMetric::SubmittedInstances => format_profile_count(profile.submitted_instances),
        ProfilerMetric::PeakParticles => format_profile_count(profile.peak_particles),
        ProfilerMetric::ParticleCapacity => format_profile_count(profile.particle_capacity),
        ProfilerMetric::TrailCapacity => format_profile_count(profile.trail_capacity),
        ProfilerMetric::OccupiedTrails => format_profile_count(profile.occupied_trails),
        ProfilerMetric::RetiredTrails => format_profile_count(profile.retired_trails),
        ProfilerMetric::TrailEvictions => format_profile_count(profile.trail_evictions),
        ProfilerMetric::Emitters => format_profile_count(profile.emitter_count),
        ProfilerMetric::DrawCalls => format_profile_count(profile.draw_calls),
        ProfilerMetric::Dispatches => format_profile_count(profile.dispatch_count),
        ProfilerMetric::BufferMemory => format_profile_memory(profile.buffer_memory_bytes),
    }
}

fn format_profile_duration(value: ProfileValue<u64>) -> (String, ProfileValueSource) {
    let source = value.source();
    let Some(nanoseconds) = value.value() else {
        return ("—".into(), source);
    };
    let display = if nanoseconds >= 1_000_000 {
        format!("{:.3} ms", nanoseconds as f64 / 1_000_000.0)
    } else if nanoseconds >= 1_000 {
        format!("{:.1} µs", nanoseconds as f64 / 1_000.0)
    } else {
        format!("{nanoseconds} ns")
    };
    (display, source)
}

fn format_profile_count(value: ProfileValue<u32>) -> (String, ProfileValueSource) {
    (
        value
            .value()
            .map_or_else(|| "—".into(), |value| value.to_string()),
        value.source(),
    )
}

fn format_profile_memory(value: ProfileValue<u64>) -> (String, ProfileValueSource) {
    let source = value.source();
    let Some(bytes) = value.value() else {
        return ("—".into(), source);
    };
    let display = if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    };
    (display, source)
}

fn profile_source_label(source: ProfileValueSource, localizer: &Localizer) -> String {
    localizer.text(match source {
        ProfileValueSource::Measured => "profiler-source-measured",
        ProfileValueSource::Estimated => "profiler-source-estimated",
        ProfileValueSource::Unavailable => "profiler-source-unavailable",
    })
}

fn profile_source_color(source: ProfileValueSource) -> Color {
    match source {
        ProfileValueSource::Measured => Color::srgb(0.35, 0.88, 0.57),
        ProfileValueSource::Estimated => Color::srgb(1.0, 0.74, 0.30),
        ProfileValueSource::Unavailable => theme::TEXT_FAINT,
    }
}

fn profiler_emitter_value(
    emitter: &aestra_runtime::EmitterProfile,
    localizer: &Localizer,
) -> String {
    let mut args = FluentArgs::new();
    args.set("live", emitter.alive_particles);
    args.set("peak", emitter.peak_particles);
    args.set("capacity", emitter.particle_capacity);
    localizer.text_with("profiler-emitter-summary", &args)
}

fn profiler_history_summary(history: &VecDeque<u64>, localizer: &Localizer) -> String {
    if history.is_empty() {
        return localizer.text("profiler-history-collecting");
    }
    let total = history.iter().copied().map(u128::from).sum::<u128>();
    let average = (total / history.len() as u128).min(u128::from(u64::MAX)) as u64;
    let maximum = history.iter().copied().max().unwrap_or_default();
    let mut args = FluentArgs::new();
    args.set("count", history.len());
    args.set(
        "average",
        format_profile_duration(ProfileValue::Measured(average)).0,
    );
    args.set(
        "maximum",
        format_profile_duration(ProfileValue::Measured(maximum)).0,
    );
    localizer.text_with("profiler-history-summary", &args)
}

#[allow(clippy::type_complexity)]
fn update_profiler_labels(
    profiler: Res<ProfilerState>,
    localizer: Res<Localizer>,
    mut labels: Query<
        (
            &mut Text,
            &mut TextColor,
            Option<&ProfilerMetricText>,
            Option<&ProfilerEmitterValue>,
            Option<&ProfilerHistorySummary>,
        ),
        Or<(
            With<ProfilerMetricText>,
            With<ProfilerEmitterValue>,
            With<ProfilerHistorySummary>,
        )>,
    >,
    mut bars: Query<(&ProfilerHistoryBar, &mut Node, &mut BackgroundColor)>,
) {
    if !profiler.is_changed() && !localizer.is_changed() {
        return;
    }
    if let Some(profile) = &profiler.profile {
        for (mut text, mut color, metric, emitter, summary) in &mut labels {
            if let Some(metric) = metric {
                let (value, source) = profiler_metric_display(profile, metric.metric);
                match metric.part {
                    ProfilerMetricPart::Value => {
                        text.0 = value;
                        color.0 = theme::TEXT;
                    }
                    ProfilerMetricPart::Source => {
                        text.0 = profile_source_label(source, &localizer);
                        color.0 = profile_source_color(source);
                    }
                }
            } else if let Some(emitter) = emitter {
                if let Some(profile) = profile.emitters.get(emitter.0) {
                    text.0 = profiler_emitter_value(profile, &localizer);
                }
            } else if summary.is_some() {
                text.0 = profiler_history_summary(&profiler.cpu_history_ns, &localizer);
            }
        }
    }

    let history_len = profiler.cpu_history_ns.len().min(PROFILER_HISTORY_SAMPLES);
    let first_bar = PROFILER_HISTORY_SAMPLES - history_len;
    let maximum = profiler
        .cpu_history_ns
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    for (bar, mut node, mut color) in &mut bars {
        if bar.0 < first_bar {
            node.height = Val::Px(1.0);
            color.0 = theme::ACCENT_DIM;
            continue;
        }
        let history_index = bar.0 - first_bar;
        let value = profiler
            .cpu_history_ns
            .get(history_index)
            .copied()
            .unwrap_or_default();
        node.height = Val::Px(2.0 + 72.0 * value as f32 / maximum as f32);
        color.0 = if bar.0 + 1 == PROFILER_HISTORY_SAMPLES {
            theme::ACCENT
        } else {
            theme::ACCENT_DIM
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn frame_ingestion_preserves_preview_state_and_provenance() {
        let session = test_support::session_with_timing_slack();
        let mut baseline = session.preview.as_ref().unwrap().clone();
        let mut profiled = baseline.clone();
        baseline.seek(1.25);
        profiled.seek(1.25);
        let mut baseline_samples = Vec::new();
        let mut profiled_samples = Vec::new();
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);

        let time_before = profiled.time();
        let seed_before = profiled.seed();
        let mut profiler = ProfilerState::default();
        assert_eq!(
            profiler.ingest(ProfilerFrameSample::new(
                profiled.effect(),
                &profiled_samples,
                Duration::from_micros(125),
            )),
            ProfilerIngestOutcome::ProfileRebuilt
        );
        assert_eq!(profiled.time(), time_before);
        assert_eq!(profiled.seed(), seed_before);
        let profile = profiler.profile.as_ref().unwrap();
        assert_eq!(
            profile.alive_particles,
            ProfileValue::Measured(profiled_samples.len() as u32)
        );
        assert_eq!(profile.gpu_time_ns, ProfileValue::Unavailable);
        assert_eq!(
            profile
                .emitters
                .iter()
                .map(|emitter| emitter.alive_particles)
                .sum::<u32>(),
            profiled_samples.len() as u32
        );
        let expected_submissions = profiled_samples.iter().fold(0_u32, |total, sample| {
            total.saturating_add(
                profiled.effect().emitters[sample.emitter_index]
                    .renderers
                    .len() as u32,
            )
        });
        assert_eq!(
            profile.submitted_instances,
            ProfileValue::Measured(expected_submissions)
        );

        baseline.advance(1.0 / 60.0);
        profiled.advance(1.0 / 60.0);
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);
    }

    #[test]
    fn history_is_bounded_and_resettable() {
        let session = test_support::session_with_timing_slack();
        let compiled = session.preview.as_ref().unwrap().effect();
        let mut profiler = ProfilerState::default();
        for frame in 0..(PROFILER_HISTORY_SAMPLES + 12) {
            profiler.ingest(ProfilerFrameSample::new(
                compiled,
                &session.samples,
                Duration::from_micros(frame as u64 + 1),
            ));
        }
        assert_eq!(profiler.cpu_history_ns.len(), PROFILER_HISTORY_SAMPLES);
        let profile = profiler.profile.as_mut().unwrap();
        profile.alive_particles = ProfileValue::Measured(3);
        profile.peak_particles = ProfileValue::Measured(9);
        profile.emitters[0].alive_particles = 2;
        profile.emitters[0].peak_particles = 7;
        profiler.reset_peaks();
        assert!(profiler.cpu_history_ns.is_empty());
        let profile = profiler.profile.as_ref().unwrap();
        assert_eq!(profile.peak_particles, ProfileValue::Measured(3));
        assert_eq!(profile.emitters[0].peak_particles, 2);
    }

    #[test]
    fn subsequent_frames_update_the_existing_profile() {
        let session = test_support::session_with_timing_slack();
        let compiled = session.preview.as_ref().unwrap().effect();
        let mut profiler = ProfilerState::default();
        profiler.ingest(ProfilerFrameSample::new(
            compiled,
            &session.samples,
            Duration::from_micros(1),
        ));
        assert_eq!(
            profiler.ingest(ProfilerFrameSample::new(
                compiled,
                &session.samples,
                Duration::from_micros(2),
            )),
            ProfilerIngestOutcome::Updated
        );
        assert_eq!(profiler.cpu_history_ns.len(), 2);
    }
}
