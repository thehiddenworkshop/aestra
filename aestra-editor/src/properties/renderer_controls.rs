//! Renderer actions, synchronization, and Properties card composition.

use super::*;

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct RendererEnabledControl(pub(super) RendererId);

#[derive(Component, Debug, Clone, Copy)]
pub(super) enum RendererNumberControl {
    Softness(RendererId),
    Uv(RendererId, u8),
    FlipbookFrameRate(RendererId),
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct RendererSliderControl(pub(super) RendererNumberControl);

#[derive(Component, Debug, Clone, Copy)]
pub(super) enum RendererToggleControl {
    FlipbookLooping(RendererId),
    FlipbookRandomStart(RendererId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticMaterialValueSlot {
    Constant,
    RandomMinimum,
    RandomMaximum,
}

#[derive(Component, Debug, Clone)]
pub(super) struct SemanticMaterialNumberControl {
    instance: MaterialId,
    parameter: MaterialParameterId,
    slot: SemanticMaterialValueSlot,
    component: u8,
    fallback: MaterialValue,
}

#[derive(Component, Debug, Clone)]
pub(super) struct SemanticMaterialToggleControl {
    instance: MaterialId,
    parameter: MaterialParameterId,
    fallback: bool,
}

pub(super) fn handle_renderer_action(
    action: PropertiesAction,
    session: &mut EditorSession,
    palette: &mut ModulePaletteState,
    workspace: &mut CurvesState,
    layout: &mut WorkspaceLayout,
) -> bool {
    match action {
        PropertiesAction::AddSpriteRenderer => {
            session.add_sprite_renderer();
            palette.open = false;
        }
        PropertiesAction::AddFlipbookRenderer => {
            session.add_flipbook_renderer();
            palette.open = false;
        }
        PropertiesAction::SetRendererMaterial(id, index) => {
            if let Some(material) = session
                .effect
                .materials
                .get(index)
                .map(|material| material.id)
            {
                session.set_renderer_material(id, material);
            }
        }
        PropertiesAction::SetRendererBlend(id, blend) => {
            session.set_renderer_blend(id, blend);
        }
        PropertiesAction::SetRendererTexture(id, index) => {
            let texture = index
                .and_then(|index| session.effect.assets.get(index))
                .filter(|asset| asset.kind == aestra_core::AssetKind::Texture)
                .map(|asset| asset.id);
            session.set_renderer_texture(id, texture);
        }
        PropertiesAction::SetRendererFlipbook(id, index) => {
            if let Some(flipbook) = session
                .effect
                .flipbooks
                .get(index)
                .map(|flipbook| flipbook.id)
            {
                session.set_renderer_flipbook(id, flipbook);
            }
        }
        PropertiesAction::SetFlipbookTimeSource(id, value) => {
            session.set_flipbook_time_source(id, value);
        }
        PropertiesAction::SetFlipbookPlayback(id, value) => {
            session.set_flipbook_playback(id, value);
        }
        PropertiesAction::DuplicateRenderer(id) => session.duplicate_renderer(id),
        PropertiesAction::DeleteRenderer(id) => {
            if preview_renderer_deletion(session, id) {
                reveal_dock_panel(layout, session, DockPanel::Changes);
                workspace.clear();
            }
        }
        _ => return false,
    }
    true
}

fn preview_renderer_deletion(session: &mut EditorSession, renderer: RendererId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete renderer",
        EffectCommand::RemoveRenderer { emitter, renderer },
    ))
}

pub(super) fn sync_renderer_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RendererNumberControl), Added<RendererNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = renderer_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

pub(super) fn sync_renderer_slider_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RendererSliderControl), Added<RendererSliderControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = renderer_number_input_value(&session, control.0) else {
            continue;
        };
        commands.entity(entity).insert(SliderValue(value));
    }
}

pub(super) fn sync_semantic_material_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &SemanticMaterialNumberControl), Added<SemanticMaterialNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = semantic_material_number_value(&session, control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

pub(super) fn handle_semantic_material_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&SemanticMaterialNumberControl>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if semantic_material_number_value(&session, control)
        .is_some_and(|current| (current - change.value).abs() <= f32::EPSILON)
    {
        return;
    }
    let Some(value) = updated_semantic_material_value(&session, control, change.value) else {
        return;
    };
    commit_semantic_material_value(
        &mut session,
        catalog.as_deref(),
        control.instance,
        control.parameter,
        value,
    );
}

