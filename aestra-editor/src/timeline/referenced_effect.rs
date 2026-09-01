use super::*;
use std::time::Duration;

const REFERENCED_EMITTER_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Component, Clone, Copy)]
pub(super) struct ReferencedEmitterTrackHeader;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineReferencedEmitter {
    pub(super) clip: EffectClipId,
    pub(super) source_start: f32,
    pub(super) source_duration: f32,
}

#[derive(Component)]
pub(super) struct TimelineReferencedEmitterControl;

#[derive(Clone, Debug)]
pub(super) struct ReferencedEmitterClick {
    path: EffectClipPath,
    emitter: EmitterId,
    at: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EffectClipPath(Vec<EffectClipId>);

impl EffectClipPath {
    pub(crate) fn root_path(clip: EffectClipId) -> Self {
        Self(vec![clip])
    }

    pub(crate) fn child(&self, clip: EffectClipId) -> Self {
        let mut path = self.0.clone();
        path.push(clip);
        Self(path)
    }

    pub(crate) fn root(&self) -> EffectClipId {
        self.0[0]
    }

    pub(crate) fn ids(&self) -> &[EffectClipId] {
        &self.0
    }

    pub(super) fn starts_with(&self, ancestor: &Self) -> bool {
        self.0.starts_with(&ancestor.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EffectClipChildSelection {
    EffectClip {
        path: EffectClipPath,
    },
    Emitter {
        path: EffectClipPath,
        emitter: EmitterId,
    },
}

impl EffectClipChildSelection {
    pub(super) fn is_descendant_of(&self, ancestor: &EffectClipPath) -> bool {
        match self {
            Self::EffectClip { path } | Self::Emitter { path, .. } => path.starts_with(ancestor),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReferencedTimingContext {
    pub(super) root_source_start: f32,
    pub(super) local_source_offset: f32,
    pub(super) duration: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReferencedTrackTiming {
    pub(super) root_source_start: f32,
    pub(super) duration: f32,
}

#[derive(Clone, Debug)]
pub(super) enum ReferencedTrackKind {
    EffectClip {
        clip: EffectClip,
        source_name: String,
        child_count: usize,
    },
    Emitter {
        emitter: Emitter,
    },
}

#[derive(Clone, Debug)]
pub(super) struct ReferencedTrackProjection {
    pub(super) path: EffectClipPath,
    pub(super) depth: usize,
    pub(super) timing: Option<ReferencedTrackTiming>,
    pub(super) kind: ReferencedTrackKind,
}

pub(super) fn effect_clip_source_name(
    catalog: &ProjectEffectCatalog,
    source: EffectAssetRef,
) -> String {
    catalog
        .entries()
        .iter()
        .find(|entry| entry.reference == Some(source))
        .map_or_else(
            || format!("Missing effect {}", source.id),
            |entry| entry.display_name.clone(),
        )
}

pub(crate) fn resolve_effect_clip_path(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    path: &EffectClipPath,
) -> Option<(EffectClip, EffectAsset)> {
    let (root, descendants) = path.ids().split_first()?;
    let mut clip = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == *root)?
        .clone();
    let mut source = catalog.load_effect(clip.source).ok()?;
    for id in descendants {
        clip = source
            .effect_clips
            .iter()
            .find(|clip| clip.id == *id)?
            .clone();
        source = catalog.load_effect(clip.source).ok()?;
    }
    Some((clip, source))
}

pub(super) fn map_referenced_interval(
    context: ReferencedTimingContext,
    item_start: f32,
    item_duration: f32,
) -> Option<(ReferencedTrackTiming, f32)> {
    let source_start = item_start.max(context.local_source_offset);
    let source_end =
        (item_start + item_duration).min(context.local_source_offset + context.duration);
    (source_end > source_start).then_some((
        ReferencedTrackTiming {
            root_source_start: context.root_source_start + source_start
                - context.local_source_offset,
            duration: source_end - source_start,
        },
        source_start - item_start,
    ))
}

fn append_referenced_track_projections(
    rows: &mut Vec<ReferencedTrackProjection>,
    catalog: &ProjectEffectCatalog,
    state: &TimelineState,
    source: &EffectAsset,
    parent_path: &EffectClipPath,
    context: Option<ReferencedTimingContext>,
    depth: usize,
) {
    for track in normalized_choreography_order(source) {
        match track {
            ChoreographyTrackId::EffectClip(id) => {
                let Some(clip) = source.effect_clips.iter().find(|clip| clip.id == id) else {
                    continue;
                };
                let path = parent_path.child(clip.id);
                let mapped = context.and_then(|context| {
                    map_referenced_interval(context, clip.start_time, clip.duration)
                });
                let child_source = catalog.load_effect(clip.source).ok();
                let child_count = child_source.as_ref().map_or(0, |effect| {
                    effect.effect_clips.len() + effect.emitters.len()
                });
                rows.push(ReferencedTrackProjection {
                    path: path.clone(),
                    depth,
                    timing: mapped.map(|(timing, _)| timing),
                    kind: ReferencedTrackKind::EffectClip {
                        clip: clip.clone(),
                        source_name: effect_clip_source_name(catalog, clip.source),
                        child_count,
                    },
                });
                if state.expanded_effect_clips.contains(&path)
                    && let Some(child_source) = child_source
                {
                    let child_context =
                        mapped.map(|(timing, clipped_offset)| ReferencedTimingContext {
                            root_source_start: timing.root_source_start,
                            local_source_offset: clip.source_offset + clipped_offset,
                            duration: timing.duration,
                        });
                    append_referenced_track_projections(
                        rows,
                        catalog,
                        state,
                        &child_source,
                        &path,
                        child_context,
                        depth + 1,
                    );
                }
            }
            ChoreographyTrackId::Emitter(id) => {
                let Some(emitter) = source.emitters.iter().find(|emitter| emitter.id == id) else {
                    continue;
                };
                rows.push(ReferencedTrackProjection {
                    path: parent_path.clone(),
                    depth,
                    timing: context
                        .and_then(|context| {
                            map_referenced_interval(context, emitter.start_time, emitter.duration)
                        })
                        .map(|(timing, _)| timing),
                    kind: ReferencedTrackKind::Emitter {
                        emitter: emitter.clone(),
                    },
                });
            }
        }
    }
}

pub(super) fn referenced_track_projections(
    catalog: &ProjectEffectCatalog,
    state: &TimelineState,
    clip: &EffectClip,
) -> Vec<ReferencedTrackProjection> {
    let path = EffectClipPath::root_path(clip.id);
    if !state.expanded_effect_clips.contains(&path) {
        return Vec::new();
    }
    let Ok(source) = catalog.load_effect(clip.source) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    append_referenced_track_projections(
        &mut rows,
        catalog,
        state,
        &source,
        &path,
        Some(ReferencedTimingContext {
            root_source_start: clip.source_offset,
            local_source_offset: clip.source_offset,
            duration: clip.duration,
        }),
        1,
    );
    rows
}

pub(super) fn spawn_referenced_emitter_track_header(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    path: &EffectClipPath,
    depth: usize,
    emitter: &Emitter,
    grid_row: i16,
) {
    let selected = state.inspected_child.as_ref()
        == Some(&EffectClipChildSelection::Emitter {
            path: path.clone(),
            emitter: emitter.id,
        });
    let mut args = FluentArgs::new();
    args.set("name", emitter.name.as_str());
    let label = localizer.text_with("timeline-inspect-referenced-emitter", &args);
    let color = layer_color(emitter.id, emitter.display_color);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        ReferencedEmitterTrackHeader,
        TimelineChoreographyGridRow(grid_row),
        ChoreographyAction::SelectEffectClipEmitter {
            path: path.clone(),
            emitter: emitter.id,
        },
        AccessibleLabel(label.clone()),
        EditorTooltip::description(label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(27.0),
            flex_shrink: 0.0,
            padding: UiRect::new(
                Val::Px(7.0 + depth as f32 * 22.0),
                Val::Px(7.0),
                Val::Px(0.0),
                Val::Px(0.0),
            ),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            border: UiRect::bottom(Val::Px(1.0)),
            grid_row: GridPlacement::start(grid_row),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION.with_alpha(0.75)
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.4)),
    ));
    if selected {
        header.insert(Selected);
    }
    header.observe(open_referenced_emitter_source_from_header);
    header.with_children(|row| {
        row.spawn((
            Text::new("└"),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
        row.spawn((
            Node {
                width: Val::Px(9.0),
                height: Val::Px(9.0),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(&emitter.name),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(if emitter.enabled {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_FAINT
            }),
            TextLayout::no_wrap(),
            Node {
                min_width: Val::Px(0.0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(localizer.text("timeline-read-only-short")),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
}

pub(super) fn spawn_referenced_effect_clip_track_header(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    path: &EffectClipPath,
    depth: usize,
    clip: &EffectClip,
    source_name: &str,
    child_count: usize,
    grid_row: i16,
    asset_server: &AssetServer,
) {
    let selected = state.inspected_child.as_ref()
        == Some(&EffectClipChildSelection::EffectClip { path: path.clone() });
    let expanded = state.expanded_effect_clips.contains(path);
    let label = emitter_timing_label(localizer, "timeline-select-effect-clip", source_name);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        ReferencedEmitterTrackHeader,
        TimelineChoreographyGridRow(grid_row),
        ChoreographyAction::SelectReferencedEffectClip(path.clone()),
        AccessibleLabel(label.clone()),
        EditorTooltip::description(label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(27.0),
            flex_shrink: 0.0,
            padding: UiRect::new(
                Val::Px(7.0 + depth as f32 * 22.0),
                Val::Px(7.0),
                Val::Px(0.0),
                Val::Px(0.0),
            ),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            border: UiRect::bottom(Val::Px(1.0)),
            grid_row: GridPlacement::start(grid_row),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION.with_alpha(0.75)
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.4)),
    ));
    if selected {
        header.insert(Selected);
    }
    header.with_children(|row| {
        let disclosure = mini_button(
            row,
            "",
            ChoreographyAction::ToggleEffectClipExpanded(path.clone()),
        );
        row.commands().entity(disclosure).insert(Node {
            display: if child_count > 0 {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(20.0),
            height: Val::Px(21.0),
            flex_shrink: 0.0,
            ..default()
        });
        row.commands().entity(disclosure).with_children(|button| {
            button.spawn((
                Node {
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    ..default()
                },
                UiSvg(load_svg_icon(
                    asset_server,
                    if expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                )),
                SvgColor(theme::TEXT),
                Pickable::IGNORE,
            ));
        });
        row.spawn((
            Node {
                width: Val::Px(13.0),
                height: Val::Px(13.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(effect_reference_color(clip.source)),
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new("FX"),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::PANEL_DARK),
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(source_name),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_MUTED),
            TextLayout::no_wrap(),
            Node {
                min_width: Val::Px(0.0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(localizer.text("timeline-read-only-short")),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
}

pub(super) fn spawn_referenced_track_row(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    root_clip: &EffectClip,
    projection: &ReferencedTrackProjection,
    muted: bool,
    suppressed: bool,
    grid_row: i16,
) {
    let (selected, color, name, action) = match &projection.kind {
        ReferencedTrackKind::EffectClip {
            clip, source_name, ..
        } => (
            state.inspected_child.as_ref()
                == Some(&EffectClipChildSelection::EffectClip {
                    path: projection.path.clone(),
                }),
            effect_reference_color(clip.source),
            source_name.as_str(),
            ChoreographyAction::SelectReferencedEffectClip(projection.path.clone()),
        ),
        ReferencedTrackKind::Emitter { emitter, .. } => (
            state.inspected_child.as_ref()
                == Some(&EffectClipChildSelection::Emitter {
                    path: projection.path.clone(),
                    emitter: emitter.id,
                }),
            layer_color(emitter.id, emitter.display_color),
            emitter.name.as_str(),
            ChoreographyAction::SelectEffectClipEmitter {
                path: projection.path.clone(),
                emitter: emitter.id,
            },
        ),
    };
    parent
        .spawn((
            TimelineChoreographyGridRow(grid_row),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(27.0),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
        ))
        .with_children(|track| {
            let Some(timing) = projection.timing else {
                return;
            };
            let mut child_node = Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(0.0),
                top: Val::Px(4.0),
                width: Val::Percent(1.0),
                height: Val::Px(19.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(7.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                border: UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
                overflow: Overflow::clip(),
                ..default()
            };
            apply_timeline_bar_geometry(
                &mut child_node,
                root_clip.start_time + timing.root_source_start - root_clip.source_offset,
                timing.duration,
                state.view,
            );
            track
                .spawn((
                    TimelineReferencedEmitter {
                        clip: root_clip.id,
                        source_start: timing.root_source_start,
                        source_duration: timing.duration,
                    },
                    child_node,
                    BackgroundColor(color.with_alpha(if muted || suppressed {
                        0.08
                    } else {
                        0.20
                    })),
                    BorderColor::all(if selected {
                        theme::TEXT
                    } else {
                        color.with_alpha(0.75)
                    }),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineReferencedEmitterControl,
                        action,
                        AccessibleLabel(localizer.text("timeline-inspect-referenced-emitter-bar")),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .observe(select_timeline_referenced_emitter)
                    .observe(stop_timeline_control_press);
                    bar.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::no_wrap(),
                        Node {
                            min_width: Val::Px(0.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
        });
}

pub(super) fn handle_referenced_effect_action(
    action: &ChoreographyAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    curves: &mut CurvesState,
    state: &mut TimelineState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    match action.clone() {
        ChoreographyAction::SelectEffectClipEmitter { path, emitter } => {
            state.selected_automation_key = None;
            let source =
                resolve_effect_clip_path(session, catalog, &path).map(|(_, source)| source);
            if source
                .as_ref()
                .is_some_and(|effect| effect.emitters.iter().any(|item| item.id == emitter))
            {
                state.clear_emitter_selection();
                session.selection.select_effect_clip(path.root());
                state.inspected_child = Some(EffectClipChildSelection::Emitter { path, emitter });
                session.status = localizer.text("timeline-selected-referenced-emitter");
                session.ui_revision += 1;
                curves.clear();
            }
        }
        ChoreographyAction::SelectReferencedEffectClip(path) => {
            state.selected_automation_key = None;
            if resolve_effect_clip_path(session, catalog, &path).is_some() {
                state.clear_emitter_selection();
                session.selection.select_effect_clip(path.root());
                state.inspected_child = Some(EffectClipChildSelection::EffectClip { path });
                session.status = localizer.text("timeline-selected-referenced-effect");
                session.ui_revision += 1;
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEffectClipExpanded(path) => {
            if !state.expanded_effect_clips.remove(&path) {
                state.expanded_effect_clips.insert(path);
            } else if state
                .inspected_child
                .as_ref()
                .is_some_and(|child| child.is_descendant_of(&path))
            {
                state.inspected_child = None;
            }
            session.ui_revision += 1;
        }
        ChoreographyAction::EditEffectClipEmitterSource { path, emitter } => {
            if let Some((clip, source)) = resolve_effect_clip_path(session, catalog, &path)
                && source
                    .emitters
                    .iter()
                    .any(|candidate| candidate.id == emitter)
            {
                commands.trigger(DocumentAction::OpenSourceEmitter(clip.source, emitter));
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
pub(super) fn mapped_referenced_emitter_timing(
    clip: &EffectClip,
    emitter: &Emitter,
) -> Option<(f32, f32)> {
    map_referenced_interval(
        ReferencedTimingContext {
            root_source_start: clip.source_offset,
            local_source_offset: clip.source_offset,
            duration: clip.duration,
        },
        emitter.start_time,
        emitter.duration,
    )
    .map(|(timing, _)| {
        (
            clip.start_time + timing.root_source_start - clip.source_offset,
            timing.duration,
        )
    })
}

pub(super) fn referenced_emitter_click_action(
    state: &mut TimelineState,
    now: Duration,
    click_count: u8,
    selection: &ChoreographyAction,
) -> ChoreographyAction {
    let ChoreographyAction::SelectEffectClipEmitter { path, emitter } = selection else {
        state.referenced_emitter_click = None;
        return selection.clone();
    };
    let repeated = state
        .referenced_emitter_click
        .as_ref()
        .is_some_and(|previous| {
            previous.path == *path
                && previous.emitter == *emitter
                && now.saturating_sub(previous.at) <= REFERENCED_EMITTER_DOUBLE_CLICK_INTERVAL
        });
    if click_count >= 2 || repeated {
        state.referenced_emitter_click = None;
        ChoreographyAction::EditEffectClipEmitterSource {
            path: path.clone(),
            emitter: *emitter,
        }
    } else {
        state.referenced_emitter_click = Some(ReferencedEmitterClick {
            path: path.clone(),
            emitter: *emitter,
            at: now,
        });
        selection.clone()
    }
}

pub(super) fn select_timeline_referenced_emitter(
    click: On<Pointer<Click>>,
    actions: Query<&ChoreographyAction, With<TimelineReferencedEmitterControl>>,
    mut state: ResMut<TimelineState>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(referenced_emitter_click_action(
            &mut state,
            time.elapsed(),
            click.count,
            action,
        ));
    }
}

pub(super) fn open_referenced_emitter_source_from_header(
    click: On<Pointer<Click>>,
    headers: Query<&ChoreographyAction, With<ReferencedEmitterTrackHeader>>,
    mut state: ResMut<TimelineState>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = headers.get(click.event_target()) else {
        return;
    };
    let resolved = referenced_emitter_click_action(&mut state, time.elapsed(), click.count, action);
    if matches!(
        resolved,
        ChoreographyAction::EditEffectClipEmitterSource { .. }
    ) {
        commands.trigger(resolved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referenced_emitter_timing_is_clipped_and_mapped_into_parent_time() {
        let source = EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xfeed));
        let mut clip = EffectClip::new(source, 1.0, 2.0);
        clip.source_offset = 0.5;
        let mut emitter = Emitter::basic_sprite("Child", 2.0);
        emitter.start_time = 0.25;
        emitter.duration = 1.5;

        let (start, duration) = mapped_referenced_emitter_timing(&clip, &emitter).unwrap();
        assert!((start - 1.0).abs() < 0.000_1);
        assert!((duration - 1.25).abs() < 0.000_1);

        emitter.start_time = 4.0;
        assert!(mapped_referenced_emitter_timing(&clip, &emitter).is_none());
    }

    #[test]
    fn nested_reference_timing_is_clipped_through_every_ancestor() {
        let root = ReferencedTimingContext {
            root_source_start: 0.5,
            local_source_offset: 0.5,
            duration: 2.0,
        };
        let (nested_clip, clipped_offset) = map_referenced_interval(root, 0.25, 1.5).unwrap();
        assert!((nested_clip.root_source_start - 0.5).abs() < 0.000_1);
        assert!((nested_clip.duration - 1.25).abs() < 0.000_1);
        assert!((clipped_offset - 0.25).abs() < 0.000_1);

        let nested_source = ReferencedTimingContext {
            root_source_start: nested_clip.root_source_start,
            local_source_offset: 0.2 + clipped_offset,
            duration: nested_clip.duration,
        };
        let (nested_emitter, _) = map_referenced_interval(nested_source, 0.4, 1.0).unwrap();
        assert!((nested_emitter.root_source_start - 0.5).abs() < 0.000_1);
        assert!((nested_emitter.duration - 0.95).abs() < 0.000_1);
        assert!(map_referenced_interval(nested_source, 2.0, 0.25).is_none());
    }

    #[test]
    fn effect_clip_paths_distinguish_reused_nested_sources() {
        let root = EffectClipPath::root_path(EffectClipId::new());
        let first_branch = root.child(EffectClipId::new());
        let second_branch = root.child(EffectClipId::new());
        let shared_source_clip = EffectClipId::new();
        let first_leaf = first_branch.child(shared_source_clip);
        let second_leaf = second_branch.child(shared_source_clip);

        assert_ne!(first_leaf, second_leaf);
        assert!(first_leaf.starts_with(&root));
        assert!(first_leaf.starts_with(&first_branch));
        assert!(!first_leaf.starts_with(&second_branch));
    }

    #[test]
    fn double_click_opens_referenced_emitter_source_with_the_emitter_target() {
        let path = EffectClipPath::root_path(EffectClipId::new());
        let emitter = EmitterId::new();
        let mut state = TimelineState::framed(1.0);
        let selection = ChoreographyAction::SelectEffectClipEmitter {
            path: path.clone(),
            emitter,
        };

        assert_eq!(
            referenced_emitter_click_action(&mut state, Duration::ZERO, 1, &selection),
            selection
        );
        assert_eq!(
            referenced_emitter_click_action(&mut state, Duration::from_millis(200), 1, &selection,),
            ChoreographyAction::EditEffectClipEmitterSource { path, emitter }
        );
    }
}
