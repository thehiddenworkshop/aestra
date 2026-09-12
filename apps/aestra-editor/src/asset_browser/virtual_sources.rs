//! Non-file browser sources. Deliberately no BrowserRow/ProjectSourceId: file actions
//! cannot rename, move, duplicate or delete these document-owned/immutable resources.
use super::{actions::BrowserAction, panel::BrowserSearch, state::*};
use crate::{
    asset_drop::{
        AssetPayload, VirtualAsset,
        virtual_sources::{Entry, entries},
    },
    *,
};
use bevy::ui_widgets::Activate;

#[cfg(test)]
mod tests;

#[derive(Component, Clone)]
struct VirtualRow(Entry, AssetPayload);
#[derive(Component, Clone, Copy)]
enum Create {
    Material,
    Flipbook,
}
#[derive(Resource, Default)]
struct Selection {
    document: Option<(aestra_core::EffectId, u64)>,
    asset: Option<VirtualAsset>,
}
#[derive(Component)]
struct VirtualUi {
    list: Entity,
    footer: Entity,
    previous: Option<(
        Vec<Entry>,
        String,
        ViewMode,
        usize,
        aestra_project::ProjectContentVersion,
        u64,
        u64,
    )>,
}
#[derive(Resource, Default)]
struct Drag {
    origin: Option<Entity>,
    preview: Option<Entity>,
    payload: Option<AssetPayload>,
    ended: bool,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Selection>()
        .init_resource::<Drag>()
        .add_observer(select)
        .add_observer(create)
        .add_observer(begin_drag)
        .add_observer(end_drag)
        .add_systems(
            Update,
            sync.after(DockingSet::Sync).before(AestraFeathersSet::Sync),
        )
        .add_systems(PostUpdate, cleanup_drag.before(bevy::ui::UiSystems::Layout));
}

fn column() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        min_height: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    state: &AssetBrowserState,
    localizer: &Localizer,
) {
    let mut list = Entity::PLACEHOLDER;
    let mut footer = Entity::PLACEHOLDER;
    let mut host = parent.spawn(Node {
        flex_grow: 1.0,
        ..column()
    });
    host.with_children(|root| {
        root.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(4.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|bar| {
            for (key, view) in [
                ("browser-list", ViewMode::List),
                ("browser-grid", ViewMode::Grid),
            ] {
                spawn_feathers_action_button(
                    bar,
                    &localizer.text(key),
                    BrowserAction::View(view),
                    state.view == view,
                );
            }
            if state.scope == SourceScope::CurrentDocument {
                spawn_feathers_action_button(
                    bar,
                    &localizer.text("browser-create-local-material"),
                    Create::Material,
                    false,
                );
                spawn_feathers_action_button(
                    bar,
                    &localizer.text("browser-create-local-flipbook"),
                    Create::Flipbook,
                    false,
                );
            }
        });
        spawn_search_field(
            root,
            &state.query,
            &localizer.text("browser-search"),
            &localizer.text("library-search-clear"),
            BrowserSearch,
        );
        root.spawn((
            Text::new(localizer.text(if state.scope == SourceScope::BuiltIns {
                "browser-builtins-help"
            } else {
                "browser-document-help"
            })),
            TextColor(theme::TEXT_MUTED),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
        ));
        root.spawn(Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            flex_direction: FlexDirection::Row,
            ..column()
        })
        .with_children(|scroll| {
            list = spawn_vertical_scroll_area(
                scroll,
                if state.scope == SourceScope::BuiltIns {
                    ScrollMemoryKey::AssetBrowserBuiltIns
                } else {
                    ScrollMemoryKey::AssetBrowserDocument
                },
                Node {
                    flex_grow: 1.0,
                    ..column()
                },
                |_| {},
            );
        });
        root.spawn(Node {
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|bar| {
            spawn_feathers_action_button(bar, "‹", Page(false), false);
            footer = bar
                .spawn((
                    Text::new(""),
                    TextColor(theme::TEXT_MUTED),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                ))
                .id();
            spawn_feathers_action_button(bar, "›", Page(true), false);
        });
    });
    host.insert(VirtualUi {
        list,
        footer,
        previous: None,
    });
}

#[derive(Component, Clone, Copy)]
struct Page(bool);

fn kind(asset: VirtualAsset) -> Kind {
    match asset {
        VirtualAsset::BuiltInPreset(_) => Kind::Preset,
        VirtualAsset::Material(_) => Kind::Material,
        VirtualAsset::Texture(_)
        | VirtualAsset::Flipbook(_)
        | VirtualAsset::FlipbookDeclaration(_) => Kind::Texture,
        VirtualAsset::Mesh(_) => Kind::Mesh,
    }
}

