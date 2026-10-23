//! Native tab input; the persisted docking model remains the selection authority.
use super::*;
use bevy::{
    input_focus::{FocusCause, InputFocus, InputFocusSystems, InputFocusVisible},
    picking::hover::Hovered,
    ui::InteractionDisabled,
    ui_widgets::{MenuFocusSystem, SelectedTab, Tab, TabList, ValueChange},
};

#[derive(Component)]
pub(super) struct DockTabStrip(pub(super) DockNodeId);

#[derive(EntityEvent)]
pub(super) struct SelectDockTab {
    pub(super) entity: Entity,
}

type AncestorState = (
    Option<&'static ChildOf>,
    Option<&'static Node>,
    Option<&'static Visibility>,
    Has<InteractionDisabled>,
);

#[derive(SystemParam)]
pub(super) struct DockTabTargets<'w, 's> {
    tabs: Query<'w, 's, (Entity, &'static DockTabButton, &'static ChildOf), With<Tab>>,
    strips: Query<'w, 's, &'static DockTabStrip, With<TabList>>,
    ancestors: Query<'w, 's, AncestorState>,
}

impl DockTabTargets<'_, '_> {
    fn visible(&self, entity: Entity) -> bool {
        let mut current = Some(entity);
        while let Some(entity) = current {
            let Ok((parent, node, visibility, disabled)) = self.ancestors.get(entity) else {
                return false;
            };
            if disabled
                || node.is_some_and(|node| node.display == Display::None)
                || visibility.is_some_and(|visibility| *visibility == Visibility::Hidden)
            {
                return false;
            }
            current = parent.map(ChildOf::parent);
        }
        true
    }

    pub(super) fn resolve(&self, entity: Entity, layout: &WorkspaceLayout) -> Option<DockTab> {
        let (_, tab, parent) = self.tabs.get(entity).ok()?;
        let strip = self.strips.get(parent.parent()).ok()?;
        let DockNode::Tabs { stack, .. } = super::find_dock_node(&layout.root, strip.0)? else {
            return None;
        };
        (stack.tabs.contains(&tab.0) && self.visible(entity)).then_some(tab.0)
    }
}

pub(super) fn request_selection(
    event: On<ValueChange<Option<Entity>>>,
    targets: DockTabTargets,
    layout: Res<WorkspaceLayout>,
    drag: Option<Res<DockDragState>>,
    mut commands: Commands,
) {
    if drag.is_some_and(|drag| drag.0.is_some()) {
        return;
    }
    let Some(entity) = event.value.filter(|_| event.is_final) else {
        return;
    };
    if targets
        .tabs
        .get(entity)
        .is_ok_and(|(_, _, parent)| parent.parent() == event.source)
        && targets.resolve(entity, &layout).is_some()
    {
        commands.trigger(SelectDockTab { entity });
    }
}

/// Native TabList intentionally emits nothing for its selected tab. Aestra still
/// needs that click to refocus the document when another dock pane owned tools.
pub(super) fn focus_or_reselect(
    mut click: On<PointerClick>,
    targets: DockTabTargets,
    selections: Query<&SelectedTab>,
    layout: Res<WorkspaceLayout>,
    drag: Option<Res<DockDragState>>,
    mut focus: ResMut<InputFocus>,
    mut focus_visible: ResMut<InputFocusVisible>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary
        || drag.is_some_and(|drag| drag.0.is_some())
        || targets.resolve(click.entity, &layout).is_none()
    {
        return;
    }
    focus.set(click.entity, FocusCause::Pressed);
    focus_visible.0 = false;
    let Ok((_, _, parent)) = targets.tabs.get(click.entity) else {
        return;
    };
    if selections
        .get(parent.parent())
        .is_ok_and(|selected| selected.0 == Some(click.entity))
    {
        click.propagate(false);
        commands.trigger(SelectDockTab {
            entity: click.entity,
        });
    }
    // Unselected clicks bubble to native TabList and produce one ValueChange.
}

pub(super) fn stop_close_click(mut click: On<PointerClick>) {
    if click.button == PointerButton::Primary {
        click.propagate(false);
    }
}

#[derive(Resource, Default)]
struct DockTabFocusMemory(Option<(Entity, DockNodeId, DockTab)>);

pub(crate) fn register(app: &mut App) {
    app.init_resource::<DockTabFocusMemory>()
        .add_observer(super::select_dock_tab)
        .add_systems(
            PostUpdate,
            (sync_selection, restore_focus)
                .chain()
                .before(MenuFocusSystem),
        )
        .add_systems(
            PostUpdate,
            sync_style.after(InputFocusSystems::FocusChangeEvents),
        )
        .add_systems(
            PostUpdate,
            reveal_focused_tab.after(bevy::ui::UiSystems::PostLayout),
        );
}

