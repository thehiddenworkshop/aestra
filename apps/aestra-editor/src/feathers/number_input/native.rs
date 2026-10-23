//! Typed native-widget bridge. No pointer delta arithmetic or private Bevy markers.
use super::{ScrubbableNumber, arrows::CustomNumberStep, widget::*};
use bevy::{
    feathers::controls::{
        HardLimit, NumberInputPrecision, NumberInputStep, NumberInputUnits, UnitsRegistry,
    },
    input::keyboard::KeyboardInput,
    input_focus::{FocusGained, FocusLost, FocusedInput, InputFocus},
    picking::{
        PickingSystems,
        events::PointerPress,
        pointer::{PointerAction, PointerButton, PointerId, PointerInput},
    },
    prelude::*,
    text::{EditableText, TextEdit, TextEditChange},
    ui::InteractionDisabled,
    ui_widgets::ValueChange,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditPhase {
    Preview,
    Commit,
    Cancel,
}

/// Empty submission means "required/unset" for an optional application value,
/// represented by a final String event rather than inventing a numeric sentinel.
#[derive(Component)]
pub(crate) struct OptionalNumberInput;

/// Domain-owned controls consume this instead of a second pointer drag controller.
#[derive(EntityEvent)]
pub(crate) struct NumberEdit {
    pub(crate) entity: Entity,
    pub(crate) value: NumberInputValue,
    pub(crate) phase: EditPhase,
}

/// Display normalization by a domain adapter, without replacing the gesture baseline.
#[derive(EntityEvent)]
pub(crate) struct DisplayNumberInput {
    pub(crate) entity: Entity,
    pub(crate) value: NumberInputValue,
}

fn display(
    event: On<DisplayNumberInput>,
    mut models: Query<(&mut Model, &Children)>,
    natives: Query<(), With<NativeNumber>>,
    focus: Res<InputFocus>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if !finite(event.value) {
        return;
    }
    if let Ok((mut model, children)) = models.get_mut(event.entity) {
        model.displayed = Some(event.value);
        if model.dirty
            && focus.get().is_some_and(|e| {
                e == event.entity || parents.iter_ancestors(e).any(|p| p == event.entity)
            })
        {
            return;
        }
        if let Some(native) = children.iter().find(|&e| natives.contains(e)) {
            commands.entity(native).insert(event.value);
        }
    }
}

#[derive(Component, Default)]
pub(super) struct Model {
    pub(super) value: Option<NumberInputValue>,
    pub(super) displayed: Option<NumberInputValue>,
    dirty: bool,
    typing: bool,
    cancelled: bool,
    reset_display: bool,
    pointer: Option<PointerId>,
    context: Option<(u64, u64)>,
    pending: bool,
}

fn context(session: Option<&crate::EditorSession>) -> Option<(u64, u64)> {
    session.map(|s| (s.document_revision(), s.ui_revision))
}

pub(super) fn register(app: &mut App) {
    app.add_observer(update_model)
        .add_observer(cancel_preview)
        .add_observer(removed)
        .add_observer(display)
        .add_observer(changed_f32)
        .add_observer(changed_f64)
        .add_observer(changed_i32)
        .add_observer(changed_i64)
        .add_observer(press)
        .add_observer(focus)
        .add_observer(key)
        .add_observer(blur)
        .add_observer(text_changed)
        .add_systems(
            PreUpdate,
            cancel_pointer.before(PickingSystems::ProcessInput),
        )
        .add_systems(
            Update,
            (sync, sync_text).chain().in_set(super::NumberScrubSet),
        );
}

pub(super) fn enabled(
    entity: Entity,
    parents: &Query<&ChildOf>,
    disabled: &Query<(), With<InteractionDisabled>>,
) -> bool {
    !std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .any(|e| disabled.contains(e))
}

fn owner(entity: Entity, parents: &Query<&ChildOf>, models: &Query<&mut Model>) -> Option<Entity> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find(|&e| models.contains(e))
}

fn update_model(
    event: On<UpdateNumberInput>,
    mut models: Query<&mut Model>,
    formats: Query<&NumberFormat>,
    mut commands: Commands,
) {
    if formats
        .get(event.entity)
        .is_ok_and(|f| !f.matches(event.value))
        || !finite(event.value)
    {
        return;
    }
    if let Ok(mut model) = models.get_mut(event.entity) {
        // A preview owner may echo its current draft while synchronizing related controls.
        if model.dirty && model.displayed == Some(event.value) {
            return;
        }
        if model.value != Some(event.value) {
            model.value = Some(event.value);
            model.pending = true;
            if model.dirty || model.context.is_some() {
                model.cancelled = true;
                model.reset_display = false;
            }
        }
    } else {
        // Updates can precede deferred BSN composition, including the format component.
        if let Ok(mut entity) = commands.get_entity(event.entity) {
            entity.insert(Model {
                value: Some(event.value),
                pending: true,
                ..default()
            });
        }
    }
}

