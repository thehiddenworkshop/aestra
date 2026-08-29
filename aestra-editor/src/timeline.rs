use crate::feathers::color_picker::{ColorPickerLabels, spawn_color_picker};
use crate::feathers::scroll::{spawn_horizontal_scrollbar, spawn_vertical_scrollbar};
use crate::library::ProjectEffectRow;
use crate::{
    ComboOption, CurvesState, DockPanel, EditorNativeControl, EditorTooltip, FeathersActionButton,
    KeyboardNavigableList, KeyboardNavigableListRow, Localizer, MenuState, ModulePaletteState,
    PendingFeathersActivation, TransportAction, WorkspaceLayout, mini_button, reveal_dock_panel,
    session::EditorSession, spawn_combo_control, spawn_feathers_action_button, spawn_text_input,
    theme, ui_shell,
};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_bevy::EmitterId;
#[cfg(test)]
use bevy::ui_widgets::{ControlOrientation, Scrollbar};
use bevy::{
    feathers::{
        controls::ButtonVariant,
        cursor::{EntityCursor, OverrideCursor},
    },
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    input_focus::{InputFocus, tab_navigation::TabIndex},
    picking::{
        events::{Click, Drag, DragDrop, DragEnd, DragEnter, DragLeave, DragStart, Pointer, Press},
        pointer::PointerButton,
    },
    prelude::*,
    text::EditableText,
    ui::{RelativeCursorPosition, Selected},
    ui_widgets::{
        Activate, ListBox, ListItem, ScrollArea, ValueChange,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
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
            .add_observer(activate_timeline_track_entry)
            .add_observer(handle_timeline_track_name_change)
            .add_observer(handle_timeline_track_color_change)
            .add_observer(reject_project_effect_drop)
            .add_observer(execute_choreography_action)
            .add_systems(
                Update,
                (
                    choreography_keyboard_input,
                    navigate_timeline,
                    dismiss_timeline_color_picker,
                )
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
                    sync_timeline_vertical_scroll,
                    sync_timeline_horizontal_scroll,
                    update_track_header_hover_actions,
                    tick_invalid_timeline_drop_feedback,
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
    ToggleEmitterSolo(EmitterId),
    ToggleEmitterMenu(EmitterId),
    ToggleEmitterColorPicker(EmitterId),
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

fn activate_timeline_track_entry(
    change: On<ValueChange<Entity>>,
    lists: Query<(), With<KeyboardNavigableList>>,
    actions: Query<&ChoreographyAction, With<EmitterTrackHeader>>,
    mut commands: Commands,
) {
    if lists.contains(change.source)
        && let Ok(action) = actions.get(change.value)
    {
        commands.trigger(*action);
    }
}

fn handle_timeline_track_name_change(
    change: On<ValueChange<String>>,
    controls: Query<&TimelineTrackNameControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let name = change.value.trim();
    if name.is_empty() {
        session.status = localizer.text("timeline-emitter-name-required");
        session.ui_revision += 1;
        return;
    }
    if session.set_emitter_name(control.emitter, name) {
        let mut args = FluentArgs::new();
        args.set("name", name);
        session.status = localizer.text_with("timeline-emitter-renamed", &args);
    }
}

fn handle_timeline_track_color_change(
    change: On<ValueChange<Option<[f32; 4]>>>,
    controls: Query<&EmitterTrackColorPicker>,
    mut swatches: Query<(&EmitterTrackColorSwatch, &mut BackgroundColor)>,
    mut clips: Query<(&TimelineClip, &mut BackgroundColor), Without<EmitterTrackColorSwatch>>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if let Some([red, green, blue, alpha]) = change.value {
        let preview = Color::srgba(red, green, blue, alpha);
        for (swatch, mut color) in &mut swatches {
            if swatch.emitter == control.emitter {
                color.0 = preview;
            }
        }
        for (clip, mut color) in &mut clips {
            if clip.emitter == control.emitter {
                color.0 = preview;
            }
        }
    }
    if !change.is_final {
        return;
    }
    if session.set_emitter_display_color(control.emitter, change.value) {
        session.status = localizer.text("timeline-emitter-color-updated");
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
        state.color_picker_emitter = None;
        state.context_emitter = (state.context_emitter != Some(emitter)).then_some(emitter);
        if session.ui_revision == revision {
            session.ui_revision += 1;
        }
        return;
    }

    if let ChoreographyAction::ToggleEmitterColorPicker(emitter) = *action {
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
        state.context_emitter = None;
        state.color_picker_emitter =
            (state.color_picker_emitter != Some(emitter)).then_some(emitter);
        if session.ui_revision == revision {
            session.ui_revision += 1;
        }
        return;
    }

    let revision = session.ui_revision;
    let closed_context_menu =
        state.context_emitter.take().is_some() | state.color_picker_emitter.take().is_some();
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
        ChoreographyAction::ToggleEmitterSolo(emitter) => {
            if session.toggle_preview_solo(emitter) {
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEmitterMenu(_)
        | ChoreographyAction::ToggleEmitterColorPicker(_) => unreachable!(),
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

    #[test]
    fn color_picker_prefers_upward_alignment_near_the_window_bottom() {
        let positions = color_picker_popover_positions();

        assert_eq!(positions[0].side, PopoverSide::Right);
        assert_eq!(positions[0].align, PopoverAlign::End);
        assert!(positions.iter().any(|position| {
            position.side == PopoverSide::Top && position.align == PopoverAlign::Start
        }));
        assert!(positions.iter().any(|position| {
            position.side == PopoverSide::Bottom && position.align == PopoverAlign::Start
        }));
    }
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
    fn trim_handles_only_appear_for_real_boundaries_inside_the_view() {
        let view = TimelineView {
            start: 2.0,
            end: 2.1,
        };
        assert!(!timeline_boundary_is_visible(0.0, view));
        assert!(timeline_boundary_is_visible(2.0, view));
        assert!(timeline_boundary_is_visible(2.05, view));
        assert!(timeline_boundary_is_visible(2.1, view));
        assert!(!timeline_boundary_is_visible(2.8, view));
    }

    #[test]
    fn timeline_drag_converts_screen_pixels_to_logical_ui_units() {
        assert_eq!(screen_distance_to_logical(24.0, 1.0), 24.0);
        assert_eq!(screen_distance_to_logical(24.0, 1.5), 16.0);
        assert_eq!(screen_distance_to_logical(-30.0, 2.0), -15.0);
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
    fn timeline_wheel_routes_scroll_pan_and_zoom_like_a_track_editor() {
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, false, false),
            TimelineWheelIntent::ScrollTracks(-21.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, false, true),
            TimelineWheelIntent::PanTime(-1.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, true, false),
            TimelineWheelIntent::ZoomTime(-1.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(2.0, 0.25), 5.25, false, false),
            TimelineWheelIntent::PanTime(2.0)
        );
    }

    #[test]
    fn synchronized_vertical_scroll_prefers_tracks_and_clamps_to_overflow() {
        assert_eq!(resolved_vertical_scroll(12.0, None, None, 90.0), 12.0);
        assert_eq!(resolved_vertical_scroll(12.0, Some(28.0), None, 90.0), 28.0);
        assert_eq!(
            resolved_vertical_scroll(12.0, Some(28.0), Some(44.0), 90.0),
            44.0
        );
        assert_eq!(
            resolved_vertical_scroll(12.0, None, Some(144.0), 90.0),
            90.0
        );
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
            (update_timeline_visuals, sync_timeline_horizontal_scroll).chain(),
        );

        app.update();
    }

    #[test]
    fn rejected_project_effect_drop_is_feedback_only() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let original = session.effect.clone();
        let original_revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(reject_project_effect_drop);
        let feedback = app
            .world_mut()
            .spawn((TimelineInvalidDropFeedback::default(), Node::default()))
            .id();

        app.world_mut().trigger(RejectProjectEffectDrop);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original);
        assert_eq!(session.ui_revision, original_revision);
        assert!(!session.dirty);
        assert!(!session.can_undo());
        assert_eq!(
            session.status,
            "This effect cannot be placed on the timeline yet"
        );
        let feedback_state = app
            .world()
            .get::<TimelineInvalidDropFeedback>(feedback)
            .unwrap();
        assert!(feedback_state.rejected);
        assert!(!feedback_state.timer.is_paused());
        assert_eq!(
            app.world().get::<Node>(feedback).unwrap().display,
            Display::Flex
        );
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
    fn authored_names_and_display_color_are_projected_into_the_timeline() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        assert!(session.set_effect_name("Renamed Effect"));
        assert!(session.set_selected_emitter_name("Renamed Emitter"));
        let emitter = session.selected_layer().id;
        let authored_color = [0.28, 0.78, 0.45, 1.0];
        assert!(session.set_emitter_display_color(emitter, Some(authored_color)));
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

        let heading = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Text, With<TimelineEffectHeading>>();
            query.single(world).unwrap().0.clone()
        };
        assert!(heading.contains("Renamed Effect"));
        assert!(heading.ends_with(" · EMITTERS"));
        let renamed_track = {
            let world = app.world_mut();
            let mut controls = world.query::<(&TimelineTrackNameControl, &Children)>();
            let mut names = world.query::<&EditableText>();
            controls
                .iter(world)
                .find(|(control, _)| control.emitter == emitter)
                .and_then(|(_, children)| {
                    children
                        .iter()
                        .find_map(|child| names.get(world, child).ok())
                })
                .map(|name| name.value().to_string())
        };
        assert_eq!(renamed_track.as_deref(), Some("Renamed Emitter"));
        let chip_color = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<&BackgroundColor, With<EmitterTrackColorSwatch>>();
            query
                .iter(world)
                .find(|color| color.0 == Color::srgba(0.28, 0.78, 0.45, 1.0))
                .map(|color| color.0)
                .unwrap()
        };
        assert_eq!(chip_color, Color::srgba(0.28, 0.78, 0.45, 1.0));
    }

    #[test]
    fn inline_track_name_edit_is_undoable_and_rejects_empty_names() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let emitter = session.selected_layer().id;
        let original = session.selected_layer().name.clone();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(handle_timeline_track_name_change);
        let control = app
            .world_mut()
            .spawn(TimelineTrackNameControl { emitter })
            .id();

        app.world_mut().trigger(ValueChange {
            source: control,
            value: "Timeline Rename".to_owned(),
            is_final: true,
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .name,
            "Timeline Rename"
        );

        app.world_mut().trigger(ValueChange {
            source: control,
            value: "   ".to_owned(),
            is_final: true,
        });
        app.update();
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(session.selected_layer().name, "Timeline Rename");
        assert_eq!(session.status, "An emitter track name is required");
        session.undo();
        assert_eq!(session.selected_layer().name, original);
    }

    #[test]
    fn timeline_color_picker_targets_the_track_and_commits_one_semantic_edit() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let emitter = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.add_observer(handle_timeline_track_color_change);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterColorPicker(emitter));
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().color_picker_emitter,
            Some(emitter)
        );
        assert_eq!(
            app.world().resource::<EditorSession>().selected_layer().id,
            emitter
        );

        let picker = app
            .world_mut()
            .spawn(EmitterTrackColorPicker { emitter })
            .id();
        let swatch = app
            .world_mut()
            .spawn((
                EmitterTrackColorSwatch { emitter },
                BackgroundColor(Color::BLACK),
            ))
            .id();
        let color = [0.13, 0.62, 0.91, 1.0];
        app.world_mut().trigger(ValueChange {
            source: picker,
            value: Some(color),
            is_final: false,
        });
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(swatch).unwrap().0,
            Color::srgba(color[0], color[1], color[2], color[3])
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .display_color,
            None
        );
        assert!(!app.world().resource::<EditorSession>().can_undo());

        app.world_mut().trigger(ValueChange {
            source: picker,
            value: Some(color),
            is_final: true,
        });
        app.update();

        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(session.selected_layer().display_color, Some(color));
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.selected_layer().display_color, None);
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
        let solo = session.effect.emitters[0].id;
        assert!(session.toggle_preview_solo(solo));
        session.diagnostics.push(aestra_bevy::Diagnostic::error(
            aestra_bevy::DiagnosticCode::InvalidTiming,
            "effect.emitters[1].duration",
            "test diagnostic",
        ));
        let emitter_count = session.effect.emitters.len();
        let duration = session.playback_duration();
        let mut app = App::new();
        let mut timeline = TimelineState::framed(duration);
        timeline.vertical_scroll = 72.0;
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .insert_resource(session)
        .insert_resource(timeline)
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
                Has<ListItem>,
                Has<KeyboardNavigableListRow>,
                Has<Selected>,
            )>();
            query
                .iter(world)
                .map(
                    |(header, action, label, button, list_item, keyboard_row, selected)| {
                        (
                            header.emitter,
                            *action,
                            label.0.clone(),
                            button,
                            list_item,
                            keyboard_row,
                            selected,
                        )
                    },
                )
                .collect::<Vec<_>>()
        };
        let clips = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &TimelineClipInteraction,
                &ChoreographyAction,
                &AccessibleLabel,
                Has<EditorTooltip>,
            )>();
            query
                .iter(world)
                .filter(|(clip, _, _, _)| clip.kind == TimelineDragKind::Move)
                .map(|(clip, action, label, tooltip)| {
                    (clip.emitter, *action, label.0.clone(), tooltip)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(headers.len(), emitter_count);
        assert_eq!(clips.len(), emitter_count);
        assert_eq!(headers.iter().filter(|header| header.6).count(), 1);
        for (emitter, action, label, button, list_item, keyboard_row, _) in headers {
            assert!(button);
            assert!(list_item);
            assert!(keyboard_row);
            assert_eq!(action, ChoreographyAction::SelectEmitter(emitter));
            assert_eq!(
                clips.iter().find(|clip| clip.0 == emitter).unwrap().1,
                action
            );
            let clip = clips.iter().find(|clip| clip.0 == emitter).unwrap();
            assert!(!clip.2.is_empty());
            assert!(clip.3);
            assert!(!label.is_empty());
        }
        let track_controls = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &ChoreographyAction,
                &AccessibleLabel,
                Has<EditorTooltip>,
                &ButtonVariant,
                Has<Selected>,
            )>();
            query
                .iter(world)
                .filter(|(action, _, _, _, _)| {
                    matches!(
                        action,
                        ChoreographyAction::SetEmitterEnabled { .. }
                            | ChoreographyAction::ToggleEmitterSolo(_)
                    )
                })
                .map(|(action, label, tooltip, variant, selected)| {
                    (*action, label.0.clone(), tooltip, variant.clone(), selected)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(track_controls.len(), emitter_count * 2);
        assert!(
            track_controls
                .iter()
                .all(|(_, label, tooltip, _, _)| *tooltip && label != "M" && label != "S")
        );
        assert_eq!(
            track_controls
                .iter()
                .filter(|(_, _, _, variant, selected)| {
                    *selected && *variant == ButtonVariant::Primary
                })
                .count(),
            2
        );
        let timeline_icon_controls = {
            let world = app.world_mut();
            let mut transport =
                world.query::<(&TransportAction, &AccessibleLabel, Has<EditorTooltip>)>();
            transport
                .iter(world)
                .map(|(_, label, tooltip)| (label.0.clone(), tooltip))
                .collect::<Vec<_>>()
        };
        assert_eq!(timeline_icon_controls.len(), 4);
        assert!(timeline_icon_controls.iter().all(|(label, tooltip)| {
            *tooltip && !matches!(label.as_str(), "<" | ">" | "+" | "-")
        }));
        let clip_controls = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &TimelineClipInteraction,
                &AccessibleLabel,
                Has<EditorTooltip>,
            )>();
            query
                .iter(world)
                .map(|(_, label, tooltip)| (label.0.clone(), tooltip))
                .collect::<Vec<_>>()
        };
        assert_eq!(clip_controls.len(), emitter_count * 3);
        assert!(
            clip_controls
                .iter()
                .all(|(label, tooltip)| *tooltip && !label.is_empty())
        );
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

        let (panes, track_target) = {
            let world = app.world_mut();
            let mut query = world.query::<(Entity, &TimelineVerticalPane, &ScrollPosition)>();
            let panes = query
                .iter(world)
                .map(|(entity, kind, position)| (entity, *kind, position.y))
                .collect::<Vec<_>>();
            let track_target = panes
                .iter()
                .find(|(_, kind, _)| *kind == TimelineVerticalPane::Tracks)
                .map(|(entity, _, _)| *entity)
                .unwrap();
            (panes, track_target)
        };
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().all(|(_, _, offset)| *offset == 72.0));
        let header_navigation = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(
                Has<ListBox>,
                Has<ScrollArea>,
                Has<KeyboardNavigableList>,
                Has<TabIndex>,
            ), With<TimelineVerticalPane>>();
            query.iter(world).find(|(list, _, _, _)| *list).unwrap()
        };
        assert_eq!(header_navigation, (true, true, true, true));
        let horizontal_target = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<Entity, With<TimelineHorizontalScrollViewport>>();
            query.single(world).unwrap()
        };
        let scrollbar_targets = {
            let world = app.world_mut();
            let mut query = world.query::<&Scrollbar>();
            query
                .iter(world)
                .map(|scrollbar| (scrollbar.orientation, scrollbar.target))
                .collect::<Vec<_>>()
        };
        assert!(scrollbar_targets.contains(&(ControlOrientation::Vertical, track_target)));
        assert!(scrollbar_targets.contains(&(ControlOrientation::Horizontal, horizontal_target)));
        assert_eq!(scrollbar_targets.len(), 2);

        let constrained_ancestors = {
            let world = app.world_mut();
            let mut bodies = world.query_filtered::<&Node, With<TimelineBody>>();
            let body = bodies.single(world).unwrap();
            let body_constraints = (body.min_width, body.min_height, body.overflow);
            let mut headers = world.query_filtered::<&Node, With<TimelineHeaderColumn>>();
            let header = headers.single(world).unwrap();
            let header_constraints = (header.min_width, header.min_height);
            let mut canvases = world.query_filtered::<&Node, With<TimelineCanvas>>();
            let canvas = canvases.single(world).unwrap();
            let canvas_constraints = (canvas.min_width, canvas.min_height);
            (body_constraints, header_constraints, canvas_constraints)
        };
        assert_eq!(
            constrained_ancestors,
            (
                (Val::Px(0.0), Val::Px(0.0), Overflow::clip()),
                (Val::Px(0.0), Val::Px(0.0)),
                (Val::Px(0.0), Val::Px(0.0)),
            )
        );

        let (
            gutter_height,
            scrollbar_position,
            scrollbar_height,
            scrollbar_padding,
            horizontal_scrollbar_gutter,
            vertical_gutter_width,
            vertical_gutter_border,
        ) = {
            let world = app.world_mut();
            let mut gutters = world.query_filtered::<&Node, With<TimelineHorizontalGutter>>();
            let gutter_height = gutters.single(world).unwrap().height;
            let mut scrollbars = world.query::<(&Scrollbar, &Node)>();
            let (_, scrollbar) = scrollbars
                .iter(world)
                .find(|(scrollbar, _)| scrollbar.orientation == ControlOrientation::Horizontal)
                .unwrap();
            let scrollbar_layout = (scrollbar.position_type, scrollbar.height, scrollbar.padding);
            let mut scrollbar_gutters =
                world.query_filtered::<&Node, With<TimelineHorizontalScrollbarGutter>>();
            let horizontal_scrollbar_gutter = scrollbar_gutters.single(world).unwrap();
            let horizontal_scrollbar_gutter = (
                horizontal_scrollbar_gutter.height,
                horizontal_scrollbar_gutter.border.top,
            );
            let mut vertical_gutters =
                world.query_filtered::<&Node, With<TimelineVerticalScrollbarGutter>>();
            let vertical_gutter = vertical_gutters.single(world).unwrap();
            (
                gutter_height,
                scrollbar_layout.0,
                scrollbar_layout.1,
                scrollbar_layout.2,
                horizontal_scrollbar_gutter,
                vertical_gutter.width,
                vertical_gutter.border.left,
            )
        };
        assert_eq!(gutter_height, Val::Px(15.0));
        assert_eq!(scrollbar_position, PositionType::Relative);
        assert_eq!(scrollbar_height, Val::Px(10.0));
        assert_eq!(scrollbar_padding.top, Val::Px(3.0));
        assert_eq!(scrollbar_padding.bottom, Val::Px(3.0));
        assert_eq!(horizontal_scrollbar_gutter, (Val::Px(15.0), Val::Px(1.0)));
        assert_eq!(vertical_gutter_width, Val::Px(15.0));
        assert_eq!(vertical_gutter_border, Val::Px(1.0));
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
    fn track_list_value_change_selects_the_emitter_through_its_semantic_action() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.add_observer(activate_timeline_track_entry);
        let list = app.world_mut().spawn(KeyboardNavigableList).id();
        let row = app
            .world_mut()
            .spawn((
                EmitterTrackHeader { emitter: target },
                ChoreographyAction::SelectEmitter(target),
            ))
            .id();

        app.world_mut().trigger(ValueChange::<Entity> {
            source: list,
            value: row,
            is_final: true,
        });
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.selection.emitter(&session.effect), Some(target));
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
    fn emitter_solo_is_preview_only_and_isolates_runtime_output() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = session.effect.emitters[1].id;
        let original_effect = session.effect.clone();
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterSolo(target));
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.solo_emitter, Some(target));
        assert_eq!(session.effect, original_effect);
        assert!(!session.dirty);
        let preview = session.preview.as_ref().unwrap().effect();
        assert!(
            preview
                .emitters
                .iter()
                .find(|emitter| emitter.source == target)
                .unwrap()
                .enabled
        );
        assert!(
            preview
                .emitters
                .iter()
                .filter(|emitter| emitter.source != target)
                .all(|emitter| !emitter.enabled)
        );

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterSolo(target));
        app.update();
        assert_eq!(app.world().resource::<EditorSession>().solo_emitter, None);
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
    mut clips: Query<
        (&TimelineClip, &mut Node),
        (Without<Playhead>, Without<TimelineClipInteraction>),
    >,
    mut clip_controls: Query<
        (&TimelineClipInteraction, &mut Node),
        (Without<TimelineClip>, Without<Playhead>),
    >,
    mut playheads: Query<
        &mut Node,
        (
            With<Playhead>,
            Without<TimelineClip>,
            Without<TimelineClipInteraction>,
        ),
    >,
    mut guides: Query<
        &mut Node,
        (
            With<TimelineSnapGuide>,
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineRulerTick>,
            Without<TimelineClipInteraction>,
        ),
    >,
    mut ticks: Query<
        (&TimelineRulerTick, &Children, &mut Node),
        (
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineSnapGuide>,
            Without<TimelineClipInteraction>,
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

    for (control, mut node) in &mut clip_controls {
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == control.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let (start, duration) = state
            .drag
            .filter(|drag| drag.emitter == control.emitter)
            .map_or((emitter.start_time, emitter.duration), |drag| {
                (drag.current_start, drag.current_duration)
            });
        let boundary_visible = match control.kind {
            TimelineDragKind::Move => true,
            TimelineDragKind::TrimStart => timeline_boundary_is_visible(start, view),
            TimelineDragKind::TrimEnd => timeline_boundary_is_visible(start + duration, view),
        };
        node.display = if boundary_visible {
            Display::Flex
        } else {
            Display::None
        };
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

fn sync_timeline_horizontal_scroll(
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut viewports: Query<
        (&ComputedNode, &mut ScrollPosition),
        With<TimelineHorizontalScrollViewport>,
    >,
    mut contents: Query<
        &mut Node,
        (
            With<TimelineHorizontalScrollContent>,
            Without<TimelineHorizontalGutter>,
            Without<TimelineHorizontalScrollbarGutter>,
        ),
    >,
    mut gutters: Query<
        &mut Node,
        (
            Or<(
                With<TimelineHorizontalGutter>,
                With<TimelineHorizontalScrollbarGutter>,
            )>,
            Without<TimelineHorizontalScrollContent>,
        ),
    >,
) {
    let duration = session.playback_duration().max(0.05);
    let span = state.view.span().clamp(0.001, duration);
    let overflow = span < duration * 0.999;
    for mut content in &mut contents {
        content.width = Val::Percent((duration / span * 100.0).clamp(100.0, 100_000.0));
    }
    for mut gutter in &mut gutters {
        gutter.display = if overflow {
            Display::Flex
        } else {
            Display::None
        };
    }

    let Ok((computed, mut position)) = viewports.single_mut() else {
        return;
    };
    let maximum = (computed.content_size().x - computed.size().x).max(0.0);
    let scrollable_time = (duration - span).max(0.0);
    if !position.is_added() && position.is_changed() && maximum > 0.5 {
        let start = (position.x / maximum).clamp(0.0, 1.0) * scrollable_time;
        state.view.start = start;
        state.view.end = start + span;
    }
    let desired = if scrollable_time > 0.0 && maximum > 0.5 {
        (state.view.start / scrollable_time).clamp(0.0, 1.0) * maximum
    } else {
        0.0
    };
    if (position.x - desired).abs() > 0.01 || position.y.abs() > 0.01 {
        position.0 = Vec2::new(desired, 0.0);
    }
}

fn sync_timeline_vertical_scroll(
    mut state: ResMut<TimelineState>,
    mut panes: Query<(&TimelineVerticalPane, &ComputedNode, &mut ScrollPosition)>,
    mut gutters: Query<&mut Node, With<TimelineVerticalScrollbarGutter>>,
) {
    let mut changed_header = None;
    let mut changed_tracks = None;
    for (kind, computed, position) in &mut panes {
        if position.is_added() || !position.is_changed() || computed.size().y <= 0.5 {
            continue;
        }
        let maximum = (computed.content_size().y - computed.size().y).max(0.0);
        let value = position.y.clamp(0.0, maximum);
        match kind {
            TimelineVerticalPane::Headers => changed_header = Some(value),
            TimelineVerticalPane::Tracks => changed_tracks = Some(value),
        }
    }

    let maximum = panes
        .iter()
        .filter(|(kind, computed, _)| {
            **kind == TimelineVerticalPane::Tracks && computed.size().y > 0.5
        })
        .map(|(_, computed, _)| (computed.content_size().y - computed.size().y).max(0.0))
        .next()
        .or_else(|| {
            panes
                .iter()
                .filter(|(kind, computed, _)| {
                    **kind == TimelineVerticalPane::Headers && computed.size().y > 0.5
                })
                .map(|(_, computed, _)| (computed.content_size().y - computed.size().y).max(0.0))
                .next()
        });
    let Some(maximum) = maximum else {
        return;
    };
    for mut gutter in &mut gutters {
        gutter.display = if maximum > 0.5 {
            Display::Flex
        } else {
            Display::None
        };
    }
    state.vertical_scroll = resolved_vertical_scroll(
        state.vertical_scroll,
        changed_header,
        changed_tracks,
        maximum,
    );
    for (_, _, mut position) in &mut panes {
        if (position.y - state.vertical_scroll).abs() > 0.01 || position.x.abs() > 0.01 {
            position.0 = Vec2::new(0.0, state.vertical_scroll);
        }
    }
}

fn resolved_vertical_scroll(
    previous: f32,
    changed_header: Option<f32>,
    changed_tracks: Option<f32>,
    maximum: f32,
) -> f32 {
    changed_tracks
        .or(changed_header)
        .unwrap_or(previous)
        .clamp(0.0, maximum.max(0.0))
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

#[derive(Component)]
struct TimelineInvalidDropFeedback {
    timer: Timer,
    rejected: bool,
}

impl Default for TimelineInvalidDropFeedback {
    fn default() -> Self {
        let mut timer = Timer::from_seconds(1.6, TimerMode::Once);
        timer.pause();
        Self {
            timer,
            rejected: false,
        }
    }
}

#[derive(Event)]
struct RejectProjectEffectDrop;

#[derive(Component)]
struct TimelineBody;

#[derive(Component)]
struct TimelineHeaderColumn;

#[derive(Component)]
struct TimelineEffectHeading;

#[derive(Component)]
struct TimelineHorizontalGutter;

#[derive(Component)]
struct TimelineHorizontalScrollbarGutter;

#[derive(Component)]
struct TimelineVerticalScrollbarGutter;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineVerticalPane {
    Headers,
    Tracks,
}

#[derive(Component, Clone, Copy)]
struct EmitterTrackHeader {
    emitter: EmitterId,
}

#[derive(Component, Clone, Copy)]
struct TimelineTrackNameControl {
    emitter: EmitterId,
}

#[derive(Component)]
struct EmitterTrackDiagnostic;

#[derive(Component)]
struct EmitterTrackDisabled;

#[derive(Component)]
struct EmitterTrackColorChip;

#[derive(Component)]
struct EmitterTrackColorSwatch {
    emitter: EmitterId,
}

#[derive(Component)]
struct EmitterTrackColorPicker {
    emitter: EmitterId,
}

#[derive(Component)]
struct EmitterTrackColorPickerPopover;

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
struct TimelineHorizontalScrollViewport;

#[derive(Component)]
struct TimelineHorizontalScrollContent;

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
    context_emitter: Option<EmitterId>,
    color_picker_emitter: Option<EmitterId>,
    vertical_scroll: f32,
    known_duration: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::framed(1.0)
    }
}

impl TimelineState {
    pub(crate) fn framed(duration: f32) -> Self {
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
            context_emitter: None,
            color_picker_emitter: None,
            vertical_scroll: 0.0,
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

fn describe_timeline_control(
    parent: &mut ChildSpawnerCommands,
    entity: Entity,
    description: String,
) {
    parent.commands().entity(entity).insert((
        AccessibleLabel(description.clone()),
        EditorTooltip::description(description),
    ));
}

fn emitter_timing_label(localizer: &Localizer, message_id: &str, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    localizer.text_with(message_id, &args)
}

fn timeline_effect_heading(localizer: &Localizer, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    localizer.text_with("timeline-effect-emitters", &args)
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
                        min_width: Val::Px(0.0),
                        height: Val::Px(38.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(14.0),
                        overflow: Overflow::clip(),
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
                    let previous = mini_button(header, "<", TransportAction::StepFrame(-1));
                    describe_timeline_control(
                        header,
                        previous,
                        localizer.text("timeline-previous-frame"),
                    );
                    let next = mini_button(header, ">", TransportAction::StepFrame(1));
                    describe_timeline_control(
                        header,
                        next,
                        localizer.text("timeline-next-frame"),
                    );
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
                    let decrease_seed =
                        mini_button(header, "-", TransportAction::AdjustPreviewSeed(-1));
                    describe_timeline_control(
                        header,
                        decrease_seed,
                        localizer.text("timeline-decrease-seed"),
                    );
                    let increase_seed =
                        mini_button(header, "+", TransportAction::AdjustPreviewSeed(1));
                    describe_timeline_control(
                        header,
                        increase_seed,
                        localizer.text("timeline-increase-seed"),
                    );
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
                    let decrease_duration =
                        mini_button(header, "-", TimelineAction::AdjustEffectDuration(-0.25));
                    describe_timeline_control(
                        header,
                        decrease_duration,
                        localizer.text("timeline-decrease-duration"),
                    );
                    let increase_duration =
                        mini_button(header, "+", TimelineAction::AdjustEffectDuration(0.25));
                    describe_timeline_control(
                        header,
                        increase_duration,
                        localizer.text("timeline-increase-duration"),
                    );
                });
            timeline
                .spawn((
                    TimelineBody,
                    Node {
                        flex_grow: 1.0,
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|body| {
                    body.spawn((
                        TimelineHeaderColumn,
                        Node {
                            width: Val::Px(224.0),
                            min_width: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            min_height: Val::Px(0.0),
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
                                flex_shrink: 0.0,
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                ..default()
                            })
                            .with_children(|toolbar| {
                                toolbar.spawn((
                                    Text::new(timeline_effect_heading(localizer, &session.effect.name)),
                                    TimelineEffectHeading,
                                    TextFont {
                                        font_size: FontSize::Px(9.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT_FAINT),
                                    TextLayout::no_wrap(),
                                    Node {
                                        min_width: Val::Px(0.0),
                                        flex_shrink: 1.0,
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                ));
                                toolbar.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                let add = mini_button(toolbar, "+", ChoreographyAction::AddEmitter);
                                describe_timeline_control(
                                    toolbar,
                                    add,
                                    localizer.text("edit-add-emitter"),
                                );
                            });
                        labels
                            .spawn((
                                TimelineVerticalPane::Headers,
                                RelativeCursorPosition::default(),
                                ScrollPosition(Vec2::new(0.0, state.vertical_scroll)),
                                ScrollArea,
                                ListBox,
                                KeyboardNavigableList,
                                TabIndex(0),
                                AccessibleLabel(localizer.text("timeline-emitters")),
                                Node {
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    overflow: Overflow::scroll_y(),
                                    scrollbar_width: 0.0,
                                    ..default()
                                },
                            ))
                            .with_children(|headers| {
                                for (index, emitter) in session.effect.emitters.iter().enumerate() {
                                    spawn_emitter_track_header(
                                        headers,
                                        session,
                                        state,
                                        localizer,
                                        index,
                                        emitter.id,
                                        &emitter.name,
                                        emitter.enabled,
                                        emitter.display_color,
                                    );
                                }
                            });
                        labels.spawn((
                            TimelineHorizontalGutter,
                            Node {
                                display: if state.view.span()
                                    < session.playback_duration().max(0.05) * 0.999
                                {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                width: Val::Percent(100.0),
                                height: Val::Px(15.0),
                                flex_shrink: 0.0,
                                border: UiRect::top(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL_DARK),
                            BorderColor::all(theme::BORDER),
                        ));
                    });
                    let mut vertical_scroll_target = None;
                    body.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineCanvas,
                        AccessibleLabel(localizer.text("timeline-canvas-accessible")),
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            min_height: Val::Px(0.0),
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
                    .observe(show_invalid_timeline_drop_feedback)
                    .observe(hide_invalid_timeline_drop_feedback)
                    .observe(drop_project_effect_on_timeline)
                    .with_children(|tracks| {
                        spawn_ruler(tracks);
                        vertical_scroll_target =
                            Some(
                                tracks
                                    .spawn((
                                        TimelineVerticalPane::Tracks,
                                        ScrollPosition(Vec2::new(0.0, state.vertical_scroll)),
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_grow: 1.0,
                                            min_height: Val::Px(0.0),
                                            flex_direction: FlexDirection::Column,
                                            overflow: Overflow::scroll_y(),
                                            scrollbar_width: 0.0,
                                            ..default()
                                        },
                                    ))
                                    .with_children(|rows| {
                                        for (index, emitter) in
                                            session.effect.emitters.iter().enumerate()
                                        {
                                            rows.spawn(Node {
                                                width: Val::Percent(100.0),
                                                height: Val::Px(31.0),
                                                flex_shrink: 0.0,
                                                position_type: PositionType::Relative,
                                                border: UiRect::bottom(Val::Px(1.0)),
                                                ..default()
                                            })
                                            .with_children(|track| {
                                                let audible_in_preview = emitter.enabled
                                                    && session
                                                        .solo_emitter
                                                        .is_none_or(|solo| solo == emitter.id);
                                                let move_label = emitter_timing_label(
                                                    localizer,
                                                    "timeline-move-emitter-clip",
                                                    &emitter.name,
                                                );
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
                                                            border_radius: BorderRadius::all(
                                                                Val::Px(3.0),
                                                            ),
                                                            border: UiRect::all(Val::Px(1.0)),
                                                            overflow: Overflow::clip(),
                                                            ..default()
                                                        },
                                                        BackgroundColor(
                                                            layer_color(index, emitter.display_color).with_alpha(
                                                                if audible_in_preview {
                                                                    0.28
                                                                } else {
                                                                    0.10
                                                                },
                                                            ),
                                                        ),
                                                        BorderColor::all(
                                                            layer_color(index, emitter.display_color).with_alpha(
                                                                if audible_in_preview { 1.0 } else { 0.45 },
                                                            ),
                                                        ),
                                                    ))
                                                    .with_children(|clip| {
                                                        clip.spawn((
                                                            Button,
                                                            EditorNativeControl,
                                                            TimelineClipInteraction {
                                                                emitter: emitter.id,
                                                                kind: TimelineDragKind::Move,
                                                            },
                                                            ChoreographyAction::SelectEmitter(
                                                                emitter.id,
                                                            ),
                                                            AccessibleLabel(move_label.clone()),
                                                            EditorTooltip::description(move_label),
                                                            EntityCursor::System(
                                                                SystemCursorIcon::Grab,
                                                            ),
                                                            Node {
                                                                position_type:
                                                                    PositionType::Absolute,
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
                                                            let boundary_label =
                                                                emitter_timing_label(
                                                                    localizer,
                                                                    match kind {
                                                                        TimelineDragKind::TrimStart => {
                                                                            "timeline-trim-emitter-start"
                                                                        }
                                                                        TimelineDragKind::TrimEnd => {
                                                                            "timeline-trim-emitter-end"
                                                                        }
                                                                        TimelineDragKind::Move => {
                                                                            unreachable!()
                                                                        }
                                                                    },
                                                                    &emitter.name,
                                                                );
                                                            let boundary = match kind {
                                                                TimelineDragKind::TrimStart => {
                                                                    emitter.start_time
                                                                }
                                                                TimelineDragKind::TrimEnd => {
                                                                    emitter.start_time
                                                                        + emitter.duration
                                                                }
                                                                TimelineDragKind::Move => {
                                                                    unreachable!()
                                                                }
                                                            };
                                                            clip.spawn((
                                                                Button,
                                                                EditorNativeControl,
                                                                TimelineClipInteraction {
                                                                    emitter: emitter.id,
                                                                    kind,
                                                                },
                                                                ChoreographyAction::SelectEmitter(
                                                                    emitter.id,
                                                                ),
                                                                AccessibleLabel(
                                                                    boundary_label.clone(),
                                                                ),
                                                                EditorTooltip::description(
                                                                    boundary_label,
                                                                ),
                                                                EntityCursor::System(
                                                                    SystemCursorIcon::EwResize,
                                                                ),
                                                                Node {
                                                                    display: if timeline_boundary_is_visible(
                                                                        boundary,
                                                                        state.view,
                                                                    ) {
                                                                        Display::Flex
                                                                    } else {
                                                                        Display::None
                                                                    },
                                                                    position_type:
                                                                        PositionType::Absolute,
                                                                    left,
                                                                    right,
                                                                    top: Val::Px(0.0),
                                                                    width: Val::Px(8.0),
                                                                    height: Val::Percent(100.0),
                                                                    align_items: AlignItems::Center,
                                                                    justify_content:
                                                                        JustifyContent::Center,
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
                                                                BackgroundColor(layer_color(index, emitter.display_color)),
                                                                Pickable::IGNORE,
                                                            ));
                                                        }
                                                    });
                                            });
                                        }
                                    })
                                    .id(),
                            );
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
                                TimelineInvalidDropFeedback::default(),
                                AccessibleLabel(
                                    localizer.text("timeline-drop-effect-unsupported-title"),
                                ),
                                Node {
                                    display: Display::None,
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(10.0),
                                    right: Val::Px(10.0),
                                    top: Val::Px(10.0),
                                    bottom: Val::Px(10.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(5.0),
                                    padding: UiRect::all(Val::Px(16.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_DARK.with_alpha(0.94)),
                                BorderColor::all(theme::ACCENT),
                                Pickable::IGNORE,
                                ZIndex(4),
                            ))
                            .with_children(|feedback| {
                                feedback.spawn((
                                    Text::new(
                                        localizer.text("timeline-drop-effect-unsupported-title"),
                                    ),
                                    TextFont {
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(theme::ACCENT),
                                    TextLayout::justify(Justify::Center),
                                    Pickable::IGNORE,
                                ));
                                feedback.spawn((
                                    Text::new(localizer.text(
                                        "timeline-drop-effect-unsupported-message",
                                    )),
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT_MUTED),
                                    TextLayout::justify(Justify::Center),
                                    Pickable::IGNORE,
                                ));
                            });
                        tracks
                            .spawn((
                                TimelineHorizontalScrollbarGutter,
                                Node {
                                    display: Display::None,
                                    width: Val::Percent(100.0),
                                    height: Val::Px(15.0),
                                    flex_shrink: 0.0,
                                    align_items: AlignItems::Stretch,
                                    padding: UiRect {
                                        top: Val::Px(4.0),
                                        left: Val::Px(6.0),
                                        right: Val::Px(6.0),
                                        ..default()
                                    },
                                    border: UiRect::top(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_DARK),
                                BorderColor::all(theme::BORDER),
                            ))
                            .observe(stop_timeline_control_press)
                            .observe(stop_timeline_control_drag)
                            .with_children(|gutter| {
                                let scroll_target = gutter
                                    .spawn((
                                        TimelineHorizontalScrollViewport,
                                        ScrollPosition::default(),
                                        Pickable::IGNORE,
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: Val::Px(0.0),
                                            right: Val::Px(0.0),
                                            top: Val::Px(0.0),
                                            bottom: Val::Px(0.0),
                                            overflow: Overflow::scroll_x(),
                                            scrollbar_width: 0.0,
                                            ..default()
                                        },
                                    ))
                                    .with_child((
                                        TimelineHorizontalScrollContent,
                                        Pickable::IGNORE,
                                        Node {
                                            width: Val::Percent(
                                                (session.playback_duration().max(0.05)
                                                    / state.view.span().max(0.001)
                                                    * 100.0)
                                                    .clamp(100.0, 100_000.0),
                                            ),
                                            height: Val::Px(1.0),
                                            flex_shrink: 0.0,
                                            ..default()
                                        },
                                    ))
                                    .id();
                                let scrollbar = spawn_horizontal_scrollbar(gutter, scroll_target);
                                gutter
                                    .commands()
                                    .entity(scrollbar)
                                    .observe(stop_timeline_control_press)
                                    .observe(stop_timeline_control_drag);
                            });
                    });
                    if let Some(target) = vertical_scroll_target {
                        body.spawn((
                            TimelineVerticalScrollbarGutter,
                            Node {
                                display: Display::None,
                                width: Val::Px(15.0),
                                height: Val::Auto,
                                flex_shrink: 0.0,
                                align_self: AlignSelf::Stretch,
                                align_items: AlignItems::Stretch,
                                margin: UiRect {
                                    top: Val::Px(25.0),
                                    bottom: Val::Px(15.0),
                                    ..default()
                                },
                                padding: UiRect::left(Val::Px(4.0)),
                                border: UiRect::left(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL_DARK),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|gutter| {
                            let scrollbar = spawn_vertical_scrollbar(gutter, target);
                            gutter.commands().entity(scrollbar).insert(Node {
                                width: Val::Px(10.0),
                                height: Val::Percent(100.0),
                                display: Display::None,
                                padding: UiRect::horizontal(Val::Px(3.0)),
                                ..default()
                            });
                        });
                    }
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
    display_color: Option<[f32; 4]>,
) {
    let selected = session.selected_layer().id == emitter;
    let diagnostic = emitter_has_diagnostic(session, index);
    let mut args = FluentArgs::new();
    args.set("name", name);
    let soloed = session.solo_emitter == Some(emitter);
    let state_message = match (enabled, soloed, session.solo_emitter.is_some()) {
        (false, true, _) => "timeline-emitter-muted-soloed-status",
        (true, true, _) => "timeline-emitter-soloed-status",
        (true, false, true) => "timeline-emitter-suppressed-status",
        (true, false, false) => "timeline-emitter-enabled-status",
        (false, false, _) => "timeline-emitter-muted-status",
    };
    args.set("state", localizer.text(state_message));
    let accessible_label = localizer.text_with("timeline-select-emitter-status", &args);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        EmitterTrackHeader { emitter },
        ChoreographyAction::SelectEmitter(emitter),
        AccessibleLabel(accessible_label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(31.0),
            flex_shrink: 0.0,
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
    if selected {
        header.insert(Selected);
    }
    if !enabled {
        header.insert(EmitterTrackDisabled);
    }
    header
        .observe(open_timeline_track_context_menu)
        .with_children(|row| {
            let track_color = layer_color(index, display_color);
            let color_label =
                emitter_timing_label(localizer, "timeline-change-emitter-color", name);
            let mut color_button = row.spawn_empty();
            color_button
                .apply_scene(ui_shell::feathers_plain_button())
                .insert((
                    EmitterTrackColorChip,
                    RelativeCursorPosition::default(),
                    FeathersActionButton,
                    ChoreographyAction::ToggleEmitterColorPicker(emitter),
                    AccessibleLabel(color_label.clone()),
                    EditorTooltip::description(color_label),
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(21.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ));
            if state.color_picker_emitter == Some(emitter) {
                color_button.insert((Selected, ButtonVariant::Primary));
            }
            color_button.with_children(|chip| {
                chip.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(track_color),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    EmitterTrackColorSwatch { emitter },
                    Pickable::IGNORE,
                ));
                if state.color_picker_emitter == Some(emitter) {
                    spawn_emitter_color_picker(
                        chip,
                        localizer,
                        emitter,
                        display_color,
                        color_components(layer_color(index, None)),
                    );
                }
            });
            let muted = !enabled;
            let mute = mini_button(
                row,
                "M",
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
            );
            let mute_label = localizer.text(if muted {
                "timeline-unmute-emitter"
            } else {
                "timeline-mute-emitter"
            });
            row.commands().entity(mute).insert((
                AccessibleLabel(mute_label.clone()),
                EditorTooltip::description(mute_label),
            ));
            if muted {
                row.commands()
                    .entity(mute)
                    .insert((Selected, ButtonVariant::Primary));
            }
            let solo = mini_button(row, "S", ChoreographyAction::ToggleEmitterSolo(emitter));
            let solo_label = localizer.text(if soloed {
                "timeline-unsolo-emitter"
            } else {
                "timeline-solo-emitter"
            });
            row.commands().entity(solo).insert((
                AccessibleLabel(solo_label.clone()),
                EditorTooltip::description(solo_label),
            ));
            if soloed {
                row.commands()
                    .entity(solo)
                    .insert((Selected, ButtonVariant::Primary));
            }
            row.spawn((
                Text::new(format!("{:02}", index + 1)),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            let rename_label = emitter_timing_label(localizer, "timeline-rename-emitter", name);
            let name_control = spawn_text_input(
                row,
                name,
                &rename_label,
                TimelineTrackNameControl { emitter },
            );
            row.commands().entity(name_control).insert((
                Node {
                    min_width: Val::Px(0.0),
                    height: Val::Px(23.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                EditorTooltip::description(rename_label),
            ));
            if diagnostic {
                row.spawn((
                    Text::new("!"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::ACCENT),
                    AccessibleLabel(localizer.text("timeline-emitter-diagnostic")),
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
                describe_timeline_control(
                    actions,
                    duplicate,
                    localizer.text("edit-duplicate-emitter"),
                );
                let more = mini_button(
                    actions,
                    "...",
                    ChoreographyAction::ToggleEmitterMenu(emitter),
                );
                describe_timeline_control(
                    actions,
                    more,
                    localizer.text("timeline-more-emitter-actions"),
                );
            });
            if state.context_emitter == Some(emitter) {
                spawn_emitter_context_menu(row, localizer, emitter, enabled, soloed);
            }
        });
}

fn spawn_emitter_context_menu(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    emitter: EmitterId,
    enabled: bool,
    soloed: bool,
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
                    "timeline-mute-emitter"
                } else {
                    "timeline-unmute-emitter"
                }),
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
                false,
            );
            spawn_feathers_action_button(
                menu,
                &localizer.text(if soloed {
                    "timeline-unsolo-emitter"
                } else {
                    "timeline-solo-emitter"
                }),
                ChoreographyAction::ToggleEmitterSolo(emitter),
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

fn spawn_emitter_color_picker(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    emitter: EmitterId,
    display_color: Option<[f32; 4]>,
    automatic_color: [f32; 4],
) {
    parent
        .spawn((
            Popover {
                positions: color_picker_popover_positions(),
                window_margin: 9.0,
            },
            EmitterTrackColorPickerPopover,
            RelativeCursorPosition::default(),
            OverrideClip,
            GlobalZIndex(260),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(258.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.62),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(3.0),
                Val::Px(5.0),
            ),
        ))
        .with_children(|popup| {
            popup.spawn((
                Text::new(localizer.text("timeline-color-picker-title")),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            spawn_color_picker(
                popup,
                display_color,
                automatic_color,
                ColorPickerLabels {
                    accessible: localizer.text("timeline-color-picker-accessible"),
                    hue_saturation: localizer.text("timeline-color-picker-hue-saturation"),
                    lightness: localizer.text("timeline-color-picker-lightness"),
                    alpha: localizer.text("timeline-color-picker-alpha"),
                    automatic: localizer.text("timeline-color-picker-auto"),
                    rgb: localizer.text("timeline-color-picker-rgb"),
                    hsl: localizer.text("timeline-color-picker-hsl"),
                    red: localizer.text("timeline-color-picker-red"),
                    green: localizer.text("timeline-color-picker-green"),
                    blue: localizer.text("timeline-color-picker-blue"),
                    hue: localizer.text("timeline-color-picker-hue"),
                    saturation: localizer.text("timeline-color-picker-saturation"),
                    hex: localizer.text("timeline-color-picker-hex"),
                },
                EmitterTrackColorPicker { emitter },
            );
        });
}

fn color_picker_popover_positions() -> Vec<PopoverPlacement> {
    [
        (PopoverSide::Right, PopoverAlign::End),
        (PopoverSide::Left, PopoverAlign::End),
        (PopoverSide::Top, PopoverAlign::Start),
        (PopoverSide::Right, PopoverAlign::Start),
        (PopoverSide::Left, PopoverAlign::Start),
        (PopoverSide::Bottom, PopoverAlign::Start),
    ]
    .into_iter()
    .map(|(side, align)| PopoverPlacement {
        side,
        align,
        gap: 7.0,
    })
    .collect()
}

fn emitter_has_diagnostic(session: &EditorSession, index: usize) -> bool {
    let prefix = format!("effect.emitters[{index}]");
    session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(&prefix))
}

fn layer_color(index: usize, display_color: Option<[f32; 4]>) -> Color {
    if let Some([red, green, blue, alpha]) = display_color {
        return Color::srgba(red, green, blue, alpha);
    }
    match index % 4 {
        0 => Color::srgb(0.48, 0.31, 0.98),
        1 => Color::srgb(0.17, 0.75, 0.95),
        2 => Color::srgb(0.98, 0.47, 0.21),
        _ => Color::srgb(0.84, 0.29, 0.72),
    }
}

fn color_components(color: Color) -> [f32; 4] {
    let color = color.to_srgba();
    [color.red, color.green, color.blue, color.alpha]
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
    track_panes: Query<(&TimelineVerticalPane, &ComputedNode), Without<TimelineCanvas>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if state.drag.take().is_some() {
            state.snap_guide = None;
            override_cursor.0 = None;
            **cursor = CursorIcon::System(SystemCursorIcon::Default);
        }
        if state.context_emitter.take().is_some() | state.color_picker_emitter.take().is_some() {
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

    let (scroll, track_scroll) =
        wheel
            .read()
            .fold((Vec2::ZERO, 0.0), |(time_sum, track_sum), event| {
                let time_scale = match event.unit {
                    MouseScrollUnit::Line => 1.0,
                    MouseScrollUnit::Pixel => 0.01,
                };
                let track_scale = match event.unit {
                    MouseScrollUnit::Line => 21.0,
                    MouseScrollUnit::Pixel => 1.0,
                };
                (
                    time_sum + Vec2::new(event.x, event.y) * time_scale,
                    track_sum + event.y * track_scale,
                )
            });
    // The native ScrollArea on the header list owns vertical wheel scrolling. The synchronized
    // pane system mirrors that offset to the track canvas on the same frame.
    let Some((cursor, _)) = hovered else {
        return;
    };
    if scroll == Vec2::ZERO {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    match timeline_wheel_intent(scroll, track_scroll, control, shift) {
        TimelineWheelIntent::ZoomTime(amount) => {
            if let Some(position) = cursor.normalized {
                let anchor = state.view.time_at(timeline_cursor_fraction(position.x));
                state.zoom_at(
                    anchor,
                    0.82_f32.powf(amount),
                    session.playback_duration(),
                    session.clock.tick_rate(),
                );
            }
        }
        TimelineWheelIntent::PanTime(amount) => {
            let span = state.view.span();
            state.pan_by(-amount * span * 0.08, session.playback_duration());
        }
        TimelineWheelIntent::ScrollTracks(amount) => {
            let maximum = track_panes
                .iter()
                .find(|(kind, _)| **kind == TimelineVerticalPane::Tracks)
                .map(|(_, computed)| (computed.content_size().y - computed.size().y).max(0.0))
                .unwrap_or(0.0);
            state.vertical_scroll = (state.vertical_scroll - amount).clamp(0.0, maximum);
        }
    }
}

fn dismiss_timeline_color_picker(
    buttons: Res<ButtonInput<MouseButton>>,
    surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<EmitterTrackColorChip>,
            With<EmitterTrackColorPickerPopover>,
        )>,
    >,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if state.color_picker_emitter.is_none() || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if surfaces.iter().any(RelativeCursorPosition::cursor_over) {
        return;
    }
    state.color_picker_emitter = None;
    session.ui_revision += 1;
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TimelineWheelIntent {
    ScrollTracks(f32),
    PanTime(f32),
    ZoomTime(f32),
}

fn timeline_wheel_intent(
    time_delta: Vec2,
    track_delta: f32,
    control: bool,
    shift: bool,
) -> TimelineWheelIntent {
    let dominant = if time_delta.x.abs() > time_delta.y.abs() {
        time_delta.x
    } else {
        time_delta.y
    };
    if control {
        TimelineWheelIntent::ZoomTime(dominant)
    } else if shift || time_delta.x.abs() > time_delta.y.abs() {
        TimelineWheelIntent::PanTime(dominant)
    } else {
        TimelineWheelIntent::ScrollTracks(track_delta)
    }
}

fn is_project_effect_drag(
    mut entity: Entity,
    rows: &Query<(), With<ProjectEffectRow>>,
    parents: &Query<&ChildOf>,
) -> bool {
    loop {
        if rows.contains(entity) {
            return true;
        }
        let Ok(parent) = parents.get(entity) else {
            return false;
        };
        entity = parent.parent();
    }
}

fn show_invalid_timeline_drop_feedback(
    mut enter: On<Pointer<DragEnter>>,
    rows: Query<(), With<ProjectEffectRow>>,
    parents: Query<&ChildOf>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
) {
    if !is_project_effect_drag(enter.dragged, &rows, &parents) {
        return;
    }
    for (mut feedback, mut node) in &mut feedback {
        feedback.rejected = false;
        feedback.timer.reset();
        feedback.timer.pause();
        node.display = Display::Flex;
    }
    enter.propagate(false);
}

fn hide_invalid_timeline_drop_feedback(
    mut leave: On<Pointer<DragLeave>>,
    rows: Query<(), With<ProjectEffectRow>>,
    parents: Query<&ChildOf>,
    mut feedback: Query<(&TimelineInvalidDropFeedback, &mut Node)>,
) {
    if !is_project_effect_drag(leave.dragged, &rows, &parents) {
        return;
    }
    for (feedback, mut node) in &mut feedback {
        if !feedback.rejected {
            node.display = Display::None;
        }
    }
    leave.propagate(false);
}

fn drop_project_effect_on_timeline(
    mut drop: On<Pointer<DragDrop>>,
    rows: Query<(), With<ProjectEffectRow>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if !is_project_effect_drag(drop.dropped, &rows, &parents) {
        return;
    }
    commands.trigger(RejectProjectEffectDrop);
    drop.propagate(false);
}

fn reject_project_effect_drop(
    _drop: On<RejectProjectEffectDrop>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
) {
    session.status = localizer.text("timeline-drop-effect-unsupported-status");
    for (mut feedback, mut node) in &mut feedback {
        feedback.rejected = true;
        feedback.timer.reset();
        feedback.timer.unpause();
        node.display = Display::Flex;
    }
}

fn tick_invalid_timeline_drop_feedback(
    time: Res<Time>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
) {
    for (mut feedback, mut node) in &mut feedback {
        if feedback.timer.is_paused() {
            continue;
        }
        feedback.timer.tick(time.delta());
        if feedback.timer.just_finished() {
            feedback.rejected = false;
            feedback.timer.pause();
            node.display = Display::None;
        }
    }
}

fn seek_timeline_on_press(
    press: On<Pointer<Press>>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if press.button == PointerButton::Primary {
        if state.context_emitter.take().is_some() | state.color_picker_emitter.take().is_some() {
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

fn timeline_boundary_is_visible(time: f32, view: TimelineView) -> bool {
    (view.start..=view.end).contains(&time)
}

fn screen_distance_to_logical(distance: f32, scale_factor: f32) -> f32 {
    distance / scale_factor.max(0.01)
}

fn stop_timeline_control_press(mut press: On<Pointer<Press>>) {
    press.propagate(false);
}

fn stop_timeline_control_drag(mut drag: On<Pointer<Drag>>) {
    drag.propagate(false);
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
    window: Single<&Window, With<PrimaryWindow>>,
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
    let logical_distance = screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let pointer_time = logical_distance / width * state.view.span();
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