pub(super) fn handle_semantic_material_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&SemanticMaterialToggleControl>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let current = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == control.instance)
        .and_then(|instance| instance.values.get(&control.parameter))
        .and_then(|value| match value {
            MaterialParameterValue::Constant(MaterialValue::Bool(value)) => Some(*value),
            _ => None,
        })
        .unwrap_or(control.fallback);
    if current != change.value {
        commit_semantic_material_value(
            &mut session,
            catalog.as_deref(),
            control.instance,
            control.parameter,
            MaterialParameterValue::Constant(MaterialValue::Bool(change.value)),
        );
    }
}

fn commit_semantic_material_value(
    session: &mut EditorSession,
    catalog: Option<&ProjectEffectCatalog>,
    instance: MaterialId,
    parameter: MaterialParameterId,
    value: MaterialParameterValue,
) {
    let Some(catalog) = catalog else {
        session.status = "Material program catalog is unavailable".into();
        return;
    };
    match catalog.material_programs_for_effect(&session.effect) {
        Ok(programs) => {
            session.set_material_instance_parameter(&programs, instance, parameter, Some(value));
        }
        Err(error) => session.status = format!("Material program unavailable: {error}"),
    }
}

fn semantic_material_number_value(
    session: &EditorSession,
    control: &SemanticMaterialNumberControl,
) -> Option<f32> {
    let authored = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == control.instance)
        .and_then(|instance| instance.values.get(&control.parameter));
    let value = match (control.slot, authored) {
        (SemanticMaterialValueSlot::Constant, Some(MaterialParameterValue::Constant(value))) => {
            value
        }
        (
            SemanticMaterialValueSlot::RandomMinimum,
            Some(MaterialParameterValue::RandomRange { min, .. }),
        ) => min,
        (
            SemanticMaterialValueSlot::RandomMaximum,
            Some(MaterialParameterValue::RandomRange { max, .. }),
        ) => max,
        _ => &control.fallback,
    };
    material_value_component(value, control.component)
}

fn updated_semantic_material_value(
    session: &EditorSession,
    control: &SemanticMaterialNumberControl,
    value: f32,
) -> Option<MaterialParameterValue> {
    let instance = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == control.instance)?;
    match control.slot {
        SemanticMaterialValueSlot::Constant => {
            let mut updated = match instance.values.get(&control.parameter) {
                Some(MaterialParameterValue::Constant(value)) => value.clone(),
                _ => control.fallback.clone(),
            };
            set_material_value_component(&mut updated, control.component, value)?;
            Some(MaterialParameterValue::Constant(updated))
        }
        SemanticMaterialValueSlot::RandomMinimum | SemanticMaterialValueSlot::RandomMaximum => {
            let MaterialParameterValue::RandomRange { min, max, domain } =
                instance.values.get(&control.parameter)?
            else {
                return None;
            };
            let mut min = min.clone();
            let mut max = max.clone();
            let target = if control.slot == SemanticMaterialValueSlot::RandomMinimum {
                &mut min
            } else {
                &mut max
            };
            set_material_value_component(target, control.component, value)?;
            Some(MaterialParameterValue::RandomRange {
                min,
                max,
                domain: *domain,
            })
        }
    }
}

fn material_value_component(value: &MaterialValue, component: u8) -> Option<f32> {
    match value {
        MaterialValue::Float(value) if component == 0 => Some(*value),
        MaterialValue::Vec2(value) => value.get(component as usize).copied(),
        MaterialValue::Vec3(value) => value.get(component as usize).copied(),
        MaterialValue::Vec4(value) | MaterialValue::ColorSrgb(value) => {
            value.get(component as usize).copied()
        }
        MaterialValue::Texture2D(_) | MaterialValue::Bool(_) | MaterialValue::Float(_) => None,
    }
}

