//! The released upstream chevrons are decorative, not clickable buttons. Keep
//! accessible app-owned steppers as siblings of the native control, never by
//! inspecting or modifying its private scrubber/chevron hierarchy.
use super::*;
use bevy::{
    feathers::{constants::icons, display::icon},
    picking::hover::HoverMap,
    ui_widgets::{Activate, Button},
};

#[derive(Component, Clone, Default)]
pub(super) struct Step(i32);

pub(super) fn scene(direction: i32) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: {if direction < 0 { px(0) } else { Val::Auto }},
            right: {if direction > 0 { px(0) } else { Val::Auto }},
            top: px(0), width: px(20), height: percent(100),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(2)),
        }
        ZIndex(2)
        BackgroundColor({crate::theme::MENU_ITEM_HOVER})
        Visibility::Hidden
        Button
        Step({direction})
        Children [
            @icon(icons::CHEVRON_RIGHT)
            Node { width: px(10), height: px(12) }
            UiTransform { rotation: {Rot2::radians(if direction < 0 { std::f32::consts::PI } else { 0.0 })} }
            Pickable::IGNORE
        ]
    }
}

pub(super) fn install(app: &mut App) {
    app.add_observer(step)
        .add_systems(Update, decorate.in_set(crate::EditorSet::UiSync));
}

#[allow(clippy::too_many_arguments)]
fn decorate(
    hover: Res<HoverMap>,
    focus: Res<InputFocus>,
    localizer: Res<Localizer>,
    parents: Query<&ChildOf>,
    disabled_inputs: Query<(), With<InteractionDisabled>>,
    texts: Query<(), With<EditableText>>,
    mut buttons: Query<(Entity, &Step, &mut Visibility, Option<&AccessibleLabel>)>,
    mut commands: Commands,
) {
    for (entity, step, mut visibility, label) in &mut buttons {
        let Ok(parent) = parents.get(entity) else {
            continue;
        };
        let field = parent.parent();
        let belongs = |e| e == field || parents.iter_ancestors(e).any(|p| p == field);
        let hovered = hover.values().any(|hits| hits.keys().copied().any(belongs));
        let editing = focus.get().is_some_and(|e| texts.contains(e) && belongs(e));
        let focused = focus.get() == Some(entity);
        let next =
            if (hovered || focused) && !editing && !disabled(entity, &parents, &disabled_inputs) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        if *visibility != next {
            *visibility = next;
        }
        if label.is_none() {
            commands
                .entity(entity)
                .insert(AccessibleLabel(localizer.text(if step.0 < 0 {
                    "number-decrease-value"
                } else {
                    "number-increase-value"
                })));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn step(
    event: On<Activate>,
    buttons: Query<&Step>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    disabled_inputs: Query<(), With<InteractionDisabled>>,
    mut inputs: Query<(Entity, &NumberInputValue, &mut AutosaveNumber)>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    let Ok(step) = buttons.get(event.entity) else {
        return;
    };
    let Ok(parent) = parents.get(event.entity) else {
        return;
    };
    if disabled(event.entity, &parents, &disabled_inputs) {
        return;
    }
    let Some(owner) = children
        .iter_descendants(parent.parent())
        .find(|&e| inputs.contains(e))
    else {
        return;
    };
    if disabled(owner, &parents, &disabled_inputs) {
        return;
    }
    let (_, value, mut input) = inputs.get_mut(owner).unwrap();
    let NumberInputValue::I32(value) = *value else {
        return;
    };
    let multiplier = crate::feathers::number_input::scrub_multiplier(
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
    );
    input.cancelled = false;
    input.model = settings.general.autosave_interval_seconds;
    commands.trigger(ValueChange {
        source: owner,
        value: value.saturating_add(step.0 * (multiplier as i32).max(1)),
        is_final: true,
    });
}
