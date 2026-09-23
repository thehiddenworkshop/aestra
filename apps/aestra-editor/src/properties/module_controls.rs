use super::*;

pub(crate) type PropertySourceKind = InputSourceKind;

pub(super) fn handle_module_action(
    action: PropertiesAction,
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    palette: &mut ModulePaletteState,
    workspace: &mut CurvesState,
    layout: &mut WorkspaceLayout,
    localizer: &Localizer,
) -> bool {
    match action {
        PropertiesAction::OpenModulePalette(stage) => {
            palette.open = true;
            palette.stage = stage;
            palette.query.clear();
            session.ui_revision += 1;
        }
        PropertiesAction::CloseModulePalette => {
            palette.open = false;
            session.ui_revision += 1;
        }
        PropertiesAction::AddModule(index) => {
            let module = registry
                .iter()
                .nth(index)
                .and_then(|metadata| registry.instantiate(&metadata.type_id));
            if let Some(module) = module {
                session.add_module(module);
                palette.open = false;
            } else {
                set_properties_status(
                    session,
                    localizer,
                    PropertiesStatus::ModuleRegistryUnavailable,
                );
            }
        }
        PropertiesAction::SetModuleChoice {
            module,
            input,
            choice,
        } => set_module_choice(session, registry, module, input, choice, localizer),
        PropertiesAction::MoveModule(id, direction) => {
            session.move_module(id, direction);
        }
        PropertiesAction::DuplicateModule(id) => session.duplicate_module(id),
        PropertiesAction::DeleteModule(id) => {
            if preview_module_deletion(session, id) {
                reveal_dock_panel(layout, session, ToolPanel::Changes);
                workspace.clear();
            }
        }
        PropertiesAction::ToggleModuleInputPublic { module, input } => {
            toggle_module_input_public(session, registry, module, input, localizer);
        }
        PropertiesAction::SetModuleInputSource {
            module,
            input,
            source,
        } => {
            set_module_input_source(session, registry, module, input, source, localizer);
        }
        _ => return false,
    }
    true
}

pub(super) fn preview_module_deletion(session: &mut EditorSession, module: ModuleId) -> bool {
    let Some(selected_layer) = session.selected_layer() else {
        return false;
    };
    let emitter = selected_layer.id;
    session.preview_transaction(EffectTransaction::single(
        "Delete module",
        EffectCommand::RemoveModule { emitter, module },
    ))
}

fn unique_effect_parameter_name_from_base(effect: &EffectAsset, base: &str) -> String {
    if !effect
        .parameters
        .iter()
        .any(|parameter| parameter.name == base)
    {
        return base.to_owned();
    }
    (2..)
        .map(|index| format!("{base} {index}"))
        .find(|name| {
            !effect
                .parameters
                .iter()
                .any(|parameter| &parameter.name == name)
        })
        .expect("the unbounded numeric suffix always yields a unique parameter name")
}

pub(super) fn toggle_module_input_public(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((_, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let binding = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|module| module.id == module_id)
        .and_then(|module| module.bindings.get(input_name))
        .copied();
    let Some(parameter_id) = binding else {
        return expose_module_input(session, registry, module_id, input_index, localizer);
    };
    update_effect_parameter(session, localizer, parameter_id, |parameter| {
        parameter.exposed = !parameter.exposed;
    })
}

pub(super) fn expose_module_input(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let Some(module) = session
        .effect
        .emitters
        .iter()
        .find(|candidate| candidate.id == emitter)
        .and_then(|emitter| {
            emitter
                .modules
                .iter()
                .find(|candidate| candidate.id == module_id)
        })
    else {
        return false;
    };
    if module.bindings.contains_key(input_name) {
        return false;
    }
    let Some(mut default) = module_parameter(module, input_name) else {
        return false;
    };
    default.regenerate_ids();
    let metadata = registry.get(&module.module_type);
    let display_name = metadata
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
        .map_or_else(
            || input_name.to_owned(),
            |input| localized_properties_input(localizer, input.name, input.display_name, false),
        );
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: unique_effect_parameter_name_from_base(&session.effect, &display_name),
        default,
        exposed: true,
    };
    let parameter_id = parameter.id;
    let parameter_index = session.effect.parameters.len();
    session.execute_transaction(
        EffectTransaction::new(
            localizer.text("properties-expose-module-input-command"),
            vec![
                EffectCommand::AddParameter {
                    parameter,
                    index: parameter_index,
                },
                EffectCommand::BindModuleParameter {
                    emitter,
                    module: module_id,
                    parameter: input_name.to_owned(),
                    source: parameter_id,
                },
            ],
        ),
        true,
    )
}