fn set_material_value_component(
    target: &mut MaterialValue,
    component: u8,
    value: f32,
) -> Option<()> {
    match target {
        MaterialValue::Float(target) if component == 0 => *target = value,
        MaterialValue::Vec2(target) => *target.get_mut(component as usize)? = value,
        MaterialValue::Vec3(target) => *target.get_mut(component as usize)? = value,
        MaterialValue::Vec4(target) | MaterialValue::ColorSrgb(target) => {
            *target.get_mut(component as usize)? = value;
        }
        MaterialValue::Texture2D(_) | MaterialValue::Bool(_) | MaterialValue::Float(_) => {
            return None;
        }
    }
    Some(())
}

pub(super) fn renderer_number_input_value(
    session: &EditorSession,
    control: RendererNumberControl,
) -> Option<f32> {
    let renderer_id = match control {
        RendererNumberControl::Softness(renderer)
        | RendererNumberControl::Uv(renderer, _)
        | RendererNumberControl::FlipbookFrameRate(renderer) => renderer,
    };
    let renderer = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == renderer_id)?;
    match control {
        RendererNumberControl::Softness(_) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { softness, .. } = &material.properties;
            match softness {
                MaterialInput::Constant(value) => Some(*value),
                MaterialInput::Parameter(_) => None,
            }
        }
        RendererNumberControl::Uv(_, component) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { uv, .. } = &material.properties;
            match component {
                0 => Some(uv.min[0]),
                1 => Some(uv.min[1]),
                2 => Some(uv.max[0]),
                3 => Some(uv.max[1]),
                _ => None,
            }
        }
        RendererNumberControl::FlipbookFrameRate(_) => {
            let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
                return None;
            };
            session
                .effect
                .flipbooks
                .iter()
                .find(|definition| definition.id == flipbook)
                .map(|definition| definition.frame_rate)
        }
    }
}

pub(super) fn renderer_number_step(control: RendererNumberControl) -> f32 {
    match control {
        RendererNumberControl::Softness(_) => 0.1,
        RendererNumberControl::Uv(_, _) => 0.05,
        RendererNumberControl::FlipbookFrameRate(_) => 1.0,
    }
}

pub(super) fn normalize_renderer_uv_scrub_value(
    session: &EditorSession,
    renderer: RendererId,
    component: u8,
    value: f32,
) -> f32 {
    let Some(material) = session
        .selected_layer()
        .renderers
        .iter()
        .find(|candidate| candidate.id == renderer)
        .and_then(|renderer| {
            session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)
        })
    else {
        return value.clamp(0.0, 1.0);
    };
    let MaterialProperties::Sprite { uv, .. } = &material.properties;
    match component {
        0 => value.clamp(0.0, uv.max[0]),
        1 => value.clamp(0.0, uv.max[1]),
        2 => value.clamp(uv.min[0], 1.0),
        3 => value.clamp(uv.min[1], 1.0),
        _ => value.clamp(0.0, 1.0),
    }
}

pub(super) fn renderer_numeric_scrub_command(
    session: &EditorSession,
    control: RendererNumberControl,
    value: f32,
) -> Option<EffectCommand> {
    let renderer_id = match control {
        RendererNumberControl::Softness(id)
        | RendererNumberControl::Uv(id, _)
        | RendererNumberControl::FlipbookFrameRate(id) => id,
    };
    let renderer = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == renderer_id)?;
    match control {
        RendererNumberControl::Softness(_) | RendererNumberControl::Uv(_, _) => {
            let mut material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?
                .clone();
            let MaterialProperties::Sprite { softness, uv, .. } = &mut material.properties;
            match control {
                RendererNumberControl::Softness(_) => {
                    let MaterialInput::Constant(current) = softness else {
                        return None;
                    };
                    *current = value.max(0.0);
                }
                RendererNumberControl::Uv(_, component) => match component {
                    0 => uv.min[0] = value.clamp(0.0, uv.max[0]),
                    1 => uv.min[1] = value.clamp(0.0, uv.max[1]),
                    2 => uv.max[0] = value.clamp(uv.min[0], 1.0),
                    3 => uv.max[1] = value.clamp(uv.min[1], 1.0),
                    _ => return None,
                },
                RendererNumberControl::FlipbookFrameRate(_) => unreachable!(),
            }
            Some(EffectCommand::SetMaterial {
                id: material.id,
                material,
            })
        }
        RendererNumberControl::FlipbookFrameRate(_) => {
            let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
                return None;
            };
            let mut definition = session
                .effect
                .flipbooks
                .iter()
                .find(|definition| definition.id == flipbook)?
                .clone();
            definition.frame_rate = value.clamp(1.0, 120.0);
            Some(EffectCommand::SetFlipbook {
                id: flipbook,
                flipbook: definition,
            })
        }
    }
}

