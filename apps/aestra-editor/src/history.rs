//! Undo/redo actions, shortcuts, menu activation, and availability synchronization.

#[cfg(test)]
use crate::{
    FeathersActionButton, ModulePaletteState, PendingFeathersActivation, ProjectEffectCatalog,
    menus::{MenuState, RedoMenuItem, UndoMenuItem},
    session::EditorSession,
    theme,
};
#[cfg(test)]
use aestra_core::material::{MaterialFunction, MaterialProgram};
use bevy::prelude::*;
#[cfg(test)]
use bevy::ui::InteractionDisabled;
#[cfg(test)]
use bevy::ui_widgets::Activate;

mod action;
pub(crate) use action::{HistoryAction, queue_history_action_activation};
mod material_program;
mod ownership;
pub(crate) mod publication;
pub(crate) use material_program::MaterialProgramEditHistory;
pub(crate) mod asset_order;

pub(crate) struct EditorHistoryPlugin;

mod focus;
#[path = "history/focus_pointer_019.rs"]
mod focus_pointer;
pub(crate) use focus::HistoryScope;
use focus::{capture_history_focus, capture_history_keyboard_focus};

impl HistoryScope {
    pub(crate) fn for_panel(panel: Option<crate::docking::ToolPanel>, standalone: bool) -> Self {
        use crate::docking::ToolPanel;
        match panel {
            Some(ToolPanel::MaterialGraph) => Self::Material,
            Some(ToolPanel::Properties) if standalone => Self::Material,
            Some(
                ToolPanel::Properties
                | ToolPanel::Timeline
                | ToolPanel::Curves
                | ToolPanel::Viewport,
            ) => Self::Effect,
            _ => Self::Neutral,
        }
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HistorySet {
    Input,
    Actions,
    Sync,
}

impl Plugin for EditorHistoryPlugin {
    fn build(&self, app: &mut App) {
        crate::material_function_editor::register(app);
        app.init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<asset_order::AssetOrder>()
            .add_observer(queue_history_action_activation)
            .add_observer(execute_history_action)
            .add_observer(capture_history_focus)
            .add_systems(
                Update,
                (
                    (capture_history_keyboard_focus, history_keyboard_input)
                        .chain()
                        .in_set(HistorySet::Input),
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

mod material_program_execution;

mod compound;
mod document_execution;
pub(crate) use document_execution::EditorHistoryLedger;

#[path = "history/control_input_019.rs"]
mod control_input;
mod controls;
use controls::{audit_history_controls, handle_history_buttons, update_history_availability};
mod keyboard;
pub(crate) use keyboard::history_keyboard_input;

pub(crate) mod routing;

pub(crate) fn execute_history_action(
    action: On<HistoryAction>,
    routing: routing::Routing,
    mut commands: Commands,
) {
    routing.execute(
        *action,
        &mut commands,
        crate::asset_browser::deletion::history_step,
    );
}

mod synchronization;
use synchronization::sync_effect_history_ledger;
pub(crate) use synchronization::{clear_document_order, clear_presentation_redo};

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
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>();
    }

    fn edit_shared(app: &mut App, before: &MaterialProgram, name: &str) {
        let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
        let mut catalog = app
            .world_mut()
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        session.open_material_program(&catalog, before.id).unwrap();
        let mut after = before.clone();
        after.name = name.into();
        app.world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(
                &mut session,
                &mut catalog,
                "Rename material",
                before.clone(),
                after,
            )
            .unwrap();
        app.world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        app.insert_resource(session).insert_resource(catalog);
    }

    fn press_history_scope(app: &mut App, scope: HistoryScope) {
        use bevy::{
            camera::NormalizedRenderTarget,
            picking::{
                backend::HitData,
                pointer::{Location, PointerId},
            },
        };
        let parent = app.world_mut().spawn(scope).id();
        let target = app.world_mut().spawn(ChildOf(parent)).id();
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
            Press {
                button: PointerButton::Primary,
                count: 1,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            target,
        ));
    }

    #[test]
    fn standalone_history_is_per_program_and_pointer_focus_preserves_effect_redo() {
        let root = tempfile::tempdir().unwrap();
        let first = MaterialProgram::additive_sprite("First").normalized();
        let second = MaterialProgram::additive_sprite("Second").normalized();
        first
            .save_ron(root.path().join("first.aestra.material.ron"))
            .unwrap();
        second
            .save_ron(root.path().join("second.aestra.material.ron"))
            .unwrap();
        let session = edited_session();
        let edited_duration = session.effect.duration;
        let mut app = App::new();
        app.insert_resource(session);
        add_history_resources(&mut app);
        app.insert_resource(ProjectEffectCatalog::scan(root.path()));
        app.add_observer(execute_history_action)
            .add_observer(capture_history_focus);
        app.world_mut().trigger(HistoryAction::Undo);
        let effect = app.world().resource::<EditorSession>().effect.clone();
        let selection = app.world().resource::<EditorSession>().selection;
        assert_eq!(app.world().resource::<EditorSession>().effect_redo_len(), 1);
        edit_shared(&mut app, &first, "First edit");
        edit_shared(&mut app, &second, "Second edit");
        assert_eq!(app.world().resource::<EditorSession>().effect_redo_len(), 1);
        app.world_mut().trigger(HistoryAction::Undo);
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(catalog.material_program(second.id).unwrap(), second);
        assert_eq!(
            catalog.material_program(first.id).unwrap().name,
            "First edit"
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(app.world().resource::<EditorSession>().selection, selection);

        // Clicking the viewport/timeline changes history focus, not the open graph.
        press_history_scope(&mut app, HistoryScope::Effect);
        assert!(
            !app.world()
                .resource::<EditorSession>()
                .material_history_active
        );
        app.world_mut().trigger(HistoryAction::Redo);
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            edited_duration
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material(),
            Some(second.id)
        );
        press_history_scope(&mut app, HistoryScope::Material);
        press_history_scope(&mut app, HistoryScope::Neutral);
        assert!(
            app.world()
                .resource::<EditorSession>()
                .material_history_active
        );
        app.world_mut().trigger(HistoryAction::Redo);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(second.id)
                .unwrap()
                .name,
            "Second edit"
        );
        let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
        session
            .open_material_program(app.world().resource::<ProjectEffectCatalog>(), first.id)
            .unwrap();
        app.insert_resource(session);
        app.world_mut().trigger(HistoryAction::Undo);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(first.id)
                .unwrap(),
            first
        );
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            edited_duration
        );
    }

