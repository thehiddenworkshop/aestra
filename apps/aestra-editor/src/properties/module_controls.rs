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
        // A host-bound input keeps its constant as the fallback (host bindings HB4).
        PropertySourceKind::HostBinding => Some(current.clone()),
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

/// The module's stack title — type name plus its label or repeat number (§28.5).
fn module_title(session: &EditorSession, module: &ModuleInstance, display_name: &str) -> String {
    session.selected_layer().map_or_else(
        || display_name.to_owned(),
        |layer| aestra_compiler::module_instance_title(layer, module, display_name),
    )
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
        .any(|diagnostic| diagnostic_belongs_to(&diagnostic.path, diagnostic_path))
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

/// Tags a module stack row as a drag source for reordering (extensible-stages M9, §28.4). Carries the
/// row's module id.
#[derive(Component, Clone, Copy)]
pub(super) struct ModuleRowDrag(ModuleId);

/// The floating copy of the dragged row that tracks the cursor (§28.4).
#[derive(Component)]
pub(super) struct ModuleDragGhost;

/// An animated vertical offset for a row that slides aside during a drag (§28.4). Rows move with a
/// `UiTransform`, never by changing layout, so nothing shifts under the cursor mid-drag.
#[derive(Component, Default)]
pub(super) struct ModuleRowSlide {
    target: f32,
    current: f32,
}

/// One same-stage row captured when a drag starts: its module, entity and (UI-unit) vertical center.
/// Insertion is computed against these frozen positions, so sliding rows never feed back into it.
struct DragSlot {
    module: ModuleId,
    entity: Entity,
    center_y: f32,
}

struct ActiveModuleDrag {
    /// The entity the drag started on (the row or one of its children); drag events target it.
    source: Entity,
    /// The original row, hidden while its copy is dragged.
    row: Entity,
    ghost: Entity,
    /// The ghost's top-left at drag start (the row's own position), in UI units.
    ghost_origin: Vec2,
    /// Distance between consecutive rows, in UI units.
    pitch: f32,
    /// The dragged row's stage, in visual order (includes the dragged row).
    slots: Vec<DragSlot>,
    dragged_slot: usize,
    /// Where the dragged row would land, as an index into the final order of `slots`.
    insert_at: usize,
}

/// Tracks the active module drag (§28.4).
#[derive(Resource, Default)]
pub(crate) struct ModuleDragState {
    active: Option<ActiveModuleDrag>,
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

/// Starts a module drag (§28.4). Captures the dragged row's stage in visual order, hides the original
/// row (keeping its layout slot, so nothing moves), and spawns a copy exactly over it. That copy then
/// moves by the cursor's travel, so the row is picked up where it is instead of jumping to the pointer.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_module_drag(
    mut event: On<Pointer<DragStart>>,
    rows: Query<&ModuleRowDrag>,
    parents: Query<&ChildOf>,
    row_geometry: Query<(Entity, &ModuleRowDrag, &ComputedNode, &UiGlobalTransform)>,
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
    // Pointer events bubble through the row's ancestors; handle the drag once.
    event.propagate(false);
    if state
        .active
        .as_ref()
        .is_some_and(|drag| drag.row == row_entity)
    {
        return;
    }
    if let Some(stale) = state.active.take() {
        commands.entity(stale.ghost).try_despawn();
        commands.entity(stale.row).insert(Visibility::Inherited);
    }
    let Some(layer) = session.selected_layer() else {
        return;
    };
    let Some(module) = layer.modules.iter().find(|module| module.id == module_id) else {
        return;
    };
    let stage_of = |id: ModuleId| {
        layer
            .modules
            .iter()
            .find(|candidate| candidate.id == id)
            .map(|candidate| &candidate.stage)
    };
    let Ok((_, _, row_node, row_transform)) = row_geometry.get(row_entity) else {
        return;
    };
    // UiGlobalTransform and ComputedNode are physical pixels; inverse_scale_factor gives UI units,
    // the same units the ghost's absolute `left`/`top` use.
    let scale = row_node.inverse_scale_factor();
    let row_size = row_node.size() * scale;
    let row_top_left = (row_transform.translation - row_node.size() * 0.5) * scale;
    let mut slots: Vec<DragSlot> = row_geometry
        .iter()
        // Rows hidden by the stack filter have no size; they are not drop positions.
        .filter(|(_, row, node, _)| stage_of(row.0) == Some(&module.stage) && node.size().y > 0.0)
        .map(|(entity, row, node, transform)| DragSlot {
            module: row.0,
            entity,
            center_y: transform.translation.y * node.inverse_scale_factor(),
        })
        .collect();
    slots.sort_by(|a, b| a.center_y.total_cmp(&b.center_y));
    let Some(dragged_slot) = slots.iter().position(|slot| slot.entity == row_entity) else {
        return;
    };
    let type_name = registry
        .0
        .get(&module.module_type)
        .map_or(module.module_type.0.as_str(), |metadata| {
            metadata.display_name
        });
    let name = aestra_compiler::module_instance_title(layer, module, type_name);
    let summary = aestra_compiler::module_summary(module);

    // Hide the original but keep its slot: the neighbours slide over it as the insertion point moves.
    commands.entity(row_entity).insert(Visibility::Hidden);
    for slot in &slots {
        if slot.entity != row_entity {
            commands.entity(slot.entity).insert((
                ModuleRowSlide::default(),
                EntityCursor::System(SystemCursorIcon::Grabbing),
            ));
        }
    }
    let ghost = commands
        .spawn((
            ModuleDragGhost,
            Pickable::IGNORE,
            GlobalZIndex(400),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(row_top_left.x),
                top: Val::Px(row_top_left.y),
                width: Val::Px(row_size.x),
                height: Val::Px(row_size.y),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::ACCENT),
            BoxShadow::new(
                Color::BLACK.with_alpha(0.45),
                Val::Px(0.0),
                Val::Px(4.0),
                Val::Px(0.0),
                Val::Px(10.0),
            ),
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
    state.active = Some(ActiveModuleDrag {
        source: event.entity,
        row: row_entity,
        ghost,
        ghost_origin: row_top_left,
        // Row height plus its 1px top and bottom margins.
        pitch: row_size.y + 2.0,
        slots,
        dragged_slot,
        insert_at: dragged_slot,
    });
}

/// Where the dragged row lands for a given ghost center: after every other row whose (captured)
/// center is above it. Returns an index into the final order of `slots`.
fn module_drag_insert_index(slots: &[DragSlot], dragged_slot: usize, ghost_center_y: f32) -> usize {
    slots
        .iter()
        .enumerate()
        .filter(|(index, slot)| *index != dragged_slot && slot.center_y < ghost_center_y)
        .count()
}

/// Maps a finished drag of `modules[from]` to final position `to` onto a session reorder: land before
/// the row now at `to`, or after the last row when dropped at the end. `None` when nothing moved.
fn module_drag_drop_target(
    modules: &[ModuleId],
    from: usize,
    to: usize,
) -> Option<(ModuleId, bool)> {
    if from == to {
        return None;
    }
    let others: Vec<ModuleId> = modules
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != from)
        .map(|(_, module)| *module)
        .collect();
    match others.get(to) {
        Some(&before) => Some((before, false)),
        None => others.last().map(|&last| (last, true)),
    }
}

/// The slide target for the row at `index` when the dragged row moves from `from` to `to`: rows it
/// passes over step one pitch toward the dragged row's old slot; everything else stays put.
fn module_drag_slide_target(index: usize, from: usize, to: usize, pitch: f32) -> f32 {
    if from < to && index > from && index <= to {
        -pitch
    } else if from > to && index >= to && index < from {
        pitch
    } else {
        0.0
    }
}

/// Moves the dragged copy with the cursor (vertically, keeping the grab offset) and slides the other
/// rows of the stage aside to open a space where it would land (§28.4).
pub(super) fn move_module_drag(
    mut event: On<Pointer<Drag>>,
    ui_scale: Res<UiScale>,
    mut state: ResMut<ModuleDragState>,
    mut ghosts: Query<&mut Node, With<ModuleDragGhost>>,
    mut slides: Query<&mut ModuleRowSlide>,
) {
    let Some(drag) = state.active.as_mut() else {
        return;
    };
    if event.entity != drag.source {
        return;
    }
    event.propagate(false);
    let travel = crate::feathers::pointer_travel_to_ui_units(event.distance.y, ui_scale.0);
    if let Ok(mut node) = ghosts.get_mut(drag.ghost) {
        node.top = Val::Px(drag.ghost_origin.y + travel);
    }
    let ghost_center = drag.slots[drag.dragged_slot].center_y + travel;
    let insert_at = module_drag_insert_index(&drag.slots, drag.dragged_slot, ghost_center);
    if insert_at == drag.insert_at {
        return;
    }
    drag.insert_at = insert_at;
    for (index, slot) in drag.slots.iter().enumerate() {
        if index == drag.dragged_slot {
            continue;
        }
        if let Ok(mut slide) = slides.get_mut(slot.entity) {
            slide.target =
                module_drag_slide_target(index, drag.dragged_slot, insert_at, drag.pitch);
        }
    }
}

/// Eases each sliding row toward its target offset every frame, so rows glide aside rather than jump.
pub(super) fn animate_module_row_slides(
    time: Res<Time>,
    mut rows: Query<(&mut UiTransform, &mut ModuleRowSlide)>,
) {
    let blend = 1.0 - (-time.delta_secs() * 20.0).exp();
    for (mut transform, mut slide) in &mut rows {
        if slide.current == slide.target {
            continue;
        }
        slide.current += (slide.target - slide.current) * blend;
        if (slide.target - slide.current).abs() < 0.1 {
            slide.current = slide.target;
        }
        transform.translation = Val2::px(0.0, slide.current);
    }
}

/// Finishes a module drag (§28.4). If the row moved, commits the reorder (one undoable command) and
/// lets the panel rebuild in the new order; otherwise slides everything back and reveals the row.
/// Deciding here from the tracked insertion point — not from whatever is under the pointer at
/// release — is what makes the drop land reliably.
pub(super) fn end_module_drag(
    mut event: On<Pointer<DragEnd>>,
    mut state: ResMut<ModuleDragState>,
    mut slides: Query<&mut ModuleRowSlide>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
) {
    if state
        .active
        .as_ref()
        .is_none_or(|drag| event.entity != drag.source)
    {
        return;
    }
    event.propagate(false);
    let Some(drag) = state.active.take() else {
        return;
    };
    commands.entity(drag.ghost).try_despawn();
    commands.entity(drag.row).insert(Visibility::Inherited);
    let modules: Vec<ModuleId> = drag.slots.iter().map(|slot| slot.module).collect();
    if let Some((target, after)) =
        module_drag_drop_target(&modules, drag.dragged_slot, drag.insert_at)
    {
        session.reorder_module_relative(modules[drag.dragged_slot], target, after);
        // Rebuild in the new order right away (the slid rows are replaced by it).
        session.ui_revision += 1;
        return;
    }
    for slot in &drag.slots {
        if let Ok(mut slide) = slides.get_mut(slot.entity) {
            slide.target = 0.0;
        }
        commands
            .entity(slot.entity)
            .insert(EntityCursor::System(SystemCursorIcon::Grab));
    }
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
    let title = module_title(session, module, display_name);
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
            StackRowSearchText::new(&title, &summary, &module.module_type.0),
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
                Text::new(title.as_str()),
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
            spawn_row_diagnostics_badge(row, diagnostic_path, session);
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
    let title = module_title(session, module, display_name);
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
                    Text::new(title),
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
            spawn_instance_label_field(
                card,
                module.label.as_deref(),
                InstanceLabelControl::Module(module.id),
            );
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

/// The module's parameter controls (extensible-stages M9/M10): a registered module — built-in or from a
/// linked extension — renders editable controls for its declared inputs; a plugin module whose extension
/// is missing, or whose payload schema the installed plugin cannot read (M11), renders an explanation
/// and its preserved [`ModuleParameters::Custom`] payload as read-only value rows (§20, §28.7). Inline
/// diagnostics close out both paths. Shared so the inspector and any future embedded view render
/// identical controls.
fn spawn_module_input_controls(
    card: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let extensions = aestra_compiler::ExtensionRegistry::linked();
    let state = module_extension_state(module, &extensions, &session.effect);
    match (metadata, &state) {
        (Some(metadata), ModuleExtensionState::BuiltIn | ModuleExtensionState::Provided { .. }) => {
            if let ModuleExtensionState::Provided { provider } = &state {
                spawn_extension_note(card, &format!("Provided by {provider}"), theme::TEXT_FAINT);
            }
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
        _ => {
            // Missing plugin, or a payload the installed plugin cannot read (§20, §28.7, §35): the
            // authored data is shown read-only and preserved exactly.
            spawn_extension_unavailable_notice(card, &module.module_type.0, &state);
            if let aestra_core::ModuleParameters::Custom(values) = &module.parameters {
                spawn_custom_module_properties(card, values);
            }
        }
    }
    spawn_inline_diagnostics(card, diagnostic_path, session);
}

/// How a module relates to the installed extensions (extensible-stages M11, §28.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModuleExtensionState {
    /// A core `aestra.*` module.
    BuiltIn,
    /// A plugin module whose plugin is installed and can read its payload.
    Provided { provider: String },
    /// The plugin that provides this type is not installed (or no longer provides it).
    Missing {
        plugin: String,
        requirement: Option<String>,
    },
    /// The plugin is installed but the payload's schema version is newer, or could not be migrated.
    SchemaMismatch {
        provider: String,
        stored: u32,
        current: u32,
    },
}

pub(super) fn module_extension_state(
    module: &ModuleInstance,
    extensions: &aestra_compiler::ExtensionRegistry,
    effect: &aestra_core::EffectAsset,
) -> ModuleExtensionState {
    let Some(plugin) = aestra_core::plugin_of(&module.module_type.0) else {
        return if extensions.modules.get(&module.module_type).is_some() {
            ModuleExtensionState::BuiltIn
        } else {
            ModuleExtensionState::Missing {
                plugin: String::new(),
                requirement: None,
            }
        };
    };
    let manifest = extensions
        .installed_manifest(&plugin)
        .filter(|_| extensions.modules.get(&module.module_type).is_some());
    let Some(manifest) = manifest else {
        return ModuleExtensionState::Missing {
            plugin: plugin.as_str().to_string(),
            requirement: effect
                .extension_requirement(&plugin)
                .map(|requirement| requirement.version.clone()),
        };
    };
    let provider = format!("{} {}", manifest.display_name, manifest.version);
    match extensions.module_schema_status(module) {
        Some(
            aestra_compiler::SchemaStatus::Newer { stored, current }
            | aestra_compiler::SchemaStatus::Older { stored, current },
        ) => ModuleExtensionState::SchemaMismatch {
            provider,
            stored,
            current,
        },
        _ => ModuleExtensionState::Provided { provider },
    }
}

fn spawn_extension_note(card: &mut ChildSpawnerCommands, text: &str, color: Color) {
    card.spawn((
        Text::new(text),
        bevy::feathers::theme::ThemedText,
        TextColor(color),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        Node {
            margin: UiRect::axes(Val::Px(2.0), Val::Px(4.0)),
            ..default()
        },
    ));
}

/// The §28.7 "MISSING EXTENSION" block (or its schema-mismatch variant): what is unavailable, which
/// plugin provides it, and that the authored data is preserved.
fn spawn_extension_unavailable_notice(
    card: &mut ChildSpawnerCommands,
    type_id: &str,
    state: &ModuleExtensionState,
) {
    let warning = super::stack_panel::diagnostic_color(aestra_core::DiagnosticSeverity::Warning);
    match state {
        ModuleExtensionState::SchemaMismatch {
            provider,
            stored,
            current,
        } => {
            spawn_extension_note(card, "INCOMPATIBLE EXTENSION DATA", warning);
            spawn_properties_read_only_control(card, "Type", type_id);
            spawn_properties_read_only_control(card, "Installed", provider);
            spawn_properties_read_only_control(
                card,
                "Schema",
                &format!("saved v{stored}, installed v{current}"),
            );
            spawn_extension_note(
                card,
                "The authored data is preserved read-only. Install a plugin version that supports it \
                 to edit or compile this module.",
                theme::TEXT_FAINT,
            );
        }
        ModuleExtensionState::Missing {
            plugin,
            requirement,
        } => {
            spawn_extension_note(card, "MISSING EXTENSION", warning);
            spawn_properties_read_only_control(card, "Type", type_id);
            if !plugin.is_empty() {
                let required = match requirement {
                    Some(requirement) => format!("{plugin} {requirement}"),
                    None => plugin.clone(),
                };
                spawn_properties_read_only_control(card, "Required plugin", &required);
            }
            spawn_extension_note(
                card,
                "The authored data is preserved. Compilation is unavailable until the extension is \
                 installed.",
                theme::TEXT_FAINT,
            );
        }
        ModuleExtensionState::BuiltIn | ModuleExtensionState::Provided { .. } => {}
    }
}

/// One read-only row per authored property of a preserved plugin payload (extensible-stages M9 phase
/// 9b, §20). The data is never dropped; a plugin that can read it turns these into editable controls.
fn spawn_custom_module_properties(
    card: &mut ChildSpawnerCommands,
    values: &std::collections::BTreeMap<String, Value>,
) {
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

    /// The inspector's view of a module against the installed extensions (extensible-stages M11,
    /// §28.7): built-in, provided (with its provider), missing (with the recorded requirement), or a
    /// schema the installed plugin cannot read.
    #[test]
    fn module_extension_state_explains_plugin_availability() {
        let mut installed = aestra_compiler::ExtensionRegistry::builtin();
        installed
            .install(&aestra_example_extension::ExampleExtension)
            .unwrap();
        let missing = aestra_compiler::ExtensionRegistry::builtin();
        let vortex_type = aestra_core::ModuleTypeId::new(aestra_example_extension::MODULE_VORTEX);
        let vortex = installed.modules.instantiate(&vortex_type).unwrap();
        let mut effect = aestra_core::EffectAsset::new("Plugins", 1.0);
        effect.extensions = vec![aestra_core::ExtensionRequirement::new(
            aestra_example_extension::PLUGIN_ID,
            "^0.1.0",
        )];

        let motion = ModuleInstance::motion([0.0; 3], 0.0, 0.0);
        assert_eq!(
            module_extension_state(&motion, &installed, &effect),
            ModuleExtensionState::BuiltIn
        );
        assert!(matches!(
            module_extension_state(&vortex, &installed, &effect),
            ModuleExtensionState::Provided { provider } if provider.starts_with("Aestra Example Extension")
        ));
        assert_eq!(
            module_extension_state(&vortex, &missing, &effect),
            ModuleExtensionState::Missing {
                plugin: aestra_example_extension::PLUGIN_ID.into(),
                requirement: Some("^0.1.0".into()),
            }
        );
        let mut newer = vortex.clone();
        newer.schema_version = Some(aestra_example_extension::VORTEX_SCHEMA_VERSION + 1);
        assert!(matches!(
            module_extension_state(&newer, &installed, &effect),
            ModuleExtensionState::SchemaMismatch { stored, current, .. }
                if stored == aestra_example_extension::VORTEX_SCHEMA_VERSION + 1
                    && current == aestra_example_extension::VORTEX_SCHEMA_VERSION
        ));
    }

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
    fn drag_insertion_follows_the_ghost_center_against_captured_rows() {
        let slot = |center_y| DragSlot {
            module: ModuleId::new(),
            entity: Entity::PLACEHOLDER,
            center_y,
        };
        let slots = [slot(10.0), slot(40.0), slot(70.0)];
        // Dragging the first row: it lands after every other row whose center it has passed.
        assert_eq!(module_drag_insert_index(&slots, 0, 5.0), 0);
        assert_eq!(module_drag_insert_index(&slots, 0, 30.0), 0);
        assert_eq!(module_drag_insert_index(&slots, 0, 50.0), 1);
        assert_eq!(module_drag_insert_index(&slots, 0, 80.0), 2);
        // Dragging the last row upward.
        assert_eq!(module_drag_insert_index(&slots, 2, 25.0), 1);
        assert_eq!(module_drag_insert_index(&slots, 2, 0.0), 0);
    }

    #[test]
    fn rows_passed_over_slide_one_pitch_toward_the_vacated_slot() {
        // Moving the first of three rows to the end: the two rows below it slide up.
        assert_eq!(module_drag_slide_target(1, 0, 2, 30.0), -30.0);
        assert_eq!(module_drag_slide_target(2, 0, 2, 30.0), -30.0);
        // Moving the last row to the top: the two rows above it slide down.
        assert_eq!(module_drag_slide_target(0, 2, 0, 30.0), 30.0);
        assert_eq!(module_drag_slide_target(1, 2, 0, 30.0), 30.0);
        // A partial move leaves rows outside the travelled range alone.
        assert_eq!(module_drag_slide_target(2, 0, 1, 30.0), 0.0);
        // No move, no slide.
        assert_eq!(module_drag_slide_target(0, 1, 1, 30.0), 0.0);
    }

    #[test]
    fn every_drag_drop_in_a_stage_produces_the_expected_order() {
        let stage_order = |session: &EditorSession| -> Vec<ModuleId> {
            session
                .selected_layer()
                .unwrap()
                .modules
                .iter()
                .filter(|module| module.stage == StageKind::ParticleUpdate)
                .map(|module| module.id)
                .collect()
        };
        for from in 0..3 {
            for to in 0..3 {
                let mut session = crate::test_support::session_with_timing_slack();
                let layer = session.selected_layer_index().unwrap();
                session.effect.emitters[layer]
                    .modules
                    .push(ModuleInstance::persistent());
                let before = stage_order(&session);
                assert_eq!(before.len(), 3, "three particle-update modules");
                let mut expected = before.clone();
                let moved = expected.remove(from);
                expected.insert(to, moved);

                if let Some((target, after)) = module_drag_drop_target(&before, from, to) {
                    session.reorder_module_relative(before[from], target, after);
                }
                assert_eq!(stage_order(&session), expected, "drag {from} -> {to}");
            }
        }
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