fn properties_module_input_target<'a>(
    session: &EditorSession,
    registry: &'a ModuleRegistry,
    module: ModuleId,
    input: u8,
) -> Option<(EmitterId, &'a str)> {
    let (emitter, module) = session.effect.emitters.iter().find_map(|emitter| {
        emitter
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .map(|module| (emitter.id, module))
    })?;
    let metadata = registry.get(&module.module_type)?;
    metadata
        .inputs
        .get(input as usize)
        .map(|input| (emitter, input.name))
}

pub(super) fn set_module_input_source(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module: ModuleId,
    input_index: u8,
    source: PropertySourceKind,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, parameter)) =
        properties_module_input_target(session, registry, module, input_index)
    else {
        return false;
    };
    let Some((module_instance, input)) = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|candidate| candidate.id == module)
        .and_then(|module| {
            registry
                .get(&module.module_type)
                .and_then(|metadata| metadata.inputs.get(input_index as usize))
                .map(|input| (module, input))
        })
    else {
        return false;
    };
    if !input.sources.contains(&source) {
        return false;
    }
    if module_instance.property_source(parameter) == Some(source) {
        return false;
    }
    let Some(current) = properties_module_parameter(session, module, parameter) else {
        return false;
    };
    let mut commands = Vec::with_capacity(4);
    let Some(active_source) = module_instance.property_source(parameter) else {
        return false;
    };
    let active_has_stored_value = module_instance
        .property_source_values
        .get(parameter)
        .is_some_and(|values| {
            values
                .iter()
                .any(|candidate| candidate.source == active_source)
        });
    if active_source != PropertySourceKind::Constant
        && (module_instance.bindings.contains_key(parameter) || !active_has_stored_value)
    {
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source: active_source,
            value: current.clone(),
        });
    }
    let target_value = if source == PropertySourceKind::Constant {
        module_instance.parameter_value(parameter)
    } else {
        module_instance
            .property_value_for_source(parameter, source)
            .or_else(|| initial_property_source_value(input, &current, source))
    };
    if source != PropertySourceKind::Constant
        && module_instance
            .property_value_for_source(parameter, source)
            .is_none()
    {
        let Some(value) = target_value.clone() else {
            return false;
        };
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source,
            value,
        });
    }
    if let Some(parameter_id) = module_instance.bindings.get(parameter) {
        let Some(mut effect_parameter) = session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .cloned()
        else {
            return false;
        };
        let Some(value) = target_value else {
            return false;
        };
        effect_parameter.default = detached_property_value(value);
        commands.push(EffectCommand::SetParameter {
            id: *parameter_id,
            parameter: effect_parameter,
        });
    } else if source == PropertySourceKind::Constant
        && active_source != PropertySourceKind::Constant
        && !active_has_stored_value
    {
        let Some(value) = target_value else {
            return false;
        };
        commands.push(EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: parameter.to_owned(),
            value: detached_property_value(value),
        });
    }
    commands.push(EffectCommand::SetModulePropertySource {
        emitter,
        module,
        parameter: parameter.to_owned(),
        source,
    });
    session.execute_transaction(
        EffectTransaction::new(localizer.text("properties-change-source-command"), commands),
        true,
    )
}

fn detached_property_value(mut value: Value) -> Value {
    value.regenerate_ids();
    value
}

