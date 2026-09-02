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

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialStackPropertyNumberControl {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    property: MaterialStackProperty,
    component: u8,
    value: MaterialValue,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialStackPropertyToggleControl {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    property: MaterialStackProperty,
    value: bool,
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

pub(super) fn sync_material_stack_property_number_inputs(
    mut commands: Commands,
    controls: Query<
        (Entity, &MaterialStackPropertyNumberControl),
        Added<MaterialStackPropertyNumberControl>,
    >,
) {
    for (entity, control) in &controls {
        let Some(value) = material_value_component(&control.value, control.component) else {
            continue;
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

pub(super) fn handle_material_stack_property_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&MaterialStackPropertyNumberControl>,
    mut catalog: Option<ResMut<ProjectEffectCatalog>>,
    mut material_history: Option<ResMut<MaterialProgramEditHistory>>,
    mut history_ledger: Option<ResMut<EditorHistoryLedger>>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let Some(current) = material_value_component(&control.value, control.component) else {
        return;
    };
    if (current - change.value).abs() <= f32::EPSILON {
        return;
    }
    let mut value = control.value.clone();
    if set_material_value_component(&mut value, control.component, change.value).is_none() {
        return;
    }
    apply_material_program_edit(
        &mut session,
        catalog.as_deref_mut(),
        material_history.as_deref_mut(),
        history_ledger.as_deref_mut(),
        control.program,
        "Edited material modifier",
        |_, current| {
            MaterialCompiler
                .plan_stack_set_property(current, control.expression, control.property, value)
                .map(|plan| plan.replacement)
                .map_err(|error| error.to_string())
        },
    );
}

pub(super) fn handle_material_stack_property_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&MaterialStackPropertyToggleControl>,
    mut commands: Commands,
    mut catalog: Option<ResMut<ProjectEffectCatalog>>,
    mut material_history: Option<ResMut<MaterialProgramEditHistory>>,
    mut history_ledger: Option<ResMut<EditorHistoryLedger>>,
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
    if control.value == change.value {
        return;
    }
    apply_material_program_edit(
        &mut session,
        catalog.as_deref_mut(),
        material_history.as_deref_mut(),
        history_ledger.as_deref_mut(),
        control.program,
        "Edited material modifier",
        |_, current| {
            MaterialCompiler
                .plan_stack_set_property(
                    current,
                    control.expression,
                    control.property,
                    MaterialValue::Bool(change.value),
                )
                .map(|plan| plan.replacement)
                .map_err(|error| error.to_string())
        },
    );
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

pub(super) fn set_semantic_material_source(
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    instance: MaterialId,
    parameter: MaterialParameterId,
    source: SemanticMaterialSourceChoice,
) -> bool {
    let programs = match catalog.material_programs_for_effect(&session.effect) {
        Ok(programs) => programs,
        Err(error) => {
            session.status = format!("Material program unavailable: {error}");
            return false;
        }
    };
    let Some(material_instance) = session
        .effect
        .material_instances
        .iter()
        .find(|candidate| candidate.id == instance)
    else {
        session.status = "Material instance is no longer available".into();
        return false;
    };
    let Some(program) = programs
        .iter()
        .find(|program| program.id == material_instance.program.id())
    else {
        session.status = format!(
            "Material program {} is unavailable",
            material_instance.program.id()
        );
        return false;
    };
    let controls = match MaterialCompiler.reflect_controls(program, Some(material_instance)) {
        Ok(controls) => controls,
        Err(error) => {
            session.status = format!("Material controls unavailable: {error}");
            return false;
        }
    };
    let Some(descriptor) = controls
        .parameters
        .iter()
        .find(|descriptor| descriptor.id == parameter)
    else {
        session.status = "Material parameter is no longer available".into();
        return false;
    };
    if !semantic_material_source_is_supported(descriptor, source, session) {
        session.status = "That material source is incompatible with this parameter".into();
        return false;
    }
    let value = match source {
        SemanticMaterialSourceChoice::Constant => {
            let Some(value) = semantic_material_constant_value(descriptor, session) else {
                session.status = "No compatible constant value is available".into();
                return false;
            };
            MaterialParameterValue::Constant(value)
        }
        SemanticMaterialSourceChoice::RandomRange => {
            if let Some(MaterialParameterValue::RandomRange { .. }) = &descriptor.current_value {
                descriptor
                    .current_value
                    .clone()
                    .expect("matched material range must exist")
            } else {
                let Some(value) = semantic_material_constant_value(descriptor, session) else {
                    session.status = "No compatible random-range value is available".into();
                    return false;
                };
                MaterialParameterValue::RandomRange {
                    min: value.clone(),
                    max: value,
                    domain: descriptor.evaluation_domain,
                }
            }
        }
        SemanticMaterialSourceChoice::EffectParameter(parameter) => {
            MaterialParameterValue::EffectParameter(parameter)
        }
        SemanticMaterialSourceChoice::EmitterParameter(parameter) => {
            MaterialParameterValue::EmitterParameter(parameter)
        }
    };
    session.set_material_instance_parameter(&programs, instance, parameter, Some(value))
}

pub(super) fn set_semantic_material_render_state(
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    instance: MaterialId,
    render_state: MaterialRenderState,
) -> bool {
    let programs = match catalog.material_programs_for_effect(&session.effect) {
        Ok(programs) => programs,
        Err(error) => {
            session.status = format!("Material program unavailable: {error}");
            return false;
        }
    };
    session.set_material_instance_render_state(&programs, instance, render_state)
}

fn semantic_material_source_is_supported(
    descriptor: &MaterialControlDescriptor,
    source: SemanticMaterialSourceChoice,
    session: &EditorSession,
) -> bool {
    let (kind, binding) = match source {
        SemanticMaterialSourceChoice::Constant => (MaterialControlSource::Constant, None),
        SemanticMaterialSourceChoice::RandomRange => (MaterialControlSource::RandomRange, None),
        SemanticMaterialSourceChoice::EffectParameter(parameter) => {
            (MaterialControlSource::EffectParameter, Some(parameter))
        }
        SemanticMaterialSourceChoice::EmitterParameter(parameter) => {
            (MaterialControlSource::EmitterParameter, Some(parameter))
        }
    };
    descriptor.supported_sources.contains(&kind)
        && binding.is_none_or(|binding| {
            session.effect.parameters.iter().any(|parameter| {
                parameter.id == binding
                    && parameter.exposed
                    && descriptor
                        .value_type
                        .accepts_effect_value(&parameter.default)
            })
        })
}

fn semantic_material_constant_value(
    descriptor: &MaterialControlDescriptor,
    session: &EditorSession,
) -> Option<MaterialValue> {
    match descriptor.current_value.as_ref() {
        Some(MaterialParameterValue::Constant(value)) => Some(value.clone()),
        Some(MaterialParameterValue::RandomRange { min, max, .. }) => {
            midpoint_material_value(min, max)
        }
        Some(
            MaterialParameterValue::EffectParameter(_)
            | MaterialParameterValue::EmitterParameter(_),
        )
        | None => descriptor
            .default_value
            .clone()
            .or_else(|| fallback_material_value(descriptor.value_type, session)),
    }
}

fn midpoint_material_value(min: &MaterialValue, max: &MaterialValue) -> Option<MaterialValue> {
    fn midpoint(min: f32, max: f32) -> f32 {
        min + (max - min) * 0.5
    }
    match (min, max) {
        (MaterialValue::Float(min), MaterialValue::Float(max)) => {
            Some(MaterialValue::Float(midpoint(*min, *max)))
        }
        (MaterialValue::Vec2(min), MaterialValue::Vec2(max)) => Some(MaterialValue::Vec2([
            midpoint(min[0], max[0]),
            midpoint(min[1], max[1]),
        ])),
        (MaterialValue::Vec3(min), MaterialValue::Vec3(max)) => Some(MaterialValue::Vec3([
            midpoint(min[0], max[0]),
            midpoint(min[1], max[1]),
            midpoint(min[2], max[2]),
        ])),
        (MaterialValue::Vec4(min), MaterialValue::Vec4(max)) => Some(MaterialValue::Vec4([
            midpoint(min[0], max[0]),
            midpoint(min[1], max[1]),
            midpoint(min[2], max[2]),
            midpoint(min[3], max[3]),
        ])),
        (MaterialValue::ColorSrgb(min), MaterialValue::ColorSrgb(max)) => {
            Some(MaterialValue::ColorSrgb([
                midpoint(min[0], max[0]),
                midpoint(min[1], max[1]),
                midpoint(min[2], max[2]),
                midpoint(min[3], max[3]),
            ]))
        }
        _ => None,
    }
}

fn fallback_material_value(
    value_type: MaterialValueType,
    session: &EditorSession,
) -> Option<MaterialValue> {
    match value_type {
        MaterialValueType::Float => Some(MaterialValue::Float(0.0)),
        MaterialValueType::Vec2 => Some(MaterialValue::Vec2([0.0; 2])),
        MaterialValueType::Vec3 => Some(MaterialValue::Vec3([0.0; 3])),
        MaterialValueType::Vec4 => Some(MaterialValue::Vec4([0.0; 4])),
        MaterialValueType::Color => Some(MaterialValue::ColorSrgb([1.0; 4])),
        MaterialValueType::Texture2D(_) => session
            .effect
            .assets
            .iter()
            .find(|asset| asset.kind == AssetKind::Texture)
            .map(|asset| MaterialValue::Texture2D(asset.id)),
        MaterialValueType::Bool => Some(MaterialValue::Bool(false)),
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
    inspector: &MaterialStackInspectorState,
    asset_server: &AssetServer,
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
    let stack = MaterialCompiler
        .project_stack(program)
        .map_err(|error| error.to_string())?;

    spawn_properties_read_only_control(parent, "Material", &controls.name);
    spawn_semantic_material_stack(parent, program, &stack, inspector);
    for descriptor in &controls.parameters {
        spawn_semantic_material_parameter(parent, descriptor, instance.id, session, asset_server);
    }
    spawn_semantic_material_render_state_controls(
        parent,
        instance.id,
        &controls.render_state_policy,
        controls.current_render_state,
    );
    Ok(true)
}

fn spawn_semantic_material_stack(
    parent: &mut ChildSpawnerCommands,
    program: &aestra_core::material::MaterialProgram,
    projection: &MaterialStackProjection,
    inspector: &MaterialStackInspectorState,
) {
    match projection {
        MaterialStackProjection::Stack { entries } if entries.is_empty() => {
            spawn_properties_read_only_control(parent, "Stack", "No semantic modifiers");
        }
        MaterialStackProjection::Stack { entries } => {
            spawn_properties_read_only_control(parent, "Material Stack", "Semantic modifiers");
            let preset_targets = MaterialCompiler
                .stack_preset_targets(program)
                .unwrap_or_default();
            let preset_options =
                material_stack_preset_options(program.id, entries, &preset_targets);
            if !preset_options.is_empty() {
                spawn_properties_combo_row(
                    parent,
                    "Preset",
                    "Choose",
                    &preset_options,
                    Some("Insert a configured modifier chain as one undoable material edit."),
                );
            }
            let insert_targets = MaterialCompiler
                .stack_insert_targets(program)
                .unwrap_or_default();
            let insert_options =
                material_stack_insert_options(program.id, entries, &insert_targets);
            if !insert_options.is_empty() {
                spawn_properties_combo_row(
                    parent,
                    "Add",
                    "Modifier",
                    &insert_options,
                    Some("Insert a compatible semantic modifier at a valid stack position."),
                );
            }
            for (index, entry) in entries.iter().enumerate() {
                let targets = MaterialCompiler
                    .stack_move_targets(program, entry.expression)
                    .unwrap_or_default();
                let options = material_stack_modifier_options(program, entries, index, &targets);
                let current = if entry.enabled {
                    entry.kind.display_name().to_owned()
                } else {
                    format!("{} (Disabled)", entry.kind.display_name())
                };
                if options.is_empty() {
                    spawn_properties_read_only_control(parent, "Modifier", &current);
                } else {
                    spawn_properties_combo_row(
                        parent,
                        "Modifier",
                        &current,
                        &options,
                        Some("Enable, disable, remove, or move this semantic modifier."),
                    );
                }
                if inspector.selected == Some((program.id, entry.expression)) {
                    spawn_material_stack_modifier_inspector(parent, program, entry);
                }
            }
        }
        MaterialStackProjection::Advanced { reason } => {
            spawn_properties_read_only_control(parent, "Material Stack", reason.display_name());
        }
    }
}

fn material_stack_preset_options(
    program: MaterialProgramId,
    entries: &[MaterialStackEntry],
    targets: &[MaterialStackPresetTarget],
) -> Vec<ComboOption<PropertiesAction>> {
    targets
        .iter()
        .map(|target| {
            let position = target.index.checked_sub(1).map_or_else(
                || "First".to_owned(),
                |index| format!("After {}", entries[index].kind.display_name()),
            );
            ComboOption {
                label: format!("{} · {position}", target.preset.display_name()),
                selected: false,
                action: PropertiesAction::InsertSemanticMaterialPreset {
                    program,
                    preset: target.preset,
                    target_index: target.index,
                },
            }
        })
        .collect()
}

fn material_stack_insert_options(
    program: MaterialProgramId,
    entries: &[MaterialStackEntry],
    targets: &[MaterialStackInsertTarget],
) -> Vec<ComboOption<PropertiesAction>> {
    targets
        .iter()
        .map(|target| {
            let position = target.index.checked_sub(1).map_or_else(
                || "First".to_owned(),
                |index| format!("After {}", entries[index].kind.display_name()),
            );
            ComboOption {
                label: format!("{} · {position}", target.kind.display_name()),
                selected: false,
                action: PropertiesAction::InsertSemanticMaterialModifier {
                    program,
                    kind: target.kind,
                    target_index: target.index,
                },
            }
        })
        .collect()
}

fn material_stack_modifier_options(
    program: &aestra_core::material::MaterialProgram,
    entries: &[MaterialStackEntry],
    from_index: usize,
    targets: &[MaterialStackMoveTarget],
) -> Vec<ComboOption<PropertiesAction>> {
    let entry = &entries[from_index];
    let mut options = vec![ComboOption {
        label: "Edit settings".to_owned(),
        selected: false,
        action: PropertiesAction::InspectSemanticMaterialModifier {
            program: program.id,
            expression: entry.expression,
        },
    }];
    if MaterialCompiler
        .plan_stack_set_enabled(program, entry.expression, !entry.enabled)
        .is_ok()
    {
        options.push(ComboOption {
            label: if entry.enabled { "Disable" } else { "Enable" }.to_owned(),
            selected: false,
            action: PropertiesAction::SetSemanticMaterialModifierEnabled {
                program: program.id,
                expression: entry.expression,
                enabled: !entry.enabled,
            },
        });
    }
    options.extend(targets.iter().filter_map(|target| {
        let anchor = entries.get(target.index)?.kind.display_name();
        let label = if target.index < from_index {
            format!("Before {anchor}")
        } else {
            format!("After {anchor}")
        };
        Some(ComboOption {
            label,
            selected: false,
            action: PropertiesAction::MoveSemanticMaterialModifier {
                program: program.id,
                expression: entry.expression,
                target_index: target.index,
            },
        })
    }));
    if MaterialCompiler
        .plan_stack_remove(program, entry.expression)
        .is_ok()
    {
        options.push(ComboOption {
            label: "Remove".to_owned(),
            selected: false,
            action: PropertiesAction::RemoveSemanticMaterialModifier {
                program: program.id,
                expression: entry.expression,
            },
        });
    }
    options
}

fn spawn_material_stack_modifier_inspector(
    parent: &mut ChildSpawnerCommands,
    program: &aestra_core::material::MaterialProgram,
    entry: &MaterialStackEntry,
) {
    let properties = MaterialCompiler
        .stack_modifier_properties(program, entry.expression)
        .unwrap_or_default();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(7.0)),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::ACCENT_DIM),
        ))
        .with_children(|inspector| {
            inspector.spawn((
                Text::new(format!("{} Settings", entry.kind.display_name())),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            if properties.is_empty() {
                spawn_properties_read_only_control(inspector, "Settings", "No editable constants");
                return;
            }
            for descriptor in properties {
                spawn_material_stack_property(inspector, program.id, entry.expression, descriptor);
            }
        });
}

fn spawn_material_stack_property(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    descriptor: aestra_compiler::MaterialStackPropertyDescriptor,
) {
    let components: &[&str] = match &descriptor.value {
        MaterialValue::Float(_) => &[""],
        MaterialValue::Vec2(_) => &["X", "Y"],
        MaterialValue::Vec3(_) => &["X", "Y", "Z"],
        MaterialValue::Vec4(_) | MaterialValue::ColorSrgb(_) => &["X", "Y", "Z", "W"],
        MaterialValue::Bool(value) => {
            spawn_material_stack_property_toggle(
                parent,
                descriptor.name,
                MaterialStackPropertyToggleControl {
                    program,
                    expression,
                    property: descriptor.property,
                    value: *value,
                },
            );
            return;
        }
        MaterialValue::Texture2D(_) => {
            spawn_properties_read_only_control(parent, descriptor.name, "Texture source");
            return;
        }
    };
    for (component, suffix) in components.iter().enumerate() {
        let title = if suffix.is_empty() {
            descriptor.name.to_owned()
        } else {
            format!("{} {suffix}", descriptor.name)
        };
        spawn_material_stack_property_number(
            parent,
            &title,
            MaterialStackPropertyNumberControl {
                program,
                expression,
                property: descriptor.property,
                component: component as u8,
                value: descriptor.value.clone(),
            },
        );
    }
}

fn spawn_material_stack_property_number(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    control: MaterialStackPropertyNumberControl,
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

fn spawn_material_stack_property_toggle(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    control: MaterialStackPropertyToggleControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
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
            if control.value {
                checkbox.insert(Checked);
            }
        });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticRenderStateField {
    Blend,
    DepthTest,
    DepthWrite,
    CullMode,
}

fn spawn_semantic_material_render_state_controls(
    parent: &mut ChildSpawnerCommands,
    instance: MaterialId,
    policy: &MaterialRenderStatePolicy,
    current: MaterialRenderState,
) {
    spawn_semantic_render_state_row(
        parent,
        "Blend",
        instance,
        current,
        semantic_render_state_candidates(policy, current, SemanticRenderStateField::Blend),
        semantic_blend_label,
        "Blend mode. Only transitions allowed by this material program are available.",
    );
    spawn_semantic_render_state_row(
        parent,
        "Depth Test",
        instance,
        current,
        semantic_render_state_candidates(policy, current, SemanticRenderStateField::DepthTest),
        semantic_depth_test_label,
        "Depth comparison. Only transitions allowed by this material program are available.",
    );
    spawn_semantic_render_state_row(
        parent,
        "Depth Write",
        instance,
        current,
        semantic_render_state_candidates(policy, current, SemanticRenderStateField::DepthWrite),
        semantic_depth_write_label,
        "Controls depth-buffer writes. Only transitions allowed by this material program are available.",
    );
    spawn_semantic_render_state_row(
        parent,
        "Cull Mode",
        instance,
        current,
        semantic_render_state_candidates(policy, current, SemanticRenderStateField::CullMode),
        semantic_cull_mode_label,
        "Face culling. Only transitions allowed by this material program are available.",
    );
}

fn spawn_semantic_render_state_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    instance: MaterialId,
    current: MaterialRenderState,
    candidates: Vec<MaterialRenderState>,
    label: fn(MaterialRenderState) -> &'static str,
    description: &str,
) {
    let current_label = label(current);
    if candidates.len() <= 1 {
        spawn_properties_read_only_control(parent, title, current_label);
        return;
    }
    let options = candidates
        .into_iter()
        .map(|render_state| ComboOption {
            label: label(render_state).into(),
            selected: render_state == current,
            action: PropertiesAction::SetSemanticMaterialRenderState {
                instance,
                render_state,
            },
        })
        .collect::<Vec<_>>();
    spawn_properties_combo_row(parent, title, current_label, &options, Some(description));
}

fn semantic_render_state_candidates(
    policy: &MaterialRenderStatePolicy,
    current: MaterialRenderState,
    field: SemanticRenderStateField,
) -> Vec<MaterialRenderState> {
    policy
        .allowed
        .iter()
        .copied()
        .filter(|candidate| match field {
            SemanticRenderStateField::Blend => {
                candidate.depth_test == current.depth_test
                    && candidate.depth_write == current.depth_write
                    && candidate.cull_mode == current.cull_mode
            }
            SemanticRenderStateField::DepthTest => {
                candidate.blend == current.blend
                    && candidate.depth_write == current.depth_write
                    && candidate.cull_mode == current.cull_mode
            }
            SemanticRenderStateField::DepthWrite => {
                candidate.blend == current.blend
                    && candidate.depth_test == current.depth_test
                    && candidate.cull_mode == current.cull_mode
            }
            SemanticRenderStateField::CullMode => {
                candidate.blend == current.blend
                    && candidate.depth_test == current.depth_test
                    && candidate.depth_write == current.depth_write
            }
        })
        .collect()
}

fn semantic_blend_label(state: MaterialRenderState) -> &'static str {
    match state.blend {
        BlendMode::Alpha => "Alpha",
        BlendMode::Additive => "Additive",
        BlendMode::Multiply => "Multiply",
    }
}

fn semantic_depth_test_label(state: MaterialRenderState) -> &'static str {
    match state.depth_test {
        MaterialDepthTest::Disabled => "Disabled",
        MaterialDepthTest::Less => "Less",
        MaterialDepthTest::LessEqual => "Less or Equal",
        MaterialDepthTest::Always => "Always",
    }
}

