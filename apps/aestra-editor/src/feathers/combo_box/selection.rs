//! Production generic selection-combo construction, shared with editor consumers.
use crate::feathers::{action_activation::FeathersActionButton, scenes};
use crate::{EditorAction, theme};
use bevy::{
    feathers::{
        constants::icons,
        controls::{
            FeathersLazyMenu, FeathersListRow, FeathersMenuButton, FeathersMenuItem,
            FeathersMenuPopup, FeathersSelect,
        },
        display::{caption, icon},
        theme::ThemedText,
    },
    prelude::*,
    scene::Ready,
    ui::{InteractionDisabled, Selected},
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    ui_widgets::{Activate, ValueChange},
};
use std::sync::Arc;
pub(crate) struct ComboOption<A = EditorAction> {
    pub(crate) label: String,
    pub(crate) selected: bool,
    pub(crate) action: A,
}

pub(crate) fn spawn_combo_control<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    accessible_label: &str,
    options: &[ComboOption<A>],
    width: f32,
) {
    // An unselected "Add..." picker is an action menu, not a select: choosing
    // the same action twice must still dispatch twice. Native selects intentionally
    // treat reselect as a no-op.
    if options.iter().filter(|option| option.selected).count() == 1 {
        spawn_native_select(parent, accessible_label, options, width);
        return;
    }
    spawn_action_combo_control(parent, value, accessible_label, options, width);
}

/// An action-only picker (including multi-select/toggle filters), whose selected
/// indicators must not turn repeated commands into a select's reselect no-op.
pub(crate) fn spawn_action_combo_control<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    accessible_label: &str,
    options: &[ComboOption<A>],
    width: f32,
) {
    parent
        .spawn(Node {
            width: Val::Px(width),
            min_width: Val::Px(112.0),
            ..default()
        })
        .with_children(|wrapper| {
            let owner = spawn_lazy_action_menu(wrapper, options);
            wrapper.commands().entity(owner).with_children(|menu| {
                menu.spawn_empty()
                    .apply_scene(scenes::feathers_menu_button())
                    .insert((
                        AccessibleLabel(accessible_label.to_owned()),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(28.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(Val::Px(8.0)),
                            ..default()
                        },
                    ))
                    .with_children(|button| {
                        if options.is_empty() {
                            let entity = button.target_entity();
                            button.commands().entity(entity).insert(InteractionDisabled);
                        }
                        button.spawn((
                            Text::new(value),
                            ThemedText,
                            Pickable::IGNORE,
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        button
                            .spawn_empty()
                            .apply_scene(icon(icons::CHEVRON_DOWN))
                            .insert(Pickable::IGNORE);
                    });
            });
        });
}

fn spawn_native_select<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption<A>],
    width: f32,
) {
    let rows = options
        .iter()
        .map(|option| -> Box<dyn SceneList> {
            let label = option.label.clone();
            let action = option.action;
            let selected = option.selected;
            bsn! {
                @FeathersListRow
                on(move |ready: On<Ready>, mut commands: Commands| {
                    let mut row = commands.entity(ready.entity);
                    row.insert((action, FeathersActionButton));
                    if selected { row.insert(Selected); }
                })
                Children [ @caption(label) ]
            }
            .into()
        })
        .collect::<Vec<_>>();
    let rows: Box<dyn SceneList> = Box::new(rows);
    let label = accessible_label.to_owned();
    parent
        .spawn(Node {
            width: px(width),
            min_width: px(112),
            ..default()
        })
        .with_children(|wrapper| {
            wrapper.spawn_empty().apply_scene(bsn! {
                @FeathersSelect { @options: rows }
                Node { width: percent(100), min_width: px(0) }
                on(move |ready: On<Ready>, children: Query<&Children>, buttons: Query<(), With<FeathersMenuButton>>, mut commands: Commands| {
                    for button in children.iter_descendants(ready.entity).filter(|e| buttons.contains(*e)) {
                        commands.entity(button).insert((AccessibleLabel(label.clone()), Node {
                            width: percent(100), height: px(28), min_width: px(0),
                            align_items: AlignItems::Center, padding: UiRect::horizontal(px(8)),
                            ..default()
                        }));
                    }
                    capture_context(&mut commands, ready.entity);
                })
                on(select_action::<A>)
                on(refresh_context_on_open)
            });
        });
}

