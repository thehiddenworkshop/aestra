use super::{
    panel::{BrowserItems, BrowserRow, BrowserSearch, SourcesSplitter},
    state::*,
};
use crate::*;
use aestra_project::{ProjectAssetId, ProjectSourceId};
use bevy::{
    input_focus::{FocusedInput, InputFocus},
    ui_widgets::{Activate, ActiveDescendant},
};

/// Count double-clicks by source row, not by the label/icon hit entity.
#[derive(Resource, Default)]
pub(super) struct BrowserClickState(
    Option<(
        ProjectSourceId,
        f64,
        Vec2,
        bevy::picking::pointer::PointerId,
    )>,
);

#[derive(Event)]
pub(super) struct OpenMaterial(pub(super) aestra_core::MaterialProgramId);

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserAction {
    Legacy(bool),
    Navigate(ProjectSourceId),
    Expand(ProjectSourceId),
    Back,
    Forward,
    Up,
    Sources,
    View(ViewMode),
    Recursive,
    Kind(Option<Kind>),
    Sort(Sort),
    Page(bool),
    TreePage(bool),
    OpenProject,
    Refresh,
    OpenSelected,
}

pub(super) fn activate_button(
    event: On<Activate>,
    actions: Query<&BrowserAction>,
    mut commands: Commands,
) {
    if let Ok(action) = actions.get(event.entity) {
        commands.trigger(*action);
    }
}

pub(super) fn handle_action(
    event: On<BrowserAction>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
    mut clicks: ResMut<BrowserClickState>,
) {
    clicks.0 = None;
    let content = catalog.content();
    match *event {
        BrowserAction::Legacy(legacy) => {
            if state.legacy != legacy {
                state.legacy = legacy;
                session.ui_revision += 1;
            }
        }
        BrowserAction::Navigate(id) => state.navigate(content, id),
        BrowserAction::Expand(id) => {
            if !state.expanded.remove(&id) {
                state.expanded.insert(id);
            }
        }
        BrowserAction::Back => state.history(content, false),
        BrowserAction::Forward => state.history(content, true),
        BrowserAction::Up => {
            if let Some(parent) = content
                .source(state.folder_id(content))
                .and_then(|e| e.parent)
            {
                state.navigate(content, parent);
            }
        }
        BrowserAction::Sources => state.sources_visible = !state.sources_visible,
        BrowserAction::View(view) => state.view = view,
        BrowserAction::Recursive => {
            state.recursive = !state.recursive;
            state.page = 0;
        }
        BrowserAction::Kind(kind) => {
            if let Some(kind) = kind {
                if !state.kinds.remove(&kind) {
                    state.kinds.insert(kind);
                }
            } else {
                state.kinds.clear();
            }
            state.page = 0;
        }
        BrowserAction::Sort(sort) => {
            state.sort = sort;
            state.page = 0;
        }
        BrowserAction::Page(next) => {
            state.page = page_step(state.page, state.filtered(content).len(), next);
        }
        BrowserAction::TreePage(next) => {
            state.tree_page = page_step(state.tree_page, state.folders(content).len(), next);
        }
        BrowserAction::OpenProject => commands.trigger(DocumentAction::OpenProject),
        BrowserAction::Refresh => commands.trigger(crate::library::LibraryAction::RefreshProject),
        BrowserAction::OpenSelected => {
            if let Some(id) = state.selected {
                open_source(id, &catalog, &mut state, &mut commands);
            }
        }
    }
}

fn page_step(page: usize, count: usize, next: bool) -> usize {
    if next {
        (page + 1).min(count.saturating_sub(1) / PAGE_SIZE)
    } else {
        page.saturating_sub(1)
    }
}

pub(super) fn change_search(
    event: On<ValueChange<String>>,
    inputs: Query<(), With<BrowserSearch>>,
    mut state: ResMut<AssetBrowserState>,
) {
    if inputs.contains(event.source) && state.query != event.value {
        state.query.clone_from(&event.value);
        state.page = 0;
    }
}

pub(super) fn select_row(
    event: On<ValueChange<Entity>>,
    rows: Query<&BrowserRow>,
    lists: Query<(), With<BrowserItems>>,
    mut state: ResMut<AssetBrowserState>,
) {
    if lists.contains(event.source)
        && let Ok(row) = rows.get(event.value)
    {
        state.selected = Some(row.0);
    }
}

