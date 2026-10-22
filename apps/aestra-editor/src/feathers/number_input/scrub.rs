//! Shared policy and behavior for Blender-style numeric scrubbing.
//!
//! Domain-specific value lookup and semantic command commits stay with the owning panel. Generic
//! controls, such as the color picker, can opt into [`ScrubbableNumber`] and receive ordinary
//! `ValueChange<f32>` events while dragging.

use bevy::{
    input_focus::InputFocus,
    picking::pointer::PointerButton,
    prelude::*,
    text::{EditableText, TextEdit},
    ui::InteractionDisabled,
    ui_widgets::ValueChange,
    window::{CursorIcon, CursorOptions, PrimaryWindow, SystemCursorIcon},
};

use super::pointer::{EntityCursor, OverrideCursor, PointerDrag, PointerDragEnd, PointerDragStart};

pub(crate) struct NumberScrubPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NumberScrubSet;

impl Plugin for NumberScrubPlugin {
    fn build(&self, app: &mut App) {
        super::widget::register(app);
        app.init_resource::<NumberScrubState>()
            .add_observer(begin_number_scrub)
            .add_observer(update_number_scrub)
            .add_observer(finish_number_scrub)
            .add_systems(Update, decorate_scrubbable_numbers.in_set(NumberScrubSet));
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ScrubbableNumber {
    pub(crate) value: f32,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) step: f32,
}

impl ScrubbableNumber {
    pub(crate) fn new(value: f32, min: f32, max: f32, step: f32) -> Self {
        Self {
            value,
            min,
            max,
            step,
        }
    }

