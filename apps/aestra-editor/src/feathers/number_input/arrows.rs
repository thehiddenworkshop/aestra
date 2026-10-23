//! Clickable steppers are app-owned siblings of the native control, not a second scrubber.
use super::{
    NumberInputValue, ScrubbableNumber, decimal_places,
    native::{self, Model},
    rounded, scrub_multiplier,
    widget::{EditorNumberInput, NativeNumber},
};
use bevy::{
    feathers::{constants::icons, display::icon},
    input_focus::InputFocus,
    picking::{cursor::EntityCursor, hover::HoverMap},
    prelude::*,
    text::EditableText,
    ui::InteractionDisabled,
    ui_widgets::{Activate, Button},
    window::SystemCursorIcon,
};

pub(crate) struct NumberArrowsPlugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NumberArrowsSet;

impl Plugin for NumberArrowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(activate_arrow)
            .add_observer(step_number)
            .add_systems(
                Update,
                (decorate, sync_visibility).chain().in_set(NumberArrowsSet),
            );
    }
}

#[derive(Component)]
struct NumberArrowsDecorated;
#[derive(Component)]
pub(crate) struct CustomNumberStep;
#[derive(EntityEvent)]
pub(crate) struct StepNumber {
    pub(crate) entity: Entity,
    pub(crate) direction: i8,
}
#[derive(Component)]
struct NumberArrow {
    owner: Entity,
    direction: i8,
}

fn decorate(
    mut commands: Commands,
    localizer: Res<crate::Localizer>,
    inputs: Query<Entity, (With<EditorNumberInput>, Without<NumberArrowsDecorated>)>,
) {
    for owner in &inputs {
        commands.entity(owner).insert(NumberArrowsDecorated).with_children(|parent| {
            for direction in [-1, 1] {
                parent.spawn((
                    NumberArrow { owner, direction }, Button,
                    AccessibleLabel(localizer.text(if direction < 0 { "number-decrease-value" } else { "number-increase-value" })),
                    EntityCursor::System(SystemCursorIcon::Default), Visibility::Hidden, ZIndex(2),
                    BackgroundColor(crate::theme::MENU_ITEM_HOVER),
                    Node {
                        position_type: PositionType::Absolute,
                        grid_column: GridPlacement::start(2), grid_row: GridPlacement::start(1),
                        left: if direction < 0 { px(0) } else { Val::Auto },
                        right: if direction > 0 { px(0) } else { Val::Auto },
                        top: px(0), width: px(20), height: percent(100),
                        align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(px(2)), ..default()
                    },
                )).with_children(|button| {
                    button.spawn_empty().apply_scene(bsn! {
                        @icon(icons::CHEVRON_RIGHT)
                        Node { width: px(10), height: px(12) }
                        UiTransform { rotation: {Rot2::radians(if direction < 0 { std::f32::consts::PI } else { 0.0 })} }
                        Pickable::IGNORE
                    });
                });
            }
        });
    }
}

fn sync_visibility(
    hover: Res<HoverMap>,
    focus: Res<InputFocus>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    texts: Query<(), With<EditableText>>,
    mut arrows: Query<(Entity, &NumberArrow, &mut Visibility)>,
) {
    for (entity, arrow, mut visibility) in &mut arrows {
        let belongs = |e| e == arrow.owner || parents.iter_ancestors(e).any(|p| p == arrow.owner);
        let hovered = hover.values().any(|hits| hits.keys().copied().any(belongs));
        let editing = focus.get().is_some_and(|e| texts.contains(e) && belongs(e));
        let next = if (hovered || focus.get() == Some(entity))
            && !editing
            && native::enabled(entity, &parents, &disabled)
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

fn activate_arrow(
    event: On<Activate>,
    arrows: Query<&NumberArrow>,
    disabled: Query<(), With<InteractionDisabled>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if let Ok(arrow) = arrows.get(event.entity)
        && native::enabled(event.entity, &parents, &disabled)
    {
        commands.trigger(StepNumber {
            entity: arrow.owner,
            direction: arrow.direction,
        });
    }
}

fn step_number(
    event: On<StepNumber>,
    mut models: Query<
        (&mut Model, &Children, Option<&ScrubbableNumber>),
        Without<CustomNumberStep>,
    >,
    natives: Query<Option<&bevy::feathers::controls::HardLimit>, With<NativeNumber>>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if !native::enabled(event.entity, &parents, &disabled) {
        return;
    }
    let Ok((mut model, children, policy)) = models.get_mut(event.entity) else {
        return;
    };
    let Some(native) = children.iter().find(|&e| natives.contains(e)) else {
        return;
    };
    let Some(value) = model.displayed.or(model.value) else {
        return;
    };
    let multiplier = scrub_multiplier(
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
    );
    let direction = event.direction.signum();
    let next = match value {
        NumberInputValue::F32(value) => {
            let policy =
                policy
                    .copied()
                    .unwrap_or(ScrubbableNumber::new(value, -f32::MAX, f32::MAX, 0.01));
            NumberInputValue::F32(rounded(
                (value + direction as f32 * policy.step * multiplier).clamp(policy.min, policy.max),
                decimal_places(policy.step * multiplier),
            ))
        }
        NumberInputValue::F64(v) => {
            NumberInputValue::F64(v + direction as f64 * 0.01 * multiplier as f64)
        }
        NumberInputValue::I32(v) => {
            NumberInputValue::I32(v.saturating_add(direction as i32 * (multiplier as i32).max(1)))
        }
        NumberInputValue::I64(v) => {
            NumberInputValue::I64(v.saturating_add(direction as i64 * (multiplier as i64).max(1)))
        }
    };
    let next = natives
        .get(native)
        .ok()
        .flatten()
        .map_or(next, |limit| limit.0.clamp(next));
    native::step(event.entity, native, next, &mut model, &mut commands);
}