    #[test]
    fn material_history_follows_active_view_instead_of_session_target() {
        use crate::document::{DocumentKey, DocumentManager};
        use crate::editor_view::{
            ActiveEditorContext, EditorViewKind, EditorViewManager, open_document_view,
        };

        let root = tempfile::tempdir().unwrap();
        let first = MaterialProgram::additive_sprite("First").normalized();
        let second = MaterialProgram::additive_sprite("Second").normalized();
        first
            .save_ron(root.path().join("first.aestra.material.ron"))
            .unwrap();
        second
            .save_ron(root.path().join("second.aestra.material.ron"))
            .unwrap();

        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack());
        add_history_resources(&mut app);
        app.insert_resource(ProjectEffectCatalog::scan(root.path()));
        app.add_observer(execute_history_action);
        edit_shared(&mut app, &first, "First edit");
        edit_shared(&mut app, &second, "Second edit");
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material(),
            Some(second.id)
        );

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(first.id),
            EditorViewKind::MaterialGraph,
        );
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(second.id),
            EditorViewKind::MaterialGraph,
        );
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(first.id),
            EditorViewKind::MaterialGraph,
        );
        app.insert_resource(documents)
            .insert_resource(views)
            .insert_resource(active);

        app.world_mut().trigger(HistoryAction::Undo);
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(catalog.material_program(first.id).unwrap(), first);
        assert_eq!(
            catalog.material_program(second.id).unwrap().name,
            "Second edit"
        );

        app.world_mut().trigger(HistoryAction::Redo);
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(
            catalog.material_program(first.id).unwrap().name,
            "First edit"
        );
        assert_eq!(
            catalog.material_program(second.id).unwrap().name,
            "Second edit"
        );
    }

    #[test]
    fn failed_standalone_history_does_not_pop_entries_or_touch_effect() {
        let root = tempfile::tempdir().unwrap();
        let before = MaterialProgram::additive_sprite("Before").normalized();
        let path = root.path().join("source.aestra.material.ron");
        before.save_ron(&path).unwrap();
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack());
        add_history_resources(&mut app);
        app.insert_resource(ProjectEffectCatalog::scan(root.path()));
        app.add_observer(execute_history_action);
        edit_shared(&mut app, &before, "Draft");
        // A conflicting newer draft must never be overwritten by an old inverse command.
        let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
        let current = catalog.material_program(before.id).unwrap();
        let mut conflicting = current.clone();
        conflicting.name = "Newer".into();
        catalog
            .replace_material_program(&current, &conflicting)
            .unwrap();
        let effect = app.world().resource::<EditorSession>().effect.clone();
        app.world_mut().trigger(HistoryAction::Undo);
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(before.id)
                .unwrap(),
            conflicting
        );
        let session = app.world().resource::<EditorSession>();
        let history = app
            .world()
            .resource::<MaterialProgramEditHistory>()
            .for_target(session)
            .unwrap();
        assert_eq!(history.undo.len(), 1);
        assert!(history.redo.is_empty());
        assert_eq!(MaterialProgram::load_ron(path).unwrap(), before);
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
    fn fast_azerty_history_chords_keep_order_and_respect_text_focus() {
        use crate::input::tests::{key, keyboard_app, tap};
        use bevy::input::{ButtonState, keyboard::Key};
        use bevy::{input_focus::InputFocus, text::EditableText};
        #[derive(Resource, Default)]
        struct Actions(Vec<HistoryAction>);
        let (mut app, window, text) = keyboard_app();
        app.init_resource::<ModulePaletteState>()
            .init_resource::<Actions>()
            .add_observer(|event: On<HistoryAction>, mut actions: ResMut<Actions>| {
                actions.0.push(*event.event())
            })
            .add_systems(Update, history_keyboard_input);
        // Editable focus must not leak into document history.
        key(
            &mut app,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
        );
        tap(&mut app, window, KeyCode::KeyW, Key::Character("z".into()));
        key(
            &mut app,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Released,
        );
        app.world_mut().run_schedule(PreUpdate);
        app.world_mut().run_schedule(Update);
        assert!(app.world().resource::<Actions>().0.is_empty());
        assert!(
            app.world()
                .get::<EditableText>(text)
                .unwrap()
                .pending_edits
                .is_empty()
        );
        app.world_mut().resource_mut::<InputFocus>().clear();
        key(
            &mut app,
            window,
            KeyCode::ControlRight,
            Key::Control,
            ButtonState::Pressed,
        );
        tap(&mut app, window, KeyCode::KeyW, Key::Character("z".into()));
        key(
            &mut app,
            window,
            KeyCode::ShiftRight,
            Key::Shift,
            ButtonState::Pressed,
        );
        tap(&mut app, window, KeyCode::KeyW, Key::Character("Z".into()));
        key(
            &mut app,
            window,
            KeyCode::ShiftRight,
            Key::Shift,
            ButtonState::Released,
        );
        tap(&mut app, window, KeyCode::KeyY, Key::Character("y".into()));
        key(
            &mut app,
            window,
            KeyCode::ControlRight,
            Key::Control,
            ButtonState::Released,
        );
        app.world_mut().run_schedule(PreUpdate);
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().resource::<Actions>().0,
            [
                HistoryAction::Undo,
                HistoryAction::Redo,
                HistoryAction::Redo
            ]
        );
        app.world_mut().run_schedule(PreUpdate);
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Actions>().0.len(), 3);
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
    fn undo_menu_availability_follows_active_material_view() {
        use crate::document::{DocumentKey, DocumentManager};
        use crate::editor_view::{
            ActiveEditorContext, EditorViewKind, EditorViewManager, open_document_view,
        };

        let root = tempfile::tempdir().unwrap();
        let first = MaterialProgram::additive_sprite("First").normalized();
        let second = MaterialProgram::additive_sprite("Second").normalized();
        first
            .save_ron(root.path().join("first.aestra.material.ron"))
            .unwrap();
        second
            .save_ron(root.path().join("second.aestra.material.ron"))
            .unwrap();
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack());
        add_history_resources(&mut app);
        app.insert_resource(ProjectEffectCatalog::scan(root.path()));
        edit_shared(&mut app, &first, "First edit");
        let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
        app.world_mut()
            .resource_mut::<EditorSession>()
            .open_material_program(&catalog, second.id)
            .unwrap();

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(first.id),
            EditorViewKind::MaterialGraph,
        );
        app.insert_resource(documents)
            .insert_resource(views)
            .insert_resource(active);
        app.add_systems(Update, update_history_availability);
        let undo = app.world_mut().spawn(UndoMenuItem).id();
        let redo = app.world_mut().spawn(RedoMenuItem).id();

        app.update();
        assert!(!app.world().entity(undo).contains::<InteractionDisabled>());
        assert!(app.world().entity(redo).contains::<InteractionDisabled>());
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
    fn feathers_history_button_dispatches_to_real_history_and_closes_menus_once() {
        let mut app = App::new();
        let changed_duration = edited_session().effect.duration;
        app.insert_resource(edited_session())
            .insert_resource(MenuState {
                open: Some(crate::menus::MenuKind::Edit),
                panels_open: true,
                tab_context: Some(crate::menus::TabContextMenu {
                    panel: crate::docking::ToolPanel::MaterialGraph,
                    position: [12.0, 24.0],
                }),
                ..default()
            });
        add_history_resources(&mut app);
        app.add_observer(queue_history_action_activation)
            .add_observer(execute_history_action)
            .add_systems(Update, handle_history_buttons);
        let background = BackgroundColor(theme::BUTTON);
        let button = app
            .world_mut()
            .spawn((
                HistoryAction::Undo,
                FeathersActionButton,
                Interaction::None,
                background,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
        assert!(app.world().resource::<MenuState>().open.is_some());

        app.world_mut().trigger(Activate { entity: button });
        app.update();
        let undone_duration = app.world().resource::<EditorSession>().effect.duration;
        assert_ne!(undone_duration, changed_duration);
        let menu = app.world().resource::<MenuState>();
        assert!(menu.open.is_none());
        assert!(!menu.panels_open);
        assert!(menu.tab_context.is_none());
        assert!(
            app.world()
                .get::<PendingFeathersActivation>(button)
                .is_none()
        );
        assert_eq!(
            app.world().get::<Interaction>(button),
            Some(&Interaction::None)
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(button),
            Some(&background)
        );
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            undone_duration
        );

        app.world_mut()
            .entity_mut(button)
            .insert(HistoryAction::Redo);
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
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
        // Fill the bounded effect history before interleaving a material edit.
        for index in 0..256 {
            assert!(session.set_effect_name(format!("History {index}")));
            ledger.capture_effect_changes(&session);
        }
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
                .preview()
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
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(execute_history_action);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), original);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), original);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .preview()
                .unwrap()
                .effect()
                .material_program(original.id)
                .unwrap()
                .name,
            original.name
        );

        app.world_mut().trigger(HistoryAction::Redo);
        app.update();
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), original);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .preview()
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
                default: None,
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
            custom_wesl: None,
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
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), before);
        assert!(session.dirty);
        catalog.save_material_drafts().unwrap();
        session.set_material_drafts(catalog.material_drafts.clone());
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), after);
        history.undo(&mut session, &mut catalog).unwrap();
        assert!(session.dirty);
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), after);
        assert!(catalog.material_functions().unwrap().is_empty());
        history.redo(&mut session, &mut catalog).unwrap();
        assert!(catalog.material_drafts.is_empty());
    }
}
