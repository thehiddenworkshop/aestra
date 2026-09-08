use super::{actions::BrowserAction, state::*};
use crate::{
    feathers::{
        breadcrumb::{BreadcrumbItem, BreadcrumbProps, spawn_breadcrumb},
        icon::load_svg_icon,
        node_graph::FeathersGraphNavigationBlocker,
    },
    *,
};
use aestra_project::{ProjectContent, ProjectSourceId};
use bevy::{feathers::controls::FeathersTextInput, ui::Selected, ui_widgets::ActiveDescendant};
use bevy_resvg::prelude::{SvgColor, UiSvg};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Component)]
pub(super) struct BrowserItems;
#[derive(Component)]
pub(super) struct PendingLocateRow(Entity);

/// Layout must settle before revealing a located item; this also works for wrapped grid rows.
pub(super) fn scroll_to_located_row(
    mut commands: Commands,
    mut lists: Query<(
        Entity,
        &PendingLocateRow,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
    rows: Query<(&ComputedNode, &UiGlobalTransform), With<BrowserRow>>,
) {
    for (entity, pending, viewport, transform, mut scroll) in &mut lists {
        let Ok((row, row_transform)) = rows.get(pending.0) else {
            commands.entity(entity).remove::<PendingLocateRow>();
            continue;
        };
        if viewport.size().y <= 0.0 || row.size().y <= 0.0 {
            continue;
        }
        let top = row_transform.translation.y
            - row.size().y * 0.5
            - (transform.translation.y - viewport.size().y * 0.5);
        let bottom = top + row.size().y;
        let delta = if top < 0.0 {
            top
        } else {
            (bottom - viewport.size().y).max(0.0)
        };
        scroll.y += delta * viewport.inverse_scale_factor();
        commands.entity(entity).remove::<PendingLocateRow>();
    }
}
#[derive(Component)]
pub(super) struct BrowserRow(pub(super) ProjectSourceId);
#[derive(Component)]
pub(super) struct BrowserSearch;
#[derive(Component)]
pub(super) struct SourcesPane;
#[derive(Component, Default)]
pub(super) struct SourcesSplitter {
    pub(super) drag_start_width: Option<f32>,
}
#[derive(Component)]
pub(super) struct BrowserFolderButton;
#[derive(Component)]
pub(crate) struct BrowserSurface;

/// A menu can extend beyond the panel bounds. Include its open surface in navigation
/// isolation, and remove the blocker when hidden (relative cursor data can stay stale).
pub(super) fn block_browser_popovers(
    popovers: Query<
        (Entity, &Visibility, Has<BrowserSurface>),
        With<bevy::ui_widgets::popover::Popover>,
    >,
    surfaces: Query<(), With<BrowserSurface>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    for (entity, visibility, blocked) in &popovers {
        if !parents
            .iter_ancestors(entity)
            .any(|parent| surfaces.contains(parent))
        {
            continue;
        }
        if *visibility != Visibility::Hidden && !blocked {
            commands.entity(entity).insert((
                BrowserSurface,
                FeathersGraphNavigationBlocker,
                RelativeCursorPosition::default(),
            ));
        } else if *visibility == Visibility::Hidden && blocked {
            commands
                .entity(entity)
                .remove::<(BrowserSurface, FeathersGraphNavigationBlocker)>();
        }
    }
}

struct CachedRow {
    entity: Entity,
    icon: Entity,
    kind: Kind,
}

#[derive(Component)]
pub(super) struct BrowserUi {
    toolbar: Entity,
    toolbar_actions: Vec<(Entity, BrowserAction)>,
    breadcrumbs: Entity,
    filters: Entity,
    sources: Entity,
    splitter: Entity,
    tree: Entity,
    tree_footer: Entity,
    items: Entity,
    footer: Entity,
    selection_count: Entity,
    rows: BTreeMap<ProjectSourceId, CachedRow>,
    visible: Vec<ProjectSourceId>,
    rendered: Option<AssetBrowserState>,
}

pub(crate) fn spawn_assets_panel(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    library: &LibraryState,
    state: &AssetBrowserState,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BrowserSurface,
            RelativeCursorPosition::default(),
            FeathersGraphNavigationBlocker,
        ))
        .with_children(|root| {
            root.spawn(row_node()).with_children(|bar| {
                for (label, legacy) in [("browser-mode", false), ("browser-legacy", true)] {
                    spawn_feathers_action_button(
                        bar,
                        &localizer.text(label),
                        BrowserAction::Legacy(legacy),
                        state.legacy == legacy,
                    );
                }
            });
            if state.legacy {
                spawn_library(root, session, catalog, library, localizer);
            } else {
                spawn_browser(root, state, localizer);
            }
        });
}

