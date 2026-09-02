//! Referenced-effect navigation and Properties presentation.

use super::*;
use crate::feathers::breadcrumb::{BreadcrumbItem, BreadcrumbProps, spawn_breadcrumb};
use crate::timeline::resolve_effect_clip_path;

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EffectClipRepairState {
    pub(super) query: String,
}

#[derive(Component)]
pub(super) struct EffectClipRepairSearchInput;

#[derive(Component, Debug, Clone)]
pub(super) struct EffectClipRepairCandidate {
    search_text: String,
}

#[derive(Component)]
pub(super) struct EffectClipRepairEmpty;

pub(super) fn update_effect_clip_repair_query(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<EffectClipRepairSearchInput>>,
    mut state: ResMut<EffectClipRepairState>,
) {
    if inputs.contains(change.source) && state.query != change.value {
        state.query.clone_from(&change.value);
    }
}

pub(super) fn sync_effect_clip_repair_candidates(
    state: Res<EffectClipRepairState>,
    mut candidates: Query<(&EffectClipRepairCandidate, &mut Node)>,
    mut empty_states: Query<
        &mut Node,
        (
            With<EffectClipRepairEmpty>,
            Without<EffectClipRepairCandidate>,
        ),
    >,
) {
    let query = state.query.trim().to_lowercase();
    let mut visible = 0;
    for (candidate, mut node) in &mut candidates {
        let matches = query.is_empty() || candidate.search_text.contains(&query);
        node.display = if matches {
            visible += 1;
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut empty_states {
        node.display = if visible == 0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[derive(Component, Clone, Copy)]
pub(super) struct EffectClipPropertiesTimingText {
    clip: EffectClipId,
    field: EffectClipPropertiesTimingField,
}

#[derive(Clone, Copy)]
enum EffectClipPropertiesTimingField {
    Start,
    SourceOffset,
    Duration,
}

pub(super) fn sync_effect_clip_properties_timing(
    session: Res<EditorSession>,
    timeline: Option<Res<TimelineState>>,
    mut texts: Query<(&EffectClipPropertiesTimingText, &mut Text)>,
) {
    for (marker, mut text) in &mut texts {
        let timing = timeline
            .as_ref()
            .and_then(|state| state.effect_clip_preview_timing(marker.clip))
            .or_else(|| {
                session
                    .effect
                    .effect_clips
                    .iter()
                    .find(|clip| clip.id == marker.clip)
                    .map(|clip| (clip.start_time, clip.source_offset, clip.duration))
            });
        let Some((start, source_offset, duration)) = timing else {
            continue;
        };
        let value = match marker.field {
            EffectClipPropertiesTimingField::Start => start,
            EffectClipPropertiesTimingField::SourceOffset => source_offset,
            EffectClipPropertiesTimingField::Duration => duration,
        };
        text.0 = format!("{value:.3} s");
    }
}

fn effect_clip_catalog_name(catalog: &ProjectEffectCatalog, source: EffectAssetRef) -> String {
    catalog
        .entries()
        .iter()
        .find(|entry| entry.reference == Some(source))
        .map_or_else(|| source.id.to_string(), |entry| entry.display_name.clone())
}

pub(super) fn effect_clip_breadcrumbs(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    path: &EffectClipPath,
) -> Vec<(String, Option<DocumentAction>)> {
    let mut breadcrumbs = vec![(session.effect.name.clone(), None)];
    let mut effect = session.effect.clone();
    for id in path.ids() {
        let Some(clip) = effect.effect_clips.iter().find(|clip| clip.id == *id) else {
            break;
        };
        breadcrumbs.push((
            effect_clip_catalog_name(catalog, clip.source),
            Some(DocumentAction::OpenSource(clip.source)),
        ));
        let Ok(source) = catalog.load_effect(clip.source) else {
            break;
        };
        effect = source;
    }
    breadcrumbs
}

pub(super) fn spawn_source_navigation_row(
    parent: &mut ChildSpawnerCommands,
    breadcrumbs: &[(String, Option<DocumentAction>)],
    trailing_action: Option<(DocumentAction, &str)>,
    explode_clip: Option<(EffectClipId, &str)>,
    asset_server: &AssetServer,
) {
    let items = breadcrumbs
        .iter()
        .map(|(label, action)| BreadcrumbItem {
            label: label.clone(),
            action: *action,
        })
        .collect::<Vec<_>>();
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(theme::BORDER.with_alpha(0.65)))
        .with_children(|row| {
            spawn_breadcrumb(
                row,
                &items,
                BreadcrumbProps {
                    height: 28.0,
                    font: fonts::REGULAR,
                    font_size: 9.0,
                    text_offset_y: 0.0,
                    uppercase: false,
                    flex_grow: 1.0,
                    max_ancestor_width: 180.0,
                    max_current_width: 180.0,
                    ancestor_color: theme::TEXT,
                    current_color: theme::ACCENT,
                    compact_ancestors: false,
                    overflow_label: "",
                    current_tooltip: None,
                    ancestor_tooltips: false,
                },
                asset_server,
            );
            if let Some((action, label)) = trailing_action {
                spawn_feathers_action_button(row, label, action, false);
            }
            if let Some((clip, label)) = explode_clip {
                spawn_feathers_action_button(
                    row,
                    label,
                    crate::library::LibraryAction::ExplodeEffectClip(clip),
                    false,
                );
            }
        });
}

fn spawn_edit_source_navigation(
    parent: &mut ChildSpawnerCommands,
    breadcrumbs: &[(String, Option<DocumentAction>)],
    source: EffectAssetRef,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    asset_server: &AssetServer,
    explode_clip: Option<EffectClipId>,
) {
    if catalog.openable_path(source).is_some() {
        spawn_source_navigation_row(
            parent,
            breadcrumbs,
            Some((
                DocumentAction::OpenSource(source),
                &localizer.text("properties-edit-source"),
            )),
            explode_clip
                .map(|clip| (clip, localizer.text("properties-explode-effect-clip")))
                .as_ref()
                .map(|(clip, label)| (*clip, label.as_str())),
            asset_server,
        );
    }
}

pub(super) fn effect_clip_repair_source(
    catalog: &ProjectEffectCatalog,
    owner: &EffectAsset,
    clip: &EffectClip,
    source: EffectAssetRef,
) -> Result<EffectAsset, String> {
    if source == clip.source {
        return Err("select a different source effect".into());
    }
    let source_effect = catalog.effect_for_placement(owner, source)?;
    let source_end = clip.source_offset + clip.duration;
    if !source_effect.playback_mode.is_looping()
        && source_end > source_effect.duration + f32::EPSILON
    {
        return Err(format!(
            "the clip window ends at {source_end:.3} s, beyond the source duration of {:.3} s",
            source_effect.duration
        ));
    }
    Ok(source_effect)
}

fn repair_candidate_matches(query: &str, name: &str, path: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || name.to_lowercase().contains(&query) || path.to_lowercase().contains(&query)
}

fn spawn_effect_clip_repair(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    repair: &EffectClipRepairState,
    localizer: &Localizer,
    clip: &EffectClip,
    dependency_error: &str,
) {
    spawn_read_only_card(
        parent,
        localizer.text("properties-repair-reference"),
        |card| {
            card.spawn((
                Text::new(dependency_error),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Pickable::IGNORE,
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|search| {
                spawn_search_field(
                    search,
                    &repair.query,
                    &localizer.text("properties-repair-search-placeholder"),
                    &localizer.text("properties-repair-search-clear"),
                    EffectClipRepairSearchInput,
                );
            });

            let mut compatible = 0;
            for entry in catalog.entries() {
                let Some(reference) = entry.reference else {
                    continue;
                };
                if effect_clip_repair_source(catalog, &session.effect, clip, reference).is_err() {
                    continue;
                }
                let path = entry.path.display().to_string();
                let visible = repair_candidate_matches(&repair.query, &entry.display_name, &path);
                let accessible = format!(
                    "{} {}",
                    localizer.text("properties-repair-reference"),
                    entry.display_name
                );
                let row = spawn_action_list_row(
                    card,
                    &entry.display_name,
                    Some(&path),
                    None,
                    &accessible,
                    PropertiesAction::RepairEffectClipSource {
                        clip: clip.id,
                        source: reference,
                    },
                );
                card.commands()
                    .entity(row)
                    .insert(EffectClipRepairCandidate {
                        search_text: format!("{} {}", entry.display_name, path).to_lowercase(),
                    });
                compatible += usize::from(visible);
            }
            let empty = spawn_list_empty_state(
                card,
                &localizer.text("properties-repair-no-results-title"),
                &localizer.text("properties-repair-no-results-message"),
                theme::TEXT_MUTED,
                if compatible == 0 {
                    Display::Flex
                } else {
                    Display::None
                },
            );
            card.commands().entity(empty).insert(EffectClipRepairEmpty);
        },
    );
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EffectClipParameterEntry {
    pub(super) id: ParameterId,
    pub(super) name: String,
    pub(super) value: Value,
    pub(super) overridden: bool,
    pub(super) issue: Option<EffectClipParameterIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EffectClipParameterIssue {
    Missing,
    Hidden,
    TypeMismatch {
        expected: ValueType,
        actual: ValueType,
    },
    SourceUnavailable,
}

pub(super) fn effect_clip_parameter_entries(
    clip: &EffectClip,
    source: Option<&EffectAsset>,
) -> Vec<EffectClipParameterEntry> {
    let mut entries = Vec::new();
    if let Some(source) = source {
        for parameter in source
            .parameters
            .iter()
            .filter(|parameter| parameter.exposed)
        {
            let authored = clip.parameter_overrides.get(&parameter.id);
            let issue = authored.and_then(|value| {
                (value.value_type() != parameter.default.value_type()).then_some(
                    EffectClipParameterIssue::TypeMismatch {
                        expected: parameter.default.value_type(),
                        actual: value.value_type(),
                    },
                )
            });
            entries.push(EffectClipParameterEntry {
                id: parameter.id,
                name: parameter.name.clone(),
                value: authored
                    .cloned()
                    .unwrap_or_else(|| parameter.default.clone()),
                overridden: authored.is_some(),
                issue,
            });
        }
        for (&id, value) in &clip.parameter_overrides {
            let parameter = source
                .parameters
                .iter()
                .find(|parameter| parameter.id == id);
            if parameter.is_some_and(|parameter| parameter.exposed) {
                continue;
            }
            entries.push(EffectClipParameterEntry {
                id,
                name: parameter.map_or_else(|| id.to_string(), |parameter| parameter.name.clone()),
                value: value.clone(),
                overridden: true,
                issue: Some(if parameter.is_some() {
                    EffectClipParameterIssue::Hidden
                } else {
                    EffectClipParameterIssue::Missing
                }),
            });
        }
    } else {
        entries.extend(clip.parameter_overrides.iter().map(|(&id, value)| {
            EffectClipParameterEntry {
                id,
                name: id.to_string(),
                value: value.clone(),
                overridden: true,
                issue: Some(EffectClipParameterIssue::SourceUnavailable),
            }
        }));
    }
    entries
}

fn spawn_effect_clip_instance_parameters(
    parent: &mut ChildSpawnerCommands,
    clip: &EffectClip,
    source: Option<&EffectAsset>,
    localizer: &Localizer,
) {
    let entries = effect_clip_parameter_entries(clip, source);
    spawn_read_only_card(
        parent,
        localizer.text("properties-instance-parameters"),
        |card| {
            if entries.is_empty() {
                card.spawn_empty().apply_scene(label_dim(
                    localizer.text("properties-instance-parameters-empty"),
                ));
                return;
            }
            for entry in &entries {
                spawn_effect_clip_parameter_row(card, clip.id, entry, localizer);
            }
        },
    );
}

fn spawn_effect_clip_parameter_row(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    entry: &EffectClipParameterEntry,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            padding: UiRect::vertical(Val::Px(3.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|column| {
            column
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(27.0),
                    min_width: Val::Px(0.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(5.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(&entry.name),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            width: Val::Px(92.0),
                            flex_shrink: 0.0,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                    ));
                    row.spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        column_gap: Val::Px(4.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|controls| {
                        if entry.issue.is_none() {
                            spawn_effect_clip_parameter_control(
                                controls,
                                clip,
                                entry.id,
                                &entry.name,
                                &entry.value,
                            );
                        } else {
                            controls
                                .spawn_empty()
                                .apply_scene(label_dim(format_value(entry.value.clone())));
                        }
                    });
                    if entry.overridden {
                        row.spawn((
                            Text::new(localizer.text("properties-parameter-overridden")),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::ACCENT),
                            EffectClipParameterOverrideIndicator(entry.id),
                            Pickable::IGNORE,
                        ));
                        let reset = mini_button(
                            row,
                            "↺",
                            PropertiesAction::ResetEffectClipParameter {
                                clip,
                                parameter: entry.id,
                            },
                        );
                        row.commands().entity(reset).insert((
                            EditorTooltip::description(
                                localizer.text("properties-reset-to-source"),
                            ),
                            AccessibleLabel(localizer.text("properties-reset-to-source")),
                        ));
                    }
                });
            if let Some(issue) = entry.issue {
                column.spawn((
                    Text::new(effect_clip_parameter_issue_text(localizer, issue)),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.94, 0.55, 0.27)),
                    EffectClipParameterDiagnostic(entry.id),
                    Pickable::IGNORE,
                ));
            }
        });
}

fn spawn_effect_clip_parameter_control(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    parameter: ParameterId,
    name: &str,
    value: &Value,
) {
    match value {
        Value::Bool(value) => {
            let mut checkbox = parent.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                EffectClipParameterToggleControl {
                    clip,
                    parameter,
                    value: *value,
                },
                AccessibleLabel(name.to_owned()),
            ));
            if *value {
                checkbox.insert(Checked);
            }
        }
        Value::U32(_) => {
            parent
                .spawn_empty()
                .apply_scene(ui_shell::feathers_integer_input())
                .insert((
                    EffectClipParameterNumberControl {
                        clip,
                        parameter,
                        value: value.clone(),
                        component: 0,
                    },
                    AccessibleLabel(name.to_owned()),
                ));
        }
        Value::Scalar(_) => {
            spawn_effect_clip_parameter_scalar_input(parent, clip, parameter, name, value, "", 0)
        }
        Value::Vec2(_) => {
            for (axis, component) in [("X", 0), ("Y", 1)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Vec3(_) => {
            for (axis, component) in [("X", 0), ("Y", 1), ("Z", 2)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Vec4(_) => {
            for (axis, component) in [("X", 0), ("Y", 1), ("Z", 2), ("W", 3)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Range(_) => {
            for (axis, component) in [("MIN", 0), ("MAX", 1)] {
                spawn_effect_clip_parameter_scalar_input(
                    parent, clip, parameter, name, value, axis, component,
                );
            }
        }
        Value::Text(value) => {
            spawn_text_input(
                parent,
                value,
                name,
                EffectClipParameterTextControl {
                    clip,
                    parameter,
                    value: value.clone(),
                },
            );
        }
        _ => {
            parent
                .spawn_empty()
                .apply_scene(label_dim(format_value(value.clone())));
        }
    }
}

fn spawn_effect_clip_parameter_scalar_input(
    parent: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    parameter: ParameterId,
    name: &str,
    value: &Value,
    axis: &'static str,
    component: u8,
) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_width: Val::Px(44.0),
            ..default()
        })
        .with_children(|wrapper| {
            let mut input = wrapper.spawn_empty();
            if axis.is_empty() {
                input.apply_scene(ui_shell::feathers_scalar_input());
            } else {
                let color = match axis {
                    "X" | "MIN" => tokens::TEXT_INPUT_X_AXIS,
                    "Y" | "MAX" => tokens::TEXT_INPUT_Y_AXIS,
                    "Z" => tokens::TEXT_INPUT_Z_AXIS,
                    _ => tokens::TEXT_INPUT_BG,
                };
                input.apply_scene(ui_shell::feathers_labeled_scalar_input(axis, color));
            }
            input.insert((
                EffectClipParameterNumberControl {
                    clip,
                    parameter,
                    value: value.clone(),
                    component,
                },
                AccessibleLabel(if axis.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name} {axis}")
                }),
            ));
        });
}

fn effect_clip_parameter_issue_text(
    localizer: &Localizer,
    issue: EffectClipParameterIssue,
) -> String {
    match issue {
        EffectClipParameterIssue::Missing => {
            localizer.text("properties-parameter-override-missing")
        }
        EffectClipParameterIssue::Hidden => localizer.text("properties-parameter-override-hidden"),
        EffectClipParameterIssue::TypeMismatch { expected, actual } => {
            let mut args = FluentArgs::new();
            args.set("expected", format!("{expected:?}"));
            args.set("actual", format!("{actual:?}"));
            localizer.text_with("properties-parameter-override-type-mismatch", &args)
        }
        EffectClipParameterIssue::SourceUnavailable => {
            localizer.text("properties-parameter-override-source-unavailable")
        }
    }
}

pub(super) fn spawn_effect_clip_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    repair: &EffectClipRepairState,
    localizer: &Localizer,
    id: EffectClipId,
    asset_server: &AssetServer,
) -> bool {
    let Some(clip) = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == id)
    else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    let source = catalog.load_effect(clip.source).ok();
    let dependency_error = catalog.effect_clip_dependency_error(&session.effect, clip.id);
    spawn_read_only_properties_shell(parent, &source_name, localizer, true, |stack| {
        spawn_edit_source_navigation(
            stack,
            &[
                (session.effect.name.clone(), None),
                (
                    source_name.clone(),
                    Some(DocumentAction::OpenSource(clip.source)),
                ),
            ],
            clip.source,
            catalog,
            localizer,
            asset_server,
            Some(clip.id),
        );
        spawn_read_only_card(stack, localizer.text("properties-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_start_reference_controls(
                card,
                session,
                StartReferenceTarget::EffectClip(clip.id),
                localizer,
            );
            let start = spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", clip.start_time),
            );
            card.commands()
                .entity(start)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::Start,
                });
            let source_offset = spawn_read_only_row(
                card,
                localizer.text("properties-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            card.commands()
                .entity(source_offset)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::SourceOffset,
                });
            let duration = spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", clip.duration),
            );
            card.commands()
                .entity(duration)
                .insert(EffectClipPropertiesTimingText {
                    clip: clip.id,
                    field: EffectClipPropertiesTimingField::Duration,
                });
            spawn_read_only_row(
                card,
                localizer.text("properties-seed"),
                format!("{:?}", clip.seed),
            );
        });
        spawn_effect_clip_transform_controls(stack, clip.id);
        spawn_effect_clip_instance_parameters(stack, clip, source.as_ref(), localizer);
        if let Some(error) = dependency_error.as_deref() {
            spawn_effect_clip_repair(stack, session, catalog, repair, localizer, clip, error);
        }
        spawn_read_only_card(stack, localizer.text("properties-source-summary"), |card| {
            if let Some(source) = &source {
                spawn_read_only_row(card, localizer.text("properties-name"), &source.name);
                spawn_read_only_row(
                    card,
                    localizer.text("properties-duration"),
                    format!("{:.3} s", source.duration),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-emitters"),
                    source.emitters.len().to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-looping"),
                    source.playback_mode.is_looping().to_string(),
                );
            } else {
                spawn_read_only_row(
                    card,
                    localizer.text("properties-status"),
                    localizer.text("properties-source-unavailable"),
                );
            }
        });
        stack.spawn((
            Text::new(localizer.text("properties-effect-clip-read-only-description")),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
    true
}

pub(super) fn spawn_referenced_emitter_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
    selected_emitter: EmitterId,
    asset_server: &AssetServer,
) -> bool {
    let Some((clip, source)) = resolve_effect_clip_path(session, catalog, path) else {
        return false;
    };
    let Some(emitter) = source
        .emitters
        .iter()
        .find(|emitter| emitter.id == selected_emitter)
    else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    spawn_read_only_properties_shell(parent, &emitter.name, localizer, false, |stack| {
        let mut breadcrumbs = effect_clip_breadcrumbs(session, catalog, path);
        breadcrumbs.push((emitter.name.clone(), None));
        spawn_edit_source_navigation(
            stack,
            &breadcrumbs,
            clip.source,
            catalog,
            localizer,
            asset_server,
            None,
        );
        spawn_read_only_card(stack, localizer.text("properties-reference"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_read_only_row(card, localizer.text("properties-emitter"), &emitter.name);
            spawn_read_only_row(
                card,
                localizer.text("properties-mode"),
                localizer.text("properties-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-emitter"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-enabled"),
                emitter.enabled.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-capacity"),
                emitter.max_particles.to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", emitter.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", emitter.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-domain"),
                format!("{:?}", emitter.simulation_domain),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    emitter.transform.translation[0],
                    emitter.transform.translation[1],
                    emitter.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-rotation"),
                format!(
                    "{:.3}, {:.3}, {:.3}, {:.3}",
                    emitter.transform.rotation[0],
                    emitter.transform.rotation[1],
                    emitter.transform.rotation[2],
                    emitter.transform.rotation[3]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-scale"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    emitter.transform.scale[0],
                    emitter.transform.scale[1],
                    emitter.transform.scale[2]
                ),
            );
        });
        for module in &emitter.modules {
            spawn_read_only_card(stack, &module.module_type.0, |card| {
                spawn_read_only_row(
                    card,
                    localizer.text("properties-stage"),
                    format!("{:?}", module.stage),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-enabled"),
                    module.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-parameters"),
                    format!("{:?}", module.parameters),
                );
            });
        }
        for renderer in &emitter.renderers {
            spawn_read_only_card(stack, &renderer.renderer_type.0, |card| {
                spawn_read_only_row(
                    card,
                    localizer.text("properties-enabled"),
                    renderer.enabled.to_string(),
                );
                spawn_read_only_row(
                    card,
                    localizer.text("properties-section-properties"),
                    format!("{:?}", renderer.properties),
                );
            });
        }
    });
    true
}

pub(super) fn spawn_referenced_effect_clip_properties(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    path: &EffectClipPath,
    asset_server: &AssetServer,
) -> bool {
    let Some((clip, source)) = resolve_effect_clip_path(session, catalog, path) else {
        return false;
    };
    let source_name = effect_clip_catalog_name(catalog, clip.source);
    spawn_read_only_properties_shell(parent, &source_name, localizer, false, |stack| {
        spawn_edit_source_navigation(
            stack,
            &effect_clip_breadcrumbs(session, catalog, path),
            clip.source,
            catalog,
            localizer,
            asset_server,
            None,
        );
        spawn_read_only_card(stack, localizer.text("properties-effect-clip"), |card| {
            spawn_read_only_row(card, localizer.text("properties-source"), &source_name);
            spawn_read_only_row(
                card,
                localizer.text("properties-start"),
                format!("{:.3} s", clip.start_time),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-source-offset"),
                format!("{:.3} s", clip.source_offset),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", clip.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-seed"),
                format!("{:?}", clip.seed),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-mode"),
                localizer.text("properties-read-only"),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-transform"), |card| {
            spawn_read_only_row(
                card,
                localizer.text("properties-position"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.translation[0],
                    clip.transform.translation[1],
                    clip.transform.translation[2]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-rotation"),
                format!(
                    "{:.3}, {:.3}, {:.3}, {:.3}",
                    clip.transform.rotation[0],
                    clip.transform.rotation[1],
                    clip.transform.rotation[2],
                    clip.transform.rotation[3]
                ),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-scale"),
                format!(
                    "{:.3}, {:.3}, {:.3}",
                    clip.transform.scale[0], clip.transform.scale[1], clip.transform.scale[2]
                ),
            );
        });
        spawn_read_only_card(stack, localizer.text("properties-source-summary"), |card| {
            spawn_read_only_row(card, localizer.text("properties-name"), &source.name);
            spawn_read_only_row(
                card,
                localizer.text("properties-duration"),
                format!("{:.3} s", source.duration),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-emitters"),
                source.emitters.len().to_string(),
            );
            spawn_read_only_row(
                card,
                localizer.text("properties-looping"),
                source.playback_mode.is_looping().to_string(),
            );
        });
        stack.spawn((
            Text::new(localizer.text("properties-effect-clip-read-only-description")),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
    true
}