pub(super) fn click_row(
    mut event: On<Pointer<Click>>,
    rows: Query<&BrowserRow, With<ListItem>>,
    parents: Query<&ChildOf>,
    lists: Query<(), With<BrowserItems>>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    time: Res<Time<Real>>,
    mut clicks: ResMut<BrowserClickState>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    // Resolve the row immediately, before another widget consumes the descendant click.
    let target = event.entity;
    let row_entity = std::iter::once(target)
        .chain(parents.iter_ancestors(target))
        .find(|entity| rows.contains(*entity));
    if let Some(row_entity) = row_entity {
        let row = rows.get(row_entity).unwrap();
        let list = parents
            .iter_ancestors(row_entity)
            .find(|entity| lists.contains(*entity));
        if let Some(list) = list {
            focus.set(list, bevy::input_focus::FocusCause::Navigated);
            commands
                .entity(list)
                .insert(ActiveDescendant(Some(row_entity)));
        }
        state.selected = Some(row.0);
        let now = time.elapsed_secs_f64();
        let double = clicks.0.is_some_and(|(id, at, position, pointer)| {
            id == row.0
                && pointer == event.pointer_id
                && now - at <= 0.5
                && position.distance(event.pointer_location.position) <= 6.0
        });
        clicks.0 = if double {
            None
        } else {
            Some((
                row.0,
                now,
                event.pointer_location.position,
                event.pointer_id,
            ))
        };
        if double {
            open_source(row.0, &catalog, &mut state, &mut commands);
        }
        event.propagate(false);
    } else if lists.contains(event.entity) {
        clicks.0 = None;
        focus.set(event.entity, bevy::input_focus::FocusCause::Navigated);
        if event.original_event_target() == event.entity {
            state.selected = None;
            event.propagate(false);
        }
    }
}

pub(super) fn open_source(
    id: ProjectSourceId,
    catalog: &ProjectEffectCatalog,
    state: &mut AssetBrowserState,
    commands: &mut Commands,
) {
    let content = catalog.content();
    if content
        .source(id)
        .is_some_and(|entry| Kind::of(entry) == Kind::Folder)
    {
        state.navigate(content, id);
    } else if let Some(ProjectAssetId::Effect(effect)) = content.asset_for_source(id) {
        let reference = EffectAssetRef::new(effect);
        // A duplicate/invalid source must not silently open a different file with the same ID.
        if catalog.openable_path(reference).is_some() {
            commands.trigger(DocumentAction::OpenCatalog(reference));
        }
    } else if let Some(ProjectAssetId::MaterialProgram(program)) = content.asset_for_source(id)
        && content
            .cached_material_program(aestra_core::material::MaterialProgramRef::Project(program))
            .is_ok()
    {
        commands.trigger(OpenMaterial(program));
    }
}

pub(super) fn open_material(
    event: On<OpenMaterial>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
) {
    let renderer = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| &emitter.renderers)
        .find(|renderer| {
            session.effect.material_instances.iter().any(|instance| {
                instance.id == renderer.material && instance.program.id() == event.0
            })
        })
        .map(|renderer| renderer.id);
    if let Some(renderer) = renderer {
        session.selection.primary = SemanticTarget::Renderer(renderer);
        session.selected_emitter_region = None;
        session.ui_revision += 1;
        session.status = localizer.text("browser-material-opened");
        reveal_dock_panel(&mut layout, &mut session, DockPanel::MaterialGraph);
    } else {
        session.status = localizer.text("browser-material-context");
        session.ui_revision += 1;
    }
}

pub(super) fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    lists: Query<&ActiveDescendant, With<BrowserItems>>,
    rows: Query<&BrowserRow>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
    mut clicks: ResMut<BrowserClickState>,
) {
    let Ok(active) = lists.get(event.focused_entity) else {
        return;
    };
    if event.input.state != ButtonState::Pressed || event.input.repeat {
        return;
    }
    match event.input.key_code {
        KeyCode::Enter => {
            if let Some(row) = active.0.and_then(|id| rows.get(id).ok()) {
                open_source(row.0, &catalog, &mut state, &mut commands);
            }
        }
        KeyCode::Escape => state.selected = None,
        KeyCode::Backspace => commands.trigger(BrowserAction::Up),
        _ => return,
    }
    clicks.0 = None;
    event.propagate(false);
}

pub(super) fn resize_sources(
    mut event: On<Pointer<Drag>>,
    splitters: Query<&ComputedNode, With<SourcesSplitter>>,
    mut state: ResMut<AssetBrowserState>,
) {
    if event.button == PointerButton::Primary
        && let Ok(node) = splitters.get(event.entity)
    {
        event.propagate(false);
        state.sources_width =
            (state.sources_width + event.delta.x * node.inverse_scale_factor()).clamp(90.0, 360.0);
    }
}