fn semantic_depth_write_label(state: MaterialRenderState) -> &'static str {
    if state.depth_write {
        "Enabled"
    } else {
        "Disabled"
    }
}

fn semantic_cull_mode_label(state: MaterialRenderState) -> &'static str {
    match state.cull_mode {
        MaterialCullMode::None => "None",
        MaterialCullMode::Front => "Front",
        MaterialCullMode::Back => "Back",
    }
}

fn spawn_semantic_material_parameter(
    parent: &mut ChildSpawnerCommands,
    descriptor: &MaterialControlDescriptor,
    instance: MaterialId,
    session: &EditorSession,
    asset_server: &AssetServer,
) {
    let source = semantic_material_source_label(descriptor, session);
    spawn_semantic_material_source_row(
        parent,
        descriptor,
        instance,
        session,
        asset_server,
        &source,
    );
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

fn spawn_semantic_material_source_row(
    parent: &mut ChildSpawnerCommands,
    descriptor: &MaterialControlDescriptor,
    instance: MaterialId,
    session: &EditorSession,
    asset_server: &AssetServer,
    source_label: &str,
) {
    let current = semantic_material_source_choice(descriptor.current_value.as_ref());
    let mut options = Vec::new();
    if descriptor
        .supported_sources
        .contains(&MaterialControlSource::Constant)
        && semantic_material_constant_value(descriptor, session).is_some()
    {
        options.push(ComboOption {
            label: "Constant".into(),
            selected: current == Some(SemanticMaterialSourceChoice::Constant),
            action: PropertiesAction::SetSemanticMaterialSource {
                instance,
                parameter: descriptor.id,
                source: SemanticMaterialSourceChoice::Constant,
            },
        });
    }
    if descriptor
        .supported_sources
        .contains(&MaterialControlSource::RandomRange)
        && semantic_material_constant_value(descriptor, session).is_some()
    {
        options.push(ComboOption {
            label: "Random range".into(),
            selected: current == Some(SemanticMaterialSourceChoice::RandomRange),
            action: PropertiesAction::SetSemanticMaterialSource {
                instance,
                parameter: descriptor.id,
                source: SemanticMaterialSourceChoice::RandomRange,
            },
        });
    }
    for parameter in session.effect.parameters.iter().filter(|parameter| {
        parameter.exposed
            && descriptor
                .value_type
                .accepts_effect_value(&parameter.default)
    }) {
        if descriptor
            .supported_sources
            .contains(&MaterialControlSource::EffectParameter)
        {
            let choice = SemanticMaterialSourceChoice::EffectParameter(parameter.id);
            options.push(ComboOption {
                label: format!("Effect · {}", parameter.name),
                selected: current == Some(choice),
                action: PropertiesAction::SetSemanticMaterialSource {
                    instance,
                    parameter: descriptor.id,
                    source: choice,
                },
            });
        }
        if descriptor
            .supported_sources
            .contains(&MaterialControlSource::EmitterParameter)
        {
            let choice = SemanticMaterialSourceChoice::EmitterParameter(parameter.id);
            options.push(ComboOption {
                label: format!("Emitter · {}", parameter.name),
                selected: current == Some(choice),
                action: PropertiesAction::SetSemanticMaterialSource {
                    instance,
                    parameter: descriptor.id,
                    source: choice,
                },
            });
        }
    }
    let icon = if current == Some(SemanticMaterialSourceChoice::RandomRange) {
        "icons/random.svg"
    } else {
        "icons/source-constant.svg"
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(28.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, &descriptor.name);
            row.spawn((
                Text::new(source_label.to_owned()),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            spawn_icon_action_menu(
                row,
                asset_server,
                icon,
                &format!("Choose source for {}", descriptor.name),
                &format!("Current source: {source_label}"),
                &options,
            );
        });
}

fn semantic_material_source_choice(
    value: Option<&MaterialParameterValue>,
) -> Option<SemanticMaterialSourceChoice> {
    match value {
        Some(MaterialParameterValue::Constant(_)) => Some(SemanticMaterialSourceChoice::Constant),
        Some(MaterialParameterValue::RandomRange { .. }) => {
            Some(SemanticMaterialSourceChoice::RandomRange)
        }
        Some(MaterialParameterValue::EffectParameter(parameter)) => {
            Some(SemanticMaterialSourceChoice::EffectParameter(*parameter))
        }
        Some(MaterialParameterValue::EmitterParameter(parameter)) => {
            Some(SemanticMaterialSourceChoice::EmitterParameter(*parameter))
        }
        None => None,
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
    material_stack_inspector: &MaterialStackInspectorState,
    asset_server: &AssetServer,
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
            match spawn_semantic_material_controls(
                card,
                renderer,
                session,
                catalog,
                material_stack_inspector,
                asset_server,
            ) {
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
    fn material_stack_move_options_describe_final_relative_positions() {
        let entries = [
            MaterialStackEntry {
                expression: aestra_core::MaterialExpressionId::from_u128(0x7100),
                kind: aestra_compiler::MaterialStackModifierKind::PanUv,
                enabled: true,
            },
            MaterialStackEntry {
                expression: aestra_core::MaterialExpressionId::from_u128(0x7101),
                kind: aestra_compiler::MaterialStackModifierKind::RotateUv,
                enabled: true,
            },
            MaterialStackEntry {
                expression: aestra_core::MaterialExpressionId::from_u128(0x7102),
                kind: aestra_compiler::MaterialStackModifierKind::ScaleUv,
                enabled: true,
            },
        ];
        let mut program = aestra_core::material::MaterialProgram::additive_sprite("Options");
        program.id = MaterialProgramId::from_u128(0x70ff);

        assert_eq!(
            material_stack_modifier_options(
                &program,
                &entries,
                1,
                &[
                    MaterialStackMoveTarget { index: 0 },
                    MaterialStackMoveTarget { index: 2 },
                ],
            )
            .into_iter()
            .map(|option| option.label)
            .collect::<Vec<_>>(),
            vec!["Edit settings", "Before UV Pan", "After UV Scale"]
        );
    }

    #[test]
    fn material_stack_insert_options_name_the_modifier_and_edge() {
        let entries = [MaterialStackEntry {
            expression: aestra_core::MaterialExpressionId::from_u128(0x7110),
            kind: aestra_compiler::MaterialStackModifierKind::PanUv,
            enabled: true,
        }];
        let options = material_stack_insert_options(
            MaterialProgramId::from_u128(0x7111),
            &entries,
            &[
                MaterialStackInsertTarget {
                    index: 0,
                    kind: aestra_compiler::MaterialStackModifierKind::RotateUv,
                },
                MaterialStackInsertTarget {
                    index: 1,
                    kind: aestra_compiler::MaterialStackModifierKind::ScaleUv,
                },
            ],
        );
        assert_eq!(
            options
                .into_iter()
                .map(|option| option.label)
                .collect::<Vec<_>>(),
            vec!["UV Rotate · First", "UV Scale · After UV Pan"]
        );
    }

    #[test]
    fn material_stack_preset_options_name_the_chain_and_edge() {
        let entries = [MaterialStackEntry {
            expression: aestra_core::MaterialExpressionId::from_u128(0x7120),
            kind: aestra_compiler::MaterialStackModifierKind::BaseTexture,
            enabled: true,
        }];
        let options = material_stack_preset_options(
            MaterialProgramId::from_u128(0x7121),
            &entries,
            &[
                MaterialStackPresetTarget {
                    index: 0,
                    preset: MaterialStackPresetKind::UvDrift,
                },
                MaterialStackPresetTarget {
                    index: 1,
                    preset: MaterialStackPresetKind::SoftDissolve,
                },
            ],
        );
        assert_eq!(
            options
                .into_iter()
                .map(|option| option.label)
                .collect::<Vec<_>>(),
            vec!["UV Drift · First", "Soft Dissolve · After Base Texture"]
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

    #[test]
    fn semantic_material_sources_filter_bindings_by_scope_exposure_and_type() {
        let mut session = test_support::session_with_timing_slack();
        let exposed_scalar = ParameterId::from_u128(0x7300);
        let hidden_scalar = ParameterId::from_u128(0x7301);
        let exposed_vector = ParameterId::from_u128(0x7302);
        session.effect.parameters.extend([
            EffectParameter {
                id: exposed_scalar,
                name: "Intensity".into(),
                default: Value::Scalar(1.0),
                exposed: true,
            },
            EffectParameter {
                id: hidden_scalar,
                name: "Internal intensity".into(),
                default: Value::Scalar(2.0),
                exposed: false,
            },
            EffectParameter {
                id: exposed_vector,
                name: "Direction".into(),
                default: Value::Vec3([0.0, 1.0, 0.0]),
                exposed: true,
            },
        ]);
        let descriptor = MaterialControlDescriptor {
            id: MaterialParameterId::from_u128(0x7303),
            name: "Intensity".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: aestra_core::material::MaterialEvaluationDomain::Effect,
            control: MaterialControlKind::Number,
            default_value: Some(MaterialValue::Float(1.0)),
            current_value: Some(MaterialParameterValue::Constant(MaterialValue::Float(1.0))),
            value_origin: aestra_compiler::MaterialControlValueOrigin::ProgramDefault,
            supported_sources: vec![
                MaterialControlSource::Constant,
                MaterialControlSource::EffectParameter,
                MaterialControlSource::RandomRange,
            ],
            resource_constraint: None,
        };

        assert!(semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::Constant,
            &session,
        ));
        assert!(semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::RandomRange,
            &session,
        ));
        assert!(semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::EffectParameter(exposed_scalar),
            &session,
        ));
        assert!(!semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::EffectParameter(hidden_scalar),
            &session,
        ));
        assert!(!semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::EffectParameter(exposed_vector),
            &session,
        ));
        assert!(!semantic_material_source_is_supported(
            &descriptor,
            SemanticMaterialSourceChoice::EmitterParameter(exposed_scalar),
            &session,
        ));
    }

    #[test]
    fn switching_a_random_material_source_to_constant_uses_its_midpoint() {
        let session = test_support::session_with_timing_slack();
        let descriptor = MaterialControlDescriptor {
            id: MaterialParameterId::from_u128(0x7400),
            name: "Tint".into(),
            value_type: MaterialValueType::Color,
            evaluation_domain: aestra_core::material::MaterialEvaluationDomain::Instance,
            control: MaterialControlKind::Color,
            default_value: Some(MaterialValue::ColorSrgb([1.0; 4])),
            current_value: Some(MaterialParameterValue::RandomRange {
                min: MaterialValue::ColorSrgb([0.0, 0.2, 0.4, 0.6]),
                max: MaterialValue::ColorSrgb([1.0, 0.8, 0.6, 1.0]),
                domain: aestra_core::material::MaterialEvaluationDomain::Instance,
            }),
            value_origin: aestra_compiler::MaterialControlValueOrigin::InstanceOverride,
            supported_sources: vec![
                MaterialControlSource::Constant,
                MaterialControlSource::RandomRange,
            ],
            resource_constraint: None,
        };

        assert_eq!(
            semantic_material_constant_value(&descriptor, &session),
            Some(MaterialValue::ColorSrgb([0.5, 0.5, 0.5, 0.8]))
        );
    }

    #[test]
    fn reflected_render_state_options_never_create_an_invalid_intermediate_state() {
        let current = MaterialRenderState::additive_sprite();
        let alpha = MaterialRenderState {
            blend: BlendMode::Alpha,
            ..current
        };
        let depth_disabled = MaterialRenderState {
            depth_test: MaterialDepthTest::Disabled,
            ..current
        };
        let multi_field_variant = MaterialRenderState {
            blend: BlendMode::Alpha,
            cull_mode: MaterialCullMode::Back,
            ..current
        };
        let policy = MaterialRenderStatePolicy {
            default: current,
            allowed: vec![current, alpha, depth_disabled, multi_field_variant],
        };

        assert_eq!(
            semantic_render_state_candidates(&policy, current, SemanticRenderStateField::Blend),
            vec![current, alpha]
        );
        assert_eq!(
            semantic_render_state_candidates(&policy, current, SemanticRenderStateField::DepthTest,),
            vec![current, depth_disabled]
        );
        assert_eq!(
            semantic_render_state_candidates(&policy, current, SemanticRenderStateField::CullMode),
            vec![current]
        );
    }

    #[test]
    fn semantic_render_state_edits_join_shared_effect_history() {
        let mut session = test_support::session_with_timing_slack();
        let instance_id = MaterialId::from_u128(0x7500);
        let program_id = aestra_core::MaterialProgramId::from_u128(0x7501);
        let current = MaterialRenderState::additive_sprite();
        let alpha = MaterialRenderState {
            blend: BlendMode::Alpha,
            ..current
        };
        let mut program = aestra_core::material::MaterialProgram::additive_sprite("Flexible");
        program.id = program_id;
        program.render_state_policy.allowed.push(alpha);
        session
            .effect
            .material_instances
            .push(aestra_core::material::MaterialInstance {
                id: instance_id,
                program: aestra_core::material::MaterialProgramRef::Project(program_id),
                values: std::collections::BTreeMap::new(),
                render_state: current,
            });

        assert!(session.set_material_instance_render_state(&[program], instance_id, alpha));
        assert_eq!(session.effect.material_instances[0].render_state, alpha);
        assert!(session.can_undo());

        session.undo();
        assert_eq!(session.effect.material_instances[0].render_state, current);
    }
}
