//! Undo/redo actions, shortcuts, menu activation, and availability synchronization.

use crate::{
    EditorNativeControl, FeathersActionButton, ModulePaletteState, PendingFeathersActivation,
    ProjectEffectCatalog,
    menus::{MenuState, RedoMenuItem, UndoMenuItem},
    session::EditorSession,
    theme,
};
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandExecutor, MaterialTransaction,
};
use aestra_core::{
    EffectId,
    material::{MaterialFunction, MaterialProgram},
};
use bevy::{prelude::*, ui::InteractionDisabled, ui_widgets::Activate};
use std::collections::VecDeque;

const MATERIAL_HISTORY_LIMIT: usize = 256;

pub(crate) struct EditorHistoryPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HistorySet {
    Input,
    Actions,
    Sync,
}

impl Plugin for EditorHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .add_observer(queue_history_action_activation)
            .add_observer(execute_history_action)
            .add_systems(
                Update,
                (
                    history_keyboard_input.in_set(HistorySet::Input),
                    (handle_history_buttons, audit_history_controls)
                        .chain()
                        .in_set(HistorySet::Actions),
                    (sync_effect_history_ledger, update_history_availability)
                        .chain()
                        .in_set(HistorySet::Sync),
                ),
            );
    }
}

#[derive(Debug, Clone)]
struct MaterialProgramHistoryEntry {
    label: String,
    before: MaterialProgram,
    after: MaterialProgram,
    created_function: Option<MaterialFunction>,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialProgramEditHistory {
    undo: VecDeque<MaterialProgramHistoryEntry>,
    redo: Vec<MaterialProgramHistoryEntry>,
}

impl MaterialProgramEditHistory {
    pub(crate) fn execute_replacement(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        label: impl Into<String>,
        before: MaterialProgram,
        after: MaterialProgram,
    ) -> Result<(), String> {
        let label = label.into();
        apply_material_program_replacement(session, catalog, &label, &before, &after)?;
        self.undo.push_back(MaterialProgramHistoryEntry {
            label,
            before,
            after,
            created_function: None,
        });
        while self.undo.len() > MATERIAL_HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        Ok(())
    }

    pub(crate) fn execute_extraction(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        label: impl Into<String>,
        before: MaterialProgram,
        after: MaterialProgram,
        function: MaterialFunction,
    ) -> Result<(), String> {
        let label = label.into();
        apply_material_function_extraction(session, catalog, &label, &before, &after, &function)?;
        self.undo.push_back(MaterialProgramHistoryEntry {
            label,
            before,
            after,
            created_function: Some(function),
        });
        while self.undo.len() > MATERIAL_HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        Ok(())
    }

    fn undo(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
    ) -> Result<Option<String>, String> {
        let Some(entry) = self.undo.pop_back() else {
            return Ok(None);
        };
        let result = if let Some(function) = &entry.created_function {
            undo_material_function_extraction(session, catalog, &entry, function)
        } else {
            apply_material_program_replacement(
                session,
                catalog,
                &format!("Undo {}", entry.label),
                &entry.after,
                &entry.before,
            )
        };
        match result {
            Ok(()) => {
                let label = entry.label.clone();
                self.redo.push(entry);
                Ok(Some(label))
            }
            Err(error) => {
                self.undo.push_back(entry);
                Err(error)
            }
        }
    }