fn sync_selection(
    layout: Res<WorkspaceLayout>,
    targets: DockTabTargets,
    children: Query<&Children>,
    mut strips: Query<(Entity, &DockTabStrip, &mut SelectedTab)>,
) {
    for (entity, strip, mut selected) in &mut strips {
        let active = super::find_dock_node(&layout.root, strip.0).and_then(|node| match node {
            DockNode::Tabs { stack, .. } => stack.active,
            _ => None,
        });
        let value = children.get(entity).ok().and_then(|children| {
            children.iter().find(|child| {
                targets
                    .resolve(*child, &layout)
                    .is_some_and(|tab| Some(tab) == active)
            })
        });
        if selected.0 != value {
            selected.0 = value;
        }
    }
}

fn restore_focus(
    mut focus: Option<ResMut<InputFocus>>,
    mut memory: ResMut<DockTabFocusMemory>,
    targets: DockTabTargets,
    layout: Res<WorkspaceLayout>,
    entities: Query<Entity>,
) {
    let Some(focus) = focus.as_mut() else {
        return;
    };
    if let Some(entity) = focus.get().filter(|entity| entities.contains(*entity)) {
        memory.0 = targets.resolve(entity, &layout).and_then(|tab| {
            let (_, _, parent) = targets.tabs.get(entity).ok()?;
            Some((entity, targets.strips.get(parent.parent()).ok()?.0, tab))
        });
        return;
    }
    let Some((old, node, tab)) = memory.0 else {
        return;
    };
    // Explicitly clearing a still-live focus is not a rebuild; never steal it back.
    if entities.contains(old) {
        memory.0 = None;
        return;
    }
    let replacement = targets.tabs.iter().find_map(|(entity, candidate, parent)| {
        (candidate.0 == tab
            && targets
                .strips
                .get(parent.parent())
                .is_ok_and(|strip| strip.0 == node)
            && targets.resolve(entity, &layout).is_some())
        .then_some(entity)
    });
    if let Some(entity) = replacement {
        focus.set(entity, FocusCause::Navigated);
        memory.0 = Some((entity, node, tab));
    } else if !targets
        .tabs
        .iter()
        .any(|(_, candidate, _)| candidate.0 == tab)
    {
        memory.0 = None;
    }
}

fn sync_style(
    focus: Option<Res<InputFocus>>,
    focus_visible: Option<Res<InputFocusVisible>>,
    layout: Res<WorkspaceLayout>,
    mut tabs: Query<
        (
            Entity,
            &DockTabButton,
            &Hovered,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            Has<Outline>,
        ),
        With<Tab>,
    >,
    mut commands: Commands,
) {
    let focused = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .filter(|_| focus_visible.as_ref().is_some_and(|visible| visible.0));
    for (entity, tab, hovered, mut node, mut background, mut border, outlined) in &mut tabs {
        let selected = layout.is_active(tab.0);
        let color = if hovered.0 {
            theme::BUTTON_HOVER
        } else if selected {
            theme::PANEL
        } else {
            theme::PANEL_DARK
        };
        if background.0 != color {
            background.0 = color;
        }
        let color = BorderColor::all(if selected {
            theme::ACCENT_DIM
        } else {
            theme::BORDER
        });
        if *border != color {
            *border = color;
        }
        let bottom = px(if selected { 0.0 } else { 1.0 });
        if node.border.bottom != bottom {
            node.border.bottom = bottom;
        }
        if focused == Some(entity) && !outlined {
            commands
                .entity(entity)
                .insert(Outline::new(px(2), px(-2), theme::ACCENT));
        } else if focused != Some(entity) && outlined {
            commands.entity(entity).remove::<Outline>();
        }
    }
}

/// Keep the established wheel mapping. Native ScrollArea would add a second
/// wheel controller, so keyboard reveal only adjusts this strip's X offset.
fn reveal_focused_tab(
    focus: Option<Res<InputFocus>>,
    tabs: Query<(&ChildOf, &ComputedNode, &UiGlobalTransform), With<Tab>>,
    mut strips: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<DockTabStrip>>,
) {
    let Some(focus) = focus.filter(|focus| focus.is_changed()) else {
        return;
    };
    let Some(entity) = focus.get() else {
        return;
    };
    let Ok((parent, tab, transform)) = tabs.get(entity) else {
        return;
    };
    let Ok((strip, strip_transform, mut scroll)) = strips.get_mut(parent.parent()) else {
        return;
    };
    let scale = strip.inverse_scale_factor();
    let left = (transform.affine().translation.x
        - tab.size().x * 0.5
        - strip_transform.affine().translation.x
        + strip.size().x * 0.5)
        * scale;
    let right = left + tab.size().x * scale;
    let width = strip.size().x * scale;
    let delta = if left < 0.0 {
        left
    } else {
        (right - width).max(0.0)
    };
    let max = (strip.content_size().x - strip.size().x).max(0.0) * scale;
    let value = (scroll.x + delta).clamp(0.0, max);
    if scroll.x != value {
        scroll.x = value;
    }
}

#[cfg(test)]
#[path = "tests/tabs.rs"]
mod tests;