fn initial_property_source_value(
    input: &InputMetadata,
    current: &Value,
    source: PropertySourceKind,
) -> Option<Value> {
    let scalar = match current {
        Value::Scalar(value) => Some(*value),
        Value::Range(range) => Some((range.min + range.max) * 0.5),
        Value::Curve(curve) => Some(curve.sample(0.0)),
        _ => None,
    };
    let vector = match current {
        Value::Vec3(value) => Some(*value),
        Value::Vec3Range(range) => Some(std::array::from_fn(|axis| {
            (range.min[axis] + range.max[axis]) * 0.5
        })),
        Value::Vec3Curve(curves) => Some(curves.sample(0.0)),
        _ => None,
    };
    match source {
        PropertySourceKind::RandomRange => {
            let (step, min, max) = numeric_source_limits(&input.control)?;
            if let Some(value) = vector {
                let low = value
                    .map(|value| min.map_or(value - step, |minimum| (value - step).max(minimum)));
                let high = value
                    .map(|value| max.map_or(value + step, |maximum| (value + step).min(maximum)));
                return Some(Value::Vec3Range(Vec3Range::new(low, high)));
            }
            let value = scalar?;
            let low = min.map_or(value - step, |minimum| (value - step).max(minimum));
            let high = max.map_or(value + step, |maximum| (value + step).min(maximum));
            Some(Value::Range(ScalarRange::new(low.min(high), high.max(low))))
        }
        PropertySourceKind::Curve(_) => {
            if let Some(value) = vector {
                return Some(Value::Vec3Curve(Vec3Curve::constant(value)));
            }
            let value = scalar?;
            Some(Value::Curve(Curve::normalized(
                vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 0.0)],
                ScalarRange::new(value, value),
            )))
        }
        PropertySourceKind::Gradient(_) => {
            let Value::Gradient(gradient) = current else {
                return None;
            };
            let color = gradient.sample(0.0);
            Some(Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, color),
                ColorKey::new(1.0, color),
            ])))
        }
        PropertySourceKind::Constant => None,
    }
}

pub(super) fn numeric_source_limits(
    control: &InputControl,
) -> Option<(f32, Option<f32>, Option<f32>)> {
    match control {
        InputControl::Number { step, min, max }
        | InputControl::Range { step, min, max }
        | InputControl::Vector { step, min, max } => Some((*step, *min, *max)),
        InputControl::Curve { step, min, max } => Some((*step, Some(*min), Some(*max))),
        _ => None,
    }
}

pub(super) fn properties_curve_limits(
    input: &InputMetadata,
    _curve: &Curve,
) -> Option<(f32, Option<f32>, Option<f32>)> {
    numeric_source_limits(&input.control)
}

/// The selection/diagnostic border color for a module row (extensible-stages M9): red when the module
/// has diagnostics, accent when it is the current selection, otherwise the panel border. The panel
/// rebuilds on selection change (the global select observer bumps `ui_revision`), so this is recomputed
/// per render rather than needing a live-updating system.
fn module_row_border(
    module: &ModuleInstance,
    diagnostic_path: &str,
    session: &EditorSession,
) -> Color {
    if session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(diagnostic_path))
    {
        Color::srgb(0.82, 0.28, 0.24)
    } else if session.selection.primary == SemanticTarget::Module(module.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    }
}

/// The enabled checkbox and per-module action menu shared by the compact stack row and the inspector
/// header (extensible-stages M9), so a module can be toggled, reordered, duplicated or deleted from
/// either place.
fn spawn_module_header_actions(
    header: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    display_name: &str,
) {
    let mut enabled = header.spawn_empty();
    enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
        ModuleEnabledControl(module.id),
        AccessibleLabel(format!("Enable {display_name}")),
    ));
    if module.enabled {
        enabled.insert(Checked);
    }
    spawn_action_menu(
        header,
        &format!("{display_name} actions"),
        &[
            ComboOption {
                label: "Move up".into(),
                selected: false,
                action: PropertiesAction::MoveModule(module.id, -1),
            },
            ComboOption {
                label: "Move down".into(),
                selected: false,
                action: PropertiesAction::MoveModule(module.id, 1),
            },
            ComboOption {
                label: "Duplicate".into(),
                selected: false,
                action: PropertiesAction::DuplicateModule(module.id),
            },
            ComboOption {
                label: "Delete…".into(),
                selected: false,
                action: PropertiesAction::DeleteModule(module.id),
            },
        ],
    );
}