pub(super) fn handle_renderer_enabled_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererEnabledControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let enabled = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == control.0)
        .map(|renderer| renderer.enabled);
    if enabled.is_some_and(|enabled| enabled != change.value) {
        session.toggle_renderer(control.0);
    }
}

pub(super) fn handle_renderer_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&RendererNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if renderer_number_input_value(&session, *control)
        .is_some_and(|current| (change.value - current).abs() <= f32::EPSILON)
    {
        return;
    }
    match *control {
        RendererNumberControl::Softness(renderer) => {
            session.set_renderer_softness(renderer, change.value)
        }
        RendererNumberControl::Uv(renderer, component) => {
            session.set_renderer_uv(renderer, component, change.value)
        }
        RendererNumberControl::FlipbookFrameRate(renderer) => {
            session.set_flipbook_frame_rate(renderer, change.value)
        }
    }
}

pub(super) fn handle_renderer_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    match *control {
        RendererToggleControl::FlipbookLooping(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { flipbook, .. } => session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|definition| definition.id == flipbook)
                        .map(|definition| definition.looping),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_looping(renderer_id);
            }
        }
        RendererToggleControl::FlipbookRandomStart(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { random_start, .. } => Some(random_start),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_random_start(renderer_id);
            }
        }
    }
}

pub(super) fn properties_renderer_collapsed(
    settings: &EditorSettings,
    renderer: &aestra_core::RendererInstance,
) -> bool {
    properties_renderer_card_memory(renderer).collapsed(&settings.properties.section_expansion)
}

pub(super) fn properties_renderer_card_memory(
    renderer: &aestra_core::RendererInstance,
) -> RememberedPanelCard {
    RememberedPanelCard::new(properties_renderer_key(renderer), false)
}

pub(super) fn properties_renderer_key(renderer: &aestra_core::RendererInstance) -> String {
    match renderer.properties {
        RendererProperties::Sprite => "renderer/sprite",
        RendererProperties::Flipbook { .. } => "renderer/flipbook",
        _ => "renderer/unknown",
    }
    .into()
}

fn spawn_renderer_scalar_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    unit: Option<&str>,
    control: RendererNumberControl,
    session: &EditorSession,
) {
    let bounded_slider = renderer_number_input_value(session, control).and_then(|value| {
        let (min, max, step) = match control {
            RendererNumberControl::Uv(_, _) => (0.0, 1.0, 0.01),
            RendererNumberControl::FlipbookFrameRate(_) => (1.0, 120.0, 1.0),
            RendererNumberControl::Softness(_) => return None,
        };
        SliderRowProps::new(value, min, max, step)
    });
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            if let Some(props) = bounded_slider {
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|controls| {
                    spawn_slider_input_pair(
                        controls,
                        props,
                        (
                            RendererSliderControl(control),
                            AccessibleLabel(title.to_owned()),
                        ),
                        (control, AccessibleLabel(title.to_owned())),
                    );
                });
            } else {
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn(Node {
                    width: Val::Px(112.0),
                    ..default()
                })
                .with_children(|input| {
                    input
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((control, AccessibleLabel(title.to_owned())));
                });
            }
            if let Some(unit) = unit {
                row.spawn_empty().apply_scene(label_dim(unit.to_owned()));
            }
        });
}

fn spawn_renderer_toggle_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    enabled: bool,
    control: RendererToggleControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox
                .apply_scene(ui_shell::feathers_checkbox())
                .insert((control, AccessibleLabel(title.to_owned())));
            if enabled {
                checkbox.insert(Checked);
            }
        });
}