fn sync(
    mut commands: Commands,
    mut models: Query<
        (
            Entity,
            &NumberFormat,
            Option<&ScrubbableNumber>,
            Option<&AccessibleLabel>,
            &Children,
            Option<&HardLimit>,
            Option<&NumberInputStep>,
            Option<&NumberInputPrecision>,
            Option<&NumberInputUnits>,
            Option<&mut Model>,
        ),
        With<EditorNumberInput>,
    >,
    natives: Query<
        (
            &NumberInputValue,
            Option<&HardLimit>,
            Option<&NumberInputStep>,
            Option<&NumberInputPrecision>,
            Option<&NumberInputUnits>,
            Option<&AccessibleLabel>,
            Has<InteractionDisabled>,
        ),
        With<NativeNumber>,
    >,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    focus: Res<InputFocus>,
    custom: Query<(), With<CustomNumberStep>>,
) {
    for (entity, format, policy, label, children, limit, step, precision, units, model) in
        &mut models
    {
        let Some(native) = children.iter().find(|&e| natives.contains(e)) else {
            continue;
        };
        let (
            native_value,
            native_limit,
            native_step,
            native_precision,
            native_units,
            native_label,
            native_disabled,
        ) = natives.get(native).unwrap();
        let Some(mut model) = model else {
            commands.entity(entity).insert(Model {
                value: Some(format.zero()),
                pending: true,
                ..default()
            });
            continue;
        };
        if model.value.is_none() {
            model.value = Some(format.zero());
            model.pending = true;
        }
        let editing = focus
            .get()
            .is_some_and(|e| e == native || parents.iter_ancestors(e).any(|p| p == native))
            && !model.reset_display;
        // Deferred native scenes may finish after the owner's first sync pass.
        if !model.dirty && model.value.is_some_and(|value| value != *native_value) {
            model.pending = true;
        }
        if model.pending && !editing {
            let value = model
                .value
                .filter(|&v| format.matches(v))
                .unwrap_or_else(|| format.zero());
            commands.entity(native).insert(value);
            model.displayed = Some(value);
            model.pending = false;
        }
        // Native Step is a drag-speed hint (multiplied by 0.01), not a click
        // increment. Map policy once and avoid reinserting theme/layout inputs
        // every frame in inspectors with hundreds of fields.
        let policy_limit = policy.map(|p| HardLimit::f32(p.min..=p.max));
        if let Some(limit) = limit.or(policy_limit.as_ref())
            && native_limit.map(|limit| &limit.0) != Some(&limit.0)
        {
            commands.entity(native).insert(limit.clone());
        }
        let step = step
            .map(|s| s.0)
            .or_else(|| policy.map(|p| f64::from(p.step) * 12.5));
        if let Some(step) = step
            && native_step.map(|s| s.0) != Some(step)
        {
            commands.entity(native).insert(NumberInputStep(step));
        }
        let precision = precision
            .map(|p| p.0)
            .or_else(|| policy.map(|p| super::decimal_places(p.step * 0.1) as i32));
        if let Some(precision) = precision
            && native_precision.map(|p| p.0) != Some(precision)
        {
            commands
                .entity(native)
                .insert(NumberInputPrecision(precision));
        }
        if let Some(units) = units
            && native_units.map(|u| &u.0) != Some(&units.0)
        {
            commands.entity(native).insert(units.clone());
        }
        if let Some(label) = label
            && native_label.map(|l| &l.0) != Some(&label.0)
        {
            commands.entity(native).insert(label.clone());
        }
        let is_disabled = !enabled(entity, &parents, &disabled);
        if is_disabled && model.dirty {
            rollback(entity, &mut model, custom.contains(entity), &mut commands);
        }
        if is_disabled != native_disabled {
            if is_disabled {
                commands.entity(native).insert(InteractionDisabled);
            } else {
                commands.entity(native).remove::<InteractionDisabled>();
            }
        }
    }
}