    fn redo(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
    ) -> Result<Option<String>, String> {
        let Some(entry) = self.redo.pop() else {
            return Ok(None);
        };
        let result = if let Some(function) = &entry.created_function {
            apply_material_function_extraction(
                session,
                catalog,
                &format!("Redo {}", entry.label),
                &entry.before,
                &entry.after,
                function,
            )
        } else {
            apply_material_program_replacement(
                session,
                catalog,
                &format!("Redo {}", entry.label),
                &entry.before,
                &entry.after,
            )
        };
        match result {
            Ok(()) => {
                let label = entry.label.clone();
                self.undo.push_back(entry);
                Ok(Some(label))
            }
            Err(error) => {
                self.redo.push(entry);
                Err(error)
            }
        }
    }

    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn clear_redo(&mut self) {
        self.redo.clear();
    }
}

fn apply_material_function_extraction(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    label: &str,
    before: &MaterialProgram,
    after: &MaterialProgram,
    function: &MaterialFunction,
) -> Result<(), String> {
    catalog.create_material_function(function)?;
    if let Err(error) = apply_material_program_replacement(session, catalog, label, before, after) {
        return Err(match catalog.delete_material_function(function) {
            Ok(()) => error,
            Err(rollback) => {
                format!("{error}; removing the extracted function also failed: {rollback}")
            }
        });
    }
    Ok(())
}

fn undo_material_function_extraction(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    entry: &MaterialProgramHistoryEntry,
    function: &MaterialFunction,
) -> Result<(), String> {
    apply_material_program_replacement(
        session,
        catalog,
        &format!("Undo {}", entry.label),
        &entry.after,
        &entry.before,
    )?;
    if let Err(error) = catalog.delete_material_function(function) {
        return Err(
            match apply_material_program_replacement(
                session,
                catalog,
                &format!("Restore {} after failed undo", entry.label),
                &entry.before,
                &entry.after,
            ) {
                Ok(()) => error,
                Err(rollback) => format!(
                    "{error}; restoring the material after the failed function removal also failed: \
                 {rollback}"
                ),
            },
        );
    }
    Ok(())
}

fn apply_material_program_replacement(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    label: &str,
    expected: &MaterialProgram,
    replacement: &MaterialProgram,
) -> Result<(), String> {
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let functions = catalog.material_functions()?;
    let mut document = MaterialAuthoringDocument::new(session.effect.clone(), programs)
        .with_material_functions(functions);
    MaterialCommandExecutor::execute(
        &mut document,
        &MaterialTransaction::single(
            label,
            MaterialCommand::ReplaceMaterialProgram {
                id: expected.id,
                program: replacement.clone(),
            },
        ),
    )
    .map_err(|error| error.to_string())?;
    let replacement = document
        .programs
        .into_iter()
        .find(|program| program.id == expected.id)
        .ok_or_else(|| format!("material program {} disappeared", expected.id))?;
    catalog.replace_material_program(expected, &replacement)?;
    let compiled = match catalog.compile_project(&session.effect) {
        Ok(project) => project.root,
        Err(error) => {
            return Err(rollback_material_program_replacement(
                catalog,
                &replacement,
                expected,
                error,
            ));
        }
    };
    if let Err(error) = session.install_compiled_project_root(compiled) {
        return Err(rollback_material_program_replacement(
            catalog,
            &replacement,
            expected,
            error.to_string(),
        ));
    }
    Ok(())
}

fn rollback_material_program_replacement(
    catalog: &mut ProjectEffectCatalog,
    current: &MaterialProgram,
    previous: &MaterialProgram,
    error: String,
) -> String {
    match catalog.replace_material_program(current, previous) {
        Ok(()) => error,
        Err(rollback) => {
            format!("{error}; restoring the previous material also failed: {rollback}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDomain {
    Effect,
    MaterialProgram,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct EditorHistoryLedger {
    undo: Vec<HistoryDomain>,
    redo: Vec<HistoryDomain>,
    effect: Option<EffectId>,
    observed_generation: u64,
    observed_effect_undo: usize,
    observed_effect_redo: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectHistoryChange {
    None,
    Edited,
    Reset,
}

impl EditorHistoryLedger {
    fn capture_effect_changes(&mut self, session: &EditorSession) -> EffectHistoryChange {
        let effect_changed = self
            .effect
            .is_some_and(|effect| effect != session.effect.id);
        let generation_changed =
            self.effect.is_some() && self.observed_generation != session.history_generation();
        let history_cleared =
            session.effect_undo_len() < self.observed_effect_undo && session.effect_redo_len() == 0;
        if effect_changed || generation_changed || history_cleared {
            self.undo.clear();
            self.redo.clear();
            self.effect = Some(session.effect.id);
            self.observed_generation = session.history_generation();
            self.observed_effect_undo = session.effect_undo_len();
            self.observed_effect_redo = session.effect_redo_len();
            return EffectHistoryChange::Reset;
        }
        self.effect.get_or_insert(session.effect.id);
        self.observed_generation = session.history_generation();
        if session.effect_undo_len() > self.observed_effect_undo {
            self.undo.extend(std::iter::repeat_n(
                HistoryDomain::Effect,
                session.effect_undo_len() - self.observed_effect_undo,
            ));
            self.redo.clear();
            self.observed_effect_undo = session.effect_undo_len();
            self.observed_effect_redo = session.effect_redo_len();
            return EffectHistoryChange::Edited;
        }
        self.observed_effect_undo = session.effect_undo_len();
        self.observed_effect_redo = session.effect_redo_len();
        EffectHistoryChange::None
    }

    pub(crate) fn record_material_edit(&mut self, session: &mut EditorSession) {
        self.capture_effect_changes(session);
        session.clear_effect_redo();
        self.observe_effect_history(session);
        self.undo.push(HistoryDomain::MaterialProgram);
        self.redo.clear();
    }

    fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn observe_effect_history(&mut self, session: &EditorSession) {
        self.observed_effect_undo = session.effect_undo_len();
        self.observed_effect_redo = session.effect_redo_len();
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryAction {
    Undo,
    Redo,
}

fn queue_history_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<HistoryAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_history_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &HistoryAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, disabled, mut background) in &mut buttons {
        if disabled.is_some() {
            if feathers.is_none() {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::BUTTON,
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
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn execute_history_action(
    action: On<HistoryAction>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut ledger: ResMut<EditorHistoryLedger>,
) {
    match ledger.capture_effect_changes(&session) {
        EffectHistoryChange::Reset => material_history.clear(),
        EffectHistoryChange::Edited => material_history.clear_redo(),
        EffectHistoryChange::None => {}
    }
    let domain = match *action {
        HistoryAction::Undo => ledger.undo.pop(),
        HistoryAction::Redo => ledger.redo.pop(),
    };
    let Some(domain) = domain else {
        session.status = match *action {
            HistoryAction::Undo => "Nothing to undo".into(),
            HistoryAction::Redo => "Nothing to redo".into(),
        };
        return;
    };

    let result = match (*action, domain) {
        (HistoryAction::Undo, HistoryDomain::Effect) => {
            let before = session.effect_undo_len();
            session.undo();
            (session.effect_undo_len() < before).then(|| {
                ledger.redo.push(domain);
            })
        }
        (HistoryAction::Redo, HistoryDomain::Effect) => {
            let before = session.effect_redo_len();
            session.redo();
            (session.effect_redo_len() < before).then(|| {
                ledger.undo.push(domain);
            })
        }
        (HistoryAction::Undo, HistoryDomain::MaterialProgram) => {
            match material_history.undo(&mut session, &mut catalog) {
                Ok(Some(label)) => {
                    session.status = format!("Undid {label}");
                    session.ui_revision += 1;
                    ledger.redo.push(domain);
                    Some(())
                }
                Ok(None) => None,
                Err(error) => {
                    session.status = format!("Material undo failed: {error}");
                    None
                }
            }
        }
        (HistoryAction::Redo, HistoryDomain::MaterialProgram) => {
            match material_history.redo(&mut session, &mut catalog) {
                Ok(Some(label)) => {
                    session.status = format!("Redid {label}");
                    session.ui_revision += 1;
                    ledger.undo.push(domain);
                    Some(())
                }
                Ok(None) => None,
                Err(error) => {
                    session.status = format!("Material redo failed: {error}");
                    None
                }
            }
        }
    };
    if result.is_none() {
        match *action {
            HistoryAction::Undo => ledger.undo.push(domain),
            HistoryAction::Redo => ledger.redo.push(domain),
        }
    }
    ledger.observe_effect_history(&session);
}

fn history_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::KeyZ) {
        commands.trigger(HistoryAction::Undo);
    }
    if control && keys.just_pressed(KeyCode::KeyY) {
        commands.trigger(HistoryAction::Redo);
    }
}

fn update_history_availability(
    session: Res<EditorSession>,
    ledger: Res<EditorHistoryLedger>,
    mut commands: Commands,
    items: Query<
        (Entity, Has<UndoMenuItem>, Has<RedoMenuItem>),
        Or<(With<UndoMenuItem>, With<RedoMenuItem>)>,
    >,
) {
    if !session.is_changed() && !ledger.is_changed() {
        return;
    }
    for (entity, undo, redo) in &items {
        let enabled = (undo && (ledger.can_undo() || session.can_undo()))
            || (redo && (ledger.can_redo() || session.can_redo()));
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

fn sync_effect_history_ledger(
    session: Res<EditorSession>,
    mut ledger: ResMut<EditorHistoryLedger>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
) {
    match ledger.capture_effect_changes(&session) {
        EffectHistoryChange::Reset => material_history.clear(),
        EffectHistoryChange::Edited => material_history.clear_redo(),
        EffectHistoryChange::None => {}
    }
}

type UnclassifiedHistoryControl = (
    Added<HistoryAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

fn audit_history_controls(controls: Query<Entity, UnclassifiedHistoryControl>) {
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "history control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    fn edited_session() -> EditorSession {
        let mut session = test_support::session_with_timing_slack();
        session.adjust_effect_duration(0.25);
        assert!(session.can_undo());
        session
    }

    fn add_history_resources(app: &mut App) {
        app.init_resource::<ProjectEffectCatalog>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>();
    }

    #[test]
    fn history_actions_own_undo_and_redo() {
        let session = edited_session();
        let changed_duration = session.effect.duration;
        let mut app = App::new();
        app.insert_resource(session);
        add_history_resources(&mut app);
        app.add_observer(execute_history_action);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );

        app.world_mut().trigger(HistoryAction::Redo);
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
    }

    #[test]
    fn keyboard_input_routes_through_the_history_action_contract() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
        let changed_duration = edited_session().effect.duration;
        let mut app = App::new();
        app.insert_resource(edited_session())
            .insert_resource(keys)
            .init_resource::<ModulePaletteState>();
        add_history_resources(&mut app);
        app.add_observer(execute_history_action)
            .add_systems(Update, history_keyboard_input);

        app.update();

        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
    }

    #[test]
    fn availability_sync_does_not_disable_unrelated_ui() {
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack());
        add_history_resources(&mut app);
        app.add_systems(Update, update_history_availability);
        let particle_color = Color::srgba(0.8, 0.4, 1.0, 0.75);
        let particle = app.world_mut().spawn(BackgroundColor(particle_color)).id();
        let undo = app
            .world_mut()
            .spawn((UndoMenuItem, BackgroundColor(theme::PANEL)))
            .id();

        app.update();

        let world = app.world();
        assert_eq!(
            world.get::<BackgroundColor>(particle).unwrap().0,
            particle_color
        );
        assert!(!world.entity(particle).contains::<InteractionDisabled>());
        assert!(world.entity(undo).contains::<InteractionDisabled>());
    }

    #[test]
    fn feathers_activation_queues_one_history_action() {
        let mut app = App::new();
        app.add_observer(queue_history_action_activation);
        let action = app
            .world_mut()
            .spawn((HistoryAction::Undo, FeathersActionButton, Interaction::None))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn global_history_preserves_effect_and_material_edit_order() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("ordered.aestra.material.ron");
        let mut original = MaterialProgram::additive_sprite("Original");
        original.id = aestra_core::MaterialProgramId::from_u128(0x7600);
        original.save_ron(&path).unwrap();
        let original = MaterialProgram::load_ron(&path).unwrap();
        let mut replacement = original.clone();
        replacement.name = "Reordered".into();

        let mut session = test_support::session_with_timing_slack();
        session
            .effect
            .material_instances
            .push(aestra_core::material::MaterialInstance {
                id: aestra_core::MaterialId::from_u128(0x7601),
                program: aestra_core::material::MaterialProgramRef::Project(original.id),
                values: default(),
                render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
            });
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut material_history = MaterialProgramEditHistory::default();
        let mut ledger = EditorHistoryLedger::default();
        material_history
            .execute_replacement(
                &mut session,
                &mut catalog,
                "Move material modifier",
                original.clone(),
                replacement.clone(),
            )
            .unwrap();
        assert_eq!(
            session
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(original.id)
                .unwrap()
                .name,
            replacement.name
        );
        ledger.record_material_edit(&mut session);
        session.adjust_effect_duration(0.25);
        let changed_duration = session.effect.duration;

        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .insert_resource(material_history)
            .insert_resource(ledger)
            .add_observer(execute_history_action);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), replacement);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), original);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(original.id)
                .unwrap()
                .name,
            original.name
        );

        app.world_mut().trigger(HistoryAction::Redo);
        app.update();
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), replacement);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(original.id)
                .unwrap()
                .name,
            replacement.name
        );

