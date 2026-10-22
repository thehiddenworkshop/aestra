//! B20-4 pilot: upstream numeric interaction, application-owned preference commits.
//! No EditorNumberInput/ScrubbableNumber/CustomNumberStep or private child markers.
use super::*;
use bevy::{
    feathers::controls::{
        FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputStep, SoftLimit,
    },
    input::keyboard::KeyboardInput,
    input_focus::{FocusGained, FocusLost, FocusedInput, InputFocus},
    picking::{
        PickingSystems,
        events::{PointerCancel, PointerPress},
        pointer::{PointerAction, PointerId, PointerInput},
    },
    scene::SceneScope,
    text::EditableText,
    ui::InteractionDisabled,
};

#[derive(Component, Default, Clone)]
struct AutosaveNumber {
    model: u16,
    cancelled: bool,
    pointer: Option<PointerId>,
}

mod steppers;

#[derive(Component, Clone, Default)]
struct AutosaveField;

pub(super) fn scene(label: String) -> impl Scene {
    bsn! {
        Node { width: percent(100), position_type: PositionType::Relative }
        AutosaveField
        Children [
            @FeathersNumberInput
            Node { width: percent(100) }
            AccessibleLabel({label})
            NumberInputValue::I32(30)
            HardLimit::i32(5..=600)
            SoftLimit::i32(5..=600)
            NumberInputStep(1.0)
            NumberInputPrecision(0)
            AutosaveNumber::default()
            --
            {SceneScope(steppers::scene(-1))}
            --
            {SceneScope(steppers::scene(1))}
        ]
    }
}

pub(super) fn install(app: &mut App) {
    steppers::install(app);
    app.add_observer(change)
        .add_observer(press)
        .add_observer(focus)
        .add_observer(key)
        .add_observer(blur)
        .add_observer(cancel)
        .add_systems(Update, sync_model.in_set(crate::EditorSet::UiSync))
        .add_systems(
            PreUpdate,
            cancel_pointer.before(PickingSystems::ProcessInput),
        );
}

fn owner(
    entity: Entity,
    parents: &Query<&ChildOf>,
    inputs: &Query<(), With<AutosaveNumber>>,
) -> Option<Entity> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find(|&e| inputs.contains(e))
}

fn disabled(
    entity: Entity,
    parents: &Query<&ChildOf>,
    disabled: &Query<(), With<InteractionDisabled>>,
) -> bool {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .any(|e| disabled.contains(e))
}

#[allow(clippy::too_many_arguments)]
fn change(
    change: On<ValueChange<i32>>,
    mut inputs: Query<&mut AutosaveNumber>,
    parents: Query<&ChildOf>,
    disabled_inputs: Query<(), With<InteractionDisabled>>,
    mut settings: ResMut<EditorSettings>,
    mut persistence: ResMut<SettingsPersistence>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    let Ok(mut input) = inputs.get_mut(change.source) else {
        return;
    };
    let model = settings.general.autosave_interval_seconds;
    if input.cancelled
        || input.model != model
        || disabled(change.source, &parents, &disabled_inputs)
    {
        input.model = model;
        input.cancelled = true;
        commands
            .entity(change.source)
            .insert(NumberInputValue::I32(i32::from(model)));
        return;
    }
    let value = change.value.clamp(5, 600);
    // Upstream controls are controlled widgets: echo drafts for display, but
    // don't change recovery behavior, write preferences or rebuild the UI mid-drag.
    commands
        .entity(change.source)
        .insert(NumberInputValue::I32(value));
    if change.is_final {
        input.model = value as u16;
        if apply_settings_integer(&mut settings, SettingsNumber::AutosaveInterval, value) {
            session.ui_revision += 1;
            persist_editor_settings(&settings, &mut persistence, &mut session, &localizer);
        }
    }
}

fn sync_model(
    settings: Res<EditorSettings>,
    focus: Res<InputFocus>,
    parents: Query<&ChildOf>,
    mut inputs: Query<(Entity, &mut AutosaveNumber)>,
    mut commands: Commands,
) {
    let model = settings.general.autosave_interval_seconds;
    for (entity, mut input) in &mut inputs {
        if input.model == model {
            continue;
        }
        // Do not replace a focused text draft or its selection. A stale final
        // event is rejected by `change`; blur/cancel then displays the new model.
        if focus
            .get()
            .is_some_and(|f| f == entity || parents.iter_ancestors(f).any(|e| e == entity))
        {
            continue;
        }
        input.cancelled = input.model != 0;
        input.model = model;
        commands
            .entity(entity)
            .insert(NumberInputValue::I32(i32::from(model)));
    }
}

