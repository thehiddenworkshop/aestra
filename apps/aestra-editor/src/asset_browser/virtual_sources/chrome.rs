//! Read-only virtual navigation with the Project browser's shared controls and layout.
use super::*;
use crate::asset_browser::panel;
use std::collections::BTreeSet;

#[derive(Resource)]
pub(super) struct Navigation {
    pub folder: Vec<String>,
    back: Vec<Vec<String>>,
    forward: Vec<Vec<String>>,
    expanded: BTreeSet<Vec<String>>,
}
impl Default for Navigation {
    fn default() -> Self {
        Self {
            folder: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            expanded: BTreeSet::from([Vec::new(), vec!["Materials".into()]]),
        }
    }
}
#[derive(Component, Clone)]
pub(super) enum FolderAction {
    Go(Vec<String>),
    Expand(Vec<String>),
    Back,
    Forward,
    Up,
}
pub(super) struct Ui {
    toolbar: Entity,
    tree: Option<Entity>,
    pub(super) sources: Option<Entity>,
    splitter: Option<Entity>,
}

fn folders(entries: &[Entry]) -> BTreeSet<Vec<String>> {
    let mut result = BTreeSet::from([Vec::new()]);
    for entry in entries {
        for depth in 1..=entry.folder.len() {
            result.insert(entry.folder[..depth].to_vec());
        }
    }
    result
}
pub(super) fn navigate(
    event: On<Activate>,
    actions: Query<&FolderAction>,
    mut nav: ResMut<Navigation>,
    mut state: ResMut<AssetBrowserState>,
    mut selection: ResMut<Selection>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
) {
    if state.scope != SourceScope::BuiltIns {
        return;
    }
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    let valid = folders(&entries(&catalog, &session, true));
    let target = match action {
        FolderAction::Go(path) => path.clone(),
        FolderAction::Up => {
            let mut path = nav.folder.clone();
            path.pop();
            path
        }
        FolderAction::Back => {
            let Some(target) = nav.back.pop() else {
                return;
            };
            let current = nav.folder.clone();
            nav.forward.push(current);
            nav.folder = target;
            state.page = 0;
            selection.asset = None;
            return;
        }
        FolderAction::Forward => {
            let Some(target) = nav.forward.pop() else {
                return;
            };
            let current = nav.folder.clone();
            nav.back.push(current);
            nav.folder = target;
            state.page = 0;
            selection.asset = None;
            return;
        }
        FolderAction::Expand(path) => {
            if valid.contains(path) && !nav.expanded.remove(path) {
                nav.expanded.insert(path.clone());
            }
            return;
        }
    };
    if !valid.contains(&target) || target == nav.folder {
        return;
    }
    let current = nav.folder.clone();
    nav.back.push(current);
    if nav.back.len() > HISTORY_LIMIT {
        nav.back.remove(0);
    }
    nav.forward.clear();
    nav.folder = target;
    state.page = 0;
    selection.asset = None;
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    state: &AssetBrowserState,
    locale: &Localizer,
) {
    let mut ui = Ui {
        toolbar: Entity::PLACEHOLDER,
        tree: None,
        sources: None,
        splitter: None,
    };
    let mut list = Entity::PLACEHOLDER;
    let mut footer = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            flex_grow: 1.0,
            ..column()
        })
        .with_children(|host| {
            ui.toolbar = host.spawn(panel::row_node()).id();
            host.spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|body| {
                if state.scope == SourceScope::BuiltIns {
                    ui.sources = Some(
                        body.spawn((panel::SourcesPane, source_node(state)))
                            .with_children(|sources| {
                                sources.spawn(panel::row_node()).with_children(|bar| {
                                    caption(bar, locale.text("browser-folders"));
                                });
                                sources
                                    .spawn(Node {
                                        flex_grow: 1.0,
                                        ..column()
                                    })
                                    .with_children(|scroll| {
                                        ui.tree = Some(spawn_vertical_scroll_area(
                                            scroll,
                                            ScrollMemoryKey::AssetBrowserBuiltInSources,
                                            column(),
                                            |_| {},
                                        ));
                                    });
                            })
                            .id(),
                    );
                    ui.splitter = Some(
                        body.spawn((
                            panel::SourcesSplitter::default(),
                            EditorNativeControl,
                            EntityCursor::System(SystemCursorIcon::ColResize),
                            AccessibleLabel(locale.text("browser-resize-sources")),
                            Node {
                                width: Val::Px(5.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(theme::BORDER),
                        ))
                        .id(),
                    );
                }
                body.spawn(Node {
                    flex_basis: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..column()
                })
                .with_children(|content| {
                    content.spawn(panel::row_node()).with_children(|search| {
                        search
                            .spawn(Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(120.0),
                                ..default()
                            })
                            .with_children(|field| {
                                spawn_search_field(
                                    field,
                                    &state.query,
                                    &locale.text("browser-search"),
                                    &locale.text("browser-search-clear"),
                                    BrowserSearch,
                                );
                            });
                        let label = caption(
                            search,
                            locale.text(if state.scope == SourceScope::BuiltIns {
                                "browser-read-only-badge"
                            } else {
                                "browser-source-document"
                            }),
                        );
                        search
                            .commands()
                            .entity(label)
                            .insert(EditorTooltip::description(locale.text(
                                if state.scope == SourceScope::BuiltIns {
                                    "browser-builtins-help"
                                } else {
                                    "browser-document-help"
                                },
                            )));
                    });
                    content
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            ..default()
                        })
                        .with_children(|scroll| {
                            list = spawn_vertical_scroll_area(
                                scroll,
                                if state.scope == SourceScope::BuiltIns {
                                    ScrollMemoryKey::AssetBrowserBuiltIns
                                } else {
                                    ScrollMemoryKey::AssetBrowserDocument
                                },
                                panel::items_node(state.view),
                                |_| {},
                            );
                        });
                });
            });
            host.spawn(panel::row_node()).with_children(|bar| {
                spawn_feathers_action_button(bar, "‹", Page(false), false);
                footer = caption(bar, String::new());
                spawn_feathers_action_button(bar, "›", Page(true), false);
            });
            let entity = host.target_entity();
            host.commands().entity(entity).insert(VirtualUi {
                chrome: ui,
                list,
                footer,
                previous: None,
            });
        });
}