fn spawn_semantic_material_controls(
    parent: &mut ChildSpawnerCommands,
    renderer: &aestra_core::RendererInstance,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<bool, String> {
    let Some(instance) = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == renderer.material)
    else {
        return Ok(false);
    };
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let program = programs
        .iter()
        .find(|program| program.id == instance.program.id())
        .ok_or_else(|| format!("material program {} is unavailable", instance.program.id()))?;
    let controls = MaterialCompiler
        .reflect_controls(program, Some(instance))
        .map_err(|error| error.to_string())?;

    spawn_properties_read_only_control(parent, "Material", &controls.name);
    for descriptor in &controls.parameters {
        spawn_semantic_material_parameter(parent, descriptor, instance.id, session);
    }
    Ok(true)
}

fn spawn_semantic_material_parameter(
    parent: &mut ChildSpawnerCommands,
    descriptor: &MaterialControlDescriptor,
    instance: MaterialId,
    session: &EditorSession,
) {
    let source = semantic_material_source_label(descriptor, session);
    spawn_properties_read_only_control(parent, &descriptor.name, &source);
    match descriptor.current_value.as_ref() {
        Some(MaterialParameterValue::Constant(value)) => match descriptor.control {
            MaterialControlKind::Number
            | MaterialControlKind::Vector2
            | MaterialControlKind::Vector3
            | MaterialControlKind::Vector4
            | MaterialControlKind::Color => {
                spawn_semantic_material_value_rows(
                    parent,
                    instance,
                    descriptor.id,
                    SemanticMaterialValueSlot::Constant,
                    value,
                    descriptor.control,
                    None,
                );
            }
            MaterialControlKind::Toggle => {
                if let MaterialValue::Bool(value) = value {
                    spawn_semantic_material_toggle(parent, instance, descriptor.id, *value);
                }
            }
            MaterialControlKind::Texture => {
                spawn_semantic_material_texture(parent, instance, descriptor.id, value, session);
            }
        },
        Some(MaterialParameterValue::RandomRange { min, max, .. }) => {
            spawn_semantic_material_value_rows(
                parent,
                instance,
                descriptor.id,
                SemanticMaterialValueSlot::RandomMinimum,
                min,
                descriptor.control,
                Some("Min"),
            );
            spawn_semantic_material_value_rows(
                parent,
                instance,
                descriptor.id,
                SemanticMaterialValueSlot::RandomMaximum,
                max,
                descriptor.control,
                Some("Max"),
            );
        }
        Some(
            MaterialParameterValue::EffectParameter(_)
            | MaterialParameterValue::EmitterParameter(_),
        )
        | None => {}
    }
}

fn semantic_material_source_label(
    descriptor: &MaterialControlDescriptor,
    session: &EditorSession,
) -> String {
    match descriptor.current_value.as_ref() {
        Some(MaterialParameterValue::Constant(_)) => {
            if descriptor.value_origin
                == aestra_compiler::MaterialControlValueOrigin::ProgramDefault
            {
                "Constant · program default".into()
            } else {
                "Constant".into()
            }
        }
        Some(MaterialParameterValue::EffectParameter(parameter)) => {
            format!("Effect · {}", semantic_parameter_name(session, *parameter))
        }
        Some(MaterialParameterValue::EmitterParameter(parameter)) => {
            format!("Emitter · {}", semantic_parameter_name(session, *parameter))
        }
        Some(MaterialParameterValue::RandomRange { .. }) => "Random range".into(),
        None => "Required · no value".into(),
    }
}

fn semantic_parameter_name(session: &EditorSession, parameter: ParameterId) -> String {
    session
        .effect
        .parameters
        .iter()
        .find(|candidate| candidate.id == parameter)
        .map_or_else(|| parameter.to_string(), |parameter| parameter.name.clone())
}

fn spawn_semantic_material_value_rows(
    parent: &mut ChildSpawnerCommands,
    instance: MaterialId,
    parameter: MaterialParameterId,
    slot: SemanticMaterialValueSlot,
    value: &MaterialValue,
    kind: MaterialControlKind,
    prefix: Option<&str>,
) {
    let labels: &[&str] = match kind {
        MaterialControlKind::Number => &["Value"],
        MaterialControlKind::Vector2 => &["X", "Y"],
        MaterialControlKind::Vector3 => &["X", "Y", "Z"],
        MaterialControlKind::Vector4 => &["X", "Y", "Z", "W"],
        MaterialControlKind::Color => &["R", "G", "B", "A"],
        MaterialControlKind::Texture | MaterialControlKind::Toggle => return,
    };
    for (component, label) in labels.iter().enumerate() {
        let label =
            prefix.map_or_else(|| (*label).to_owned(), |prefix| format!("{prefix} {label}"));
        spawn_semantic_material_number(
            parent,
            &label,
            SemanticMaterialNumberControl {
                instance,
                parameter,
                slot,
                component: component as u8,
                fallback: value.clone(),
            },
        );
    }
}

