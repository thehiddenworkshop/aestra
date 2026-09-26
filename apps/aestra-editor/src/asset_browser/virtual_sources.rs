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
mod chrome;

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
    chrome: chrome::Ui,
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
        Vec<String>,
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
        .init_resource::<chrome::Navigation>()
        .add_observer(chrome::navigate)
        .init_resource::<Drag>()
        .add_observer(select)
        .add_observer(create)
        .add_observer(begin_drag)
        .add_observer(end_drag)
        .add_observer(cancel_drag)
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
    chrome::spawn(parent, state, localizer);
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

fn filtered_folder(entries: &[Entry], query: &str, folder: &[String]) -> Vec<Entry> {
    filtered(entries, query)
        .into_iter()
        .filter(|entry| entry.folder.starts_with(folder))
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
    nav: Res<chrome::Navigation>,
    locale: Res<Localizer>,
) {
    if state.scope == SourceScope::Project || panels.is_empty() {
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
    let folder = if state.scope == SourceScope::BuiltIns {
        nav.folder.clone()
    } else {
        Vec::new()
    };
    let visible = filtered_folder(&all, &state.query, &folder);
    let page_count = visible.len().div_ceil(PAGE_SIZE).max(1);
    let page = state.page.min(page_count - 1);
    let disabled = protection.is_open()
        || session.pending_change.is_some()
        || !crate::project_content::io::idle(tasks);
    for (entity, action) in &view_buttons {
        if let Some(selected) = match action {
            BrowserAction::View(view) => Some(*view == state.view),
            BrowserAction::Sources => Some(state.sources_visible),
            _ => None,
        } {
            commands.entity(entity).insert(if selected {
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
        let selected = selection.asset == Some(row.0.asset);
        commands.entity(entity).insert((
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(if selected { theme::ACCENT } else { Color::NONE }),
        ));
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
            folder.clone(),
        );
        chrome::sync(
            &mut commands,
            &ui.chrome,
            &state,
            &nav,
            &all,
            &assets,
            &locale,
            ui.previous.is_none() || nav.is_changed() || locale.is_changed(),
        );
        if ui.previous.as_ref() == Some(&snapshot) && !locale.is_changed() {
            continue;
        }
        if ui
            .previous
            .as_ref()
            .is_some_and(|old| old.1 != snapshot.1 || old.3 != snapshot.3 || old.7 != snapshot.7)
        {
            commands.entity(ui.list).insert(ScrollPosition::default());
        }
        ui.previous = Some(snapshot);
        commands.entity(ui.list).despawn_children();
        commands
            .entity(ui.list)
            .insert(super::panel::items_node(state.view));
        commands.entity(ui.list).with_children(|list| {
            for entry in visible.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
                let entity = spawn_action_list_row(
                    list,
                    &entry.name,
                    Some(entry.kind),
                    None,
                    &entry.name,
                    VirtualRow(
                        entry.clone(),
                        AssetPayload::capture_virtual(&catalog, &session, entry.asset),
                    ),
                );
                let selected = selection.asset == Some(entry.asset);
                list.commands()
                    .entity(entity)
                    .insert((
                        bevy::ui_widgets::Button,
                        EntityCursor::System(SystemCursorIcon::Pointer),
                        bevy::input_focus::tab_navigation::TabIndex(0),
                        super::panel::item_node(state.view),
                        BackgroundColor(if selected {
                            theme::SELECTION
                        } else {
                            theme::PANEL_DARK
                        }),
                        BorderColor::all(if selected { theme::ACCENT } else { Color::NONE }),
                        EditorTooltip::titled(entry.name.clone(), entry.description.clone()),
                    ))
                    .with_children(|row| {
                        let size = if state.view == ViewMode::Grid {
                            64.0
                        } else {
                            22.0
                        };
                        if let VirtualAsset::BuiltInPreset(preset) = entry.asset {
                            let preview = crate::material_graph::spawn_material_preset_preview(
                                row, preset, size,
                            );
                            row.commands().entity(preview).insert(Node {
                                width: Val::Px(size),
                                height: Val::Px(size),
                                min_width: Val::Px(size),
                                min_height: Val::Px(size),
                                flex_shrink: 0.0,
                                align_self: AlignSelf::Center,
                                overflow: Overflow::clip(),
                                ..default()
                            });
                        } else {
                            super::panel::icon(row, &assets, kind(entry.asset).icon(), size);
                        }
                    });
            }
        });
        if let Ok(mut text) = labels.get_mut(ui.footer) {
            text.0 = format!("{} / {page_count} · {} items", page + 1, visible.len());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn select(
    event: On<Activate>,
    rows: Query<&VirtualRow>,
    pages: Query<&Page>,
    mut selection: ResMut<Selection>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    nav: Res<chrome::Navigation>,
) {
    if let Ok(row) = rows.get(event.entity)
        && row.1.resolve(&catalog).is_ok()
        && row.1.check_document(&session).is_ok()
    {
        selection.asset = Some(row.0.asset);
    }
    if let Ok(Page(next)) = pages.get(event.entity) {
        let count = filtered_folder(
            &entries(&catalog, &session, state.scope == SourceScope::BuiltIns),
            &state.query,
            if state.scope == SourceScope::BuiltIns {
                &nav.folder
            } else {
                &[]
            },
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

fn cancel_drag(event: On<Pointer<bevy::picking::events::Cancel>>, mut drag: ResMut<Drag>) {
    if drag.origin == Some(event.original_event_target()) {
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