/// Tags a module stack row as a drag source and drop target for reordering (extensible-stages M9,
/// §28.4). Carries the row's module id so a drop can reorder the dragged module onto this one.
#[derive(Component, Clone, Copy)]
pub(super) struct ModuleRowDrag(ModuleId);

/// The module id of the stack row at `entity` or an ancestor (drops land on inner children).
fn module_row_at(
    mut entity: Entity,
    rows: &Query<&ModuleRowDrag>,
    parents: &Query<&ChildOf>,
) -> Option<ModuleId> {
    loop {
        if let Ok(row) = rows.get(entity) {
            return Some(row.0);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

/// The floating drag proxy that follows the cursor while a module row is dragged (§28.4) — a full-width
/// copy of the row, so the whole section appears to lift out and move.
#[derive(Component)]
pub(super) struct ModuleDragGhost;

/// Tracks the active module drag (§28.4): the floating proxy, the original row hidden while it drags,
/// the row currently showing an insertion gap, and the dragged module for same-stage checks.
#[derive(Resource, Default)]
pub(crate) struct ModuleDragState {
    ghost: Option<Entity>,
    hidden_row: Option<Entity>,
    gap_row: Option<Entity>,
    /// Whether the current gap is below (`true`) or above (`false`) `gap_row`, i.e. whether a drop
    /// would land after or before that row.
    gap_after: bool,
    dragged: Option<ModuleId>,
}

/// The vertical grip icon that marks a row as draggable (§28.4), reused by the row and the drag proxy
/// so they match. `Pickable::IGNORE` lets drags pass through to the row itself.
fn spawn_module_drag_handle(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer) {
    parent
        .spawn((
            Node {
                width: Val::Px(14.0),
                height: Val::Px(20.0),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_child((
            bevy_resvg::prelude::UiSvg(crate::feathers::icon::load_svg_icon(
                asset_server,
                "icons/drag-vertical.svg",
            )),
            bevy_resvg::prelude::SvgColor(theme::TEXT_MUTED),
            Node {
                width: Val::Px(14.0),
                height: Val::Px(20.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
}

/// The stack row entity and its module id at `entity` or an ancestor.
fn module_row_entity(
    mut entity: Entity,
    rows: &Query<&ModuleRowDrag>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, ModuleId)> {
    loop {
        if let Ok(row) = rows.get(entity) {
            return Some((entity, row.0));
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

/// The default top margin of a stack row (matches `spawn_module_stack_row`), restored after an
/// insertion gap is cleared.
const MODULE_ROW_MARGIN_TOP: f32 = 1.0;
/// The gap opened above the hovered row so the dragged row has room to drop (§28.4).
const MODULE_ROW_GAP: f32 = 30.0;

/// Reorders modules by drag-and-drop within the stack (§28.4): when one row is dropped onto another,
/// the dragged module takes the target's slot. The session enforces same-stage-only and undoability.
/// Registered globally (like the timeline's drop handlers); it filters to drops between module rows.
pub(super) fn reorder_modules_on_drop(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<&ModuleRowDrag>,
    parents: Query<&ChildOf>,
    state: Res<ModuleDragState>,
    mut session: ResMut<EditorSession>,
) {
    if drop.button != PointerButton::Primary {
        return;
    }
    let (Some(dragged), Some((target_entity, target))) = (
        module_row_at(drop.dropped, &rows, &parents),
        module_row_entity(drop.entity, &rows, &parents),
    ) else {
        return;
    };
    if dragged == target {
        return;
    }
    drop.propagate(false);
    // Drop on the side the insertion gap is showing for this row (pointer top/bottom half).
    let after = state.gap_row == Some(target_entity) && state.gap_after;
    session.reorder_module_relative(dragged, target, after);
}

/// Lifts a module row into a floating full-width proxy when it starts dragging (§28.4): the original
/// row is hidden (so the list closes up) and a copy at the cursor takes its place. `Pickable::IGNORE`
/// keeps the proxy from intercepting the drop target beneath the cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_module_drag(
    event: On<Pointer<DragStart>>,
    rows: Query<&ModuleRowDrag>,
    parents: Query<&ChildOf>,
    computed: Query<&ComputedNode>,
    mut nodes: Query<&mut Node>,
    session: Res<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    asset_server: Res<AssetServer>,
    mut state: ResMut<ModuleDragState>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some((row_entity, module_id)) = module_row_entity(event.entity, &rows, &parents) else {
        return;
    };
    let Some(layer) = session.selected_layer() else {
        return;
    };
    let Some(module) = layer.modules.iter().find(|module| module.id == module_id) else {
        return;
    };
    let name = registry
        .0
        .get(&module.module_type)
        .map(|metadata| metadata.display_name.to_string())
        .unwrap_or_else(|| module.module_type.0.clone());
    let summary = aestra_compiler::module_summary(module);
    let width = computed
        .get(row_entity)
        .map(|node| node.size().x * node.inverse_scale_factor())
        .unwrap_or(280.0);
    // Hide the original so the surrounding rows close up around the lifted proxy.
    if let Ok(mut node) = nodes.get_mut(row_entity) {
        node.display = Display::None;
    }
    if let Some(ghost) = state.ghost.take() {
        commands.entity(ghost).try_despawn();
    }
    let position = event.pointer_location.position;
    let ghost = commands
        .spawn((
            ModuleDragGhost,
            Pickable::IGNORE,
            GlobalZIndex(400),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x + 6.0),
                top: Val::Px(position.y - 12.0),
                width: Val::Px(width),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::ACCENT),
        ))
        .with_children(|row| {
            spawn_module_drag_handle(row, &asset_server);
            row.spawn((
                Text::new(name),
                bevy::feathers::theme::ThemedText,
                TextColor(theme::TEXT),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            if !summary.is_empty() {
                row.spawn((
                    Text::new(summary),
                    bevy::feathers::theme::ThemedText,
                    TextColor(theme::TEXT_FAINT),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextLayout {
                        linebreak: LineBreak::NoWrap,
                        ..default()
                    },
                    Node {
                        margin: UiRect::left(Val::Auto),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
        })
        .id();
    state.ghost = Some(ghost);
    state.hidden_row = Some(row_entity);
    state.gap_row = None;
    state.dragged = Some(module_id);
}

/// Moves the drag proxy to follow the cursor for the duration of the drag (§28.4).
pub(super) fn move_module_drag(
    event: On<Pointer<Drag>>,
    state: Res<ModuleDragState>,
    mut ghosts: Query<&mut Node, With<ModuleDragGhost>>,
) {
    let Some(ghost) = state.ghost else {
        return;
    };
    let Ok(mut node) = ghosts.get_mut(ghost) else {
        return;
    };
    let position = event.pointer_location.position;
    node.left = Val::Px(position.x + 6.0);
    node.top = Val::Px(position.y - 12.0);
}

/// Opens an insertion gap above the row the cursor enters during a drag (§28.4), so the other rows
/// visibly make room for the drop. Restricted to same-stage rows (reorder is same-stage only).
pub(super) fn hover_module_drag(
    event: On<Pointer<DragOver>>,
    rows: Query<&ModuleRowDrag>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut nodes: Query<&mut Node>,
    mut state: ResMut<ModuleDragState>,
) {
    let Some(dragged) = state.dragged else {
        return;
    };
    let Some((row_entity, module_id)) = module_row_entity(event.entity, &rows, &parents) else {
        return;
    };
    if Some(row_entity) == state.hidden_row || module_id == dragged {
        return;
    }
    let same_stage = session.selected_layer().is_some_and(|layer| {
        let stage_of = |id| layer.modules.iter().find(|module| module.id == id);
        matches!((stage_of(dragged), stage_of(module_id)), (Some(a), Some(b)) if a.stage == b.stage)
    });
    if !same_stage {
        return;
    }
    // Insert before or after the hovered row depending on which half the cursor is over — the
    // standard, unambiguous drag-reorder rule, and the one the drop respects.
    let Ok((node, transform)) = geometry.get(row_entity) else {
        return;
    };
    let center_y = transform.translation.y * node.inverse_scale_factor();
    let after = event.pointer_location.position.y > center_y;
    if state.gap_row == Some(row_entity) && state.gap_after == after {
        return;
    }
    restore_row_margin(&mut nodes, state.gap_row.take());
    if let Ok(mut node) = nodes.get_mut(row_entity) {
        if after {
            node.margin.bottom = Val::Px(MODULE_ROW_GAP);
        } else {
            node.margin.top = Val::Px(MODULE_ROW_GAP);
        }
    }
    state.gap_row = Some(row_entity);
    state.gap_after = after;
}

/// Restores a gapped row's top and bottom margins to the row default (§28.4).
fn restore_row_margin(nodes: &mut Query<&mut Node>, row: Option<Entity>) {
    if let Some(row) = row
        && let Ok(mut node) = nodes.get_mut(row)
    {
        node.margin.top = Val::Px(MODULE_ROW_MARGIN_TOP);
        node.margin.bottom = Val::Px(MODULE_ROW_MARGIN_TOP);
    }
}

/// Restores the hidden row and any insertion gap, and tears down the proxy when the drag ends (§28.4),
/// whether or not a reorder happened (a reorder also rebuilds the panel fresh).
pub(super) fn end_module_drag(
    _event: On<Pointer<DragEnd>>,
    mut nodes: Query<&mut Node>,
    mut state: ResMut<ModuleDragState>,
    mut commands: Commands,
) {
    if let Some(row) = state.hidden_row.take()
        && let Ok(mut node) = nodes.get_mut(row)
    {
        node.display = Display::Flex;
    }
    restore_row_margin(&mut nodes, state.gap_row.take());
    if let Some(ghost) = state.ghost.take() {
        commands.entity(ghost).try_despawn();
    }
    state.dragged = None;
}

/// A compact, selectable stack row for one module (extensible-stages M9, §28.2): the module's name, its
/// descriptor-driven summary, and the shared enabled/actions controls — no inline parameter controls.
/// Selecting the row (via the global `select_properties_header` observer that finds the
/// `PropertiesSelectionTarget`) shows the module's full controls in the inspector below the stack; the
/// row is also a drag source/target for reordering (`reorder_modules_on_drop`).
pub(super) fn spawn_module_stack_row(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    asset_server: &AssetServer,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let help = metadata.map_or(
        "This module is not available in the current registry.",
        |item| item.description,
    );
    let summary = aestra_compiler::module_summary(module);
    let base_border = module_row_border(module, diagnostic_path, session);
    let selected = session.selection.primary == SemanticTarget::Module(module.id);
    parent
        .spawn((
            PropertiesSemanticTarget {
                target: SemanticTarget::Module(module.id),
                base_border,
            },
            PropertiesSelectionTarget(SemanticTarget::Module(module.id)),
            ModuleRowDrag(module.id),
            EntityCursor::System(SystemCursorIcon::Grab),
            crate::feathers::tooltip::EditorTooltip::titled(display_name, help),
            Node {
                width: Val::Auto,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                margin: UiRect::axes(Val::Px(7.0), Val::Px(1.0)),
                min_height: Val::Px(26.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::PANEL_LIGHT
            } else if module.enabled {
                theme::PANEL
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(base_border),
        ))
        .with_children(|row| {
            // Drag handle (§28.4): the vertical grip icon marks the row as draggable to reorder.
            spawn_module_drag_handle(row, asset_server);
            row.spawn((
                Text::new(display_name),
                bevy::feathers::theme::ThemedText,
                TextColor(if module.enabled {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                }),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            // The descriptor-driven summary, right-aligned and muted, so the row reads at a glance.
            if !summary.is_empty() {
                row.spawn((
                    Text::new(summary),
                    bevy::feathers::theme::ThemedText,
                    TextColor(theme::TEXT_FAINT),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextLayout {
                        linebreak: LineBreak::NoWrap,
                        ..default()
                    },
                    Node {
                        margin: UiRect::left(Val::Auto),
                        max_width: Val::Percent(58.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            } else {
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            spawn_module_header_actions(row, module, display_name);
        });
}

/// The focused inspector view of a single module (extensible-stages M9, §28.2): a header echoing the
/// stack row (name, summary, enabled, actions) followed by the module's full parameter controls. Unlike
/// the old collapsible card this never hides its body — it is the destination for the selected row.
pub(super) fn spawn_module_inspector(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let summary = aestra_compiler::module_summary(module);
    parent
        .spawn(Node {
            width: Val::Auto,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(9.0), Val::Px(7.0)),
            ..default()
        })
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new(display_name),
                    bevy::feathers::theme::ThemedText,
                    TextColor(if module.enabled {
                        theme::TEXT
                    } else {
                        theme::TEXT_FAINT
                    }),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                ));
                if !summary.is_empty() {
                    header.spawn((
                        Text::new(summary),
                        bevy::feathers::theme::ThemedText,
                        TextColor(theme::TEXT_FAINT),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextLayout {
                            linebreak: LineBreak::NoWrap,
                            ..default()
                        },
                        Node {
                            margin: UiRect::left(Val::Auto),
                            ..default()
                        },
                    ));
                } else {
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                }
                spawn_module_header_actions(header, module, display_name);
            });
            spawn_module_input_controls(
                card,
                module,
                metadata,
                diagnostic_path,
                session,
                localizer,
                asset_server,
            );
        });
}

/// The module's parameter controls (extensible-stages M9): a registered module renders its schema of
/// inputs; an unregistered plugin/custom-stage module (no metadata in this build's registry) renders its
/// preserved [`ModuleParameters::Custom`] payload as read-only value rows so its authored data is
/// surfaced rather than dropped (§20). Editable, schema-driven controls for such modules arrive with
/// plugin loading, when a `PropertySchema` describing them is actually available. Inline diagnostics
/// close out both paths. Shared so the inspector and any future embedded view render identical controls.
fn spawn_module_input_controls(
    card: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    match metadata {
        Some(metadata) => {
            for (input_index, input) in metadata.inputs.iter().enumerate() {
                spawn_input_control(
                    card,
                    module,
                    input,
                    input_index as u8,
                    session,
                    localizer,
                    asset_server,
                );
            }
        }
        None => {
            if let aestra_core::ModuleParameters::Custom(values) = &module.parameters {
                spawn_custom_module_properties(card, values);
            }
        }
    }
    spawn_inline_diagnostics(card, diagnostic_path, session);
}

/// Surfaces an unregistered plugin/custom module's preserved payload in the inspector (extensible-stages
/// M9 phase 9b, §20): a short note that the module is not installed, then one read-only row per authored
/// property. The data is never dropped; installing the plugin (which publishes a `PropertySchema`) is
/// what turns these into editable controls.
fn spawn_custom_module_properties(
    card: &mut ChildSpawnerCommands,
    values: &std::collections::BTreeMap<String, Value>,
) {
    card.spawn((
        Text::new("Plugin module not installed — authored values are preserved and read-only."),
        bevy::feathers::theme::ThemedText,
        TextColor(theme::TEXT_FAINT),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        Node {
            margin: UiRect::axes(Val::Px(2.0), Val::Px(4.0)),
            ..default()
        },
    ));
    if values.is_empty() {
        spawn_properties_read_only_control(card, "Properties", "None authored");
        return;
    }
    for (name, value) in values {
        spawn_properties_read_only_control(card, name, &format_custom_property_value(value));
    }
}

/// A concise, read-only rendering of one preserved custom-property [`Value`] (extensible-stages M9
/// phase 9b). Covers the common authored types; anything richer falls back to its type name so the row
/// is never blank.
fn format_custom_property_value(value: &Value) -> String {
    fn num(value: f32) -> String {
        if value == value.trunc() && value.abs() < 1.0e7 {
            format!("{}", value as i64)
        } else {
            format!("{value:.3}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    }
    match value {
        Value::Bool(on) => if *on { "On" } else { "Off" }.to_string(),
        Value::U32(count) => count.to_string(),
        Value::Scalar(scalar) => num(*scalar),
        Value::Vec2(components) => format!("{}, {}", num(components[0]), num(components[1])),
        Value::Vec3(components) => format!(
            "{}, {}, {}",
            num(components[0]),
            num(components[1]),
            num(components[2])
        ),
        Value::Vec4(components) => format!(
            "{}, {}, {}, {}",
            num(components[0]),
            num(components[1]),
            num(components[2]),
            num(components[3])
        ),
        Value::Text(text) => text.clone(),
        Value::Range(range) => format!("{} – {}", num(range.min), num(range.max)),
        Value::Curve(_) => "Curve".to_string(),
        Value::Vec3Range(_) => "Vec3 range".to_string(),
        Value::Vec3Curve(_) => "Vec3 curve".to_string(),
        Value::Gradient(_) => "Gradient".to_string(),
        other => format!("{:?}", other.value_type()),
    }
}

fn spawn_input_control(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let display_name = localized_properties_input(localizer, input.name, input.display_name, false);
    let description = localized_properties_input(localizer, input.name, input.description, true);
    let Some(value) = properties_module_parameter(session, module.id, input.name) else {
        spawn_properties_read_only_control(parent, &display_name, "Missing authored value");
        return;
    };
    let public =
        public_module_input_control(session, module, input, input_index, &value, localizer);
    let source = property_source_for_input(module, input, &value);
    if input.sources.len() > 1
        && matches!(&input.control, InputControl::Vector { .. })
        && matches!(
            &value,
            Value::Vec3(_) | Value::Vec3Range(_) | Value::Vec3Curve(_)
        )
    {
        spawn_properties_vector_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if input.sources.len() > 1
        && matches!(&input.control, InputControl::Number { .. })
        && matches!(&value, Value::Scalar(_))
    {
        spawn_properties_scalar_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if source == PropertySourceKind::RandomRange && matches!(&value, Value::Range(_)) {
        spawn_properties_range_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if matches!(source, PropertySourceKind::Curve(_))
        && let Value::Curve(curve) = &value
    {
        spawn_properties_curve_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            curve,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    match (&input.control, value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => {
            spawn_properties_curve_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                &curve,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Gradient, Value::Gradient(gradient)) => {
            spawn_properties_gradient_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                &description,
                &gradient,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Toggle, Value::Bool(value)) => {
            spawn_properties_toggle_control(
                parent,
                module.id,
                input,
                &display_name,
                &description,
                value,
                public,
            );
        }
        (InputControl::Number { .. }, Value::U32(_)) => {
            spawn_properties_integer_control(
                parent,
                module.id,
                input,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
            );
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Scalar,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("", value, 0)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1), ("Z", value[2], 2)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[
                    ("X", value[0], 0),
                    ("Y", value[1], 1),
                    ("Z", value[2], 2),
                    ("W", value[3], 3),
                ],
                public,
            );
        }
        (InputControl::Range { .. }, Value::Range(_)) => {
            spawn_properties_range_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Choice, value) => spawn_properties_choice_control(
            parent,
            module.id,
            input_index,
            &display_name,
            &description,
            &value,
        ),
        (_, value) => {
            spawn_properties_read_only_control(parent, &display_name, &format_value(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_limits_follow_numeric_control_metadata() {
        let number = InputControl::Number {
            step: 0.25,
            min: Some(0.0),
            max: Some(2.0),
        };
        assert_eq!(
            numeric_source_limits(&number),
            Some((0.25, Some(0.0), Some(2.0)))
        );
        assert_eq!(numeric_source_limits(&InputControl::Toggle), None);
    }

    #[test]
    fn custom_property_values_render_concisely_for_the_read_only_inspector() {
        use aestra_core::ScalarRange;
        assert_eq!(format_custom_property_value(&Value::Scalar(2.5)), "2.5");
        assert_eq!(format_custom_property_value(&Value::Scalar(3.0)), "3");
        assert_eq!(format_custom_property_value(&Value::Bool(true)), "On");
        assert_eq!(format_custom_property_value(&Value::U32(7)), "7");
        assert_eq!(
            format_custom_property_value(&Value::Vec3([0.0, 1.5, -2.0])),
            "0, 1.5, -2"
        );
        assert_eq!(
            format_custom_property_value(&Value::Text("FLIP".into())),
            "FLIP"
        );
        assert_eq!(
            format_custom_property_value(&Value::Range(ScalarRange::new(0.2, 0.8))),
            "0.2 – 0.8"
        );
    }
}