    pub(super) fn normalize(self, value: f32, multiplier: f32) -> f32 {
        rounded(
            value.clamp(self.min, self.max),
            decimal_places(self.step * multiplier),
        )
    }
}

#[derive(Component)]
pub(crate) struct ScrubCursorDecorated;

#[derive(Debug, Clone, Copy)]
struct ActiveNumberScrub {
    entity: Entity,
    initial: f32,
    raw: f32,
    current: f32,
}

#[derive(Resource, Default)]
pub(crate) struct NumberScrubState {
    active: Option<ActiveNumberScrub>,
}

impl NumberScrubState {
    pub(super) fn is_scrubbing(&self, entity: Entity) -> bool {
        self.active.is_some_and(|active| active.entity == entity)
    }
}

pub(crate) fn decorate_scrubbable_numbers(
    mut commands: Commands,
    children: Query<&Children>,
    inputs: Query<Entity, (With<ScrubbableNumber>, Without<ScrubCursorDecorated>)>,
) {
    for entity in &inputs {
        commands.entity(entity).insert((
            ScrubCursorDecorated,
            EntityCursor::System(SystemCursorIcon::EwResize),
        ));
        if let Ok(children) = children.get(entity) {
            for child in children.iter() {
                commands
                    .entity(child)
                    .insert(EntityCursor::System(SystemCursorIcon::EwResize));
            }
        }
    }
}

pub(crate) fn begin_number_scrub(
    mut drag: On<PointerDragStart>,
    inputs: Query<&ScrubbableNumber>,
    hierarchy: (Query<&ChildOf>, Query<(), With<InteractionDisabled>>),
    focus: Res<InputFocus>,
    mut state: ResMut<NumberScrubState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursors: Query<(&mut CursorIcon, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let (parents, disabled) = hierarchy;
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some((entity, input)) = resolve_scrubbable_number(drag.entity, &parents, &inputs) else {
        return;
    };
    if disabled.contains(entity)
        || parents
            .iter_ancestors(entity)
            .any(|ancestor| disabled.contains(ancestor))
        || focus
            .get()
            .is_some_and(|focused| event_belongs_to(focused, entity, &parents))
    {
        return;
    }
    drag.propagate(false);
    state.active = Some(ActiveNumberScrub {
        entity,
        initial: input.value,
        raw: input.value,
        current: input.value,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::EwResize));
    if let Ok((mut icon, mut options)) = cursors.single_mut() {
        *icon = CursorIcon::System(SystemCursorIcon::EwResize);
        options.visible = false;
    }
}

pub(crate) fn update_number_scrub(
    mut drag: On<PointerDrag>,
    keys: Res<ButtonInput<KeyCode>>,
    inputs: Query<&ScrubbableNumber>,
    hierarchy: (Query<&ChildOf>, Query<&Children>),
    mut texts: Query<&mut EditableText>,
    mut state: ResMut<NumberScrubState>,
    mut commands: Commands,
) {
    let (parents, children) = hierarchy;
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if !event_belongs_to(drag.entity, active.entity, &parents) {
        return;
    }
    let Ok(input) = inputs.get(active.entity) else {
        return;
    };
    drag.propagate(false);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let multiplier = scrub_multiplier(shift, control);
    let delta = scrub_delta(drag.delta.x, input.step, multiplier);
    if delta == 0.0 {
        return;
    }
    active.raw += delta;
    active.current = input.normalize(active.raw, multiplier);
    replace_number_text(
        active.entity,
        formatted(active.current, decimal_places(input.step * multiplier)),
        &children,
        &mut texts,
    );
    commands.trigger(ValueChange {
        source: active.entity,
        value: active.current,
        is_final: false,
    });
}

pub(crate) fn finish_number_scrub(
    mut drag: On<PointerDragEnd>,
    parents: Query<&ChildOf>,
    mut state: ResMut<NumberScrubState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursors: Query<(&mut CursorIcon, &mut CursorOptions), With<PrimaryWindow>>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(active) = state.active.take() else {
        return;
    };
    if !event_belongs_to(drag.entity, active.entity, &parents) {
        state.active = Some(active);
        return;
    }
    drag.propagate(false);
    override_cursor.0 = None;
    if let Ok((mut icon, mut options)) = cursors.single_mut() {
        *icon = CursorIcon::System(SystemCursorIcon::EwResize);
        options.visible = true;
    }
    commands.trigger(ValueChange {
        source: active.entity,
        value: if (active.current - active.initial).abs() <= f32::EPSILON {
            active.initial
        } else {
            active.current
        },
        is_final: true,
    });
}

fn resolve_scrubbable_number<'a>(
    entity: Entity,
    parents: &Query<&ChildOf>,
    inputs: &'a Query<&ScrubbableNumber>,
) -> Option<(Entity, &'a ScrubbableNumber)> {
    let mut candidate = entity;
    for _ in 0..4 {
        if let Ok(input) = inputs.get(candidate) {
            return Some((candidate, input));
        }
        candidate = parents.get(candidate).ok()?.parent();
    }
    None
}

fn event_belongs_to(entity: Entity, owner: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut candidate = entity;
    for _ in 0..4 {
        if candidate == owner {
            return true;
        }
        let Ok(parent) = parents.get(candidate) else {
            return false;
        };
        candidate = parent.parent();
    }
    false
}

pub(super) fn replace_number_text(
    entity: Entity,
    value: String,
    children: &Query<&Children>,
    texts: &mut Query<&mut EditableText>,
) {
    for descendant in children.iter_descendants(entity) {
        let Ok(mut editable) = texts.get_mut(descendant) else {
            continue;
        };
        set_number_text(&mut editable, &value);
        return;
    }
}

/// Replace a model-owned value without selecting text through an asynchronously loaded layout.
/// A replacement supersedes pending edits from an earlier model value, not an active typed edit.
pub(super) fn set_number_text(editable: &mut EditableText, value: &str) {
    editable.clear();
    editable.editor_mut().set_text(value);
    editable.queue_edit(TextEdit::TextEnd(false));
}

pub(crate) fn scrub_multiplier(shift: bool, control: bool) -> f32 {
    if shift {
        0.1
    } else if control {
        10.0
    } else {
        1.0
    }
}

pub(crate) fn scrub_delta(pixel_delta: f32, step: f32, multiplier: f32) -> f32 {
    pixel_delta * step * multiplier / 8.0
}

pub(crate) fn decimal_places(effective_step: f32) -> usize {
    let effective_step = effective_step.abs();
    if effective_step >= 1.0 {
        0
    } else if effective_step >= 0.1 {
        1
    } else if effective_step >= 0.01 {
        2
    } else if effective_step >= 0.001 {
        3
    } else {
        4
    }
}

pub(crate) fn rounded(value: f32, precision: usize) -> f32 {
    let factor = 10.0_f32.powi(precision as i32);
    (value * factor).round() / factor
}

pub(crate) fn formatted(value: f32, precision: usize) -> String {
    let zero_threshold = 0.5 * 10.0_f32.powi(-(precision as i32));
    let value = if value.abs() < zero_threshold {
        0.0
    } else {
        value
    };
    let mut formatted = format!("{value:.precision$}");
    if formatted.contains('.') {
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
    }
    formatted
}

#[cfg(test)]
#[path = "scrub_tests.rs"]
mod tests;
