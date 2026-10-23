//! Production function signature observers and semantic validation; no alternate callbacks.
#[cfg(test)]
#[path = "tests/signature_input.rs"]
mod tests;
use crate::material_function_editor::{FunctionEditor, signature_model::*};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialValue, MaterialValueType,
    },
};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, ValueChange},
};

pub(super) fn register(app: &mut App) {
    app.init_resource::<FunctionEditor>()
        .add_observer(text_changed)
        .add_observer(number_changed)
        .add_observer(activate);
}

fn number_changed(
    change: On<ValueChange<f32>>,
    controls: Query<&Field>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(field) = controls.get(change.source) else {
        return;
    };
    if session.standalone_function() != Some(field.owner) {
        return;
    }
    let FieldKind::Default(id) = field.kind else {
        return;
    };
    let result = (|| {
        let mut function = session.graph_function(&catalog)?;
        let port = function
            .inputs
            .iter_mut()
            .find(|port| port.id == id)
            .ok_or("Input no longer exists")?;
        if port.value_type != MaterialValueType::Float || !change.value.is_finite() {
            return Err("Invalid scalar default".into());
        }
        port.default = Some(MaterialValue::Float(change.value));
        editor.edit(&mut session, &mut catalog, function)
    })();
    finish(&mut session, result);
}

fn text_changed(
    change: On<ValueChange<String>>,
    controls: Query<&Field>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(field) = controls.get(change.source) else {
        return;
    };
    if session.standalone_function() != Some(field.owner) {
        return;
    }
    let result = (|| {
        let mut function = session.graph_function(&catalog)?;
        let value = change.value.trim();
        if !matches!(field.kind, FieldKind::Default(_)) && value.is_empty() {
            return Err("A name is required".into());
        }
        match field.kind {
            FieldKind::Name => function.name = value.into(),
            FieldKind::PortName(Port::Input(id)) => {
                function
                    .inputs
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Input no longer exists")?
                    .name = value.into()
            }
            FieldKind::PortName(Port::Output(id)) => {
                function
                    .outputs
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Output no longer exists")?
                    .name = value.into()
            }
            FieldKind::Default(id) => {
                let port = function
                    .inputs
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Input no longer exists")?;
                port.default = parse_default(value, port.value_type)?;
            }
        }
        editor.edit(&mut session, &mut catalog, function)
    })();
    finish(&mut session, result);
}

fn activate(
    event: On<Activate>,
    controls: Query<&Action>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut placement: crate::material_graph::semantic_placement::Context,
) {
    let Ok(action) = controls.get(event.entity) else {
        return;
    };
    if session.standalone_function() != Some(action.owner) {
        return;
    }
    let result = (|| {
        let mut function = session.graph_function(&catalog)?;
        mutate(&mut function, action.kind)?;
        placement.function(&mut session, &mut catalog, &mut editor, function)
    })();
    finish(&mut session, result);
}

pub(super) fn finish(session: &mut EditorSession, result: Result<(), String>) {
    session.status = match result {
        Ok(()) => session.status.clone(),
        Err(error) => format!("Function edit rejected: {error}"),
    };
    session.ui_revision += 1;
}

pub(super) fn mutate(function: &mut MaterialFunction, action: ActionKind) -> Result<(), String> {
    match action {
        ActionKind::AddInput => {
            let name = unique_name("Input", function.inputs.iter().map(|p| p.name.as_str()));
            function.inputs.push(MaterialFunctionInput {
                id: MaterialFunctionInputId::new(),
                name,
                value_type: MaterialValueType::Float,
                default: Some(MaterialValue::Float(0.0)),
            });
        }
        ActionKind::AddOutput => {
            let expression = MaterialExpressionId::new();
            let name = unique_name("Output", function.outputs.iter().map(|p| p.name.as_str()));
            function.expressions.push(MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
            });
            function.outputs.push(MaterialFunctionOutput {
                id: MaterialFunctionOutputId::new(),
                name,
                expression,
                value_type: MaterialValueType::Float,
            });
        }
        ActionKind::Remove(Port::Input(id)) => function.inputs.retain(|p| p.id != id),
        ActionKind::Remove(Port::Output(id)) => function.outputs.retain(|p| p.id != id),
        ActionKind::Type(Port::Input(id), ty) => {
            let port = function
                .inputs
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Input no longer exists")?;
            if port.default.as_ref().is_some_and(|v| !ty.accepts(v)) {
                return Err("Clear the default before changing to an incompatible type".into());
            }
            port.value_type = ty;
        }
        ActionKind::Type(Port::Output(id), ty) => {
            function
                .outputs
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Output no longer exists")?
                .value_type = ty
        }
    }
    Ok(())
}

fn unique_name<'a>(prefix: &str, names: impl Iterator<Item = &'a str>) -> String {
    let names = names.collect::<Vec<_>>();
    (1..)
        .map(|n| format!("{prefix} {n}"))
        .find(|name| !names.contains(&name.as_str()))
        .unwrap()
}

pub(super) fn parse_default(
    text: &str,
    ty: MaterialValueType,
) -> Result<Option<MaterialValue>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    if ty == MaterialValueType::Bool {
        return text
            .parse::<bool>()
            .map(|v| Some(MaterialValue::Bool(v)))
            .map_err(|_| "Use true or false".into());
    }
    let values = text
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Enter comma-separated numeric components".to_owned())?;
    let value = match (ty, values.as_slice()) {
        (MaterialValueType::Float, [v]) => MaterialValue::Float(*v),
        (MaterialValueType::Vec2, [a, b]) => MaterialValue::Vec2([*a, *b]),
        (MaterialValueType::Vec3, [a, b, c]) => MaterialValue::Vec3([*a, *b, *c]),
        (MaterialValueType::Vec4, [a, b, c, d]) => MaterialValue::Vec4([*a, *b, *c, *d]),
        (MaterialValueType::Color, [a, b, c, d]) => MaterialValue::ColorSrgb([*a, *b, *c, *d]),
        _ => return Err("Default components must match the input type".into()),
    };
    if !value.is_valid() {
        return Err("Default must be finite".into());
    }
    Ok(Some(value))
}
