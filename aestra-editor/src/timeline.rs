use crate::{
    ComboOption, CurvesState, DockPanel, EditorNativeControl, EditorTooltip, FeathersActionButton,
    Localizer, MenuState, ModulePaletteState, PendingFeathersActivation, TransportAction,
    WorkspaceLayout, mini_button, reveal_dock_panel, session::EditorSession, spawn_combo_control,
    spawn_feathers_action_button, theme,
};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_bevy::EmitterId;
use bevy::{
    feathers::cursor::{EntityCursor, OverrideCursor},
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    input_focus::InputFocus,
    picking::{
        events::{Click, Drag, DragEnd, DragStart, Pointer, Press},
        pointer::PointerButton,
    },
    prelude::*,
    text::EditableText,
    ui::RelativeCursorPosition,
    ui_widgets::Activate,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};
use fluent_bundle::FluentArgs;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TimelineSet {
    Input,
    Actions,
    Visuals,
}

pub(crate) struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        let duration = app
            .world()
            .get_resource::<EditorSession>()
            .map_or(1.0, EditorSession::playback_duration);
        app.insert_resource(TimelineState::framed(duration))
            .add_observer(queue_timeline_action_activation)
            .add_observer(execute_timeline_action)
            .add_observer(queue_choreography_action_activation)
            .add_observer(execute_choreography_action)
            .add_systems(
                Update,
                (choreography_keyboard_input, navigate_timeline)
                    .chain()
                    .in_set(TimelineSet::Input),
            )
            .add_systems(
                Update,
                (
                    handle_timeline_action_buttons,
                    handle_choreography_action_buttons,
                    audit_timeline_controls,
                )
                    .chain()
                    .in_set(TimelineSet::Actions),
            )
            .add_systems(
                Update,
                (
                    update_timeline_time_label,
                    update_timeline_visuals,
                    update_timeline_scrollbar,
                    update_track_header_hover_actions,
                )
                    .chain()
                    .in_set(TimelineSet::Visuals),
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq)]
pub(crate) enum TimelineAction {
    AdjustEffectDuration(f32),
    SetSnap(TimelineSnapMode),
    FrameAll,
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChoreographyAction {
    SelectEmitter(EmitterId),
    AddEmitter,
    DuplicateEmitter(Option<EmitterId>),
    DeleteEmitter(Option<EmitterId>),
    SetEmitterEnabled { emitter: EmitterId, enabled: bool },
    ToggleEmitterMenu(EmitterId),
}

fn queue_timeline_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<TimelineAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn queue_choreography_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ChoreographyAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn handle_timeline_action_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &TimelineAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, pending) in &mut buttons {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        menu.open = None;
        menu.panels_open = false;
        if menu.tab_context.take().is_some() {
            session.ui_revision += 1;
        }
        commands.trigger(*action);
    }
}

fn handle_choreography_action_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &ChoreographyAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, pending) in &mut buttons {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        menu.open = None;
        menu.panels_open = false;
        if menu.tab_context.take().is_some() {
            session.ui_revision += 1;
        }
        commands.trigger(*action);
    }
}

fn execute_timeline_action(
    action: On<TimelineAction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    match *action {
        TimelineAction::AdjustEffectDuration(delta) => {
            session.adjust_effect_duration(delta);
        }
        TimelineAction::SetSnap(mode) => {
            if state.set_snap(mode) {
                session.ui_revision += 1;
            }
        }
        TimelineAction::FrameAll => state.frame_all(session.playback_duration()),
    }
}

fn execute_choreography_action(
    action: On<ChoreographyAction>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut state: ResMut<TimelineState>,
    localizer: Res<Localizer>,
) {
    if let ChoreographyAction::ToggleEmitterMenu(emitter) = *action {
        let revision = session.ui_revision;
        if session
            .effect
            .emitters
            .iter()
            .any(|item| item.id == emitter)
        {
            session.select_emitter(emitter);
            curves.clear();
        }
        state.context_emitter = (state.context_emitter != Some(emitter)).then_some(emitter);
        if session.ui_revision == revision {
            session.ui_revision += 1;
        }
        return;
    }

    let revision = session.ui_revision;
    let closed_context_menu = state.context_emitter.take().is_some();
    match *action {
        ChoreographyAction::SelectEmitter(emitter) => {
            if session
                .effect
                .emitters
                .iter()
                .any(|item| item.id == emitter)
            {
                session.select_emitter(emitter);
                curves.clear();
            }
        }
        ChoreographyAction::AddEmitter => {
            session.add_layer();
            curves.clear();
        }
        ChoreographyAction::DuplicateEmitter(target) => {
            if select_choreography_target(&mut session, target) {
                session.duplicate_selected_layer();
                curves.clear();
            }
        }
        ChoreographyAction::DeleteEmitter(target) => {
            if select_choreography_target(&mut session, target)
                && preview_selected_emitter_deletion(&mut session, &localizer)
            {
                reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                curves.clear();
            }
        }
        ChoreographyAction::SetEmitterEnabled { emitter, enabled } => {
            if select_choreography_target(&mut session, Some(emitter)) {
                session.set_selected_emitter_enabled(enabled);
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEmitterMenu(_) => unreachable!(),
    }
    if closed_context_menu && session.ui_revision == revision {
        session.ui_revision += 1;
    }
}

fn select_choreography_target(session: &mut EditorSession, target: Option<EmitterId>) -> bool {
    let Some(target) = target else {
        return true;
    };
    if !session
        .effect
        .emitters
        .iter()
        .any(|emitter| emitter.id == target)
    {
        session.status = "Emitter no longer exists".into();
        return false;
    }
    session.select_emitter(target);
    true
}

fn preview_selected_emitter_deletion(session: &mut EditorSession, localizer: &Localizer) -> bool {
    if session.effect.emitters.len() <= 1 {
        session.status = localizer.text("assets-status-minimum-emitter");
        return false;
    }
    let id = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        localizer.text("assets-change-delete-emitter"),
        EffectCommand::RemoveEmitter { id },
    ))
}