fn spawn_semantic_material_number(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    control: SemanticMaterialNumberControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn(Node {
                width: Val::Px(112.0),
                ..default()
            })
            .with_children(|input| {
                input
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert((control, AccessibleLabel(title.to_owned())));
            });
        });
}

fn spawn_semantic_material_toggle(
    parent: &mut ChildSpawnerCommands,
    instance: MaterialId,
    parameter: MaterialParameterId,
    value: bool,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, "Value");
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                SemanticMaterialToggleControl {
                    instance,
                    parameter,
                    fallback: value,
                },
                AccessibleLabel("Material parameter value".into()),
            ));
            if value {
                checkbox.insert(Checked);
            }
        });
}

fn spawn_semantic_material_texture(
    parent: &mut ChildSpawnerCommands,
    instance: MaterialId,
    parameter: MaterialParameterId,
    value: &MaterialValue,
    session: &EditorSession,
) {
    let MaterialValue::Texture2D(selected) = value else {
        return;
    };
    let options = session
        .effect
        .assets
        .iter()
        .enumerate()
        .filter(|(_, asset)| asset.kind == AssetKind::Texture)
        .map(|(index, asset)| ComboOption {
            label: asset.name.clone(),
            selected: asset.id == *selected,
            action: PropertiesAction::SetSemanticMaterialTexture {
                instance,
                parameter,
                asset: index,
            },
        })
        .collect::<Vec<_>>();
    let current = session
        .effect
        .assets
        .iter()
        .find(|asset| asset.id == *selected)
        .map_or("Missing texture", |asset| asset.name.as_str());
    spawn_properties_combo_row(parent, "Texture", current, &options, None);
}