fn spawn_browser(
    parent: &mut ChildSpawnerCommands,
    state: &AssetBrowserState,
    localizer: &Localizer,
) {
    let mut host = parent.spawn(column_node());
    let mut ui = BrowserUi {
        toolbar: Entity::PLACEHOLDER,
        toolbar_actions: Vec::new(),
        breadcrumbs: Entity::PLACEHOLDER,
        filters: Entity::PLACEHOLDER,
        sources: Entity::PLACEHOLDER,
        splitter: Entity::PLACEHOLDER,
        tree: Entity::PLACEHOLDER,
        tree_footer: Entity::PLACEHOLDER,
        items: Entity::PLACEHOLDER,
        footer: Entity::PLACEHOLDER,
        selection_count: Entity::PLACEHOLDER,
        rows: BTreeMap::new(),
        visible: Vec::new(),
        rendered: None,
    };
    host.with_children(|root| {
        root.spawn((row_node(), BackgroundColor(theme::PANEL_DARK)))
            .with_children(|header| {
                ui.toolbar = header
                    .spawn(Node {
                        width: Val::Auto,
                        ..row_node()
                    })
                    .id();
                ui.breadcrumbs = header
                    .spawn(Node {
                        width: Val::Auto,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(150.0),
                        ..row_node()
                    })
                    .id();
            });
        root.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            width: Val::Percent(100.0),
            ..default()
        })
        .with_children(|body| {
            ui.sources = body
                .spawn((
                    SourcesPane,
                    Node {
                        width: Val::Px(state.sources_width),
                        max_width: Val::Percent(55.0),
                        min_width: Val::Px(0.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .with_children(|source| {
                    source
                        .spawn(row_node())
                        .with_children(|header| text(header, localizer.text("browser-folders")));
                    source
                        .spawn(Node {
                            flex_grow: 1.0,
                            min_height: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            ..default()
                        })
                        .with_children(|scroll| {
                            ui.tree = spawn_vertical_scroll_area(
                                scroll,
                                ScrollMemoryKey::AssetBrowserSources,
                                column_node(),
                                |_| {},
                            );
                        });
                    ui.tree_footer = source.spawn(row_node()).id();
                })
                .id();
            ui.splitter = body
                .spawn((
                    SourcesSplitter::default(),
                    EditorNativeControl,
                    EntityCursor::System(SystemCursorIcon::ColResize),
                    AccessibleLabel(localizer.text("browser-resize-sources")),
                    Node {
                        width: Val::Px(5.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(theme::BORDER),
                ))
                .id();
            body.spawn(Node {
                flex_basis: Val::Px(0.0),
                ..column_node()
            })
            .with_children(|content| {
                content.spawn(row_node()).with_children(|search| {
                    search
                        .spawn(Node {
                            width: Val::Auto,
                            min_width: Val::Px(120.0),
                            flex_grow: 1.0,
                            flex_basis: Val::Px(180.0),
                            ..default()
                        })
                        .with_children(|search| {
                            spawn_search_field(
                                search,
                                &state.query,
                                &localizer.text("browser-search"),
                                &localizer.text("library-search-clear"),
                                BrowserSearch,
                            );
                        });
                    ui.filters = search
                        .spawn(Node {
                            width: Val::Auto,
                            ..row_node()
                        })
                        .id();
                });
                content
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_basis: Val::Px(0.0),
                        ..column_node()
                    })
                    .with_children(|scroll| {
                        ui.items = spawn_vertical_scroll_area(
                            scroll,
                            ScrollMemoryKey::AssetBrowserItems,
                            items_node(state.view),
                            |_| {},
                        );
                    });
            });
            body.commands().entity(ui.items).insert((
                BrowserItems,
                ListBox,
                KeyboardNavigableList,
                TabIndex(0),
                AccessibleLabel(localizer.text("browser-items")),
            ));
        });
        ui.footer = root
            .spawn(Node {
                min_height: Val::Px(22.0),
                ..row_node()
            })
            .id();
    });
    host.insert(ui);
}

fn row_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        min_height: Val::Px(28.0),
        flex_shrink: 0.0,
        padding: UiRect::all(Val::Px(3.0)),
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(3.0),
        row_gap: Val::Px(3.0),
        ..default()
    }
}