        app.world_mut().trigger(HistoryAction::Redo);
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
    }

    #[test]
    fn extracted_function_asset_participates_in_material_undo_and_redo() {
        use aestra_core::{
            MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId,
            MaterialFunctionOutputId,
            material::{
                MaterialExpression, MaterialExpressionKind, MaterialFunctionInput,
                MaterialFunctionOutput, MaterialInstance, MaterialRenderState,
                MaterialSchemaVersion, MaterialValueType,
            },
        };

        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("history.aestra.material.ron");
        let mut before = MaterialProgram::additive_sprite("History");
        before.id = aestra_core::MaterialProgramId::from_u128(0x7700);
        before.save_ron(&path).unwrap();
        let before = MaterialProgram::load_ron(&path).unwrap();
        let mut after = before.clone();
        after.name = "History after extraction".into();
        let input = MaterialFunctionInputId::from_u128(0x7702);
        let expression = MaterialExpressionId::from_u128(0x7703);
        let function = MaterialFunction {
            id: MaterialFunctionId::from_u128(0x7701),
            schema_version: MaterialSchemaVersion::CURRENT,
            name: "Extracted Function".into(),
            inputs: vec![MaterialFunctionInput {
                id: input,
                name: "Value".into(),
                value_type: MaterialValueType::Float,
            }],
            outputs: vec![MaterialFunctionOutput {
                id: MaterialFunctionOutputId::from_u128(0x7704),
                name: "Value".into(),
                value_type: MaterialValueType::Float,
                expression,
            }],
            expressions: vec![MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::FunctionInput(input),
            }],
        };
        let mut session = test_support::session_with_timing_slack();
        session.effect.material_instances.push(MaterialInstance {
            id: aestra_core::MaterialId::from_u128(0x7705),
            program: aestra_core::material::MaterialProgramRef::Project(before.id),
            values: default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut history = MaterialProgramEditHistory::default();

        history
            .execute_extraction(
                &mut session,
                &mut catalog,
                "Extract material function",
                before.clone(),
                after.clone(),
                function.clone(),
            )
            .unwrap();
        assert_eq!(
            catalog.material_functions().unwrap(),
            vec![function.clone()]
        );

        history.undo(&mut session, &mut catalog).unwrap();
        assert!(catalog.material_functions().unwrap().is_empty());
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), before);

        history.redo(&mut session, &mut catalog).unwrap();
        assert_eq!(catalog.material_functions().unwrap(), vec![function]);
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), after);
    }
}
