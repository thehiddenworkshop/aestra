//! Hover-only numeric steppers. Reserve their space so hovering never shifts the value.

use super::{ScrubbableNumber, decimal_places, formatted, replace_number_text, scrub_multiplier};
use bevy::{
    feathers::{
        controls::{FeathersNumberInput, FeathersTextInput, NumberFormat},
        cursor::EntityCursor,
    },
    input_focus::InputFocus,
    picking::hover::HoverMap,
    prelude::*,
    text::EditableText,
    ui::InteractionDisabled,
    ui_widgets::{Activate, Button, ValueChange},
    window::SystemCursorIcon,
};
use bevy_resvg::prelude::UiSvg;

pub(crate) struct NumberArrowsPlugin;

impl Plugin for NumberArrowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(activate_arrow)
            .add_observer(step_number)
            .add_systems(
                Update,
                (decorate, sync_visibility)
                    .chain()
                    .in_set(super::super::AestraFeathersSet::Sync),
            );
    }
}

#[derive(Component)]
struct NumberArrowsDecorated;

/// Panels with semantic constraints and command history handle their own step requests.
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
    assets: Res<AssetServer>,
    localizer: Res<crate::Localizer>,
    inputs: Query<(Entity, &Children), (With<FeathersNumberInput>, Without<NumberArrowsDecorated>)>,
    texts: Query<(), With<FeathersTextInput>>,
) {
    for (owner, children) in &inputs {
        let Some(index) = children.iter().position(|child| texts.contains(child)) else {
            continue;
        };
        let mut ordered: Vec<_> = children.iter().collect();
        let mut arrows = Vec::new();
        for (direction, path, label) in [
            (-1, "icons/chevron-left.svg", "number-decrease-value"),
            (1, "icons/chevron-right.svg", "number-increase-value"),
        ] {
            let arrow = commands
                .spawn((
                    NumberArrow { owner, direction },
                    Button,
                    AccessibleLabel(localizer.text(label)),
                    EntityCursor::System(SystemCursorIcon::Default),
                    Visibility::Hidden,
                    Node {
                        width: Val::Px(10.0),
                        min_width: Val::Px(10.0),
                        height: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button.spawn((
                        UiSvg(crate::feathers::icon::load_svg_icon(&assets, path)),
                        Node {
                            width: Val::Px(8.0),
                            height: Val::Px(10.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                })
                .id();
            arrows.push(arrow);
        }
        // Keep the editable child directly under the number input: Feathers relies on it.
        ordered.insert(index, arrows[0]);
        ordered.push(arrows[1]);
        commands
            .entity(owner)
            .insert(NumberArrowsDecorated)
            .replace_children(&ordered);
    }
}

fn sync_visibility(
    hover: Res<HoverMap>,
    focus: Res<InputFocus>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    texts: Query<(), With<EditableText>>,
    mut arrows: Query<(&NumberArrow, &mut Visibility)>,
) {
    for (arrow, mut visibility) in &mut arrows {
        let belongs = |entity| {
            entity == arrow.owner
                || parents
                    .iter_ancestors(entity)
                    .any(|parent| parent == arrow.owner)
        };
        let hovered = hover.values().any(|hits| hits.keys().copied().any(belongs));
        let editing = focus
            .get()
            .is_some_and(|entity| texts.contains(entity) && belongs(entity));
        let enabled = !disabled.contains(arrow.owner)
            && !parents
                .iter_ancestors(arrow.owner)
                .any(|parent| disabled.contains(parent));
        let next = if hovered && enabled && !editing {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::AssetPlugin,
        picking::{backend::HitData, pointer::PointerId},
    };
    use bevy_resvg::prelude::SvgFile;

    #[derive(Resource, Default)]
    struct Changes(Vec<(Entity, f32, bool)>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<SvgFile>()
            .init_resource::<HoverMap>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Changes>()
            .insert_resource(crate::Localizer::new("en-US").unwrap())
            .add_plugins(NumberArrowsPlugin)
            .add_observer(
                |change: On<ValueChange<f32>>, mut changes: ResMut<Changes>| {
                    changes
                        .0
                        .push((change.source, change.value, change.is_final));
                },
            );
        app
    }

    fn input(app: &mut App) -> (Entity, Entity) {
        let owner = app
            .world_mut()
            .spawn((
                FeathersNumberInput,
                NumberFormat::F32,
                ScrubbableNumber::new(0.5, 0.0, 1.0, 0.1),
            ))
            .id();
        let label = app
            .world_mut()
            .spawn((Node::default(), ChildOf(owner)))
            .id();
        let text = app
            .world_mut()
            .spawn((FeathersTextInput, EditableText::new("0.5"), ChildOf(owner)))
            .id();
        app.update();
        let children = app.world().get::<Children>(owner).unwrap();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0], label);
        assert_eq!(
            children[2], text,
            "text remains a direct child, after the left arrow"
        );
        assert!(app.world().get::<NumberArrow>(children[1]).is_some());
        assert!(app.world().get::<NumberArrow>(children[3]).is_some());
        (owner, text)
    }

    fn hover(app: &mut App, entity: Entity) {
        let mut hover = app.world_mut().resource_mut::<HoverMap>();
        hover.clear();
        hover
            .entry(PointerId::Mouse)
            .or_default()
            .insert(entity, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
    }

    fn arrows(app: &mut App, owner: Entity) -> Vec<Entity> {
        app.world_mut()
            .query::<(Entity, &NumberArrow)>()
            .iter(app.world())
            .filter_map(|(entity, arrow)| (arrow.owner == owner).then_some(entity))
            .collect()
    }

    fn assert_visibility(app: &mut App, owner: Entity, expected: Visibility) {
        for entity in arrows(app, owner) {
            assert_eq!(*app.world().get::<Visibility>(entity).unwrap(), expected);
            assert_eq!(
                app.world().get::<Node>(entity).unwrap().width,
                Val::Px(10.0)
            );
        }
    }

    #[test]
    fn numeric_arrows_follow_descendant_hover_without_shifting_layout_or_duplicating() {
        let mut app = app();
        let (owner, text) = input(&mut app);
        assert_visibility(&mut app, owner, Visibility::Hidden);
        hover(&mut app, text);
        app.update();
        assert_visibility(&mut app, owner, Visibility::Visible);
        let arrow = arrows(&mut app, owner)[0];
        hover(&mut app, arrow);
        app.update();
        assert_visibility(&mut app, owner, Visibility::Visible);
        app.world_mut().resource_mut::<HoverMap>().clear();
        app.update();
        assert_visibility(&mut app, owner, Visibility::Hidden);
        assert_eq!(arrows(&mut app, owner).len(), 2);
    }

    #[test]
    fn numeric_arrows_hide_during_text_editing_and_for_disabled_ancestors() {
        let mut app = app();
        let (owner, text) = input(&mut app);
        hover(&mut app, text);
        app.world_mut()
            .insert_resource(InputFocus::from_entity(text));
        app.update();
        assert_visibility(&mut app, owner, Visibility::Hidden);
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        assert_visibility(&mut app, owner, Visibility::Visible);
        let parent = app.world_mut().spawn(InteractionDisabled).id();
        app.world_mut().entity_mut(owner).insert(ChildOf(parent));
        app.update();
        assert_visibility(&mut app, owner, Visibility::Hidden);
        let arrow = arrows(&mut app, owner)[0];
        app.world_mut().trigger(Activate { entity: arrow });
        app.world_mut().flush();
        assert!(app.world().resource::<Changes>().0.is_empty());
    }

    #[test]
    fn numeric_arrow_activation_emits_final_change_with_scrub_step_and_limits() {
        let mut app = app();
        let (owner, _) = input(&mut app);
        let right = app
            .world_mut()
            .query::<(Entity, &NumberArrow)>()
            .iter(app.world())
            .find_map(|(entity, arrow)| (arrow.direction == 1).then_some(entity))
            .unwrap();
        app.world_mut().trigger(Activate { entity: right });
        app.world_mut().flush();
        assert_eq!(
            app.world().resource::<Changes>().0,
            vec![(owner, 0.6, true)]
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.world_mut().trigger(StepNumber {
            entity: owner,
            direction: 1,
        });
        app.world_mut().flush();
        assert_eq!(app.world().resource::<Changes>().0.last().unwrap().1, 1.0);
    }

    #[test]
    fn custom_numeric_steps_do_not_emit_a_second_generic_change() {
        let mut app = app();
        let (owner, _) = input(&mut app);
        app.world_mut().entity_mut(owner).insert(CustomNumberStep);
        app.world_mut().trigger(StepNumber {
            entity: owner,
            direction: 1,
        });
        app.world_mut().flush();
        assert!(app.world().resource::<Changes>().0.is_empty());
    }

    #[test]
    fn numeric_arrow_step_ignores_invalid_text() {
        let mut app = app();
        let (owner, text) = input(&mut app);
        app.world_mut()
            .get_mut::<EditableText>(text)
            .unwrap()
            .editor_mut()
            .set_text("-");
        app.world_mut().trigger(StepNumber {
            entity: owner,
            direction: 1,
        });
        app.world_mut().flush();
        assert!(app.world().resource::<Changes>().0.is_empty());
    }

    #[test]
    fn numeric_integer_steps_preserve_large_values_and_saturate() {
        #[derive(Resource, Default)]
        struct Integers(Vec<i64>);
        let mut app = app();
        app.init_resource::<Integers>().add_observer(
            |change: On<ValueChange<i64>>, mut values: ResMut<Integers>| {
                values.0.push(change.value)
            },
        );
        let (owner, text) = input(&mut app);
        app.world_mut().entity_mut(owner).insert(NumberFormat::I64);
        app.world_mut()
            .get_mut::<EditableText>(text)
            .unwrap()
            .editor_mut()
            .set_text("9007199254740993");
        app.world_mut().trigger(StepNumber {
            entity: owner,
            direction: 1,
        });
        app.world_mut().flush();
        assert_eq!(
            app.world().resource::<Integers>().0,
            vec![9_007_199_254_740_994]
        );
        app.world_mut()
            .get_mut::<EditableText>(text)
            .unwrap()
            .editor_mut()
            .set_text(&i64::MAX.to_string());
        app.world_mut().trigger(StepNumber {
            entity: owner,
            direction: 1,
        });
        app.world_mut().flush();
        assert_eq!(
            *app.world().resource::<Integers>().0.last().unwrap(),
            i64::MAX
        );
    }
}

fn activate_arrow(
    activate: On<Activate>,
    arrows: Query<&NumberArrow>,
    disabled: Query<(), With<InteractionDisabled>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Ok(arrow) = arrows.get(activate.entity) else {
        return;
    };
    if disabled.contains(arrow.owner)
        || parents
            .iter_ancestors(activate.entity)
            .any(|entity| disabled.contains(entity))
    {
        return;
    }
    commands.trigger(StepNumber {
        entity: arrow.owner,
        direction: arrow.direction,
    });
}

fn step_number(
    step: On<StepNumber>,
    inputs: Query<(&NumberFormat, Option<&ScrubbableNumber>), Without<CustomNumberStep>>,
    keys: Res<ButtonInput<KeyCode>>,
    children: Query<&Children>,
    mut texts: Query<&mut EditableText>,
    mut commands: Commands,
) {
    let Ok((format, scrub)) = inputs.get(step.entity) else {
        return;
    };
    let Some(text) = children
        .iter_descendants(step.entity)
        .find_map(|entity| texts.get(entity).ok().map(|text| text.value().to_string()))
    else {
        return;
    };
    let multiplier = scrub_multiplier(
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
    );
    macro_rules! change {
        ($value:expr, $text:expr) => {{
            replace_number_text(step.entity, $text, &children, &mut texts);
            commands.trigger(ValueChange {
                source: step.entity,
                value: $value,
                is_final: true,
            });
        }};
    }
    match format {
        NumberFormat::F32 => {
            let Ok(value) = text.parse::<f32>() else {
                return;
            };
            let config =
                scrub
                    .copied()
                    .unwrap_or(ScrubbableNumber::new(value, -f32::MAX, f32::MAX, 0.01));
            let next = config.normalize(
                value + step.direction as f32 * config.step * multiplier,
                multiplier,
            );
            if next.is_finite() {
                change!(
                    next,
                    formatted(next, decimal_places(config.step * multiplier))
                );
            }
        }
        NumberFormat::F64 => {
            let Ok(value) = text.parse::<f64>() else {
                return;
            };
            let next = value + step.direction as f64 * 0.01 * multiplier as f64;
            if next.is_finite() {
                change!(next, next.to_string());
            }
        }
        NumberFormat::I32 => {
            let Ok(value) = text.parse::<i32>() else {
                return;
            };
            let next = value.saturating_add(step.direction as i32 * (multiplier as i32).max(1));
            change!(next, next.to_string());
        }
        NumberFormat::I64 => {
            let Ok(value) = text.parse::<i64>() else {
                return;
            };
            let next = value.saturating_add(step.direction as i64 * (multiplier as i64).max(1));
            change!(next, next.to_string());
        }
    }
}
