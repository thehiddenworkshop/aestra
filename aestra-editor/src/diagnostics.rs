//! Diagnostics workspace, semantic navigation, and compile-status presentation.

use crate::feathers::panel::spawn_panel_empty_state;
use crate::*;
use aestra_bevy::{Diagnostic, DiagnosticCode, DiagnosticSeverity, EffectAsset, ValidationReport};
use bevy::ui_widgets::Activate;

pub(crate) struct EditorDiagnosticsPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DiagnosticsSet {
    Actions,
    Sync,
}

impl Plugin for EditorDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DiagnosticsPanelState>()
            .add_observer(queue_diagnostics_action_activation)
            .add_systems(
                Update,
                (
                    handle_diagnostics_actions.in_set(DiagnosticsSet::Actions),
                    update_compile_status.in_set(DiagnosticsSet::Sync),
                ),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticsAction {
    OpenPanel,
    SetFilter(DiagnosticsFilter),
    Select {
        source: DiagnosticSource,
        index: usize,
    },
}

#[derive(Resource, Default)]
pub(crate) struct DiagnosticsPanelState {
    filter: DiagnosticsFilter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum DiagnosticsFilter {
    #[default]
    All,
    Errors,
    Warnings,
    Info,
}

impl DiagnosticsFilter {
    const ALL: [Self; 4] = [Self::All, Self::Errors, Self::Warnings, Self::Info];

    fn message_id(self) -> &'static str {
        match self {
            Self::All => "diagnostics-filter-all",
            Self::Errors => "diagnostics-errors",
            Self::Warnings => "diagnostics-warnings",
            Self::Info => "diagnostics-info",
        }
    }

    fn matches(self, severity: DiagnosticSeverity) -> bool {
        match self {
            Self::All => true,
            Self::Errors => severity == DiagnosticSeverity::Error,
            Self::Warnings => severity == DiagnosticSeverity::Warning,
            Self::Info => severity == DiagnosticSeverity::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticSource {
    Current,
    Pending,
}

#[derive(Component)]
struct CompileStatusLabel;

#[derive(Component)]
struct CompileStatusDot;

fn queue_diagnostics_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<DiagnosticsAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_diagnostics_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &DiagnosticsAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<DiagnosticsPanelState>,
    mut workspace: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL,
            Interaction::Pressed => {
                if feathers.is_some() {
                    if pending.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                match *action {
                    DiagnosticsAction::OpenPanel => {
                        reveal_dock_panel(&mut layout, &mut session, DockPanel::Diagnostics);
                    }
                    DiagnosticsAction::SetFilter(filter) => {
                        if state.filter != filter {
                            state.filter = filter;
                            session.ui_revision += 1;
                        }
                    }
                    DiagnosticsAction::Select { source, index } => {
                        if navigate_to_diagnostic(&mut session, source, index) {
                            workspace.clear();
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Inspector);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn spawn_diagnostics_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &DiagnosticsPanelState,
    localizer: &Localizer,
) {
    let current = &session.diagnostics.diagnostics;
    let pending = session
        .pending_change
        .as_ref()
        .map(|pending| pending.diagnostics.diagnostics.as_slice())
        .unwrap_or_default();
    let all = current.iter().chain(pending.iter());
    let errors = all
        .clone()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let warnings = all
        .clone()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count();
    let info = all
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Info)
        .count();
    let visible = current
        .iter()
        .chain(pending.iter())
        .filter(|diagnostic| state.filter.matches(diagnostic.severity))
        .count();

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
                        column_gap: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new(localizer.text("diagnostics-validation")),
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
                    spawn_diagnostic_count(
                        header,
                        errors,
                        "diagnostics-errors",
                        Color::srgb(1.0, 0.38, 0.32),
                        localizer,
                    );
                    spawn_diagnostic_count(
                        header,
                        warnings,
                        "diagnostics-warnings",
                        Color::srgb(1.0, 0.74, 0.30),
                        localizer,
                    );
                    spawn_diagnostic_count(
                        header,
                        info,
                        "diagnostics-info",
                        Color::srgb(0.45, 0.70, 1.0),
                        localizer,
                    );
                });
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(36.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(10.0)),
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|filters| {
                    for filter in DiagnosticsFilter::ALL {
                        let count = match filter {
                            DiagnosticsFilter::All => errors + warnings + info,
                            DiagnosticsFilter::Errors => errors,
                            DiagnosticsFilter::Warnings => warnings,
                            DiagnosticsFilter::Info => info,
                        };
                        spawn_diagnostics_filter_button(
                            filters,
                            filter,
                            state.filter == filter,
                            count,
                            localizer,
                        );
                    }
                });

            if errors + warnings + info == 0 {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("diagnostics-no-issues"),
                    &localizer.text("diagnostics-no-issues-description"),
                    Color::srgb(0.35, 0.88, 0.57),
                );
                return;
            }
            if visible == 0 {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("diagnostics-no-matches"),
                    &localizer.text("diagnostics-no-matches-description"),
                    theme::TEXT_MUTED,
                );
                return;
            }

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Diagnostics,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(8.0)),
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                        |list| {
                            spawn_diagnostic_section(
                                list,
                                &localizer.text("diagnostics-working-effect"),
                                &session.diagnostics,
                                DiagnosticSource::Current,
                                state.filter,
                                localizer,
                            );
                            if let Some(pending) = &session.pending_change {
                                spawn_diagnostic_section(
                                    list,
                                    &localizer.text("diagnostics-pending-transaction"),
                                    &pending.diagnostics,
                                    DiagnosticSource::Pending,
                                    state.filter,
                                    localizer,
                                );
                            }
                        },
                    );
                });
        });
}