fn sync_text(
    natives: Query<
        (
            Entity,
            &ChildOf,
            &NumberInputValue,
            Option<&NumberInputUnits>,
        ),
        With<NativeNumber>,
    >,
    models: Query<&Model>,
    children: Query<&Children>,
    mut texts: Query<&mut EditableText>,
    focus: Res<InputFocus>,
    registry: Res<UnitsRegistry>,
) {
    for (native, owner, value, units) in &natives {
        for descendant in children.iter_descendants(native) {
            let Ok(mut text) = texts.get_mut(descendant) else {
                continue;
            };
            if models
                .get(owner.parent())
                .is_ok_and(|m| (m.typing || focus.get() == Some(descendant)) && !m.reset_display)
            {
                continue;
            }
            let digits = registry.resolve(units).format(*value, false);
            if text.value() != &digits || !text.pending_edits.is_empty() {
                // Supersede queued display writes even before the first shaped
                // layout, without selecting/replacing an active text draft.
                text.clear();
                text.editor_mut().set_text(&digits);
                text.queue_edit(TextEdit::TextEnd(false));
            }
        }
    }
}

pub(super) fn finite(value: NumberInputValue) -> bool {
    match value {
        NumberInputValue::F32(v) => v.is_finite(),
        NumberInputValue::F64(v) => v.is_finite(),
        _ => true,
    }
}

fn emit(
    commands: &mut Commands,
    entity: Entity,
    value: NumberInputValue,
    phase: EditPhase,
    custom: bool,
) {
    if custom {
        commands.trigger(NumberEdit {
            entity,
            value,
            phase,
        });
        return;
    }
    let is_final = phase == EditPhase::Commit;
    macro_rules! typed {
        ($v:expr) => {{
            commands.trigger(ValueChange {
                source: entity,
                value: $v,
                is_final,
            });
        }};
    }
    match value {
        NumberInputValue::F32(v) => typed!(v),
        NumberInputValue::F64(v) => typed!(v),
        NumberInputValue::I32(v) => typed!(v),
        NumberInputValue::I64(v) => typed!(v),
    }
    // Domain rollback sees the baseline first, then the shared editor preview is
    // restored. No final event or undo command is manufactured for cancellation.
    commands.trigger(NumberEdit {
        entity,
        value,
        phase,
    });
}

fn cancel_preview(event: On<NumberEdit>, mut session: Option<ResMut<crate::EditorSession>>) {
    if event.phase == EditPhase::Cancel
        && let Some(session) = session.as_mut()
    {
        session.restore_numeric_interaction_preview();
    }
}

fn removed(
    event: On<Remove<Model>>,
    models: Query<&Model>,
    mut session: Option<ResMut<crate::EditorSession>>,
) {
    if models
        .get(event.event_target())
        .is_ok_and(|model| model.dirty)
        && let Some(session) = session.as_mut()
    {
        session.restore_numeric_interaction_preview();
    }
}

pub(super) fn accept(
    entity: Entity,
    native: Entity,
    value: NumberInputValue,
    is_final: bool,
    model: &mut Model,
    custom: bool,
    commands: &mut Commands,
) {
    if model.cancelled || !finite(value) {
        return;
    }
    let initial = model.value.unwrap_or(value);
    if !is_final && model.displayed == Some(value) {
        return;
    }
    if is_final && value == initial && !model.dirty {
        return;
    }
    commands.entity(native).insert(value);
    model.typing = false;
    model.displayed = Some(value);
    model.dirty = !is_final;
    if is_final {
        model.value = Some(value);
        model.context = None;
    }
    emit(
        commands,
        entity,
        value,
        if is_final {
            EditPhase::Commit
        } else {
            EditPhase::Preview
        },
        custom,
    );
}

pub(super) fn step(
    entity: Entity,
    native: Entity,
    value: NumberInputValue,
    model: &mut Model,
    commands: &mut Commands,
) {
    model.cancelled = false;
    accept(entity, native, value, true, model, false, commands);
}

macro_rules! changed {
    ($name:ident, $ty:ty, $variant:ident) => {
        fn $name(
            event: On<ValueChange<$ty>>,
            natives: Query<&ChildOf, With<NativeNumber>>,
            mut models: Query<&mut Model>,
            custom: Query<(), With<CustomNumberStep>>,
            parents: Query<&ChildOf>,
            disabled: Query<(), With<InteractionDisabled>>,
            session: Option<Res<crate::EditorSession>>,
            mut commands: Commands,
        ) {
            let Ok(parent) = natives.get(event.source) else {
                return;
            };
            let entity = parent.parent();
            let Ok(mut model) = models.get_mut(entity) else {
                return;
            };
            if model.cancelled
                || !enabled(event.source, &parents, &disabled)
                || model
                    .context
                    .is_some_and(|c| Some(c) != context(session.as_deref()))
                || !finite(NumberInputValue::$variant(event.value))
            {
                rollback(entity, &mut model, custom.contains(entity), &mut commands);
                if let Some(value) = model.value {
                    commands.entity(event.source).insert(value);
                }
                return;
            }
            accept(
                entity,
                event.source,
                NumberInputValue::$variant(event.value),
                event.is_final,
                &mut model,
                custom.contains(entity),
                &mut commands,
            );
        }
    };
}
changed!(changed_f32, f32, F32);
changed!(changed_f64, f64, F64);
changed!(changed_i32, i32, I32);
changed!(changed_i64, i64, I64);