pub(super) fn spawn_renderer_card(
    parent: &mut ChildSpawnerCommands,
    renderer: &aestra_core::RendererInstance,
    diagnostic_path: &str,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    collapsed: bool,
) {
    let display_name = match renderer.properties {
        RendererProperties::Sprite => "Sprite Renderer",
        RendererProperties::Flipbook { .. } => "Flipbook Renderer",
        _ => "Renderer",
    };
    let base_border = if session.selection.primary == SemanticTarget::Renderer(renderer.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    spawn_remembered_panel_card(
        parent,
        PanelCardProps::new(display_name, collapsed)
            .with_memory_key(properties_renderer_key(renderer))
            .with_help("Controls how this emitter is drawn.")
            .with_enabled(renderer.enabled)
            .with_border(base_border),
        PropertiesSemanticTarget {
            target: SemanticTarget::Renderer(renderer.id),
            base_border,
        },
        PropertiesSelectionTarget(SemanticTarget::Renderer(renderer.id)),
        PropertiesAction::ToggleSection(PropertiesSection::Renderer(renderer.id)),
        |header| {
            let mut enabled = header.spawn_empty();
            enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                RendererEnabledControl(renderer.id),
                AccessibleLabel("Enable renderer".into()),
            ));
            if renderer.enabled {
                enabled.insert(Checked);
            }
            spawn_action_menu(
                header,
                "Renderer actions",
                &[
                    ComboOption {
                        label: "Duplicate".into(),
                        selected: false,
                        action: PropertiesAction::DuplicateRenderer(renderer.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: PropertiesAction::DeleteRenderer(renderer.id),
                    },
                ],
            );
        },
        |card| {
            match spawn_semantic_material_controls(card, renderer, session, catalog) {
                Ok(true) => {
                    spawn_inline_diagnostics(card, diagnostic_path, session);
                    return;
                }
                Ok(false) => {}
                Err(error) => {
                    spawn_properties_read_only_control(
                        card,
                        "Semantic material",
                        &format!("Unavailable · {error}"),
                    );
                    spawn_inline_diagnostics(card, diagnostic_path, session);
                    return;
                }
            }
            let Some(material) = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)
            else {
                spawn_properties_read_only_control(card, "Material", "Missing");
                spawn_inline_diagnostics(card, diagnostic_path, session);
                return;
            };
            let material_options = session
                .effect
                .materials
                .iter()
                .enumerate()
                .map(|(index, candidate)| ComboOption {
                    label: candidate.name.clone(),
                    selected: candidate.id == material.id,
                    action: PropertiesAction::SetRendererMaterial(renderer.id, index),
                })
                .collect::<Vec<_>>();
            spawn_properties_combo_row(card, "Material", &material.name, &material_options, None);
            let blend_options = [BlendMode::Alpha, BlendMode::Additive, BlendMode::Multiply]
                .into_iter()
                .map(|blend| ComboOption {
                    label: format!("{blend:?}"),
                    selected: blend == material.blend,
                    action: PropertiesAction::SetRendererBlend(renderer.id, blend),
                })
                .collect::<Vec<_>>();
            spawn_properties_combo_row(
                card,
                "Blend",
                &format!("{:?}", material.blend),
                &blend_options,
                None,
            );
            let MaterialProperties::Sprite {
                softness, texture, ..
            } = &material.properties;
            match softness {
                MaterialInput::Constant(_) => spawn_renderer_scalar_control(
                    card,
                    "Softness",
                    None,
                    RendererNumberControl::Softness(renderer.id),
                    session,
                ),
                MaterialInput::Parameter(parameter) => spawn_properties_read_only_control(
                    card,
                    "Softness",
                    &format!("Parameter {parameter}"),
                ),
            }
            match &renderer.properties {
                RendererProperties::Sprite => {
                    let texture_name = texture
                        .and_then(|id| session.effect.assets.iter().find(|asset| asset.id == id))
                        .map_or("Procedural", |asset| asset.name.as_str());
                    let mut texture_options = vec![ComboOption {
                        label: "Procedural".into(),
                        selected: texture.is_none(),
                        action: PropertiesAction::SetRendererTexture(renderer.id, None),
                    }];
                    texture_options.extend(
                        session
                            .effect
                            .assets
                            .iter()
                            .enumerate()
                            .filter(|(_, asset)| asset.kind == aestra_core::AssetKind::Texture)
                            .map(|(index, asset)| ComboOption {
                                label: asset.name.clone(),
                                selected: Some(asset.id) == *texture,
                                action: PropertiesAction::SetRendererTexture(
                                    renderer.id,
                                    Some(index),
                                ),
                            }),
                    );
                    spawn_properties_combo_row(
                        card,
                        "Texture",
                        texture_name,
                        &texture_options,
                        None,
                    );
                    if texture.is_some() {
                        for (label, component) in [
                            ("UV Min X", 0),
                            ("UV Min Y", 1),
                            ("UV Max X", 2),
                            ("UV Max Y", 3),
                        ] {
                            spawn_renderer_scalar_control(
                                card,
                                label,
                                None,
                                RendererNumberControl::Uv(renderer.id, component),
                                session,
                            );
                        }
                    }
                }
                RendererProperties::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => {
                    let definition = session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|item| item.id == *flipbook);
                    let flipbook_options = session
                        .effect
                        .flipbooks
                        .iter()
                        .enumerate()
                        .map(|(index, candidate)| ComboOption {
                            label: candidate.name.clone(),
                            selected: candidate.id == *flipbook,
                            action: PropertiesAction::SetRendererFlipbook(renderer.id, index),
                        })
                        .collect::<Vec<_>>();
                    spawn_properties_combo_row(
                        card,
                        "Flipbook",
                        definition.map_or("Missing", |item| item.name.as_str()),
                        &flipbook_options,
                        None,
                    );
                    if let Some(definition) = definition {
                        spawn_renderer_scalar_control(
                            card,
                            "Frame Rate",
                            Some("FPS"),
                            RendererNumberControl::FlipbookFrameRate(renderer.id),
                            session,
                        );
                        spawn_renderer_toggle_control(
                            card,
                            "Looping",
                            definition.looping,
                            RendererToggleControl::FlipbookLooping(renderer.id),
                        );
                    }
                    let time_source_options = [
                        FlipbookTimeSource::ParticleAge,
                        FlipbookTimeSource::EffectTime,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *time_source,
                        action: PropertiesAction::SetFlipbookTimeSource(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_properties_combo_row(
                        card,
                        "Time Source",
                        &format!("{time_source:?}"),
                        &time_source_options,
                        None,
                    );
                    let playback_options = [
                        FlipbookPlaybackMode::Forward,
                        FlipbookPlaybackMode::Reverse,
                        FlipbookPlaybackMode::PingPong,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *playback,
                        action: PropertiesAction::SetFlipbookPlayback(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_properties_combo_row(
                        card,
                        "Playback",
                        &format!("{playback:?}"),
                        &playback_options,
                        None,
                    );
                    spawn_renderer_toggle_control(
                        card,
                        "Random Start",
                        *random_start,
                        RendererToggleControl::FlipbookRandomStart(renderer.id),
                    );
                }
                _ => {}
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_action_handler_owns_renderer_creation_only() {
        let mut session = test_support::session_with_timing_slack();
        let initial = session.selected_layer().renderers.len();
        let mut palette = ModulePaletteState {
            open: true,
            ..default()
        };
        let mut curves = CurvesState::default();
        let mut layout = WorkspaceLayout::default();

        assert!(handle_renderer_action(
            PropertiesAction::AddSpriteRenderer,
            &mut session,
            &mut palette,
            &mut curves,
            &mut layout,
        ));
        assert_eq!(session.selected_layer().renderers.len(), initial + 1);
        assert!(!palette.open);
        assert!(!handle_renderer_action(
            PropertiesAction::CloseModulePalette,
            &mut session,
            &mut palette,
            &mut curves,
            &mut layout,
        ));
    }

    #[test]
    fn renderer_numeric_steps_match_authored_domains() {
        let id = RendererId::new();
        assert_eq!(
            renderer_number_step(RendererNumberControl::Softness(id)),
            0.1
        );
        assert_eq!(renderer_number_step(RendererNumberControl::Uv(id, 0)), 0.05);
        assert_eq!(
            renderer_number_step(RendererNumberControl::FlipbookFrameRate(id)),
            1.0
        );
    }

    #[test]
    fn semantic_material_numeric_edits_preserve_vector_and_random_components() {
        let mut session = test_support::session_with_timing_slack();
        let instance = MaterialId::from_u128(0x7200);
        let parameter = MaterialParameterId::from_u128(0x7201);
        session
            .effect
            .material_instances
            .push(aestra_core::material::MaterialInstance {
                id: instance,
                program: aestra_core::material::MaterialProgramRef::BuiltIn(
                    aestra_core::MaterialProgramId::from_u128(0x7202),
                ),
                values: std::collections::BTreeMap::new(),
                render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
            });
        let constant = SemanticMaterialNumberControl {
            instance,
            parameter,
            slot: SemanticMaterialValueSlot::Constant,
            component: 1,
            fallback: MaterialValue::Vec3([1.0, 2.0, 3.0]),
        };
        assert_eq!(
            updated_semantic_material_value(&session, &constant, 8.0),
            Some(MaterialParameterValue::Constant(MaterialValue::Vec3([
                1.0, 8.0, 3.0
            ])))
        );

        session.effect.material_instances[0].values.insert(
            parameter,
            MaterialParameterValue::RandomRange {
                min: MaterialValue::Vec2([1.0, 2.0]),
                max: MaterialValue::Vec2([3.0, 4.0]),
                domain: aestra_core::material::MaterialEvaluationDomain::Emitter,
            },
        );
        let random_maximum = SemanticMaterialNumberControl {
            instance,
            parameter,
            slot: SemanticMaterialValueSlot::RandomMaximum,
            component: 0,
            fallback: MaterialValue::Vec2([3.0, 4.0]),
        };
        assert_eq!(
            updated_semantic_material_value(&session, &random_maximum, 9.0),
            Some(MaterialParameterValue::RandomRange {
                min: MaterialValue::Vec2([1.0, 2.0]),
                max: MaterialValue::Vec2([9.0, 4.0]),
                domain: aestra_core::material::MaterialEvaluationDomain::Emitter,
            })
        );
    }
}