fn spawn_diagnostics_filter_button(
    parent: &mut ChildSpawnerCommands,
    filter: DiagnosticsFilter,
    selected: bool,
    count: usize,
    localizer: &Localizer,
) {
    let label = format!("{} {count}", localizer.text(filter.message_id()));
    let mut button = parent.spawn_empty();
    if selected {
        button.apply_scene(ui_shell::feathers_primary_button());
    } else {
        button.apply_scene(ui_shell::feathers_button());
    }
    button
        .insert((
            DiagnosticsAction::SetFilter(filter),
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
                TextColor(match filter {
                    DiagnosticsFilter::Errors => Color::srgb(1.0, 0.38, 0.32),
                    DiagnosticsFilter::Warnings => Color::srgb(1.0, 0.74, 0.30),
                    DiagnosticsFilter::All | DiagnosticsFilter::Info => theme::TEXT_MUTED,
                }),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_diagnostic_section(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    report: &ValidationReport,
    source: DiagnosticSource,
    filter: DiagnosticsFilter,
    localizer: &Localizer,
) {
    if !report
        .diagnostics
        .iter()
        .any(|diagnostic| filter.matches(diagnostic.severity))
    {
        return;
    }
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
        Node {
            margin: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
            ..default()
        },
    ));
    for (index, diagnostic) in report.diagnostics.iter().enumerate() {
        if !filter.matches(diagnostic.severity) {
            continue;
        }
        spawn_diagnostic_row(parent, diagnostic, source, index, localizer);
    }
}

fn spawn_diagnostic_row(
    parent: &mut ChildSpawnerCommands,
    diagnostic: &Diagnostic,
    source: DiagnosticSource,
    index: usize,
    localizer: &Localizer,
) {
    let (label, color) = diagnostic_severity_style(diagnostic.severity, localizer);
    let code = localizer.text(diagnostic_code_message(diagnostic.code));
    parent
        .spawn((
            Button,
            EditorNativeControl,
            DiagnosticsAction::Select { source, index },
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(64.0),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(9.0),
                align_items: AlignItems::Stretch,
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(4.0),
                    min_height: Val::Px(48.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|content| {
                content.spawn((
                    Text::new(format!("{label}  ·  {code}")),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(color),
                    Pickable::IGNORE,
                ));
                content.spawn((
                    Text::new(&diagnostic.message),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
                content.spawn((
                    Text::new(&diagnostic.path),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Pickable::IGNORE,
                ));
            });
        });
}

fn diagnostic_severity_style(
    severity: DiagnosticSeverity,
    localizer: &Localizer,
) -> (String, Color) {
    let (message, color) = match severity {
        DiagnosticSeverity::Error => ("diagnostics-severity-error", Color::srgb(1.0, 0.38, 0.32)),
        DiagnosticSeverity::Warning => {
            ("diagnostics-severity-warning", Color::srgb(1.0, 0.74, 0.30))
        }
        DiagnosticSeverity::Info => ("diagnostics-severity-info", Color::srgb(0.45, 0.70, 1.0)),
    };
    (localizer.text(message), color)
}

fn diagnostic_code_message(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::UnsupportedFormat => "diagnostics-code-unsupported-format",
        DiagnosticCode::NilId => "diagnostics-code-nil-id",
        DiagnosticCode::DuplicateId => "diagnostics-code-duplicate-id",
        DiagnosticCode::InvalidDuration => "diagnostics-code-invalid-duration",
        DiagnosticCode::InvalidTiming => "diagnostics-code-invalid-timing",
        DiagnosticCode::InvalidCapacity => "diagnostics-code-invalid-capacity",
        DiagnosticCode::MissingModule => "diagnostics-code-missing-module",
        DiagnosticCode::DuplicateModule => "diagnostics-code-duplicate-module",
        DiagnosticCode::StageMismatch => "diagnostics-code-stage-mismatch",
        DiagnosticCode::InvalidValue => "diagnostics-code-invalid-value",
        DiagnosticCode::MissingRenderer => "diagnostics-code-missing-renderer",
        DiagnosticCode::InvalidReference => "diagnostics-code-invalid-reference",
        DiagnosticCode::ReferenceCycle => "diagnostics-code-reference-cycle",
        DiagnosticCode::UnknownModule => "diagnostics-code-unknown-module",
        DiagnosticCode::UnsupportedRenderer => "diagnostics-code-unsupported-renderer",
        DiagnosticCode::MissingAttribute => "diagnostics-code-missing-attribute",
        DiagnosticCode::UnknownParameter => "diagnostics-code-unknown-parameter",
        DiagnosticCode::ParameterTypeMismatch => "diagnostics-code-parameter-type-mismatch",
    }
}

fn spawn_diagnostic_count(
    parent: &mut ChildSpawnerCommands,
    count: usize,
    message_id: &str,
    active_color: Color,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(format!("{count} {}", localizer.text(message_id))),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(if count == 0 {
            theme::TEXT_FAINT
        } else {
            active_color
        }),
    ));
}

