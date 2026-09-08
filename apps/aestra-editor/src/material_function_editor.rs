//! Function signature controls and root/identity-scoped draft history.
use crate::feathers::{
    button::spawn_action_button,
    combo_box::{ComboOption, spawn_combo_control},
    text_input::spawn_text_input,
};
use crate::{EditorSession, ProjectEffectCatalog, material_document::MaterialEditingTarget};
use aestra_authoring::MaterialAuthoringDocument;
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialValue, MaterialValueType,
    },
};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, ValueChange},
};
use std::{collections::BTreeMap, path::PathBuf};

type Key = (PathBuf, MaterialFunctionId);
#[derive(Default)]
struct History {
    undo: Vec<(MaterialFunction, MaterialFunction)>,
    redo: Vec<(MaterialFunction, MaterialFunction)>,
}
#[derive(Resource, Default)]
pub(crate) struct FunctionEditor {
    histories: BTreeMap<Key, History>,
    reports: BTreeMap<Key, String>,
}

fn key(session: &EditorSession) -> Result<Key, String> {
    match &session.material_target {
        MaterialEditingTarget::Function { root, id } => Ok((root.clone(), *id)),
        _ => Err("No function selected".into()),
    }
}

impl FunctionEditor {
    fn apply(
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        before: &MaterialFunction,
        after: &MaterialFunction,
    ) -> Result<String, String> {
        if session.graph_function(catalog)?.normalized() != before.normalized() {
            return Err("Function changed since this edit was prepared".into());
        }
        // Include every indexed program and draft, not only the active effect's consumers.
        let programs = catalog
            .content()
            .asset_index()
            .material_programs()
            .iter()
            .filter_map(|entry| entry.reference.map(|reference| reference.id()))
            .map(|id| catalog.material_program(id))
            .collect::<Result<Vec<_>, _>>()?;
        let document = MaterialAuthoringDocument::standalone(programs)
            .with_material_functions(catalog.material_functions()?);
        let plan = document
            .plan_function_edit(before.id, after.clone())
            .map_err(|error| error.to_string())?;
        let report = format!(
            "{} direct call sites checked (indexed programs and functions).\n{}",
            plan.call_sites.len(),
            plan.diagnostics
        );
        if !plan.diagnostics.is_valid() {
            return Err(report);
        }
        catalog.replace_material_function(before, after)?;
        // Catalog change detection invalidates dependent viewport/graph previews. The effect
        // and its history are never replaced by a function signature edit.
        session.set_material_drafts(catalog.material_drafts.clone());
        session.material_history_active = true;
        Ok(report)
    }

    pub(crate) fn edit(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        after: MaterialFunction,
    ) -> Result<(), String> {
        let key = key(session)?;
        let before = session.graph_function(catalog)?;
        if before == after {
            return Ok(());
        }
        if before.custom_wesl.is_some() {
            return Err("Custom WESL signatures remain read-only".into());
        }
        match Self::apply(session, catalog, &before, &after) {
            Ok(report) => {
                session.status = format!("Function signature updated (unsaved draft).\n{report}");
                self.reports.insert(key.clone(), report);
                let history = self.histories.entry(key).or_default();
                history.undo.push((before, after));
                if history.undo.len() > 256 {
                    history.undo.remove(0);
                }
                history.redo.clear();
                Ok(())
            }
            Err(error) => {
                self.reports.insert(key, error.clone());
                Err(error)
            }
        }
    }

    pub(crate) fn available(&self, session: &EditorSession, undo: bool) -> bool {
        key(session)
            .ok()
            .and_then(|key| self.histories.get(&key))
            .is_some_and(|history| {
                if undo {
                    !history.undo.is_empty()
                } else {
                    !history.redo.is_empty()
                }
            })
    }