fn choreography_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
    timelines: Query<(), With<TimelineCanvas>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
) {
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if palette.open || timelines.is_empty() || editing_text {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::Enter) {
        commands.trigger(ChoreographyAction::AddEmitter);
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        commands.trigger(ChoreographyAction::DuplicateEmitter(None));
    }
    if keys.just_pressed(KeyCode::Delete) {
        commands.trigger(ChoreographyAction::DeleteEmitter(None));
    }
}

type UnclassifiedTimelineControl = (
    Added<TimelineAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

type UnclassifiedChoreographyControl = (
    Added<ChoreographyAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

fn audit_timeline_controls(
    controls: Query<Entity, UnclassifiedTimelineControl>,
    choreography: Query<Entity, UnclassifiedChoreographyControl>,
) {
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "timeline control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
    #[cfg(debug_assertions)]
    if let Some(entity) = choreography.iter().next() {
        panic!(
            "choreography control {entity:?} must use FeathersActionButton or be explicitly \
             marked EditorNativeControl"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE, LibraryState};
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    fn spawn_test_timeline(
        mut commands: Commands,
        session: Res<EditorSession>,
        state: Res<TimelineState>,
        localizer: Res<Localizer>,
    ) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_timeline(parent, &session, &state, &localizer);
        });
    }

    fn choreography_app(session: EditorSession) -> App {
        let mut app = App::new();
        let duration = session.playback_duration();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(duration))
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_choreography_action);
        app
    }

    #[test]
    fn timeline_cursor_coordinates_cover_the_full_canvas() {
        assert_eq!(timeline_cursor_fraction(-0.5), 0.0);
        assert_eq!(timeline_cursor_fraction(0.0), 0.5);
        assert_eq!(timeline_cursor_fraction(0.5), 1.0);
    }

    #[test]
    fn timeline_zoom_keeps_the_time_under_the_pointer() {
        let mut state = TimelineState::default();
        state.frame_all(10.0);
        let anchor = state.view.time_at(0.73);

        state.zoom_at(anchor, 0.5, 10.0, 60);

        assert!((state.view.time_at(0.73) - anchor).abs() < 0.000_1);
        assert!((state.view.span() - 5.0).abs() < 0.000_1);
    }

    #[test]
    fn timeline_pan_stays_inside_the_effect() {
        let mut state = TimelineState {
            view: TimelineView {
                start: 2.0,
                end: 4.0,
            },
            ..default()
        };

        state.pan_by(-10.0, 8.0);
        assert_eq!(state.view.start, 0.0);
        assert_eq!(state.view.end, 2.0);

        state.pan_by(20.0, 8.0);
        assert_eq!(state.view.start, 6.0);
        assert_eq!(state.view.end, 8.0);
    }

    #[test]
    fn timeline_ruler_uses_human_readable_intervals() {
        assert_eq!(nice_timeline_step(10.0, 1_000.0), 1.0);
        assert_eq!(nice_timeline_step(2.8, 1_000.0), 0.5);
        assert_eq!(nice_timeline_step(0.2, 1_000.0), 0.02);
    }

    #[test]
    fn timeline_timing_commit_is_one_undoable_command() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let emitter = session.effect.emitters[0].clone();

        assert!(session.set_emitter_timing(
            emitter.id,
            emitter.start_time + 0.1,
            emitter.duration - 0.1,
            "Moved emitter on timeline",
        ));
        assert!(session.can_undo());
        session.undo();

        let restored = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        assert_eq!(restored.start_time, emitter.start_time);
        assert_eq!(restored.duration, emitter.duration);
    }

    #[test]
    fn timeline_visual_queries_initialize_without_aliasing() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let timeline = TimelineState::framed(session.playback_duration());
        let mut app = App::new();
        app.insert_resource(session);
        app.insert_resource(timeline);
        app.add_systems(
            Update,
            (update_timeline_visuals, update_timeline_scrollbar).chain(),
        );

        app.update();
    }

    #[test]
    fn timeline_actions_own_duration_snap_and_framing() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_duration = session.effect.duration;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState {
                view: TimelineView {
                    start: 0.5,
                    end: 1.0,
                },
                ..default()
            })
            .add_observer(execute_timeline_action);

        app.world_mut()
            .trigger(TimelineAction::AdjustEffectDuration(0.25));
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.duration, initial_duration + 0.25);
        assert!(session.can_undo());

        app.world_mut()
            .trigger(TimelineAction::SetSnap(TimelineSnapMode::None));
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().snap,
            TimelineSnapMode::None
        );

        app.world_mut().trigger(TimelineAction::FrameAll);
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert_eq!(state.view.start, 0.0);
        assert_eq!(state.view.end, initial_duration + 0.25);
    }

    #[test]
    fn feathers_activation_queues_one_timeline_action() {
        let mut app = App::new();
        app.add_observer(queue_timeline_action_activation);
        let action = app
            .world_mut()
            .spawn((
                TimelineAction::FrameAll,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn track_headers_and_clips_expose_the_same_stable_selection_actions() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect.emitters[1].enabled = false;
        session.diagnostics.push(aestra_bevy::Diagnostic::error(
            aestra_bevy::DiagnosticCode::InvalidTiming,
            "effect.emitters[1].duration",
            "test diagnostic",
        ));
        let emitter_count = session.effect.emitters.len();
        let duration = session.playback_duration();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .insert_resource(session)
        .insert_resource(TimelineState::framed(duration))
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);

        app.update();

        let headers = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &EmitterTrackHeader,
                &ChoreographyAction,
                &AccessibleLabel,
                Has<Button>,
            )>();
            query
                .iter(world)
                .map(|(header, action, label, button)| {
                    (header.emitter, *action, label.0.clone(), button)
                })
                .collect::<Vec<_>>()
        };
        let clips = {
            let world = app.world_mut();
            let mut query = world.query::<(&TimelineClipInteraction, &ChoreographyAction)>();
            query
                .iter(world)
                .filter(|(clip, _)| clip.kind == TimelineDragKind::Move)
                .map(|(clip, action)| (clip.emitter, *action))
                .collect::<Vec<_>>()
        };
        assert_eq!(headers.len(), emitter_count);
        assert_eq!(clips.len(), emitter_count);
        for (emitter, action, label, button) in headers {
            assert!(button);
            assert_eq!(action, ChoreographyAction::SelectEmitter(emitter));
            assert_eq!(
                clips.iter().find(|clip| clip.0 == emitter).unwrap().1,
                action
            );
            assert!(!label.is_empty());
        }
        let disabled = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<EmitterTrackDisabled>>();
            query.iter(world).count()
        };
        let diagnostics = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<EmitterTrackDiagnostic>>();
            query.iter(world).count()
        };
        assert_eq!(disabled, 1);
        assert_eq!(diagnostics, 1);
    }

    #[test]
    fn choreography_selection_is_stable_and_clears_incompatible_curve_state() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.insert_resource(LibraryState {
            query: "does not match anything".into(),
            ..default()
        });
        app.insert_resource({
            let mut curves = CurvesState::default();
            curves.select_for_test(aestra_bevy::ModuleId::new(), 0, 0);
            curves
        });

        app.world_mut()
            .trigger(ChoreographyAction::SelectEmitter(target));
        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selection
                .emitter(&app.world().resource::<EditorSession>().effect),
            Some(target)
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
        assert_eq!(
            app.world().resource::<LibraryState>().query,
            "does not match anything"
        );
    }

    #[test]
    fn choreography_add_duplicate_and_enabled_actions_remain_undoable() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let original_count = session.effect.emitters.len();
        let original = session.effect.emitters[0].clone();
        let mut app = choreography_app(session);

        app.world_mut().trigger(ChoreographyAction::AddEmitter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count + 1
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count
        );

        app.world_mut()
            .trigger(ChoreographyAction::DuplicateEmitter(Some(original.id)));
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count + 1
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count
        );

        app.world_mut()
            .trigger(ChoreographyAction::SetEmitterEnabled {
                emitter: original.id,
                enabled: !original.enabled,
            });
        app.update();
        let edited = app
            .world()
            .resource::<EditorSession>()
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == original.id)
            .unwrap();
        assert_eq!(edited.enabled, !original.enabled);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let restored = app
            .world()
            .resource::<EditorSession>()
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == original.id)
            .unwrap();
        assert_eq!(restored.enabled, original.enabled);
    }

    #[test]
    fn choreography_delete_retains_review_and_minimum_emitter_guard() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = session.effect.emitters[1].id;
        let mut app = choreography_app(session);
        app.world_mut()
            .trigger(ChoreographyAction::DeleteEmitter(Some(target)));
        app.update();
        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_some()
        );
        assert!(
            app.world()
                .resource::<WorkspaceLayout>()
                .is_visible(DockPanel::Changes)
        );

        let mut single = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        single.effect.emitters.truncate(1);
        single
            .selection
            .select_emitter(single.effect.emitters[0].id);
        let mut guarded = choreography_app(single);
        guarded
            .world_mut()
            .trigger(ChoreographyAction::DeleteEmitter(None));
        guarded.update();
        let session = guarded.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), 1);
        assert!(session.pending_change.is_none());
        assert_eq!(session.status, "An effect must keep at least one emitter");
    }

    #[test]
    fn choreography_shortcuts_require_timeline_context_and_ignore_text_editing() {
        for (timeline_visible, text_focused, expected_delta) in
            [(false, false, 0), (true, true, 0), (true, false, 1)]
        {
            let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
            let initial = session.effect.emitters.len();
            let mut app = choreography_app(session);
            app.init_resource::<ModulePaletteState>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_systems(Update, choreography_keyboard_input);
            if timeline_visible {
                app.world_mut().spawn(TimelineCanvas);
            }
            if text_focused {
                let input = app.world_mut().spawn(EditableText::new("editing")).id();
                app.insert_resource(InputFocus::from_entity(input));
            }
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::ControlLeft);
                keys.press(KeyCode::Enter);
            }

            app.update();

            assert_eq!(
                app.world()
                    .resource::<EditorSession>()
                    .effect
                    .emitters
                    .len(),
                initial + expected_delta
            );
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_timeline_visuals(
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    mut clips: Query<(&TimelineClip, &mut Node), Without<Playhead>>,
    mut playheads: Query<&mut Node, (With<Playhead>, Without<TimelineClip>)>,
    mut guides: Query<
        &mut Node,
        (
            With<TimelineSnapGuide>,
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineRulerTick>,
        ),
    >,
    mut ticks: Query<
        (&TimelineRulerTick, &Children, &mut Node),
        (
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineSnapGuide>,
        ),
    >,
    mut texts: Query<&mut Text>,
) {
    state.ensure_duration(session.playback_duration());
    let view = state.view;
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(320.0);

    for (clip, mut node) in &mut clips {
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == clip.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let (start, duration) = state
            .drag
            .filter(|drag| drag.emitter == clip.emitter)
            .map_or((emitter.start_time, emitter.duration), |drag| {
                (drag.current_start, drag.current_duration)
            });
        let end = start + duration;
        let visible_start = start.max(view.start);
        let visible_end = end.min(view.end);
        if visible_end <= visible_start {
            node.display = Display::None;
            continue;
        }
        node.display = Display::Flex;
        node.left = Val::Percent(view.normalized_time(visible_start) * 100.0);
        node.width =
            Val::Percent(((visible_end - visible_start) / view.span() * 100.0).clamp(0.05, 100.0));
    }

    let playhead_position = view.normalized_time(session.time());
    for mut node in &mut playheads {
        node.display = if (0.0..=1.0).contains(&playhead_position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(playhead_position.clamp(0.0, 1.0) * 100.0);
    }
    for mut node in &mut guides {
        if let Some(time) = state.snap_guide
            && (view.start..=view.end).contains(&time)
        {
            node.display = Display::Flex;
            node.left = Val::Percent(view.normalized_time(time) * 100.0);
        } else {
            node.display = Display::None;
        }
    }

    let step = nice_timeline_step(view.span(), width);
    let first = (view.start / step).ceil() * step;
    for (tick, children, mut node) in &mut ticks {
        let time = first + tick.0 as f32 * step;
        if time > view.end + step * 0.001 {
            node.display = Display::None;
            continue;
        }
        node.display = Display::Flex;
        node.left = Val::Percent(view.normalized_time(time) * 100.0);
        if let Some(child) = children.first()
            && let Ok(mut text) = texts.get_mut(*child)
        {
            text.0 = format_timeline_tick(time, step);
        }
    }
}

fn update_timeline_scrollbar(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut tracks: Query<
        &mut Node,
        (
            With<TimelineScrollbarTrack>,
            Without<TimelineScrollbarThumb>,
        ),
    >,
    mut thumbs: Query<
        &mut Node,
        (
            With<TimelineScrollbarThumb>,
            Without<TimelineScrollbarTrack>,
        ),
    >,
) {
    let duration = session.playback_duration().max(0.05);
    let visible_ratio = (state.view.span() / duration).clamp(0.0, 1.0);
    let overflow = visible_ratio < 0.999;
    for mut node in &mut tracks {
        node.display = if overflow {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut thumbs {
        node.left = Val::Percent((state.view.start / duration * 100.0).clamp(0.0, 100.0));
        node.width = Val::Percent((visible_ratio * 100.0).clamp(2.0, 100.0));
    }
}

fn update_timeline_time_label(
    session: Res<EditorSession>,
    mut labels: Query<&mut Text, With<TimeLabel>>,
) {
    if !session.is_changed() {
        return;
    }
    for mut text in &mut labels {
        text.0 = format!(
            "F{:05}  ·  {:02}:{:06.3}  /  00:{:06.3}  ·  {}",
            session.frame(),
            0,
            session.time(),
            session.playback_duration(),
            session.seek_status()
        );
    }
}

fn nice_timeline_step(span: f32, width: f32) -> f32 {
    let target_ticks = (width / 96.0).clamp(2.0, 24.0);
    let raw = (span / target_ticks).max(0.000_001);
    let magnitude = 10.0_f32.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let factor = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

fn format_timeline_tick(time: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{time:.1}")
    } else if step >= 0.1 {
        format!("{time:.2}")
    } else {
        format!("{time:.3}")
    }
}

fn snap_timeline_boundary(
    candidate: f32,
    emitter: EmitterId,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    match mode {
        TimelineSnapMode::None => (candidate, None),
        TimelineSnapMode::Frames => {
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let snapped = (candidate / frame).round() * frame;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Seconds => {
            let interval = nice_timeline_step(view.span(), canvas_width) / 5.0;
            let snapped = (candidate / interval).round() * interval;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Smart => {
            let threshold = view.span() / canvas_width.max(1.0) * 9.0;
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let mut targets = vec![
                0.0,
                session.playback_duration(),
                session.time(),
                (candidate / frame).round() * frame,
            ];
            for other in &session.effect.emitters {
                if other.id != emitter {
                    targets.push(other.start_time);
                    targets.push(other.start_time + other.duration);
                }
            }
            let nearest = targets.into_iter().min_by(|left, right| {
                (candidate - *left)
                    .abs()
                    .total_cmp(&(candidate - *right).abs())
            });
            nearest
                .filter(|target| (candidate - *target).abs() <= threshold)
                .map_or((candidate, None), |target| (target, Some(target)))
        }
    }
}

fn snap_moved_timing(
    start: f32,
    duration: f32,
    emitter: EmitterId,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    let start_snap = snap_timeline_boundary(start, emitter, session, mode, view, canvas_width);
    if mode != TimelineSnapMode::Smart {
        return start_snap;
    }
    let end = start + duration;
    let end_snap = snap_timeline_boundary(end, emitter, session, mode, view, canvas_width);
    let start_delta = (start_snap.0 - start).abs();
    let end_delta = (end_snap.0 - end).abs();
    match (start_snap.1, end_snap.1) {
        (None, Some(guide)) => (start + end_snap.0 - end, Some(guide)),
        (Some(_), Some(guide)) if end_delta < start_delta => {
            (start + end_snap.0 - end, Some(guide))
        }
        _ => start_snap,
    }
}

#[derive(Component)]
struct TimelineCanvas;

#[derive(Component, Clone, Copy)]
struct EmitterTrackHeader {
    emitter: EmitterId,
}

#[derive(Component)]
struct EmitterTrackDiagnostic;

#[derive(Component)]
struct EmitterTrackDisabled;

#[derive(Component)]
struct EmitterTrackHoverActions;

#[derive(Component)]
struct EmitterTrackContextMenu;

#[derive(Component, Clone, Copy)]
struct TimelineClip {
    emitter: EmitterId,
}

#[derive(Component, Clone, Copy)]
struct TimelineClipInteraction {
    emitter: EmitterId,
    kind: TimelineDragKind,
}

#[derive(Component)]
struct TimelineRulerTick(usize);

#[derive(Component)]
struct TimelineSnapGuide;

#[derive(Component)]
struct TimelineScrollbarTrack;

#[derive(Component)]
struct TimelineScrollbarThumb;

#[derive(Component)]
struct Playhead;

#[derive(Component)]
struct TimeLabel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TimelineSnapMode {
    None,
    Frames,
    Seconds,
    #[default]
    Smart,
}

impl TimelineSnapMode {
    const ALL: [Self; 4] = [Self::None, Self::Frames, Self::Seconds, Self::Smart];

    fn message_id(self) -> &'static str {
        match self {
            Self::None => "timeline-snap-off",
            Self::Frames => "timeline-snap-frames",
            Self::Seconds => "timeline-snap-time",
            Self::Smart => "timeline-snap-smart",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineDragKind {
    Move,
    TrimStart,
    TrimEnd,
}

#[derive(Clone, Copy, Debug)]
struct TimelineDrag {
    emitter: EmitterId,
    kind: TimelineDragKind,
    pointer_start: f32,
    original_start: f32,
    original_duration: f32,
    current_start: f32,
    current_duration: f32,
}

#[derive(Clone, Copy, Debug)]
struct TimelineScrollbarDrag {
    view_start: f32,
}

#[derive(Clone, Copy, Debug)]
struct TimelineView {
    start: f32,
    end: f32,
}

impl TimelineView {
    fn span(self) -> f32 {
        (self.end - self.start).max(0.000_1)
    }

    fn time_at(self, normalized: f32) -> f32 {
        self.start + normalized.clamp(0.0, 1.0) * self.span()
    }

    fn normalized_time(self, time: f32) -> f32 {
        (time - self.start) / self.span()
    }
}

#[derive(Resource, Debug)]
pub(crate) struct TimelineState {
    view: TimelineView,
    snap: TimelineSnapMode,
    drag: Option<TimelineDrag>,
    snap_guide: Option<f32>,
    panning: bool,
    scrollbar_drag: Option<TimelineScrollbarDrag>,
    context_emitter: Option<EmitterId>,
    known_duration: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::framed(1.0)
    }
}

impl TimelineState {
    fn framed(duration: f32) -> Self {
        let duration = duration.max(0.05);
        Self {
            view: TimelineView {
                start: 0.0,
                end: duration,
            },
            snap: TimelineSnapMode::Smart,
            drag: None,
            snap_guide: None,
            panning: false,
            scrollbar_drag: None,
            context_emitter: None,
            known_duration: duration,
        }
    }

    pub(crate) fn set_snap(&mut self, snap: TimelineSnapMode) -> bool {
        if self.snap == snap {
            return false;
        }
        self.snap = snap;
        self.snap_guide = None;
        true
    }

    pub(crate) fn frame_all(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        self.view = TimelineView {
            start: 0.0,
            end: duration,
        };
        self.known_duration = duration;
        self.snap_guide = None;
    }

    fn ensure_duration(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        if (duration - self.known_duration).abs() <= f32::EPSILON {
            return;
        }
        let was_framed =
            self.view.start <= f32::EPSILON && (self.view.end - self.known_duration).abs() < 0.001;
        self.known_duration = duration;
        if was_framed {
            self.frame_all(duration);
        } else {
            self.clamp_view(duration);
        }
    }

    fn zoom_at(&mut self, anchor: f32, factor: f32, duration: f32, tick_rate: u32) {
        let duration = duration.max(0.05);
        let minimum_span = (4.0 / tick_rate.max(1) as f32).max(0.01);
        let old_span = self.view.span();
        let new_span = (old_span * factor).clamp(minimum_span.min(duration), duration);
        let anchor_ratio = ((anchor - self.view.start) / old_span).clamp(0.0, 1.0);
        self.view.start = anchor - new_span * anchor_ratio;
        self.view.end = self.view.start + new_span;
        self.clamp_view(duration);
    }

    fn pan_by(&mut self, delta: f32, duration: f32) {
        self.view.start += delta;
        self.view.end += delta;
        self.clamp_view(duration.max(0.05));
    }

    fn clamp_view(&mut self, duration: f32) {
        let span = self.view.span().min(duration);
        self.view.start = self.view.start.clamp(0.0, (duration - span).max(0.0));
        self.view.end = self.view.start + span;
    }
}

pub(crate) fn spawn_timeline(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|timeline| {
            timeline
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("00:00.000  /  00:02.800"),
                        TimeLabel,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    mini_button(header, "<", TransportAction::StepFrame(-1));
                    mini_button(header, ">", TransportAction::StepFrame(1));
                    mini_button(
                        header,
                        &localizer.text("timeline-frame-all"),
                        TimelineAction::FrameAll,
                    );
                    let snap_options = TimelineSnapMode::ALL
                        .into_iter()
                        .map(|mode| ComboOption {
                            label: localizer.text(mode.message_id()),
                            selected: state.snap == mode,
                            action: TimelineAction::SetSnap(mode),
                        })
                        .collect::<Vec<_>>();
                    spawn_combo_control(
                        header,
                        &localizer.text(state.snap.message_id()),
                        &localizer.text("timeline-snapping-description"),
                        &snap_options,
                        112.0,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(format!(
                            "{} {}  ·  {} {:016x}",
                            session.clock.tick_rate(),
                            localizer.text("timeline-hertz"),
                            localizer.text("timeline-seed"),
                            session.preview_seed
                        )),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    mini_button(header, "-", TransportAction::AdjustPreviewSeed(-1));
                    mini_button(header, "+", TransportAction::AdjustPreviewSeed(1));
                    header.spawn((
                        Text::new(format!(
                            "{} {:.2}s",
                            localizer.text("timeline-duration"),
                            session.playback_duration()
                        )),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    mini_button(header, "-", TimelineAction::AdjustEffectDuration(-0.25));
                    mini_button(header, "+", TimelineAction::AdjustEffectDuration(0.25));
                });
            timeline
                .spawn(Node {
                    flex_grow: 1.0,
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Px(224.0),
                            height: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|labels| {
                        labels
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(25.0),
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                ..default()
                            })
                            .with_children(|toolbar| {
                                toolbar.spawn((
                                    Text::new(localizer.text("timeline-emitters")),
                                    TextFont {
                                        font_size: FontSize::Px(9.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT_FAINT),
                                    Pickable::IGNORE,
                                ));
                                toolbar.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                let add = mini_button(toolbar, "+", ChoreographyAction::AddEmitter);
                                toolbar
                                    .commands()
                                    .entity(add)
                                    .insert(EditorTooltip::description(
                                        localizer.text("edit-add-emitter"),
                                    ));
                            });
                        for (index, emitter) in session.effect.emitters.iter().enumerate() {
                            spawn_emitter_track_header(
                                labels,
                                session,
                                state,
                                localizer,
                                index,
                                emitter.id,
                                &emitter.name,
                                emitter.enabled,
                            );
                        }
                    });
                    body.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineCanvas,
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            position_type: PositionType::Relative,
                            padding: UiRect::top(Val::Px(25.0)),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme::TIMELINE_BG),
                    ))
                    .observe(seek_timeline_on_press)
                    .observe(seek_timeline_on_drag)
                    .with_children(|tracks| {
                        spawn_ruler(tracks);
                        for (index, emitter) in session.effect.emitters.iter().enumerate() {
                            tracks
                                .spawn(Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(31.0),
                                    position_type: PositionType::Relative,
                                    border: UiRect::bottom(Val::Px(1.0)),
                                    ..default()
                                })
                                .with_children(|track| {
                                    track
                                        .spawn((
                                            TimelineClip {
                                                emitter: emitter.id,
                                            },
                                            Node {
                                                position_type: PositionType::Absolute,
                                                left: Val::Percent(0.0),
                                                top: Val::Px(5.0),
                                                width: Val::Percent(1.0),
                                                height: Val::Px(21.0),
                                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                                border: UiRect::all(Val::Px(1.0)),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            },
                                            BackgroundColor(layer_color(index).with_alpha(0.28)),
                                            BorderColor::all(layer_color(index)),
                                        ))
                                        .with_children(|clip| {
                                            clip.spawn((
                                                Button,
                                                EditorNativeControl,
                                                TimelineClipInteraction {
                                                    emitter: emitter.id,
                                                    kind: TimelineDragKind::Move,
                                                },
                                                ChoreographyAction::SelectEmitter(emitter.id),
                                                EntityCursor::System(SystemCursorIcon::Grab),
                                                Node {
                                                    position_type: PositionType::Absolute,
                                                    left: Val::Px(8.0),
                                                    right: Val::Px(8.0),
                                                    top: Val::Px(0.0),
                                                    bottom: Val::Px(0.0),
                                                    ..default()
                                                },
                                                BackgroundColor(Color::NONE),
                                            ))
                                            .observe(begin_timeline_clip_drag)
                                            .observe(move_timeline_clip_drag)
                                            .observe(finish_timeline_clip_drag)
                                            .observe(select_timeline_clip)
                                            .observe(stop_timeline_control_press);
                                            for (kind, left, right) in [
                                                (
                                                    TimelineDragKind::TrimStart,
                                                    Val::Px(0.0),
                                                    Val::Auto,
                                                ),
                                                (
                                                    TimelineDragKind::TrimEnd,
                                                    Val::Auto,
                                                    Val::Px(0.0),
                                                ),
                                            ] {
                                                clip.spawn((
                                                    Button,
                                                    EditorNativeControl,
                                                    TimelineClipInteraction {
                                                        emitter: emitter.id,
                                                        kind,
                                                    },
                                                    ChoreographyAction::SelectEmitter(emitter.id),
                                                    EntityCursor::System(
                                                        SystemCursorIcon::EwResize,
                                                    ),
                                                    Node {
                                                        position_type: PositionType::Absolute,
                                                        left,
                                                        right,
                                                        top: Val::Px(0.0),
                                                        width: Val::Px(8.0),
                                                        height: Val::Percent(100.0),
                                                        align_items: AlignItems::Center,
                                                        justify_content: JustifyContent::Center,
                                                        ..default()
                                                    },
                                                    BackgroundColor(Color::NONE),
                                                ))
                                                .observe(begin_timeline_clip_drag)
                                                .observe(move_timeline_clip_drag)
                                                .observe(finish_timeline_clip_drag)
                                                .observe(select_timeline_clip)
                                                .observe(stop_timeline_control_press)
                                                .with_child((
                                                    Node {
                                                        width: Val::Px(2.0),
                                                        height: Val::Px(13.0),
                                                        ..default()
                                                    },
                                                    BackgroundColor(layer_color(index)),
                                                    Pickable::IGNORE,
                                                ));
                                            }
                                        });
                                });
                        }
                        tracks.spawn((
                            TimelineSnapGuide,
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::ACCENT),
                            Pickable::IGNORE,
                            ZIndex(3),
                        ));
                        tracks.spawn((
                            Playhead,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::PLAYHEAD),
                            Pickable::IGNORE,
                            ZIndex(2),
                        ));
                        tracks
                            .spawn((
                                TimelineScrollbarTrack,
                                RelativeCursorPosition::default(),
                                Node {
                                    display: Display::None,
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(6.0),
                                    right: Val::Px(6.0),
                                    bottom: Val::Px(3.0),
                                    height: Val::Px(10.0),
                                    border_radius: BorderRadius::all(Val::Px(5.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_LIGHT.with_alpha(0.88)),
                                ZIndex(5),
                            ))
                            .with_child((
                                Button,
                                EditorNativeControl,
                                TimelineScrollbarThumb,
                                EntityCursor::System(SystemCursorIcon::Grab),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(0.0),
                                    top: Val::Px(2.0),
                                    width: Val::Percent(100.0),
                                    min_width: Val::Px(20.0),
                                    height: Val::Px(6.0),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::TEXT_FAINT.with_alpha(0.75)),
                            ))
                            .observe(begin_timeline_scrollbar_drag)
                            .observe(move_timeline_scrollbar_drag)
                            .observe(finish_timeline_scrollbar_drag)
                            .observe(stop_timeline_control_press);
                    });
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_emitter_track_header(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
    index: usize,
    emitter: EmitterId,
    name: &str,
    enabled: bool,
) {
    let selected = session.selected_layer().id == emitter;
    let diagnostic = emitter_has_diagnostic(session, index);
    let mut args = FluentArgs::new();
    args.set("name", name);
    let accessible_label = localizer.text_with("timeline-select-emitter", &args);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        EmitterTrackHeader { emitter },
        ChoreographyAction::SelectEmitter(emitter),
        AccessibleLabel(accessible_label),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(31.0),
            padding: UiRect::horizontal(Val::Px(7.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            position_type: PositionType::Relative,
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.55)),
    ));
    if diagnostic {
        header.insert(EmitterTrackDiagnostic);
    }
    if !enabled {
        header.insert(EmitterTrackDisabled);
    }
    header
        .observe(select_timeline_track_header)
        .observe(open_timeline_track_context_menu)
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(4.0),
                    height: Val::Px(19.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(layer_color(index)),
                Pickable::IGNORE,
            ));
            let toggle_label = if enabled { "ON" } else { "OFF" };
            let toggle = mini_button(
                row,
                toggle_label,
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
            );
            row.commands()
                .entity(toggle)
                .insert(EditorTooltip::description(localizer.text(if enabled {
                    "timeline-disable-emitter"
                } else {
                    "timeline-enable-emitter"
                })));
            row.spawn((
                Text::new(format!("{:02}  {name}", index + 1)),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(if enabled {
                    theme::TEXT_MUTED
                } else {
                    theme::TEXT_FAINT
                }),
                Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            if diagnostic {
                row.spawn((
                    Text::new("!"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::ACCENT),
                    EditorTooltip::description(localizer.text("timeline-emitter-diagnostic")),
                ));
            }
            row.spawn((
                EmitterTrackHoverActions,
                Node {
                    display: Display::None,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(2.0),
                    ..default()
                },
            ))
            .with_children(|actions| {
                let duplicate = mini_button(
                    actions,
                    "D",
                    ChoreographyAction::DuplicateEmitter(Some(emitter)),
                );
                actions
                    .commands()
                    .entity(duplicate)
                    .insert(EditorTooltip::description(
                        localizer.text("edit-duplicate-emitter"),
                    ));
                let more = mini_button(
                    actions,
                    "...",
                    ChoreographyAction::ToggleEmitterMenu(emitter),
                );
                actions
                    .commands()
                    .entity(more)
                    .insert(EditorTooltip::description(
                        localizer.text("timeline-more-emitter-actions"),
                    ));
            });
            if state.context_emitter == Some(emitter) {
                spawn_emitter_context_menu(row, localizer, emitter, enabled);
            }
        });
}

fn spawn_emitter_context_menu(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    emitter: EmitterId,
    enabled: bool,
) {
    parent
        .spawn((
            EmitterTrackContextMenu,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(4.0),
                top: Val::Px(27.0),
                width: Val::Px(168.0),
                padding: UiRect::all(Val::Px(4.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::BORDER_BRIGHT),
            ZIndex(20),
        ))
        .with_children(|menu| {
            spawn_feathers_action_button(
                menu,
                &localizer.text(if enabled {
                    "timeline-disable-emitter"
                } else {
                    "timeline-enable-emitter"
                }),
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
                false,
            );
            spawn_feathers_action_button(
                menu,
                &localizer.text("edit-duplicate-emitter"),
                ChoreographyAction::DuplicateEmitter(Some(emitter)),
                false,
            );
            spawn_feathers_action_button(
                menu,
                &localizer.text("edit-delete-emitter"),
                ChoreographyAction::DeleteEmitter(Some(emitter)),
                false,
            );
        });
}

fn emitter_has_diagnostic(session: &EditorSession, index: usize) -> bool {
    let prefix = format!("effect.emitters[{index}]");
    session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(&prefix))
}

fn layer_color(index: usize) -> Color {
    match index % 4 {
        0 => Color::srgb(0.48, 0.31, 0.98),
        1 => Color::srgb(0.17, 0.75, 0.95),
        2 => Color::srgb(0.98, 0.47, 0.21),
        _ => Color::srgb(0.84, 0.29, 0.72),
    }
}

fn spawn_ruler(parent: &mut ChildSpawnerCommands) {
    for index in 0..32 {
        parent
            .spawn((
                TimelineRulerTick(index),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: Val::Percent(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(1.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(theme::BORDER.with_alpha(0.55)),
                Pickable::IGNORE,
            ))
            .with_child((
                Text::new("0.0"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(4.0),
                    top: Val::Px(4.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
    }
}

fn navigate_timeline(
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvases: Query<(&RelativeCursorPosition, &ComputedNode), With<TimelineCanvas>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if state.drag.take().is_some() || state.scrollbar_drag.take().is_some() {
            state.snap_guide = None;
            override_cursor.0 = None;
            **cursor = CursorIcon::System(SystemCursorIcon::Default);
        }
        if state.context_emitter.take().is_some() {
            session.ui_revision += 1;
        }
    }
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let hovered = canvases.iter().find(|(cursor, _)| cursor.cursor_over());

    if buttons.just_pressed(MouseButton::Middle) && hovered.is_some() {
        state.panning = true;
    }
    if buttons.just_released(MouseButton::Middle) {
        state.panning = false;
    }
    if state.panning
        && buttons.pressed(MouseButton::Middle)
        && let Some((_, canvas)) = hovered
    {
        let width = canvas.size().x.max(1.0);
        let delta_time = -pointer_delta.x / width * state.view.span();
        state.pan_by(delta_time, session.playback_duration());
    }

    let scroll = wheel.read().fold(Vec2::ZERO, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.01,
        };
        sum + Vec2::new(event.x, event.y) * scale
    });
    let Some((cursor, _)) = hovered else {
        return;
    };
    if scroll == Vec2::ZERO {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if shift {
        let amount = if scroll.x.abs() > scroll.y.abs() {
            scroll.x
        } else {
            scroll.y
        };
        let span = state.view.span();
        state.pan_by(-amount * span * 0.08, session.playback_duration());
    } else if let Some(position) = cursor.normalized {
        let anchor = state.view.time_at(timeline_cursor_fraction(position.x));
        state.zoom_at(
            anchor,
            0.82_f32.powf(scroll.y),
            session.playback_duration(),
            session.clock.tick_rate(),
        );
    }
}

fn seek_timeline_on_press(
    press: On<Pointer<Press>>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if press.button == PointerButton::Primary {
        if state.context_emitter.take().is_some() {
            session.ui_revision += 1;
        }
        seek_timeline_to_pointer(press.event_target(), &timelines, &state, &mut session);
    }
}

fn seek_timeline_on_drag(
    drag: On<Pointer<Drag>>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: Res<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if drag.button == PointerButton::Primary {
        seek_timeline_to_pointer(drag.event_target(), &timelines, &state, &mut session);
    }
}

fn seek_timeline_to_pointer(
    entity: Entity,
    timelines: &Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: &TimelineState,
    session: &mut EditorSession,
) {
    let Ok(cursor) = timelines.get(entity) else {
        return;
    };
    let Some(position) = cursor.normalized else {
        return;
    };
    session.seek_time(state.view.time_at(timeline_cursor_fraction(position.x)));
}

fn timeline_cursor_fraction(relative_x: f32) -> f32 {
    (relative_x + 0.5).clamp(0.0, 1.0)
}

fn stop_timeline_control_press(mut press: On<Pointer<Press>>) {
    press.propagate(false);
}

fn begin_timeline_clip_drag(
    drag: On<Pointer<DragStart>>,
    targets: Query<&TimelineClipInteraction>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == target.emitter)
    else {
        return;
    };
    state.drag = Some(TimelineDrag {
        emitter: target.emitter,
        kind: target.kind,
        pointer_start: 0.0,
        original_start: emitter.start_time,
        original_duration: emitter.duration,
        current_start: emitter.start_time,
        current_duration: emitter.duration,
    });
    override_cursor.0 = Some(EntityCursor::System(timeline_system_cursor(
        target.kind,
        true,
    )));
    **cursor = timeline_drag_cursor(target.kind, true);
}

fn move_timeline_clip_drag(
    mut drag_event: On<Pointer<Drag>>,
    targets: Query<&TimelineClipInteraction>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.drag else {
        return;
    };
    if drag.emitter != target.emitter || drag.kind != target.kind {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let pointer_time = drag_event.distance.x / width * state.view.span();
    let mut snap_guide = state.snap_guide;
    update_timeline_drag(
        &mut drag,
        pointer_time,
        &session,
        state.snap,
        state.view,
        width,
        &mut snap_guide,
    );
    state.drag = Some(drag);
    state.snap_guide = snap_guide;
}

fn finish_timeline_clip_drag(
    drag_event: On<Pointer<DragEnd>>,
    targets: Query<&TimelineClipInteraction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.drag.take() else {
        return;
    };
    if drag.emitter != target.emitter || drag.kind != target.kind {
        return;
    }
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = timeline_drag_cursor(target.kind, false);
    commit_timeline_drag(&mut session, drag);
    commands.trigger(ChoreographyAction::SelectEmitter(target.emitter));
}

fn commit_timeline_drag(session: &mut EditorSession, drag: TimelineDrag) {
    let changed = (drag.current_start - drag.original_start).abs() > 0.000_1
        || (drag.current_duration - drag.original_duration).abs() > 0.000_1;
    if changed {
        let label = match drag.kind {
            TimelineDragKind::Move => "Moved emitter on timeline",
            TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => {
                "Trimmed emitter on timeline"
            }
        };
        session.set_emitter_timing(
            drag.emitter,
            drag.current_start,
            drag.current_duration,
            label,
        );
    }
}

fn select_timeline_clip(
    click: On<Pointer<Click>>,
    actions: Query<&ChoreographyAction, With<TimelineClipInteraction>>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(*action);
    }
}

fn select_timeline_track_header(
    click: On<Pointer<Click>>,
    headers: Query<&ChoreographyAction, With<EmitterTrackHeader>>,
    mut commands: Commands,
) {
    let Ok(action) = headers.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(*action);
    }
}

fn open_timeline_track_context_menu(
    click: On<Pointer<Click>>,
    headers: Query<&EmitterTrackHeader>,
    mut commands: Commands,
) {
    let Ok(header) = headers.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Secondary {
        commands.trigger(ChoreographyAction::ToggleEmitterMenu(header.emitter));
    }
}

fn update_track_header_hover_actions(
    headers: Query<(&Interaction, &Children), (With<EmitterTrackHeader>, Changed<Interaction>)>,
    mut action_groups: Query<&mut Node, With<EmitterTrackHoverActions>>,
) {
    for (interaction, children) in &headers {
        for child in children.iter() {
            let Ok(mut node) = action_groups.get_mut(child) else {
                continue;
            };
            node.display = if *interaction == Interaction::None {
                Display::None
            } else {
                Display::Flex
            };
        }
    }
}

fn timeline_drag_cursor(kind: TimelineDragKind, active: bool) -> CursorIcon {
    CursorIcon::System(timeline_system_cursor(kind, active))
}

fn timeline_system_cursor(kind: TimelineDragKind, active: bool) -> SystemCursorIcon {
    match kind {
        TimelineDragKind::Move if active => SystemCursorIcon::Grabbing,
        TimelineDragKind::Move => SystemCursorIcon::Grab,
        TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => SystemCursorIcon::EwResize,
    }
}

fn begin_timeline_scrollbar_drag(
    drag: On<Pointer<DragStart>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if !thumbs.contains(drag.event_target()) {
        return;
    }
    state.scrollbar_drag = Some(TimelineScrollbarDrag {
        view_start: state.view.start,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
}

fn move_timeline_scrollbar_drag(
    mut drag: On<Pointer<Drag>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    tracks: Query<&ComputedNode, With<TimelineScrollbarTrack>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if !thumbs.contains(drag.event_target()) {
        return;
    }
    drag.propagate(false);
    let Some(active) = state.scrollbar_drag else {
        return;
    };
    let width = tracks
        .iter()
        .map(|track| track.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let delta = drag.distance.x / width * session.playback_duration();
    let span = state.view.span();
    state.view.start = active.view_start + delta;
    state.view.end = state.view.start + span;
    state.clamp_view(session.playback_duration());
}

fn finish_timeline_scrollbar_drag(
    drag: On<Pointer<DragEnd>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if thumbs.contains(drag.event_target()) {
        state.scrollbar_drag = None;
        override_cursor.0 = None;
        **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    }
}

fn update_timeline_drag(
    drag: &mut TimelineDrag,
    pointer_time: f32,
    session: &EditorSession,
    snap: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
    snap_guide: &mut Option<f32>,
) {
    let effect_duration = session.playback_duration();
    let minimum_duration = (1.0 / session.clock.tick_rate().max(1) as f32).max(0.001);
    let pointer_delta = pointer_time - drag.pointer_start;
    *snap_guide = None;
    match drag.kind {
        TimelineDragKind::Move => {
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(0.0, (effect_duration - drag.original_duration).max(0.0));
            let (start, guide) = snap_moved_timing(
                unsnapped,
                drag.original_duration,
                drag.emitter,
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start =
                start.clamp(0.0, (effect_duration - drag.original_duration).max(0.0));
            drag.current_duration = drag.original_duration;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimStart => {
            let end = drag.original_start + drag.original_duration;
            let unsnapped =
                (drag.original_start + pointer_delta).clamp(0.0, (end - minimum_duration).max(0.0));
            let (start, guide) =
                snap_timeline_boundary(unsnapped, drag.emitter, session, snap, view, canvas_width);
            drag.current_start = start.clamp(0.0, end - minimum_duration);
            drag.current_duration = end - drag.current_start;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimEnd => {
            let unsnapped = (drag.original_start + drag.original_duration + pointer_delta)
                .clamp(drag.original_start + minimum_duration, effect_duration);
            let (end, guide) =
                snap_timeline_boundary(unsnapped, drag.emitter, session, snap, view, canvas_width);
            let end = end.clamp(drag.original_start + minimum_duration, effect_duration);
            drag.current_start = drag.original_start;
            drag.current_duration = end - drag.original_start;
            *snap_guide = guide;
        }
    }
}