// A menu interaction owns an epoch from opening to activation. Persistent
// browser/settings selectors may open again after unrelated authoring edits.
#[derive(Component)]
pub(super) struct MenuContext(Option<(aestra_core::EffectId, u64, u64)>);

fn context(world: &World) -> Option<(aestra_core::EffectId, u64, u64)> {
    world.get_resource::<crate::EditorSession>().map(|session| {
        (
            session.effect.id,
            session.document_revision(),
            session.ui_revision,
        )
    })
}

pub(super) fn capture_context(commands: &mut Commands, entity: Entity) {
    commands.queue(move |world: &mut World| {
        let context = MenuContext(context(world));
        if let Ok(mut owner) = world.get_entity_mut(entity) {
            owner.insert(context);
        }
    });
}

fn refresh_context_on_open(event: On<bevy::ui_widgets::MenuEvent>, mut commands: Commands) {
    if matches!(
        event.action,
        bevy::ui_widgets::MenuAction::Open(_) | bevy::ui_widgets::MenuAction::Toggle
    ) {
        capture_context(&mut commands, event.event_target());
    }
}

pub(super) fn action_is_current(world: &World, owner: Entity, target: Entity) -> bool {
    world
        .get::<MenuContext>(owner)
        .is_some_and(|scope| scope.0 == context(world))
        && world.get_entity(target).is_ok()
        && std::iter::successors(Some(target), |e| {
            world.get::<ChildOf>(*e).map(ChildOf::parent)
        })
        .all(|e| world.get::<InteractionDisabled>(e).is_none())
}

fn select_action<A: Component + Copy>(
    event: On<ValueChange<Entity>>,
    selects: Query<(), With<FeathersSelect>>,
    mut commands: Commands,
) {
    // Native ListBox emits first, then FeathersSelect re-emits from its root.
    // Forward only the latter; never dispatch twice through event bubbling.
    if !event.is_final || !selects.contains(event.source) {
        return;
    }
    let owner = event.source;
    let target = event.value;
    commands.queue(move |world: &mut World| {
        let belongs = std::iter::successors(Some(target), |e| {
            world.get::<ChildOf>(*e).map(ChildOf::parent)
        })
        .any(|e| e == owner);
        if belongs && world.get::<A>(target).is_some() && action_is_current(world, owner, target) {
            world.trigger(Activate { entity: target });
        }
    });
}

#[derive(Component, Clone, Copy)]
pub(crate) struct MenuActionTarget {
    owner: Entity,
    target: Entity,
}

/// A lazy popup destroys its visual rows on close. Semantic action targets must
/// outlive those rows until the domain's Update dispatcher consumes Activate.
pub(super) fn spawn_lazy_action_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    options: &[ComboOption<A>],
) -> Entity {
    let owner = parent.spawn_empty().id();
    let mut rows = Vec::new();
    parent.commands().entity(owner).with_children(|menu| {
        for option in options {
            let target = menu
                .spawn((
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    FeathersActionButton,
                    option.action,
                ))
                .id();
            rows.push((option.label.clone(), option.selected, target));
        }
    });
    let popup: Arc<dyn Fn() -> Box<dyn Scene> + Send + Sync> = Arc::new(move || {
        let rows: Box<dyn SceneList> = Box::new(
            rows.iter()
                .map(|(label, selected, target)| {
                    lazy_menu_row(
                        label.clone(),
                        *selected,
                        MenuActionTarget {
                            owner,
                            target: *target,
                        },
                    )
                })
                .collect::<Vec<_>>(),
        );
        Box::new(bsn! {
            @FeathersMenuPopup
            Popover {
                positions: {vec![
                    PopoverPlacement { side: PopoverSide::Bottom, align: PopoverAlign::End, gap: 2.0 },
                    PopoverPlacement { side: PopoverSide::Top, align: PopoverAlign::End, gap: 2.0 },
                    PopoverPlacement { side: PopoverSide::Left, align: PopoverAlign::Start, gap: 2.0 },
                ]},
                window_margin: 8.0,
            }
            OverrideClip
            Children [ {rows} ]
        })
    });
    parent.commands().entity(owner).apply_scene(bsn! {
        @FeathersLazyMenu { popup }
        on(|ready: On<Ready>, mut commands: Commands| {
            capture_context(&mut commands, ready.entity);
        })
        on(refresh_context_on_open)
    });
    owner
}