pub(crate) fn spawn_compile_status(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let (compile_status, compile_color) = compile_status(session);
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            DiagnosticsAction::OpenPanel,
            FeathersActionButton,
            AccessibleLabel(localizer.text(compile_status)),
            Node {
                height: Val::Px(20.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                CompileStatusDot,
                Node {
                    width: Val::Px(6.0),
                    height: Val::Px(6.0),
                    margin: UiRect::right(Val::Px(7.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(compile_color),
                Pickable::IGNORE,
            ));
            button.spawn((
                CompileStatusLabel,
                Text::new(localizer.text(compile_status)),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(compile_color),
                Pickable::IGNORE,
            ));
        });
}

fn compile_status(session: &EditorSession) -> (&'static str, Color) {
    let current_errors = session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let pending_errors = session.pending_change.as_ref().map_or(0, |pending| {
        pending
            .diagnostics
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    });
    let warnings = session
        .diagnostics
        .diagnostics
        .iter()
        .chain(
            session
                .pending_change
                .iter()
                .flat_map(|pending| pending.diagnostics.diagnostics.iter()),
        )
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count();

    if current_errors > 0 {
        ("compile-failed", Color::srgb(1.0, 0.38, 0.32))
    } else if pending_errors > 0 {
        ("compile-preview-blocked", Color::srgb(1.0, 0.74, 0.30))
    } else if warnings > 0 {
        ("compile-with-warnings", Color::srgb(1.0, 0.74, 0.30))
    } else {
        ("compile-compiled", Color::srgb(0.35, 0.88, 0.57))
    }
}

fn update_compile_status(
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    mut labels: Query<(&mut Text, &mut TextColor), With<CompileStatusLabel>>,
    mut dots: Query<&mut BackgroundColor, With<CompileStatusDot>>,
) {
    if !session.is_changed() && !localizer.is_changed() {
        return;
    }
    let (label, color) = compile_status(&session);
    for (mut text, mut text_color) in &mut labels {
        text.0 = localizer.text(label);
        text_color.0 = color;
    }
    for mut background in &mut dots {
        background.0 = color;
    }
}

fn navigate_to_diagnostic(
    session: &mut EditorSession,
    source: DiagnosticSource,
    index: usize,
) -> bool {
    let diagnostic = match source {
        DiagnosticSource::Current => session.diagnostics.diagnostics.get(index),
        DiagnosticSource::Pending => session
            .pending_change
            .as_ref()
            .and_then(|pending| pending.diagnostics.diagnostics.get(index)),
    };
    let Some(diagnostic) = diagnostic else {
        session.status = "Diagnostic no longer exists".into();
        return false;
    };
    let path = diagnostic.path.clone();
    let code = diagnostic.code;
    let Some(target) = semantic_target_for_diagnostic_path(&session.effect, &path) else {
        session.status = format!("Diagnostic target no longer exists · {path}");
        return false;
    };
    if matches!(
        target,
        SemanticTarget::Emitter(_) | SemanticTarget::Module(_) | SemanticTarget::Renderer(_)
    ) {
        session.selection.primary = target;
    }
    session.status = format!("Selected {code:?} diagnostic · {path}");
    session.ui_revision += 1;
    true
}

fn semantic_target_for_diagnostic_path(effect: &EffectAsset, path: &str) -> Option<SemanticTarget> {
    if let Some(emitter_index) = diagnostic_collection_index(path, "emitters") {
        let emitter = effect.emitters.get(emitter_index)?;
        if let Some(module_index) = diagnostic_collection_index(path, "modules") {
            return emitter
                .modules
                .get(module_index)
                .map(|module| SemanticTarget::Module(module.id));
        }
        if let Some(renderer_index) = diagnostic_collection_index(path, "renderers") {
            return emitter
                .renderers
                .get(renderer_index)
                .map(|renderer| SemanticTarget::Renderer(renderer.id));
        }
        return Some(SemanticTarget::Emitter(emitter.id));
    }
    if let Some(parameter_index) = diagnostic_collection_index(path, "parameters") {
        return effect
            .parameters
            .get(parameter_index)
            .map(|parameter| SemanticTarget::Parameter(parameter.id));
    }
    if let Some(event_index) = diagnostic_collection_index(path, "events") {
        return effect
            .events
            .get(event_index)
            .map(|event| SemanticTarget::Event(event.id));
    }
    path.starts_with("effect")
        .then_some(SemanticTarget::Effect(effect.id))
}

fn diagnostic_collection_index(path: &str, collection: &str) -> Option<usize> {
    let marker = format!("{collection}[");
    let start = path.find(&marker)? + marker.len();
    let end = start + path[start..].find(']')?;
    path[start..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_action_updates_plugin_state_and_requests_a_rebuild() {
        let mut app = App::new();
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let revision = session.ui_revision;
        app.insert_resource(session)
            .init_resource::<DiagnosticsPanelState>()
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .add_systems(Update, handle_diagnostics_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            DiagnosticsAction::SetFilter(DiagnosticsFilter::Warnings),
            BackgroundColor(theme::BUTTON),
        ));

        app.update();

        assert_eq!(
            app.world().resource::<DiagnosticsPanelState>().filter,
            DiagnosticsFilter::Warnings
        );
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision + 1
        );
    }

    #[test]
    fn diagnostic_filters_match_only_the_selected_severity() {
        assert!(DiagnosticsFilter::All.matches(DiagnosticSeverity::Warning));
        assert!(DiagnosticsFilter::Errors.matches(DiagnosticSeverity::Error));
        assert!(!DiagnosticsFilter::Errors.matches(DiagnosticSeverity::Info));
        assert!(DiagnosticsFilter::Warnings.matches(DiagnosticSeverity::Warning));
        assert!(DiagnosticsFilter::Info.matches(DiagnosticSeverity::Info));
    }

    #[test]
    fn diagnostic_paths_resolve_to_semantic_targets() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        let emitter = &effect.emitters[1];
        assert_eq!(
            semantic_target_for_diagnostic_path(&effect, "effect.emitters[1].duration"),
            Some(SemanticTarget::Emitter(emitter.id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(
                &effect,
                "effect.emitters[1].modules[2].parameters.drag",
            ),
            Some(SemanticTarget::Module(emitter.modules[2].id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(
                &effect,
                "effect.emitters[1].renderers[0].renderer_type",
            ),
            Some(SemanticTarget::Renderer(emitter.renderers[0].id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(&effect, "not-a-semantic-path"),
            None
        );
    }

    #[test]
    fn diagnostic_navigation_selects_the_owning_module() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let expected = session.effect.emitters[2].modules[1].id;
        session.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidValue,
            "effect.emitters[2].modules[1].parameters",
            "invalid test value",
        ));

        assert!(navigate_to_diagnostic(
            &mut session,
            DiagnosticSource::Current,
            0,
        ));
        assert_eq!(session.selection.primary, SemanticTarget::Module(expected));
        assert_eq!(session.selected_layer_index(), 2);
    }

    #[test]
    fn compile_footer_reports_success_and_failure() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let localizer = Localizer::new("en-US").unwrap();
        assert_eq!(localizer.text(compile_status(&session).0), "COMPILED");

        session.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidDuration,
            "effect.duration",
            "invalid test duration",
        ));
        assert_eq!(localizer.text(compile_status(&session).0), "COMPILE FAILED");
    }
}