fn column_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        min_height: Val::Px(0.0),
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

fn items_node(view: ViewMode) -> Node {
    Node {
        flex_direction: if view == ViewMode::Grid {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        },
        flex_wrap: if view == ViewMode::Grid {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        align_content: AlignContent::Start,
        padding: UiRect::all(Val::Px(4.0)),
        row_gap: Val::Px(3.0),
        column_gap: Val::Px(3.0),
        overflow: Overflow::scroll_y(),
        scrollbar_width: 0.0,
        ..column_node()
    }
}

fn item_node(view: ViewMode) -> Node {
    let grid = view == ViewMode::Grid;
    Node {
        width: if grid {
            Val::Px(120.0)
        } else {
            Val::Percent(100.0)
        },
        max_width: Val::Percent(100.0),
        height: Val::Px(if grid { 132.0 } else { 36.0 }),
        min_width: Val::Px(0.0),
        flex_shrink: 0.0,
        flex_direction: if grid {
            FlexDirection::ColumnReverse
        } else {
            FlexDirection::RowReverse
        },
        align_items: if grid {
            AlignItems::Stretch
        } else {
            AlignItems::Center
        },
        padding: UiRect::all(Val::Px(6.0)),
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        column_gap: Val::Px(7.0),
        row_gap: Val::Px(5.0),
        overflow: Overflow::clip(),
        ..default()
    }
}

fn icon(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    path: &'static str,
    size: f32,
) -> Entity {
    parent
        .spawn((
            UiSvg(load_svg_icon(assets, path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                flex_shrink: 0.0,
                align_self: AlignSelf::Center,
                ..default()
            },
        ))
        .id()
}

pub(super) fn tool(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    label: String,
    path: &'static str,
    action: BrowserAction,
    disabled: bool,
    selected: bool,
) -> Entity {
    let mut button = parent.spawn_empty();
    button
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|parent| {
            icon(parent, assets, path, 14.0);
        });
    if disabled {
        button.insert(InteractionDisabled);
    }
    if selected {
        button.insert(bevy::feathers::controls::ButtonVariant::Primary);
    }
    button.id()
}

fn text(parent: &mut ChildSpawnerCommands, value: impl Into<String>) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
        Node {
            // Wrapped, shrinkable text can resolve to zero width inside an auto-sized
            // flex button. Preserve the caption's intrinsic width; the row clips long names.
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn clear(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).despawn_children();
}

pub(super) fn sync_panel(
    mut commands: Commands,
    mut panels: Query<&mut BrowserUi>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
    search: Query<&Children, With<BrowserSearch>>,
    mut editable: Query<&mut EditableText, With<FeathersTextInput>>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut last_locate: Local<u64>,
) {
    if state.legacy {
        return;
    }
    let content = catalog.content();
    let locate_requested = *last_locate != state.locate_revision;
    for mut ui in &mut panels {
        if ui.rendered.as_ref() == Some(&*state) && !localizer.is_changed() {
            continue;
        }
        let previous = ui.rendered.as_ref();
        let mut layout_only = state.clone();
        if let Some(previous) = previous {
            layout_only.selected = previous.selected;
            layout_only.sources_width = previous.sources_width;
            layout_only.inspected = previous.inspected;
            layout_only.inspection_tab = previous.inspection_tab;
            layout_only.inspection_page = previous.inspection_page;
            layout_only.locate_revision = previous.locate_revision;
        }
        let content_changed = previous != Some(&layout_only) || localizer.is_changed();
        let navigation_changed = previous.is_none_or(|old| {
            old.folder != state.folder || old.page != state.page || old.version != state.version
        });
        if previous.is_some_and(|old| old.query != state.query) {
            for children in &search {
                for entity in children.iter() {
                    if let Ok(mut text) = editable.get_mut(entity)
                        && text.value().to_string() != state.query
                    {
                        text.editor_mut().set_text(&state.query);
                        text.queue_edit(TextEdit::TextEnd(false));
                    }
                }
            }
        }
        commands.entity(ui.sources).insert(Node {
            display: if state.sources_visible {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(state.sources_width),
            max_width: Val::Percent(55.0),
            min_width: Val::Px(0.0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            ..default()
        });
        commands.entity(ui.splitter).insert(Node {
            display: if state.sources_visible {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(5.0),
            flex_shrink: 0.0,
            ..default()
        });
        sync_chrome(&mut commands, &mut ui, &state, content, &localizer, &assets);
        if content_changed {
            let entries = state.filtered(content);
            state.page = state.page.min(entries.len().saturating_sub(1) / PAGE_SIZE);
            let visible = entries
                .iter()
                .skip(state.page * PAGE_SIZE)
                .take(PAGE_SIZE)
                .map(|e| e.id)
                .collect::<Vec<_>>();
            if state
                .selected
                .is_some_and(|id| !entries.iter().any(|e| e.id == id))
            {
                state.selected = None;
            }
            // A small retained cache keeps rows stable while switching filters/pages. Never build
            // entities for every source in the project (or let browsing grow an unbounded cache).
            let wanted = visible.iter().copied().collect::<BTreeSet<_>>();
            let stale = ui
                .rows
                .iter()
                .filter(|(id, row)| content.source(**id).is_none_or(|e| Kind::of(e) != row.kind))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in stale {
                if let Some(row) = ui.rows.remove(&id) {
                    commands.entity(row.entity).despawn();
                }
            }
            let missing = visible
                .iter()
                .filter(|id| !ui.rows.contains_key(id))
                .count();
            let excess = (ui.rows.len() + missing).saturating_sub(PAGE_SIZE * 2);
            let evicted = ui
                .rows
                .keys()
                .filter(|id| !wanted.contains(id))
                .take(excess)
                .copied()
                .collect::<Vec<_>>();
            for id in evicted {
                if let Some(row) = ui.rows.remove(&id) {
                    commands.entity(row.entity).despawn();
                }
            }
            for entry in entries.iter().skip(state.page * PAGE_SIZE).take(PAGE_SIZE) {
                let kind = Kind::of(entry);
                if !ui.rows.contains_key(&entry.id) {
                    let mut cached = None;
                    commands.entity(ui.items).with_children(|parent| {
                        let entity = spawn_action_list_row(
                            parent,
                            &display_name(entry),
                            Some(&localizer.text(kind.label())),
                            None,
                            &entry.name.to_string_lossy(),
                            BrowserRow(entry.id),
                        );
                        let mut image = Entity::PLACEHOLDER;
                        parent.commands().entity(entity).with_children(|row| {
                            image = icon(row, &assets, kind.icon(), 22.0);
                        });
                        cached = Some(CachedRow {
                            entity,
                            icon: image,
                            kind,
                        });
                    });
                    ui.rows.insert(entry.id, cached.unwrap());
                }
                let row = &ui.rows[&entry.id];
                let mut description =
                    format!("{}\n{}", localizer.text(kind.label()), entry.path.display());
                if let Some(metadata) = &entry.metadata {
                    description.push_str(&format!(
                        "\n{} B{}",
                        metadata.bytes,
                        if metadata.readonly {
                            localizer.text("browser-readonly-flag")
                        } else {
                            String::new()
                        }
                    ));
                }
                if let Some(error) = &entry.error {
                    description.push_str(&format!("\n{error}"));
                }
                if kind != Kind::Folder && kind != Kind::Effect && kind != Kind::Material {
                    description.push_str(&format!("\n{}", localizer.text("browser-read-only")));
                }
                if kind == Kind::Effect
                    && catalog.entry(entry.id).is_none_or(|effect| {
                        effect
                            .reference
                            .is_none_or(|reference| catalog.openable_path(reference).is_none())
                    })
                {
                    description.push_str(&format!(
                        "\n{}",
                        localizer.text("browser-effect-unavailable")
                    ));
                }
                commands.entity(row.entity).insert((
                    item_node(state.view),
                    ListItem,
                    KeyboardNavigableListRow,
                    EntityCursor::System(SystemCursorIcon::Pointer),
                    EditorTooltip::titled(entry.name.to_string_lossy(), description),
                ));
                let size = if state.view == ViewMode::Grid {
                    64.0
                } else {
                    22.0
                };
                commands.entity(row.icon).insert(Node {
                    width: Val::Px(size),
                    height: Val::Px(size),
                    flex_shrink: 0.0,
                    align_self: AlignSelf::Center,
                    ..default()
                });
                commands.entity(row.icon).insert(SvgColor(kind_color(kind)));
            }
            for (id, row) in &ui.rows {
                if !wanted.contains(id) {
                    commands
                        .entity(row.entity)
                        .insert(Node {
                            display: Display::None,
                            ..default()
                        })
                        .remove::<(ListItem, KeyboardNavigableListRow, Selected, Outline)>();
                }
            }
            let mut children = visible
                .iter()
                .map(|id| ui.rows[id].entity)
                .collect::<Vec<_>>();
            children.extend(
                ui.rows
                    .iter()
                    .filter(|(id, _)| !wanted.contains(id))
                    .map(|(_, row)| row.entity),
            );
            commands
                .entity(ui.items)
                .replace_children(&children)
                .insert(items_node(state.view));
            if ui.visible != visible {
                commands.entity(ui.items).insert(ActiveDescendant(
                    visible.first().map(|id| ui.rows[id].entity),
                ));
            }
            ui.visible = visible;
            if navigation_changed {
                commands.entity(ui.items).insert(ScrollPosition::default());
            }
            clear(&mut commands, ui.footer);
            commands.entity(ui.footer).with_children(|parent| {
                paging(parent, state.page, entries.len(), false, &localizer);
                ui.selection_count = parent
                    .spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ))
                    .id();
                if let Some(error) = content
                    .source(state.folder_id(content))
                    .and_then(|e| e.error.as_deref())
                {
                    text(parent, error);
                } else if entries.is_empty() {
                    text(parent, localizer.text("browser-empty"));
                }
            });
        }
        if locate_requested && let Some(row) = state.selected.and_then(|id| ui.rows.get(&id)) {
            commands.entity(ui.items).insert((
                ActiveDescendant(Some(row.entity)),
                PendingLocateRow(row.entity),
            ));
            focus.set(ui.items, bevy::input_focus::FocusCause::Navigated);
            *last_locate = state.locate_revision;
        }
        for id in &ui.visible {
            let row = &ui.rows[id];
            let selected = state.selected == Some(*id);
            commands.entity(row.entity).insert((
                BackgroundColor(if selected {
                    theme::SELECTION
                } else {
                    theme::PANEL_DARK
                }),
                BorderColor::all(if selected { theme::ACCENT } else { Color::NONE }),
            ));
            if selected {
                commands.entity(row.entity).insert(Selected);
            } else {
                commands.entity(row.entity).remove::<Selected>();
            }
        }
        let count = usize::from(state.selected.is_some());
        let mut args = FluentArgs::new();
        args.set("count", count as i64);
        commands.entity(ui.selection_count).insert(Text::new(
            localizer.text_with("browser-selection-count", &args),
        ));
        ui.rendered = Some(state.clone());
    }
}

fn sync_chrome(
    commands: &mut Commands,
    ui: &mut BrowserUi,
    state: &AssetBrowserState,
    content: &ProjectContent,
    localizer: &Localizer,
    assets: &AssetServer,
) {
    let previous = ui.rendered.as_ref();
    if ui.toolbar_actions.is_empty() {
        commands.entity(ui.toolbar).with_children(|parent| {
            // Text arrows use the same Feathers button as the icon controls.
            for (label, action, disabled) in [
                ("←", BrowserAction::Back, state.back.is_empty()),
                ("→", BrowserAction::Forward, state.forward.is_empty()),
                ("↑", BrowserAction::Up, state.folder.as_os_str().is_empty()),
            ] {
                let button = mini_button(parent, label, action);
                ui.toolbar_actions.push((button, action));
                let key = match action {
                    BrowserAction::Back => "browser-back",
                    BrowserAction::Forward => "browser-forward",
                    _ => "browser-up",
                };
                parent.commands().entity(button).insert((
                    AccessibleLabel(localizer.text(key)),
                    EditorTooltip::description(localizer.text(key)),
                ));
                if disabled {
                    parent.commands().entity(button).insert(InteractionDisabled);
                }
            }
            for (key, path, action, selected) in [
                (
                    "browser-new-folder",
                    "icons/folder.svg",
                    BrowserAction::NewFolder,
                    false,
                ),
                (
                    "browser-sources",
                    "icons/folder.svg",
                    BrowserAction::Sources,
                    state.sources_visible,
                ),
                (
                    "browser-list",
                    "icons/list.svg",
                    BrowserAction::View(ViewMode::List),
                    state.view == ViewMode::List,
                ),
                (
                    "browser-grid",
                    "icons/grid.svg",
                    BrowserAction::View(ViewMode::Grid),
                    state.view == ViewMode::Grid,
                ),
                (
                    "browser-refresh",
                    "icons/loop.svg",
                    BrowserAction::Refresh,
                    false,
                ),
                (
                    "browser-locate-current",
                    "icons/center-focus.svg",
                    BrowserAction::LocateCurrentEffect,
                    false,
                ),
            ] {
                let button = tool(
                    parent,
                    assets,
                    localizer.text(key),
                    path,
                    action,
                    false,
                    selected,
                );
                ui.toolbar_actions.push((button, action));
            }
        });
    }
    // Keep keyboard focus on toolbar controls when navigating or switching layout.
    for (entity, action) in &ui.toolbar_actions {
        let disabled = match action {
            BrowserAction::Back => state.back.is_empty(),
            BrowserAction::Forward => state.forward.is_empty(),
            BrowserAction::Up => state.folder.as_os_str().is_empty(),
            BrowserAction::OpenSelected => state
                .selected
                .and_then(|id| content.source(id))
                .is_none_or(|entry| {
                    !matches!(
                        Kind::of(entry),
                        Kind::Folder | Kind::Effect | Kind::Material
                    )
                }),
            _ => false,
        };
        let selected = match action {
            BrowserAction::View(view) => state.view == *view,
            BrowserAction::Sources => state.sources_visible,
            _ => false,
        };
        let mut button = commands.entity(*entity);
        if disabled {
            button.insert(InteractionDisabled);
        } else {
            button.remove::<InteractionDisabled>();
        }
        button.insert(if selected {
            bevy::feathers::controls::ButtonVariant::Primary
        } else {
            bevy::feathers::controls::ButtonVariant::Normal
        });
    }
    if previous.is_none_or(|old| old.folder != state.folder || old.version != state.version) {
        clear(commands, ui.breadcrumbs);
        commands.entity(ui.breadcrumbs).with_children(|parent| {
            let mut items = Vec::new();
            let mut current = content.source(state.folder_id(content));
            while let Some(entry) = current {
                items.push(BreadcrumbItem {
                    label: entry.name.to_string_lossy().into_owned(),
                    action: Some(BrowserAction::Navigate(entry.id)),
                });
                current = entry.parent.and_then(|id| content.source(id));
            }
            items.reverse();
            let path = content
                .source_tree()
                .root_path()
                .join(&state.folder)
                .display()
                .to_string();
            spawn_breadcrumb(
                parent,
                &items,
                BreadcrumbProps {
                    height: 24.0,
                    font: fonts::MONO,
                    font_size: 10.0,
                    text_offset_y: 0.0,
                    uppercase: false,
                    flex_grow: 1.0,
                    max_ancestor_width: 100.0,
                    max_current_width: 160.0,
                    ancestor_color: theme::TEXT_MUTED,
                    current_color: theme::TEXT,
                    compact_ancestors: true,
                    overflow_label: &localizer.text("browser-ancestors"),
                    current_tooltip: Some(&path),
                    ancestor_tooltips: true,
                },
                assets,
            );
        });
    }
    if previous.is_none_or(|old| {
        old.kinds != state.kinds || old.sort != state.sort || old.recursive != state.recursive
    }) {
        clear(commands, ui.filters);
        commands.entity(ui.filters).with_children(|parent| {
            let label = localizer.text(if state.kinds.is_empty() {
                "browser-all-types"
            } else {
                "browser-filtered-types"
            });
            let mut options = vec![ComboOption {
                label: localizer.text("browser-all-types"),
                selected: state.kinds.is_empty(),
                action: BrowserAction::Kind(None),
            }];
            options.extend(Kind::FILTERS.into_iter().map(|kind| ComboOption {
                label: localizer.text(kind.label()),
                selected: state.kinds.contains(&kind),
                action: BrowserAction::Kind(Some(kind)),
            }));
            spawn_combo_control(
                parent,
                &label,
                &localizer.text("browser-types"),
                &options,
                112.0,
            );
            let options = [Sort::Name, Sort::Type].map(|sort| ComboOption {
                label: localizer.text(if sort == Sort::Name {
                    "browser-sort-name"
                } else {
                    "browser-sort-type"
                }),
                selected: state.sort == sort,
                action: BrowserAction::Sort(sort),
            });
            let label = &options[usize::from(state.sort == Sort::Type)].label;
            spawn_combo_control(
                parent,
                label,
                &localizer.text("browser-sort"),
                &options,
                112.0,
            );
            spawn_feathers_action_button(
                parent,
                &localizer.text("browser-recursive"),
                BrowserAction::Recursive,
                state.recursive,
            );
        });
    }
    if previous.is_none_or(|old| {
        old.expanded != state.expanded
            || old.folder != state.folder
            || old.version != state.version
            || old.tree_page != state.tree_page
    }) {
        clear(commands, ui.tree);
        clear(commands, ui.tree_footer);
        let folders = state.folders(content);
        let page = state
            .tree_page
            .min(folders.len().saturating_sub(1) / PAGE_SIZE);
        commands
            .entity(ui.tree)
            .insert(ScrollPosition::default())
            .with_children(|parent| {
                for (id, depth) in folders.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
                    let Some(entry) = content.source(*id) else {
                        continue;
                    };
                    parent
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(24.0),
                            flex_shrink: 0.0,
                            min_width: Val::Px(0.0),
                            padding: UiRect::left(Val::Px((*depth).min(8) as f32 * 9.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            let has_children = content
                                .source_tree()
                                .children(*id)
                                .any(|e| Kind::of(e) == Kind::Folder);
                            let path = if state.expanded.contains(id) {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            };
                            if has_children {
                                tool(
                                    row,
                                    assets,
                                    localizer.text("browser-expand-folder"),
                                    path,
                                    BrowserAction::Expand(*id),
                                    !has_children,
                                    false,
                                );
                            } else {
                                row.spawn(Node {
                                    width: Val::Px(26.0),
                                    height: Val::Px(24.0),
                                    flex_shrink: 0.0,
                                    ..default()
                                });
                            }
                            let mut button = row.spawn_empty();
                            button
                                .apply_scene(ui_shell::feathers_plain_button())
                                .insert((
                                    BrowserAction::Navigate(*id),
                                    BrowserFolderButton,
                                    FeathersActionButton,
                                    AccessibleLabel(entry.name.to_string_lossy().into_owned()),
                                    EditorTooltip::description(entry.path.display().to_string()),
                                    Node {
                                        flex_grow: 1.0,
                                        flex_basis: Val::Px(0.0),
                                        min_width: Val::Px(0.0),
                                        height: Val::Px(24.0),
                                        column_gap: Val::Px(5.0),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Start,
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                ))
                                .with_children(|button| {
                                    let folder = icon(button, assets, "icons/folder.svg", 13.0);
                                    button
                                        .commands()
                                        .entity(folder)
                                        .insert(SvgColor(kind_color(Kind::Folder)));
                                    text(button, entry.name.to_string_lossy());
                                });
                            if entry.relative_path == state.folder {
                                button.insert(bevy::feathers::controls::ButtonVariant::Primary);
                            }
                        });
                }
            });
        commands.entity(ui.tree_footer).with_children(|parent| {
            if folders.len() > PAGE_SIZE {
                paging(parent, page, folders.len(), true, localizer);
            }
        });
    }
}

fn display_name(entry: &aestra_project::ProjectSourceEntry) -> String {
    let filename = entry.name.to_string_lossy();
    for suffix in [
        ".aestra.material-function.ron",
        ".aestra.material-preset.ron",
        ".aestra.material.ron",
        ".aestra.ron",
    ] {
        if let Some(name) = filename.strip_suffix(suffix) {
            return name.replace('_', " ");
        }
    }
    filename.into_owned()
}

fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::Folder => Color::srgb(0.82, 0.66, 0.35),
        Kind::Effect => Color::srgb(0.66, 0.48, 1.0),
        Kind::Material | Kind::Preset => Color::srgb(0.36, 0.76, 0.95),
        Kind::Texture => Color::srgb(0.46, 0.82, 0.59),
        _ => theme::TEXT_MUTED,
    }
}