fn lazy_menu_row(label: String, selected: bool, target: MenuActionTarget) -> Box<dyn SceneList> {
    let indicator = selected.then(|| {
        bsn! {
            Node { width: px(6), height: px(6), border_radius: BorderRadius::all(px(2)) }
            BackgroundColor(theme::ACCENT)
            Pickable::IGNORE
        }
    });
    bsn! {
        @FeathersMenuItem
        bevy::input_focus::tab_navigation::TabIndex(0)
        AccessibleLabel({label.clone()})
        on(move |ready: On<Ready>, parents: Query<&ChildOf>, disabled: Query<(), With<InteractionDisabled>>, mut commands: Commands| {
            let is_disabled = std::iter::once(target.target).chain(parents.iter_ancestors(target.target))
                .any(|e| disabled.contains(e));
            let mut row = commands.entity(ready.entity);
            row.insert((target, FeathersActionButton));
            if is_disabled { row.insert((InteractionDisabled, bevy::input_focus::tab_navigation::TabIndex(-1))); }
        })
        on(forward_lazy_action)
        Children [
            Node { width: px(18), height: percent(100), align_items: AlignItems::Center, justify_content: JustifyContent::Center }
            Pickable::IGNORE
            Children [ @{indicator} ]
            --
            Text(label)
            ThemedText
            Pickable::IGNORE
        ]
    }.into()
}

fn forward_lazy_action(
    event: On<Activate>,
    rows: Query<&MenuActionTarget>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    let Ok(target) = rows.get(event.entity).copied() else {
        return;
    };
    if std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .any(|e| disabled.contains(e))
    {
        return;
    }
    commands.queue(move |world: &mut World| {
        // Closing may already have destroyed the visual row. The persistent
        // semantic target and its owner epoch are the authority after that point.
        if action_is_current(world, target.owner, target.target) {
            world.trigger(Activate {
                entity: target.target,
            });
        }
    });
}

/// Domain availability systems operate on the persistent typed action. Mirror
/// their result onto the native visual row so keyboard and pointer behavior agree.
pub(crate) fn sync_action_menu_availability(
    rows: Query<(Entity, &MenuActionTarget, Has<InteractionDisabled>)>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    for (row, target, was_disabled) in &rows {
        let is_disabled = std::iter::once(target.target)
            .chain(parents.iter_ancestors(target.target))
            .any(|e| disabled.contains(e));
        if is_disabled != was_disabled {
            if is_disabled {
                commands.entity(row).insert((
                    InteractionDisabled,
                    bevy::input_focus::tab_navigation::TabIndex(-1),
                ));
            } else {
                commands
                    .entity(row)
                    .remove::<InteractionDisabled>()
                    .insert(bevy::input_focus::tab_navigation::TabIndex(0));
            }
        }
    }
}

pub(super) fn spawn_combo_option<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    option: &ComboOption<A>,
) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu_item())
        .insert((
            super::selection_input::idle_menu_option(),
            option.action,
            FeathersActionButton,
            AccessibleLabel(option.label.clone()),
            // `FeathersMenuPopup` opens a modal tab group and focuses the first
            // indexed item. Without an explicit tab index the popup reports
            // `NoFocusableEntities` and closes before the action can run.
            bevy::input_focus::tab_navigation::TabIndex(0),
        ))
        .with_children(|item| {
            item.spawn((
                Node {
                    width: Val::Px(18.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|indicator| {
                if option.selected {
                    indicator.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT),
                        Pickable::IGNORE,
                    ));
                }
            });
            item.spawn((
                Text::new(option.label.clone()),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

#[cfg(test)]
#[path = "tests/selection.rs"]
mod tests;