fn caption(parent: &mut ChildSpawnerCommands, text: String) -> Entity {
    parent
        .spawn((
            Text::new(text),
            TextColor(theme::TEXT_MUTED),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id()
}
fn source_node(state: &AssetBrowserState) -> Node {
    Node {
        display: if state.sources_visible {
            Display::Flex
        } else {
            Display::None
        },
        width: Val::Px(state.sources_width),
        max_width: Val::Percent(55.0),
        flex_shrink: 0.0,
        ..column()
    }
}
fn button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: FolderAction,
    selected: bool,
    disabled: bool,
) -> Entity {
    let entity = crate::feathers::button::spawn_tool_button(parent, label, action);
    parent.commands().entity(entity).insert((
        Node {
            min_height: Val::Px(24.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            ..default()
        },
        if selected {
            bevy::feathers::controls::ButtonVariant::Primary
        } else {
            bevy::feathers::controls::ButtonVariant::Normal
        },
    ));
    if disabled {
        parent.commands().entity(entity).insert(InteractionDisabled);
    }
    entity
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    commands: &mut Commands,
    ui: &Ui,
    state: &AssetBrowserState,
    nav: &Navigation,
    all: &[Entry],
    assets: &AssetServer,
    locale: &Localizer,
    rebuild: bool,
) {
    if let Some(sources) = ui.sources {
        commands.entity(sources).insert(source_node(state));
    }
    if let Some(splitter) = ui.splitter {
        commands.entity(splitter).insert(Node {
            display: if state.sources_visible {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(5.0),
            flex_shrink: 0.0,
            ..default()
        });
    }
    if !rebuild {
        return;
    }
    commands.entity(ui.toolbar).despawn_children();
    commands.entity(ui.toolbar).with_children(|bar| {
        if state.scope == SourceScope::BuiltIns {
            for (label, key, action, disabled) in [
                ("←", "browser-back", FolderAction::Back, nav.back.is_empty()),
                (
                    "→",
                    "browser-forward",
                    FolderAction::Forward,
                    nav.forward.is_empty(),
                ),
                ("↑", "browser-up", FolderAction::Up, nav.folder.is_empty()),
            ] {
                let id = button(bar, label, action, false, disabled);
                bar.commands().entity(id).insert((
                    AccessibleLabel(locale.text(key)),
                    EditorTooltip::description(locale.text(key)),
                ));
            }
            panel::tool(
                bar,
                assets,
                locale.text("browser-sources"),
                "icons/folder.svg",
                BrowserAction::Sources,
                false,
                state.sources_visible,
            );
        }
        for (key, path, view) in [
            ("browser-list", "icons/list.svg", ViewMode::List),
            ("browser-grid", "icons/grid.svg", ViewMode::Grid),
        ] {
            panel::tool(
                bar,
                assets,
                locale.text(key),
                path,
                BrowserAction::View(view),
                false,
                state.view == view,
            );
        }
        if state.scope == SourceScope::BuiltIns {
            button(
                bar,
                &locale.text("browser-source-builtins"),
                FolderAction::Go(Vec::new()),
                false,
                false,
            );
            for depth in 1..=nav.folder.len() {
                caption(bar, "›".into());
                button(
                    bar,
                    &nav.folder[depth - 1],
                    FolderAction::Go(nav.folder[..depth].to_vec()),
                    false,
                    false,
                );
            }
        } else {
            spawn_feathers_action_button(
                bar,
                &locale.text("browser-create-local-material"),
                Create::Material,
                false,
            );
            spawn_feathers_action_button(
                bar,
                &locale.text("browser-create-local-flipbook"),
                Create::Flipbook,
                false,
            );
        }
    });
    if let Some(tree) = ui.tree {
        commands.entity(tree).despawn_children();
        commands.entity(tree).with_children(|tree| {
            let paths = folders(all);
            for path in &paths {
                if (0..path.len()).any(|depth| !nav.expanded.contains(&path[..depth])) {
                    continue;
                }
                tree.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::left(Val::Px(path.len() as f32 * 12.0)),
                    ..default()
                })
                .with_children(|row| {
                    let has_children = paths
                        .iter()
                        .any(|p| p.len() == path.len() + 1 && p.starts_with(path));
                    if has_children {
                        panel::tool(
                            row,
                            assets,
                            locale.text("browser-expand-folder"),
                            if nav.expanded.contains(path) {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            },
                            FolderAction::Expand(path.clone()),
                            false,
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
                    let name = path
                        .last()
                        .cloned()
                        .unwrap_or_else(|| locale.text("browser-source-builtins"));
                    let mut folder = row.spawn_empty();
                    folder
                        .apply_scene(crate::feathers::scenes::feathers_plain_button())
                        .insert((
                            FolderAction::Go(path.clone()),
                            crate::feathers::button::FeathersActionButton,
                            AccessibleLabel(name.clone()),
                            Node {
                                min_width: Val::Px(0.0),
                                flex_basis: Val::Px(0.0),
                                flex_grow: 1.0,
                                height: Val::Px(24.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Start,
                                column_gap: Val::Px(5.0),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                        ))
                        .with_children(|folder| {
                            panel::icon(folder, assets, "icons/folder.svg", 13.0);
                            folder.spawn((
                                Text::new(name),
                                TextColor(theme::TEXT_MUTED),
                                TextFont {
                                    font_size: FontSize::Px(10.0),
                                    ..default()
                                },
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                                Node {
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ));
                        });
                    if *path == nav.folder {
                        folder.insert(bevy::feathers::controls::ButtonVariant::Primary);
                    }
                });
            }
        });
    }
}
