//! Editor menu chrome, popup state, and menu-specific interaction behavior.

use crate::docking::DockingAction;
use crate::*;
use bevy::ui_widgets::Activate;
use fluent_bundle::FluentArgs;
use std::time::Duration;

const SUBMENU_HOVER_DELAY: Duration = Duration::from_millis(250);

/// Owns menu state and all menu-specific UI synchronization.
pub(crate) struct EditorMenusPlugin {
    show_grid: bool,
}

impl EditorMenusPlugin {
    pub(crate) fn new(show_grid: bool) -> Self {
        Self { show_grid }
    }
}

impl Plugin for EditorMenusPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MenuState {
            show_grid: self.show_grid,
            ..default()
        })
        .init_resource::<SubmenuHoverState>()
        .init_resource::<MenuActivationGuard>()
        .add_observer(queue_menu_activation)
        .add_systems(
            Update,
            (handle_menu_controls, open_hovered_panels_submenu)
                .chain()
                .before(crate::handle_buttons)
                .in_set(crate::EditorSet::PreViewport),
        )
        .add_systems(
            Update,
            dismiss_open_menus
                .after(crate::handle_buttons)
                .in_set(crate::EditorSet::PreViewport),
        )
        .add_systems(
            Update,
            (
                update_menu_visibility,
                update_grid_menu_check,
                update_panel_visibility_labels,
            )
                .chain()
                .in_set(crate::EditorSet::UiRebuild),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    File,
    Edit,
    View,
    Help,
}