fn press(
    event: On<PointerPress>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<AutosaveNumber>>,
    mut inputs: Query<&mut AutosaveNumber>,
) {
    if let Some(entity) = owner(event.entity, &parents, &owners) {
        let mut input = inputs.get_mut(entity).unwrap();
        input.cancelled = false;
        input.pointer = Some(event.pointer.id);
    }
}

fn focus(
    event: On<FocusGained>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<AutosaveNumber>>,
    mut inputs: Query<&mut AutosaveNumber>,
) {
    if let Some(entity) = owner(event.event_target(), &parents, &owners) {
        inputs.get_mut(entity).unwrap().cancelled = false;
    }
}

fn restore(
    entity: Entity,
    settings: &EditorSettings,
    inputs: &mut Query<&mut AutosaveNumber>,
    commands: &mut Commands,
) {
    let mut input = inputs.get_mut(entity).unwrap();
    input.model = settings.general.autosave_interval_seconds;
    input.cancelled = true;
    input.pointer = None;
    commands
        .entity(entity)
        .insert(NumberInputValue::I32(i32::from(input.model)));
}

#[allow(clippy::too_many_arguments)]
fn key(
    mut event: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<AutosaveNumber>>,
    texts: Query<&EditableText>,
    mut inputs: Query<&mut AutosaveNumber>,
    settings: Res<EditorSettings>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !event.input.state.is_pressed() {
        return;
    }
    let Some(entity) = owner(event.event_target(), &parents, &owners) else {
        return;
    };
    let invalid_enter = event.input.key_code == KeyCode::Enter
        && texts
            .get(event.event_target())
            .is_ok_and(|t| t.value().to_string().parse::<i32>().is_err());
    if event.input.key_code == KeyCode::Escape || invalid_enter {
        restore(entity, &settings, &mut inputs, &mut commands);
        event.propagate(false);
        focus.clear();
    }
}

fn blur(
    event: On<FocusLost>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<AutosaveNumber>>,
    texts: Query<&EditableText>,
    mut inputs: Query<&mut AutosaveNumber>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if texts
        .get(event.event_target())
        .is_ok_and(|t| t.value().to_string().parse::<i32>().is_err())
        && let Some(entity) = owner(event.event_target(), &parents, &owners)
    {
        restore(entity, &settings, &mut inputs, &mut commands);
    }
}

fn cancel(
    event: On<PointerCancel>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<AutosaveNumber>>,
    mut inputs: Query<&mut AutosaveNumber>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if let Some(entity) = owner(event.entity, &parents, &owners) {
        restore(entity, &settings, &mut inputs, &mut commands);
    }
}

// Bevy's target-level Cancel is sent to the *currently hovered* node, which may
// no longer be the pressed numeric field. Retain only transaction ownership,
// not scrub geometry/math, so leaving the field cannot strand a displayed draft.
fn cancel_pointer(
    mut events: MessageReader<PointerInput>,
    mut keys: MessageReader<KeyboardInput>,
    settings: Res<EditorSettings>,
    mut inputs: Query<(Entity, &mut AutosaveNumber)>,
    mut commands: Commands,
) {
    // A scrub need not have text focus. Escape cancels its preference transaction
    // even if focused-input dispatch has no target; Bevy still owns the drag.
    if keys
        .read()
        .any(|key| key.state.is_pressed() && key.key_code == KeyCode::Escape)
    {
        for (entity, mut input) in &mut inputs {
            if input.pointer.is_some() {
                input.model = settings.general.autosave_interval_seconds;
                input.cancelled = true;
                input.pointer = None;
                commands
                    .entity(entity)
                    .insert(NumberInputValue::I32(i32::from(input.model)));
            }
        }
    }
    for event in events.read() {
        if !matches!(
            event.action,
            PointerAction::Cancel | PointerAction::Release(_)
        ) {
            continue;
        }
        for (entity, mut input) in &mut inputs {
            if input.pointer == Some(event.pointer_id) {
                input.pointer = None;
                if matches!(event.action, PointerAction::Cancel) {
                    input.model = settings.general.autosave_interval_seconds;
                    input.cancelled = true;
                    commands
                        .entity(entity)
                        .insert(NumberInputValue::I32(i32::from(input.model)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