pub(super) fn sync_details(
    commands: &mut Commands,
    entity: Entity,
    selected: Option<ProjectSourceId>,
    content: &ProjectContent,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    clear(commands, entity);
    let Some(entry) = selected.and_then(|id| content.source(id)) else {
        commands.entity(entity).insert(Node {
            display: Display::None,
            ..default()
        });
        return;
    };
    let kind = Kind::of(entry);
    let mut hint = localizer.text(match kind {
        Kind::Effect | Kind::Folder => "browser-open-hint",
        Kind::Material => "browser-material-context",
        _ => "browser-read-only",
    });
    if kind == Kind::Effect
        && catalog.entry(entry.id).is_none_or(|effect| {
            effect
                .reference
                .is_none_or(|reference| catalog.openable_path(reference).is_none())
        })
    {
        hint = localizer.text("browser-effect-unavailable");
    }
    if let Some(asset) = content.asset_for_source(entry.id)
        && let Err(error) = content.unique_source_for_asset(asset)
    {
        hint = error.to_string();
    }
    if let Some(error) = &entry.error {
        hint.clone_from(error);
    }
    let diagnostics = content
        .asset_index()
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.path.as_ref() == Some(&entry.path))
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    if !diagnostics.is_empty() {
        hint = diagnostics.join("\n");
    }
    let metadata = entry
        .metadata
        .as_ref()
        .map(|metadata| {
            format!(
                " · {} B{}",
                metadata.bytes,
                if metadata.readonly {
                    localizer.text("browser-readonly-flag")
                } else {
                    String::new()
                }
            )
        })
        .unwrap_or_default();
    let description = format!(
        "{} · {}{}\n{}",
        entry.relative_path.display(),
        localizer.text(kind.label()),
        metadata,
        hint
    );
    commands
        .entity(entity)
        .insert((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(5.0)),
                row_gap: Val::Px(3.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            EditorTooltip::titled(entry.name.to_string_lossy(), description),
        ))
        .with_children(|details| {
            text(
                details,
                format!(
                    "{} · {}{}",
                    entry.relative_path.display(),
                    localizer.text(kind.label()),
                    metadata
                ),
            );
            text(details, hint);
        });
}

fn paging(
    parent: &mut ChildSpawnerCommands,
    page: usize,
    count: usize,
    tree: bool,
    localizer: &Localizer,
) {
    let pages = count.max(1).div_ceil(PAGE_SIZE);
    if pages == 1 {
        let mut args = FluentArgs::new();
        args.set("count", count as i64);
        text(parent, localizer.text_with("browser-count", &args));
        return;
    }
    for (next, symbol, disabled) in [(false, "‹", page == 0), (true, "›", page + 1 >= pages)] {
        let button = mini_button(
            parent,
            symbol,
            if tree {
                BrowserAction::TreePage(next)
            } else {
                BrowserAction::Page(next)
            },
        );
        parent
            .commands()
            .entity(button)
            .insert(AccessibleLabel(localizer.text(if next {
                "browser-next-page"
            } else {
                "browser-previous-page"
            })));
        if disabled {
            parent.commands().entity(button).insert(InteractionDisabled);
        }
    }
    let mut args = FluentArgs::new();
    args.set("page", (page + 1) as i64);
    args.set("pages", pages as i64);
    args.set("count", count as i64);
    text(parent, localizer.text_with("browser-page", &args));
}
