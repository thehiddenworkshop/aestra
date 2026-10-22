//! Function signature controls and root/identity-scoped draft history.
#[cfg(test)]
use crate::{EditorSession, ProjectEffectCatalog};
#[cfg(test)]
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId,
    material::{MaterialExpression, MaterialExpressionKind},
};
#[cfg(test)]
use bevy::prelude::*;
#[cfg(test)]
use bevy::ui_widgets::Activate;
#[cfg(test)]
use bevy::ui_widgets::ValueChange;

pub(crate) mod graph;
pub(crate) use graph::spawn as spawn_graph;

mod history;
pub(crate) use history::FunctionEditor;

mod execution;
#[cfg(test)]
use execution::key;

mod signature_input;
#[cfg(test)]
use signature_input::{mutate, parse_default};
mod signature_model;
#[cfg(test)]
use aestra_core::{
    MaterialFunctionOutputId,
    material::{MaterialFunction, MaterialFunctionOutput, MaterialValue, MaterialValueType},
};
#[cfg(test)]
use signature_model::*;
mod signature;
pub(crate) use signature::spawn;

mod registration;
pub(crate) use registration::register;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> MaterialFunction {
        let mut function = MaterialFunction::from_ron(include_str!(
            "../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
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
    fn body_edits_output_mapping_and_history_are_atomic() {
        use aestra_authoring::MaterialFunctionBodyCommand as Edit;
        let root = tempfile::tempdir().unwrap();
        let function = fixture();
        let path = root.path().join("function.aestra.material-function.ron");
        function.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut editor = FunctionEditor::default();
        let expression = MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.6)),
        };
        editor
            .edit_body(
                &mut session,
                &mut catalog,
                vec![
                    Edit::Add {
                        expression: expression.clone(),
                        index: function.expressions.len(),
                    },
                    Edit::SetOutput {
                        output: function.outputs[0].id,
                        source: expression.id,
                    },
                ],
            )
            .unwrap();
        let edited = session.graph_function(&catalog).unwrap();
        assert_eq!(edited.outputs[0].expression, expression.id);
        assert_eq!(edited.outputs[0].id, function.outputs[0].id);
        assert!(
            editor
                .edit_body(
                    &mut session,
                    &mut catalog,
                    vec![Edit::Remove {
                        expression: expression.id
                    }]
                )
                .is_err()
        );
        assert_eq!(session.graph_function(&catalog).unwrap(), edited);
        editor.step(&mut session, &mut catalog, true).unwrap();
        assert_eq!(session.graph_function(&catalog).unwrap(), function);
        editor.step(&mut session, &mut catalog, false).unwrap();
        assert_eq!(session.graph_function(&catalog).unwrap(), edited);
        editor
            .edit_body(
                &mut session,
                &mut catalog,
                vec![
                    Edit::SetOutput {
                        output: function.outputs[0].id,
                        source: function.outputs[0].expression,
                    },
                    Edit::Remove {
                        expression: expression.id,
                    },
                ],
            )
            .unwrap();
        assert_eq!(session.graph_function(&catalog).unwrap(), function);
        assert_eq!(session.effect, effect);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn function_body_rewire_rejects_cycles_without_losing_the_previous_graph() {
        use aestra_authoring::{MaterialExpressionInput, MaterialFunctionBodyCommand as Edit};
        let root = tempfile::tempdir().unwrap();
        let mut function = fixture();
        let operation = MaterialExpressionId::new();
        let input = function.expressions[0].id;
        function.expressions.push(MaterialExpression {
            id: operation,
            kind: MaterialExpressionKind::Add(input, input),
        });
        function.outputs[0].expression = operation;
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let before = session.graph_function(&catalog).unwrap();
        let mut editor = FunctionEditor::default();
        assert!(
            editor
                .edit_body(
                    &mut session,
                    &mut catalog,
                    vec![Edit::Rewire {
                        expression: operation,
                        input: MaterialExpressionInput::Left,
                        source: operation
                    }]
                )
                .is_err()
        );
        assert_eq!(session.graph_function(&catalog).unwrap(), before);
        assert!(!editor.available(&session, true));
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

        // Signature-created nodes use the same semantic placement boundary even with
        // no mounted canvas. This is a real activation, not a direct helper call.
        app.init_resource::<crate::feathers::node_graph::GraphViewportMemory>();
        let action = app
            .world_mut()
            .spawn(Action {
                owner: function.id,
                kind: ActionKind::AddOutput,
            })
            .id();
        app.world_mut().trigger(Activate { entity: action });
        let updated = app
            .world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        let created = updated
            .expressions
            .iter()
            .find(|node| !current.expressions.iter().any(|old| old.id == node.id))
            .unwrap();
        let graph = crate::material_graph::function_graph_memory_key(root.path(), function.id);
        assert!(
            app.world()
                .resource::<crate::feathers::node_graph::GraphViewportMemory>()
                .node(&graph, &created.id.to_string())
                .is_some()
        );
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
            "../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
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