fn rollback(entity: Entity, model: &mut Model, custom: bool, commands: &mut Commands) {
    if model.dirty
        && let Some(value) = model.value
    {
        emit(commands, entity, value, EditPhase::Cancel, custom);
    }
    model.dirty = false;
    model.typing = false;
    model.cancelled = true;
    model.reset_display = true;
    model.pointer = None;
    model.context = None;
    model.pending = true;
}

fn press(
    event: On<PointerPress>,
    parents: Query<&ChildOf>,
    mut models: Query<&mut Model>,
    session: Option<Res<crate::EditorSession>>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    if let Some(entity) = owner(event.entity, &parents, &models) {
        let mut model = models.get_mut(entity).unwrap();
        model.cancelled = false;
        model.reset_display = false;
        model.pointer = Some(event.pointer.id);
        model.context = context(session.as_deref());
    }
}

fn focus(
    event: On<FocusGained>,
    parents: Query<&ChildOf>,
    mut models: Query<&mut Model>,
    session: Option<Res<crate::EditorSession>>,
) {
    if let Some(entity) = owner(event.event_target(), &parents, &models) {
        let mut model = models.get_mut(entity).unwrap();
        model.cancelled = false;
        model.reset_display = false;
        model.context = context(session.as_deref());
    }
}

fn valid_text(
    entity: Entity,
    text: &EditableText,
    parents: &Query<&ChildOf>,
    natives: &Query<(&NumberInputValue, Option<&NumberInputUnits>), With<NativeNumber>>,
    registry: &UnitsRegistry,
) -> bool {
    let Ok(parent) = parents.get(entity) else {
        return true;
    };
    let Ok((value, units)) = natives.get(parent.parent()) else {
        return true;
    };
    let format = match value {
        NumberInputValue::F32(_) => bevy::feathers::controls::NumberFormat::F32,
        NumberInputValue::F64(_) => bevy::feathers::controls::NumberFormat::F64,
        NumberInputValue::I32(_) => bevy::feathers::controls::NumberFormat::I32,
        NumberInputValue::I64(_) => bevy::feathers::controls::NumberFormat::I64,
    };
    registry
        .resolve(units)
        .parse(text.value().to_string(), format)
        .is_ok_and(finite)
}

/// The released widget submits text on Enter/blur. The application also needs
/// semantic live previews while typing, using the same public units parser.
/// This observer owns no editing, focus, selection, or drag implementation.
fn text_changed(
    event: On<TextEditChange>,
    texts: Query<&EditableText>,
    focus: Res<InputFocus>,
    natives: Query<
        (
            &ChildOf,
            &NumberInputValue,
            Option<&NumberInputUnits>,
            Option<&HardLimit>,
        ),
        With<NativeNumber>,
    >,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    registry: Res<UnitsRegistry>,
    mut models: Query<&mut Model>,
    custom: Query<(), With<CustomNumberStep>>,
    session: Option<Res<crate::EditorSession>>,
    mut commands: Commands,
) {
    let text_entity = event.event_target();
    if focus.get() != Some(text_entity) || !enabled(text_entity, &parents, &disabled) {
        return;
    }
    let Ok(text) = texts.get(text_entity) else {
        return;
    };
    let Ok(parent) = parents.get(text_entity) else {
        return;
    };
    let Ok((owner, value, units, limit)) = natives.get(parent.parent()) else {
        return;
    };
    let entity = owner.parent();
    let Ok(mut model) = models.get_mut(entity) else {
        return;
    };
    if model.cancelled
        || model
            .context
            .is_some_and(|c| Some(c) != context(session.as_deref()))
    {
        return;
    }
    let format = match value {
        NumberInputValue::F32(_) => bevy::feathers::controls::NumberFormat::F32,
        NumberInputValue::F64(_) => bevy::feathers::controls::NumberFormat::F64,
        NumberInputValue::I32(_) => bevy::feathers::controls::NumberFormat::I32,
        NumberInputValue::I64(_) => bevy::feathers::controls::NumberFormat::I64,
    };
    let Ok(value) = registry
        .resolve(units)
        .parse(text.value().to_string(), format)
    else {
        return;
    };
    if !finite(value) {
        return;
    }
    let value = limit.map_or(value, |limit| limit.0.clamp(value));
    if model.displayed == Some(value) {
        return;
    }
    model.displayed = Some(value);
    model.dirty = true;
    model.typing = true;
    emit(
        &mut commands,
        entity,
        value,
        EditPhase::Preview,
        custom.contains(entity),
    );
}