#[derive(Resource)]
pub(crate) struct MenuState {
    pub(crate) open: Option<MenuKind>,
    pub(crate) panels_open: bool,
    pub(crate) tab_context: Option<TabContextMenu>,
    pub(crate) show_grid: bool,
    pub(crate) show_about: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: None,
            panels_open: false,
            tab_context: None,
            show_grid: true,
            show_about: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TabContextMenu {
    pub(crate) panel: DockPanel,
    pub(crate) position: [f32; 2],
}

#[derive(Component, Clone, Copy)]
enum MenuAction {
    Toggle(MenuKind),
    TogglePanelsSubmenu,
}

#[derive(Resource)]
struct SubmenuHoverState {
    timer: Timer,
}

#[derive(Resource, Default)]
struct MenuActivationGuard(bool);

impl Default for SubmenuHoverState {
    fn default() -> Self {
        Self {
            timer: Timer::new(SUBMENU_HOVER_DELAY, TimerMode::Once),
        }
    }
}

#[derive(Component)]
struct PanelsSubmenuTrigger;

#[derive(Component)]
pub(crate) struct DocumentMenuLabel;

#[derive(Component)]
pub(crate) struct UndoMenuItem;

#[derive(Component)]
pub(crate) struct RedoMenuItem;

#[derive(Component)]
pub(crate) struct AboutDescription;

#[derive(Component)]
struct PanelsSubmenu;

#[derive(Component)]
struct PanelVisibilityLabel(DockPanel);

#[derive(Component)]
struct MenuDropdown(MenuKind);

#[derive(Component)]
struct MenuButton;

#[derive(Component)]
struct GridMenuCheck;

#[derive(Component)]
struct MenuSurface;

#[derive(Component)]
struct AboutOverlay;

pub(crate) fn spawn_tab_context_menu(
    parent: &mut ChildSpawnerCommands,
    context: Option<TabContextMenu>,
    localizer: &Localizer,
) {
    let Some(context) = context else {
        return;
    };
    parent
        .spawn((
            GlobalZIndex(180),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(context.position[0]),
                top: Val::Px(context.position[1]),
                width: Val::Px(188.0),
                padding: UiRect::all(Val::Px(5.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|menu| {
            menu.spawn_empty()
                .apply_scene(ui_shell::feathers_plain_button())
                .insert((
                    DockingAction::Float(context.panel, context.position),
                    FeathersActionButton,
                    AccessibleLabel(localizer.text("dock-float-panel")),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ))
                .with_children(|item| {
                    item.spawn((
                        LocalizedText("dock-float-panel"),
                        Text::new(localizer.text("dock-float-panel")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        ThemedText,
                        Pickable::IGNORE,
                    ));
                });
        });
}

pub(crate) fn spawn_menu_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    menu: &MenuState,
    layout: &WorkspaceLayout,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                grid_row: GridPlacement::start(1),
                width: Val::Percent(100.0),
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
            ThemeBorderColor(tokens::PANE_HEADER_BORDER),
        ))
        .with_children(|bar| {
            spawn_file_menu(bar, localizer);
            spawn_standard_menu(
                bar,
                "menu-edit",
                MenuKind::Edit,
                &[
                    ("edit-undo", "Ctrl+Z", EditorAction::Undo),
                    ("edit-redo", "Ctrl+Y", EditorAction::Redo),
                    ("edit-add-emitter", "Ctrl+Enter", EditorAction::AddLayer),
                    (
                        "edit-duplicate-emitter",
                        "Ctrl+D",
                        EditorAction::DuplicateLayer,
                    ),
                    ("edit-delete-emitter", "Delete", EditorAction::DeleteLayer),
                ],
                localizer,
            );
            spawn_view_menu(bar, layout, menu.show_grid, localizer);
            spawn_standard_menu(
                bar,
                "menu-help",
                MenuKind::Help,
                &[("help-about", "", EditorAction::ShowAbout)],
                localizer,
            );
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let file = session
                .source_path
                .as_ref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled");
            bar.spawn((
                DocumentMenuLabel,
                Text::new(format!(
                    "{}{}  |  {}",
                    if session.dirty { "* " } else { "" },
                    session.effect.name,
                    file
                )),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_file_menu(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu_root| {
            menu_button(menu_root, "menu-file", MenuKind::File, localizer);
            menu_root
                .spawn_empty()
                .apply_scene(ui_shell::feathers_menu_popup())
                .insert((
                    MenuDropdown(MenuKind::File),
                    MenuSurface,
                    RelativeCursorPosition::default(),
                ))
                .with_children(|dropdown| {
                    for (message_id, shortcut, action) in [
                        ("file-new-effect", "Ctrl+N", DocumentAction::New),
                        ("file-open", "Ctrl+O", DocumentAction::Open),
                        ("file-save", "Ctrl+S", DocumentAction::Save),
                        ("file-save-as", "Ctrl+Shift+S", DocumentAction::SaveAs),
                    ] {
                        spawn_feathers_menu_item(dropdown, message_id, shortcut, action, localizer);
                    }
                    dropdown
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_divider());
                    spawn_feathers_menu_item(
                        dropdown,
                        "file-settings",
                        "",
                        DockingAction::Show(DockPanel::Settings),
                        localizer,
                    );
                    dropdown
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_divider());
                    spawn_feathers_menu_item(
                        dropdown,
                        "file-exit",
                        "Alt+F4",
                        DocumentAction::Exit,
                        localizer,
                    );
                });
        });
}

fn spawn_standard_menu(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    menu: MenuKind,
    items: &[(&'static str, &str, EditorAction)],
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu_root| {
            menu_button(menu_root, message_id, menu, localizer);
            spawn_dropdown(menu_root, menu, items, localizer);
        });
}

fn menu_button(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    menu: MenuKind,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_button())
        .insert((
            MenuButton,
            MenuAction::Toggle(menu),
            FeathersActionButton,
            AccessibleLabel(localizer.text(message_id)),
        ))
        .with_children(|button| {
            button.spawn((
                LocalizedText(message_id),
                Text::new(localizer.text(message_id)),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_dropdown(
    parent: &mut ChildSpawnerCommands,
    menu: MenuKind,
    items: &[(&'static str, &str, EditorAction)],
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_popup())
        .insert((
            MenuDropdown(menu),
            MenuSurface,
            RelativeCursorPosition::default(),
        ))
        .with_children(|dropdown| {
            for (message_id, shortcut, action) in items {
                let mut item =
                    spawn_feathers_menu_item(dropdown, message_id, shortcut, *action, localizer);
                match action {
                    EditorAction::Undo => {
                        item.insert(UndoMenuItem);
                    }
                    EditorAction::Redo => {
                        item.insert(RedoMenuItem);
                    }
                    _ => {}
                }
            }
        });
}

fn spawn_view_menu(
    parent: &mut ChildSpawnerCommands,
    layout: &WorkspaceLayout,
    show_grid: bool,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu_root| {
            menu_button(menu_root, "menu-view", MenuKind::View, localizer);
            menu_root
                .spawn_empty()
                .apply_scene(ui_shell::feathers_menu_popup())
                .insert((
                    MenuDropdown(MenuKind::View),
                    MenuSurface,
                    RelativeCursorPosition::default(),
                ))
                .with_children(|dropdown| {
                    spawn_checkable_menu_item(
                        dropdown,
                        "view-toggle-grid",
                        "G",
                        EditorAction::ToggleGrid,
                        show_grid,
                        localizer,
                    );
                    spawn_feathers_menu_item(
                        dropdown,
                        "view-frame-effect",
                        "F",
                        EditorAction::FramePreview,
                        localizer,
                    );
                    spawn_feathers_menu_item(
                        dropdown,
                        "view-restart-preview",
                        "R",
                        TransportAction::Restart,
                        localizer,
                    );
                    spawn_menu_action_item(
                        dropdown,
                        "view-panels",
                        ">",
                        MenuAction::TogglePanelsSubmenu,
                        localizer,
                    );
                    dropdown
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_divider());
                    spawn_feathers_menu_item(
                        dropdown,
                        "view-reset-workspace",
                        "",
                        DockingAction::ResetWorkspace,
                        localizer,
                    );

                    dropdown
                        .spawn((
                            PanelsSubmenu,
                            MenuSurface,
                            RelativeCursorPosition::default(),
                            GlobalZIndex(101),
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: Val::Percent(100.0),
                                top: Val::Px(63.0),
                                min_width: Val::Px(206.0),
                                padding: UiRect::axes(Val::Px(0.0), Val::Px(4.0)),
                                flex_direction: FlexDirection::Column,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            ThemeBackgroundColor(tokens::MENU_BG),
                            ThemeBorderColor(tokens::MENU_BORDER),
                        ))
                        .with_children(|submenu| {
                            for panel in DockPanel::ALL {
                                let visible = layout.is_visible(panel);
                                let mut item = submenu.spawn_empty();
                                item.apply_scene(ui_shell::feathers_menu_item()).insert((
                                    Interaction::None,
                                    DockingAction::Toggle(panel),
                                    FeathersActionButton,
                                    AccessibleLabel(panel_visibility_label(
                                        localizer, panel, visible,
                                    )),
                                ));
                                if !panel.closable() {
                                    item.insert(InteractionDisabled);
                                }
                                item.with_children(|row| {
                                    row.spawn((
                                        PanelVisibilityLabel(panel),
                                        Text::new(panel_visibility_label(
                                            localizer, panel, visible,
                                        )),
                                        ThemedText,
                                        Pickable::IGNORE,
                                    ));
                                });
                            }
                        });
                });
        });
}

fn spawn_feathers_menu_item<'a, A: Component>(
    parent: &'a mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    action: A,
    localizer: &Localizer,
) -> EntityCommands<'a> {
    let label = localizer.text(message_id);
    let mut item = parent.spawn_empty();
    item.apply_scene(ui_shell::feathers_menu_item())
        .insert((
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
        ))
        .with_children(|item| {
            spawn_menu_item_content(item, message_id, shortcut, label);
        });
    item
}

fn spawn_menu_action_item(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    action: MenuAction,
    localizer: &Localizer,
) {
    let label = localizer.text(message_id);
    let mut item = parent.spawn_empty();
    item.apply_scene(ui_shell::feathers_menu_item()).insert((
        Interaction::None,
        action,
        FeathersActionButton,
        AccessibleLabel(label.clone()),
    ));
    if matches!(action, MenuAction::TogglePanelsSubmenu) {
        item.insert(PanelsSubmenuTrigger);
    }
    item.with_children(|item| {
        spawn_menu_item_content(item, message_id, shortcut, label);
    });
}

fn spawn_menu_item_content(
    item: &mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    label: String,
) {
    item.spawn((
        LocalizedText(message_id),
        Text::new(label),
        ThemedText,
        Pickable::IGNORE,
    ));
    item.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    item.spawn((
        Text::new(shortcut),
        ThemeTextColor(tokens::TEXT_DIM),
        Pickable::IGNORE,
    ));
}

fn spawn_checkable_menu_item(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    action: EditorAction,
    checked: bool,
    localizer: &Localizer,
) {
    let label = localizer.text(message_id);
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_item())
        .insert((
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
        ))
        .with_children(|item| {
            item.spawn((
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    margin: UiRect::right(Val::Px(7.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(theme::TEXT_MUTED),
                Pickable::IGNORE,
            ))
            .with_child((
                GridMenuCheck,
                Node {
                    width: Val::Px(6.0),
                    height: Val::Px(6.0),
                    border_radius: BorderRadius::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(if checked { theme::ACCENT } else { Color::NONE }),
                Pickable::IGNORE,
            ));
            item.spawn((
                LocalizedText(message_id),
                Text::new(label),
                ThemedText,
                Pickable::IGNORE,
            ));
            item.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            item.spawn((
                Text::new(shortcut),
                ThemeTextColor(tokens::TEXT_DIM),
                Pickable::IGNORE,
            ));
        });
}

fn panel_visibility_label(localizer: &Localizer, panel: DockPanel, visible: bool) -> String {
    format!(
        "[{}]  {}",
        if visible { "x" } else { " " },
        localizer.text(panel.message_id())
    )
}

pub(crate) fn spawn_about_overlay(
    parent: &mut ChildSpawnerCommands,
    visible: bool,
    localizer: &Localizer,
) {
    parent
        .spawn((
            AboutOverlay,
            GlobalZIndex(200),
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(430.0),
                        padding: UiRect::all(Val::Px(24.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::ACCENT_DIM),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new("AESTRA"),
                        TextFont {
                            font_size: FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    let mut args = FluentArgs::new();
                    args.set("version", env!("CARGO_PKG_VERSION"));
                    dialog.spawn((
                        AboutDescription,
                        Text::new(localizer.text_with("about-description", &args)),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::justify(Justify::Center),
                    ));
                    localized_action_button(
                        dialog,
                        "common-close",
                        EditorAction::CloseAbout,
                        localizer,
                    );
                });
        });
}

fn handle_menu_controls(
    mut commands: Commands,
    mut controls: Query<
        (
            Entity,
            &Interaction,
            &MenuAction,
            Has<PendingFeathersActivation>,
        ),
        Changed<Interaction>,
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
    mut activation_guard: ResMut<MenuActivationGuard>,
) {
    for (entity, interaction, action, pending_activation) in &mut controls {
        match *interaction {
            Interaction::Hovered => {
                if let MenuAction::Toggle(kind) = *action
                    && let Some(next) = menu_after_hover(menu.open, kind)
                    && menu.open != Some(next)
                {
                    menu.open = Some(next);
                    menu.panels_open = false;
                }
            }
            Interaction::Pressed => {
                if !pending_activation {
                    continue;
                }
                activation_guard.0 = true;
                commands
                    .entity(entity)
                    .remove::<PendingFeathersActivation>()
                    .insert(Interaction::None);
                match *action {
                    MenuAction::Toggle(kind) => {
                        if menu.tab_context.take().is_some() {
                            session.ui_revision += 1;
                        }
                        menu.panels_open = false;
                        menu.open = if menu.open == Some(kind) {
                            None
                        } else {
                            Some(kind)
                        };
                    }
                    MenuAction::TogglePanelsSubmenu => {
                        menu.panels_open = !menu.panels_open;
                    }
                }
            }
            Interaction::None => {}
        }
    }
}

fn queue_menu_activation(
    activate: On<Activate>,
    actions: Query<(), (With<MenuAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn open_hovered_panels_submenu(
    time: Res<Time>,
    interactions: Query<&Interaction, With<PanelsSubmenuTrigger>>,
    mut hover: ResMut<SubmenuHoverState>,
    mut menu: ResMut<MenuState>,
) {
    let hovering = interactions
        .iter()
        .any(|interaction| *interaction == Interaction::Hovered)
        && menu.open == Some(MenuKind::View);
    if !hovering || menu.panels_open {
        hover.timer.reset();
        return;
    }
    hover.timer.tick(time.delta());
    if hover.timer.just_finished() {
        menu.panels_open = true;
    }
}

fn dismiss_open_menus(
    buttons: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
    mut activation_guard: ResMut<MenuActivationGuard>,
    menu_surfaces: Query<&RelativeCursorPosition, With<MenuSurface>>,
    menu_buttons: Query<(&Interaction, Has<Pressed>), With<MenuButton>>,
) {
    let menu_activated = std::mem::take(&mut activation_guard.0);
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if menu.tab_context.take().is_some() {
        session.ui_revision += 1;
    }
    if menu.open.is_none() {
        return;
    }
    if menu_activated {
        return;
    }
    let pointer_over_menu = menu_surfaces
        .iter()
        .any(RelativeCursorPosition::cursor_over);
    let menu_button_pressed = menu_buttons.iter().any(|(interaction, feathers_pressed)| {
        *interaction == Interaction::Pressed || feathers_pressed
    });
    if should_dismiss_open_menu(pointer_over_menu, menu_button_pressed) {
        menu.open = None;
        menu.panels_open = false;
    }
}

fn should_dismiss_open_menu(pointer_over_menu: bool, menu_button_pressed: bool) -> bool {
    !pointer_over_menu && !menu_button_pressed
}

fn menu_after_hover(open: Option<MenuKind>, hovered: MenuKind) -> Option<MenuKind> {
    open.map(|_| hovered)
}

#[allow(clippy::type_complexity)]
fn update_menu_visibility(
    menu: Res<MenuState>,
    mut dropdowns: Query<(&MenuDropdown, &mut Node, &mut Visibility)>,
    mut panels_submenus: Query<
        &mut Node,
        (
            With<PanelsSubmenu>,
            Without<MenuDropdown>,
            Without<AboutOverlay>,
        ),
    >,
    mut about: Query<
        &mut Node,
        (
            With<AboutOverlay>,
            Without<MenuDropdown>,
            Without<PanelsSubmenu>,
        ),
    >,
) {
    if !menu.is_changed() {
        return;
    }
    for (dropdown, mut node, mut visibility) in &mut dropdowns {
        let visible = menu.open == Some(dropdown.0);
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut node in &mut panels_submenus {
        node.display = if menu.open == Some(MenuKind::View) && menu.panels_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut about {
        node.display = if menu.show_about {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn update_grid_menu_check(
    menu: Res<MenuState>,
    mut checks: Query<&mut BackgroundColor, With<GridMenuCheck>>,
) {
    if !menu.is_changed() {
        return;
    }
    let color = if menu.show_grid {
        theme::ACCENT
    } else {
        Color::NONE
    };
    for mut background in &mut checks {
        background.0 = color;
    }
}

fn update_panel_visibility_labels(
    layout: Res<WorkspaceLayout>,
    localizer: Res<Localizer>,
    mut labels: Query<(&PanelVisibilityLabel, &mut Text)>,
) {
    if !layout.is_changed() && !localizer.is_changed() {
        return;
    }
    for (label, mut text) in &mut labels {
        text.0 = panel_visibility_label(&localizer, label.0, layout.is_visible(label.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_plugin_owns_initial_state_and_runs_without_spawned_chrome() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(EditorSession::from_embedded_sample(
                EFFECT_SOURCE,
                EFFECT_PATH,
            ))
            .insert_resource(WorkspaceLayout::default())
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorMenusPlugin::new(false));

        app.update();

        assert!(!app.world().resource::<MenuState>().show_grid);
        assert!(app.world().contains_resource::<SubmenuHoverState>());
    }

    #[test]
    fn open_menu_only_dismisses_for_clicks_outside_its_surfaces() {
        assert!(should_dismiss_open_menu(false, false));
        assert!(!should_dismiss_open_menu(true, false));
        assert!(!should_dismiss_open_menu(false, true));
    }

    #[test]
    fn feathers_menu_button_press_does_not_immediately_dismiss_its_menu() {
        let mut app = App::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        app.insert_resource(mouse);
        app.insert_resource(MenuState {
            open: Some(MenuKind::File),
            ..default()
        });
        app.init_resource::<MenuActivationGuard>();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ));
        app.world_mut()
            .spawn((MenuButton, Interaction::None, Pressed));
        app.add_systems(Update, dismiss_open_menus);

        app.update();

        assert_eq!(
            app.world().resource::<MenuState>().open,
            Some(MenuKind::File)
        );
    }

    #[test]
    fn hovering_switches_between_open_top_level_menus() {
        assert_eq!(menu_after_hover(None, MenuKind::Edit), None);
        assert_eq!(
            menu_after_hover(Some(MenuKind::File), MenuKind::Edit),
            Some(MenuKind::Edit)
        );
        assert_eq!(
            menu_after_hover(Some(MenuKind::View), MenuKind::View),
            Some(MenuKind::View)
        );
    }

    #[test]
    fn submenu_hover_uses_a_short_nonzero_delay() {
        let mut hover = SubmenuHoverState::default();
        hover
            .timer
            .tick(SUBMENU_HOVER_DELAY.saturating_sub(Duration::from_millis(1)));
        assert!(!hover.timer.is_finished());
        hover.timer.tick(Duration::from_millis(1));
        assert!(hover.timer.just_finished());
    }

    #[test]
    fn grid_menu_check_tracks_persisted_visibility() {
        let mut app = App::new();
        app.insert_resource(MenuState {
            show_grid: true,
            ..default()
        });
        let check = app
            .world_mut()
            .spawn((GridMenuCheck, BackgroundColor(Color::NONE)))
            .id();
        app.add_systems(Update, update_grid_menu_check);

        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(check).unwrap().0,
            theme::ACCENT
        );

        app.world_mut().resource_mut::<MenuState>().show_grid = false;
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(check).unwrap().0,
            Color::NONE
        );
    }

    #[test]
    fn menu_state_controls_popup_display_and_visibility() {
        let mut app = App::new();
        app.insert_resource(MenuState {
            open: Some(MenuKind::File),
            ..default()
        });
        app.add_systems(Update, update_menu_visibility);
        let file = app
            .world_mut()
            .spawn((
                MenuDropdown(MenuKind::File),
                Node::default(),
                Visibility::Hidden,
            ))
            .id();
        let edit = app
            .world_mut()
            .spawn((
                MenuDropdown(MenuKind::Edit),
                Node::default(),
                Visibility::Visible,
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<Node>(file).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Visibility>(file),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Node>(edit).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Visibility>(edit),
            Some(&Visibility::Hidden)
        );
    }

    #[test]
    fn menu_actions_own_top_level_popup_state() {
        let mut app = App::new();
        app.insert_resource(MenuState::default());
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        app.insert_resource(mouse);
        app.init_resource::<MenuActivationGuard>();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ));
        let control = app
            .world_mut()
            .spawn((
                MenuAction::Toggle(MenuKind::Edit),
                MenuButton,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        app.add_observer(queue_menu_activation)
            .add_systems(Update, (handle_menu_controls, dismiss_open_menus).chain());

        app.world_mut().trigger(Activate { entity: control });
        app.update();

        assert_eq!(
            app.world().resource::<MenuState>().open,
            Some(MenuKind::Edit)
        );
        assert!(
            !app.world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
    }

    #[test]
    fn panel_visibility_labels_use_checkbox_notation() {
        let localizer = Localizer::new("en-US").unwrap();
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::Diagnostics, true),
            "[x]  DIAGNOSTICS"
        );
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::Diagnostics, false),
            "[ ]  DIAGNOSTICS"
        );
    }
}