    pub(crate) fn step(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        undo: bool,
    ) -> Result<(), String> {
        let key = key(session)?;
        let entry = self
            .histories
            .get(&key)
            .and_then(|history| {
                if undo {
                    history.undo.last()
                } else {
                    history.redo.last()
                }
            })
            .cloned()
            .ok_or("No function history in this direction")?;
        let (before, after) = &entry;
        let report = if undo {
            Self::apply(session, catalog, after, before)
        } else {
            Self::apply(session, catalog, before, after)
        }?;
        self.reports.insert(key.clone(), report);
        let history = self.histories.get_mut(&key).unwrap();
        if undo {
            history.undo.pop();
            history.redo.push(entry);
        } else {
            history.redo.pop();
            history.undo.push(entry);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Port {
    Input(MaterialFunctionInputId),
    Output(MaterialFunctionOutputId),
}
#[derive(Component, Clone, Copy)]
struct Field {
    owner: MaterialFunctionId,
    kind: FieldKind,
}
#[derive(Clone, Copy)]
enum FieldKind {
    Name,
    PortName(Port),
    Default(MaterialFunctionInputId),
}
#[derive(Component, Clone, Copy)]
struct Action {
    owner: MaterialFunctionId,
    kind: ActionKind,
}
#[derive(Clone, Copy)]
enum ActionKind {
    AddInput,
    AddOutput,
    Remove(Port),
    Type(Port, MaterialValueType),
}

pub(crate) fn register(app: &mut App) {
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
        editor.edit(&mut session, &mut catalog, function)
    })();
    finish(&mut session, result);
}

fn finish(session: &mut EditorSession, result: Result<(), String>) {
    session.status = match result {
        Ok(()) => session.status.clone(),
        Err(error) => format!("Function edit rejected: {error}"),
    };
    session.ui_revision += 1;
}

fn mutate(function: &mut MaterialFunction, action: ActionKind) -> Result<(), String> {
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

fn parse_default(text: &str, ty: MaterialValueType) -> Result<Option<MaterialValue>, String> {
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

fn default_text(value: &Option<MaterialValue>) -> String {
    match value {
        None => String::new(),
        Some(MaterialValue::Float(v)) => v.to_string(),
        Some(MaterialValue::Bool(v)) => v.to_string(),
        Some(MaterialValue::Vec2(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Vec3(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Vec4(v) | MaterialValue::ColorSrgb(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Texture2D(id)) => id.to_string(),
    }
}

fn label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: 13.0.into(),
            ..default()
        },
        TextColor(crate::theme::TEXT_MUTED),
    ));
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands, function: &MaterialFunction) {
    if function.custom_wesl.is_some() {
        return;
    }
    label(
        parent,
        "Function signature · unsaved drafts · body editing follows in the next slice",
    );
    spawn_text_input(
        parent,
        &function.name,
        "Function name",
        Field {
            owner: function.id,
            kind: FieldKind::Name,
        },
    );
    label(
        parent,
        "Inputs · empty default means required · vectors use comma-separated components",
    );
    for port in &function.inputs {
        port_controls(
            parent,
            function.id,
            Port::Input(port.id),
            &port.name,
            port.value_type,
        );
        if !matches!(port.value_type, MaterialValueType::Texture2D(_)) {
            let entity = spawn_text_input(
                parent,
                &default_text(&port.default),
                "Input default (empty = required)",
                Field {
                    owner: function.id,
                    kind: FieldKind::Default(port.id),
                },
            );
            if let Some(MaterialValue::Float(value)) = port.default {
                parent.commands().entity(entity).insert(
                    crate::feathers::number_input::ScrubbableNumber::new(
                        value,
                        -f32::MAX,
                        f32::MAX,
                        0.01,
                    ),
                );
            }
        }
    }
    spawn_action_button(
        parent,
        "+ Input",
        Action {
            owner: function.id,
            kind: ActionKind::AddInput,
        },
        false,
    );
    label(
        parent,
        "Outputs · new outputs initially return zero; body mapping remains read-only",
    );
    for port in &function.outputs {
        port_controls(
            parent,
            function.id,
            Port::Output(port.id),
            &port.name,
            port.value_type,
        );
    }
    spawn_action_button(
        parent,
        "+ Output",
        Action {
            owner: function.id,
            kind: ActionKind::AddOutput,
        },
        false,
    );
}

fn port_controls(
    parent: &mut ChildSpawnerCommands,
    owner: MaterialFunctionId,
    port: Port,
    name: &str,
    ty: MaterialValueType,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            spawn_text_input(
                row,
                name,
                "Port name",
                Field {
                    owner,
                    kind: FieldKind::PortName(port),
                },
            );
            let options = [
                MaterialValueType::Float,
                MaterialValueType::Vec2,
                MaterialValueType::Vec3,
                MaterialValueType::Vec4,
                MaterialValueType::Color,
                MaterialValueType::Bool,
            ]
            .map(|value| ComboOption {
                label: format!("{value:?}"),
                selected: value == ty,
                action: Action {
                    owner,
                    kind: ActionKind::Type(port, value),
                },
            });
            spawn_combo_control(row, &format!("{ty:?}"), "Port type", &options, 128.0);
            spawn_action_button(
                row,
                "Remove",
                Action {
                    owner,
                    kind: ActionKind::Remove(port),
                },
                false,
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MaterialFunction {
        let mut function = MaterialFunction::from_ron(include_str!(
            "../../../assets/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        function.custom_wesl = None;
        function.name = "Signature test".into();
        function.expressions.push(MaterialExpression {
            id: function.outputs[0].expression,
            kind: MaterialExpressionKind::FunctionInput(function.inputs[0].id),
        });
        function.normalized()
    }

    #[test]
    fn signature_history_is_scoped_and_preserves_effect_and_disk() {
        let root = tempfile::tempdir().unwrap();
        let first = fixture();
        let mut second = first.clone();
        second.id = MaterialFunctionId::new();
        let path = root.path().join("first.aestra.material-function.ron");
        first.save_ron(&path).unwrap();
        second
            .save_ron(root.path().join("second.aestra.material-function.ron"))
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        let selection = session.selection;
        let mut editor = FunctionEditor::default();
        session.open_material_function(&catalog, first.id).unwrap();
        let mut edited = first.clone();
        edited.inputs[0].name = "Phase renamed".into();
        edited.inputs[0].default = Some(MaterialValue::Float(0.25));
        editor
            .edit(&mut session, &mut catalog, edited.clone())
            .unwrap();
        assert_eq!(
            session.graph_function(&catalog).unwrap(),
            edited.normalized()
        );
        assert_eq!(
            session.graph_function(&catalog).unwrap().inputs[0].id,
            first.inputs[0].id
        );
        session.open_material_function(&catalog, second.id).unwrap();
        assert!(!editor.available(&session, true));
        let mut changed_second = second.clone();
        changed_second.name = "Second renamed".into();
        editor
            .edit(&mut session, &mut catalog, changed_second)
            .unwrap();
        session.open_material_function(&catalog, first.id).unwrap();
        editor.step(&mut session, &mut catalog, true).unwrap();
        assert_eq!(session.graph_function(&catalog).unwrap(), first);
        assert!(!catalog.material_drafts.functions.contains_key(&first.id));
        assert!(catalog.material_drafts.functions.contains_key(&second.id));
        editor.step(&mut session, &mut catalog, false).unwrap();
        assert_eq!(
            session.graph_function(&catalog).unwrap(),
            edited.normalized()
        );
        assert_eq!(session.effect, effect);
        assert_eq!(session.selection, selection);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn invalid_removal_and_typed_defaults_leave_drafts_and_history_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let function = fixture();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut editor = FunctionEditor::default();
        for action in [
            ActionKind::Remove(Port::Input(function.inputs[0].id)),
            ActionKind::Type(Port::Input(function.inputs[0].id), MaterialValueType::Vec3),
        ] {
            let mut after = function.clone();
            mutate(&mut after, action).unwrap();
            if let ActionKind::Type(_, _) = action {
                after.inputs[0].default = Some(MaterialValue::Float(0.0));
            }
            assert!(editor.edit(&mut session, &mut catalog, after).is_err());
            assert_eq!(session.graph_function(&catalog).unwrap(), function);
            assert!(catalog.material_drafts.is_empty());
            assert!(!editor.available(&session, true));
        }
    }

    #[test]
    fn caller_preflight_and_default_edits_recompile_dependents() {
        use aestra_core::material::{MaterialFunctionRef, MaterialProgram};
        let root = tempfile::tempdir().unwrap();
        let mut function = fixture();
        for port in &mut function.inputs {
            port.default = Some(MaterialValue::Float(0.25));
        }
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut program = MaterialProgram::additive_sprite("Caller");
        let alpha = program
            .expressions
            .iter_mut()
            .find(|expression| expression.id == program.outputs.alpha)
            .unwrap();
        alpha.kind = MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(function.id),
            output: function.outputs[0].id,
            arguments: BTreeMap::new(),
        };
        program
            .save_ron(root.path().join("caller.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut editor = FunctionEditor::default();
        let first = aestra_compiler::MaterialCompiler
            .compile_with_functions(&program, &catalog.material_function_library().unwrap())
            .unwrap();
        let mut candidate = function.clone();
        candidate.inputs[0].default = Some(MaterialValue::Float(0.75));
        editor
            .edit(&mut session, &mut catalog, candidate.clone())
            .unwrap();
        assert!(editor.reports[&key(&session).unwrap()].contains("1 direct call sites"));
        let second = aestra_compiler::MaterialCompiler
            .compile_with_functions(&program, &catalog.material_function_library().unwrap())
            .unwrap();
        assert_ne!(format!("{first:?}"), format!("{second:?}"));
        candidate.inputs[0].default = None;
        assert!(editor.edit(&mut session, &mut catalog, candidate).is_err());
        assert_eq!(
            session.graph_function(&catalog).unwrap().inputs[0].default,
            Some(MaterialValue::Float(0.75))
        );
    }

    #[test]
    fn feather_activation_and_final_text_events_edit_the_target() {
        let root = tempfile::tempdir().unwrap();
        let function = fixture();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut app = App::new();
        app.insert_resource(session).insert_resource(catalog);
        register(&mut app);
        let field = app
            .world_mut()
            .spawn(Field {
                owner: function.id,
                kind: FieldKind::Name,
            })
            .id();
        app.world_mut().trigger(ValueChange {
            source: field,
            value: "Typing".to_owned(),
            is_final: false,
        });
        assert!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_drafts
                .is_empty()
        );
        app.world_mut().trigger(ValueChange {
            source: field,
            value: "Renamed".to_owned(),
            is_final: true,
        });
        let action = app
            .world_mut()
            .spawn(Action {
                owner: function.id,
                kind: ActionKind::AddInput,
            })
            .id();
        app.world_mut().trigger(Activate { entity: action });
        let current = app
            .world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        assert_eq!(current.name, "Renamed");
        assert_eq!(current.inputs.len(), function.inputs.len() + 1);
    }

    #[test]
    fn port_creation_and_defaults_are_typed_and_stable() {
        let mut function = fixture();
        let before = function.clone();
        mutate(&mut function, ActionKind::AddInput).unwrap();
        mutate(&mut function, ActionKind::AddOutput).unwrap();
        assert_eq!(function.inputs[..before.inputs.len()], before.inputs);
        assert_eq!(function.outputs[..before.outputs.len()], before.outputs);
        let id = function.inputs.last().unwrap().id;
        mutate(&mut function, ActionKind::Remove(Port::Input(id))).unwrap();
        assert_eq!(function.inputs, before.inputs);
        assert_eq!(parse_default("", MaterialValueType::Float).unwrap(), None);
        assert_eq!(
            parse_default("1, 2, 3", MaterialValueType::Vec3).unwrap(),
            Some(MaterialValue::Vec3([1.0, 2.0, 3.0]))
        );
        assert!(parse_default("NaN", MaterialValueType::Float).is_err());
        assert!(parse_default("1, 2", MaterialValueType::Float).is_err());
        assert_eq!(
            parse_default("true", MaterialValueType::Bool).unwrap(),
            Some(MaterialValue::Bool(true))
        );
    }

    #[test]
    fn stale_history_and_custom_wesl_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let function = fixture();
        let path = root.path().join("function.aestra.material-function.ron");
        function.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut editor = FunctionEditor::default();
        let mut changed = function.clone();
        changed.name = "Edited".into();
        editor
            .edit(&mut session, &mut catalog, changed.clone())
            .unwrap();
        let mut other = changed.clone();
        other.name = "Concurrent edit".into();
        catalog.replace_material_function(&changed, &other).unwrap();
        assert!(editor.step(&mut session, &mut catalog, true).is_err());
        assert!(editor.available(&session, true));
        let custom = MaterialFunction::from_ron(include_str!(
            "../../../assets/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        custom.save_ron(&path).unwrap();
        catalog = ProjectEffectCatalog::scan(root.path());
        session.open_material_function(&catalog, custom.id).unwrap();
        let mut renamed = custom.clone();
        renamed.name = "Attempt".into();
        assert!(
            editor
                .edit(&mut session, &mut catalog, renamed)
                .unwrap_err()
                .contains("read-only")
        );
        assert_eq!(session.graph_function(&catalog).unwrap(), custom);
    }
}