fn filtered(entries: &[Entry], query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            format!("{} {} {}", entry.name, entry.kind, entry.description)
                .to_lowercase()
                .contains(&query)
        })
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    mut panels: Query<&mut VirtualUi>,
    state: Res<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    assets: Res<AssetServer>,
    mut selection: ResMut<Selection>,
    rows: Query<(Entity, &VirtualRow)>,
    buttons: Query<(Entity, &Create)>,
    pages: Query<(Entity, &Page)>,
    mut labels: Query<&mut Text>,
    protection: Res<DocumentProtectionState>,
    tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    view_buttons: Query<(Entity, &BrowserAction)>,
) {
    if state.legacy || state.scope == SourceScope::Project || panels.is_empty() {
        return;
    }
    let document = (session.effect.id, session.history_generation());
    if selection.document != Some(document) {
        selection.document = Some(document);
        selection.asset = None;
    }
    let all = entries(&catalog, &session, state.scope == SourceScope::BuiltIns);
    if selection
        .asset
        .is_some_and(|id| !all.iter().any(|entry| entry.asset == id))
    {
        selection.asset = None;
    }
    let visible = filtered(&all, &state.query);
    let page_count = visible.len().div_ceil(PAGE_SIZE).max(1);
    let page = state.page.min(page_count - 1);
    let disabled = protection.is_open()
        || session.pending_change.is_some()
        || !crate::project_content::io::idle(tasks);
    for (entity, action) in &view_buttons {
        if let BrowserAction::View(view) = action {
            commands.entity(entity).insert(if *view == state.view {
                bevy::feathers::controls::ButtonVariant::Primary
            } else {
                bevy::feathers::controls::ButtonVariant::Normal
            });
        }
    }
    for (entity, action) in &buttons {
        let available = !disabled
            && match action {
                Create::Material => true,
                Create::Flipbook => matches!(selection.asset, Some(VirtualAsset::Texture(_))),
            };
        if available {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (entity, Page(next)) in &pages {
        if if *next {
            page + 1 >= page_count
        } else {
            page == 0
        } {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
    for (entity, row) in &rows {
        commands
            .entity(entity)
            .insert(if selection.asset == Some(row.0.asset) {
                bevy::feathers::controls::ButtonVariant::Primary
            } else {
                bevy::feathers::controls::ButtonVariant::Normal
            });
    }
    for mut ui in &mut panels {
        let snapshot = (
            all.clone(),
            state.query.clone(),
            state.view,
            page,
            catalog.content_revision(),
            session.history_generation(),
            session.document_revision(),
        );
        if ui.previous.as_ref() == Some(&snapshot) {
            continue;
        }
        if ui
            .previous
            .as_ref()
            .is_some_and(|old| old.1 != snapshot.1 || old.3 != snapshot.3)
        {
            commands.entity(ui.list).insert(ScrollPosition::default());
        }
        ui.previous = Some(snapshot);
        commands.entity(ui.list).despawn_children();
        commands.entity(ui.list).insert(Node {
            flex_direction: if state.view == ViewMode::Grid {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            },
            flex_wrap: if state.view == ViewMode::Grid {
                FlexWrap::Wrap
            } else {
                FlexWrap::NoWrap
            },
            flex_grow: 1.0,
            overflow: Overflow::scroll_y(),
            align_content: AlignContent::FlexStart,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(4.0),
            ..column()
        });
        commands.entity(ui.list).with_children(|list| {
            for entry in visible.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
                let grid = state.view == ViewMode::Grid;
                list.spawn(Node {
                    flex_shrink: 0.0,
                    width: if grid {
                        Val::Px(136.0)
                    } else {
                        Val::Percent(100.0)
                    },
                    ..column()
                })
                .with_children(|slot| {
                    // Actual button owns activation/focus. Its icon/label are non-pickable.
                    let mut button = slot.spawn_empty();
                    button.apply_scene(crate::feathers::scenes::feathers_button());
                    let entity = button.id();
                    button.insert((
                        VirtualRow(
                            entry.clone(),
                            AssetPayload::capture_virtual(&catalog, &session, entry.asset),
                        ),
                        crate::feathers::button::FeathersActionButton,
                        AccessibleLabel(entry.name.clone()),
                    ));
                    slot.commands().entity(entity).insert((
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(if grid { 108.0 } else { 40.0 }),
                            flex_direction: if grid {
                                FlexDirection::Column
                            } else {
                                FlexDirection::Row
                            },
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::all(Val::Px(6.0)),
                            column_gap: Val::Px(8.0),
                            row_gap: Val::Px(4.0),
                            ..default()
                        },
                        EditorTooltip::titled(entry.name.clone(), entry.description.clone()),
                    ));
                    slot.commands().entity(entity).with_children(|button| {
                        if let VirtualAsset::BuiltInPreset(preset) = entry.asset {
                            crate::material_graph::spawn_material_preset_preview(
                                button,
                                preset,
                                if grid { 64.0 } else { 22.0 },
                            );
                        } else {
                            super::panel::icon(
                                button,
                                &assets,
                                kind(entry.asset).icon(),
                                if grid { 42.0 } else { 22.0 },
                            );
                        }
                        button
                            .spawn((column(), Pickable::IGNORE))
                            .with_children(|labels| {
                                for (text, size, color) in [
                                    (&entry.name, 11.0, theme::TEXT),
                                    (&entry.kind.to_owned(), 10.0, theme::TEXT_MUTED),
                                ] {
                                    labels.spawn((
                                        Text::new(text),
                                        TextFont {
                                            font_size: FontSize::Px(size),
                                            ..default()
                                        },
                                        TextColor(color),
                                        Pickable::IGNORE,
                                    ));
                                }
                            });
                    });
                });
            }
        });
        if let Ok(mut text) = labels.get_mut(ui.footer) {
            text.0 = format!("{} / {page_count} · {} items", page + 1, visible.len());
        }
    }
}

fn select(
    event: On<Activate>,
    rows: Query<&VirtualRow>,
    pages: Query<&Page>,
    mut selection: ResMut<Selection>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
) {
    if let Ok(row) = rows.get(event.entity)
        && row.1.resolve(&catalog).is_ok()
        && row.1.check_document(&session).is_ok()
    {
        selection.asset = Some(row.0.asset);
    }
    if let Ok(Page(next)) = pages.get(event.entity) {
        let count = filtered(
            &entries(&catalog, &session, state.scope == SourceScope::BuiltIns),
            &state.query,
        )
        .len()
        .div_ceil(PAGE_SIZE)
        .max(1);
        state.page = if *next {
            (state.page + 1).min(count - 1)
        } else {
            state.page.saturating_sub(1)
        };
    }
}

fn create(
    event: On<Activate>,
    buttons: Query<&Create>,
    selection: Res<Selection>,
    mut session: ResMut<EditorSession>,
    guard: crate::asset_drop::AuthoringDropGuard,
) {
    let Ok(action) = buttons.get(event.entity) else {
        return;
    };
    if guard.check().is_err() || session.pending_change.is_some() {
        return;
    }
    if selection.document != Some((session.effect.id, session.history_generation())) {
        return;
    }
    match action {
        Create::Material => session.add_sprite_material(),
        Create::Flipbook => {
            if let Some(VirtualAsset::Texture(texture)) = selection.asset {
                session.add_grid_flipbook_for_texture(texture);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn begin_drag(
    event: On<Pointer<DragStart>>,
    rows: Query<&VirtualRow>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    assets: Res<AssetServer>,
    mut drag: ResMut<Drag>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary || event.entity != event.original_event_target() {
        return;
    }
    let Some((_, row)) = crate::asset_drop::nearest(event.entity, &parents, |e| rows.get(e).ok())
    else {
        return;
    };
    if matches!(row.0.asset, VirtualAsset::FlipbookDeclaration(_))
        || row.1.resolve(&catalog).is_err()
        || row.1.check_document(&session).is_err()
    {
        return;
    }
    if let Some(preview) = drag.preview.take() {
        commands.entity(preview).try_despawn();
    }
    if let Some(origin) = drag.origin.take() {
        commands.entity(origin).try_remove::<AssetPayload>();
    }
    let root = std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .filter(|e| geometry.contains(*e))
        .last();
    if let Some(root) = root
        && let Ok((node, transform)) = geometry.get(root)
    {
        let position = crate::feathers::context_menu::pointer_position_in_node(
            event.pointer_location.position,
            node,
            transform,
        ) * node.inverse_scale_factor();
        drag.preview = Some(super::drag_preview::spawn_named(
            &mut commands,
            root,
            event.entity,
            event.pointer_id,
            position,
            &row.0.name,
            kind(row.0.asset),
            row.0.kind,
            &assets,
        ));
    }
    let payload = row.1.clone();
    commands.entity(event.entity).insert(payload.clone());
    drag.origin = Some(event.entity);
    drag.payload = Some(payload);
    drag.ended = false;
}

fn end_drag(event: On<Pointer<DragEnd>>, mut drag: ResMut<Drag>) {
    if event.button == PointerButton::Primary {
        drag.ended = true;
    }
}

fn cleanup_drag(
    mut drag: ResMut<Drag>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    entities: Query<()>,
    mut commands: Commands,
) {
    let stale = drag
        .payload
        .as_ref()
        .is_some_and(|p| p.resolve(&catalog).is_err() || p.check_document(&session).is_err());
    if drag.ended
        || stale
        || crate::asset_drop::cancelled(keys.as_deref())
        || drag.origin.is_some_and(|e| !entities.contains(e))
    {
        if let Some(origin) = drag.origin.take() {
            commands.entity(origin).try_remove::<AssetPayload>();
        }
        if let Some(preview) = drag.preview.take() {
            commands.entity(preview).try_despawn();
        }
        *drag = default();
    }
}