fn key(
    mut event: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    mut models: Query<&mut Model>,
    natives: Query<(&NumberInputValue, Option<&NumberInputUnits>), With<NativeNumber>>,
    registry: Res<UnitsRegistry>,
    texts: Query<&EditableText>,
    optional: Query<(), With<OptionalNumberInput>>,
    custom: Query<(), With<CustomNumberStep>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !event.input.state.is_pressed() {
        return;
    }
    let Some(entity) = owner(event.event_target(), &parents, &models) else {
        return;
    };
    if event.input.key_code == KeyCode::Enter
        && optional.contains(entity)
        && texts
            .get(event.event_target())
            .is_ok_and(|text| text.value().to_string().trim().is_empty())
    {
        rollback(
            entity,
            &mut models.get_mut(entity).unwrap(),
            custom.contains(entity),
            &mut commands,
        );
        commands.trigger(ValueChange {
            source: entity,
            value: String::new(),
            is_final: true,
        });
        event.propagate(false);
        focus.clear();
        return;
    }
    let invalid = event.input.key_code == KeyCode::Enter
        && texts.get(event.event_target()).is_ok_and(|text| {
            !valid_text(event.event_target(), text, &parents, &natives, &registry)
        });
    if event.input.key_code == KeyCode::Escape || invalid {
        rollback(
            entity,
            &mut models.get_mut(entity).unwrap(),
            custom.contains(entity),
            &mut commands,
        );
        event.propagate(false);
        focus.clear();
    }
}

fn blur(
    event: On<FocusLost>,
    parents: Query<&ChildOf>,
    mut models: Query<&mut Model>,
    natives: Query<(&NumberInputValue, Option<&NumberInputUnits>), With<NativeNumber>>,
    registry: Res<UnitsRegistry>,
    texts: Query<&EditableText>,
    optional: Query<(), With<OptionalNumberInput>>,
    custom: Query<(), With<CustomNumberStep>>,
    mut commands: Commands,
) {
    if let Some(entity) = owner(event.event_target(), &parents, &models)
        && optional.contains(entity)
        && models.get(entity).is_ok_and(|model| !model.cancelled)
        && texts
            .get(event.event_target())
            .is_ok_and(|text| text.value().to_string().trim().is_empty())
    {
        rollback(
            entity,
            &mut models.get_mut(entity).unwrap(),
            custom.contains(entity),
            &mut commands,
        );
        commands.trigger(ValueChange {
            source: entity,
            value: String::new(),
            is_final: true,
        });
        return;
    }
    if texts
        .get(event.event_target())
        .is_ok_and(|text| !valid_text(event.event_target(), text, &parents, &natives, &registry))
        && let Some(entity) = owner(event.event_target(), &parents, &models)
    {
        rollback(
            entity,
            &mut models.get_mut(entity).unwrap(),
            custom.contains(entity),
            &mut commands,
        );
    }
}

fn cancel_pointer(
    mut events: MessageReader<PointerInput>,
    mut keys: MessageReader<KeyboardInput>,
    mut models: Query<(Entity, &mut Model)>,
    custom: Query<(), With<CustomNumberStep>>,
    mut commands: Commands,
) {
    let escape = keys
        .read()
        .any(|key| key.state.is_pressed() && key.key_code == KeyCode::Escape);
    for event in events.read() {
        for (entity, mut model) in &mut models {
            if model.pointer != Some(event.pointer_id) {
                continue;
            }
            match event.action {
                PointerAction::Cancel => {
                    rollback(entity, &mut model, custom.contains(entity), &mut commands)
                }
                PointerAction::Release(_) => model.pointer = None,
                _ => {}
            }
        }
    }
    if escape {
        for (entity, mut model) in &mut models {
            if model.pointer.is_some() {
                rollback(entity, &mut model, custom.contains(entity), &mut commands);
            }
        }
    }
}
