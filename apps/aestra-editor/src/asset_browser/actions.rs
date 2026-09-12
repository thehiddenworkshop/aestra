use super::{
    panel::{BrowserItems, BrowserRow, BrowserSearch, SourcesPane, SourcesSplitter},
    state::*,
};
use crate::*;
use aestra_project::{ProjectAssetId, ProjectContentVersion, ProjectSourceId};
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

/// `new_view` opens the asset in a fresh editor tab even when it is already open (the context menu's
/// "Open in New Tab"); otherwise the open focuses an existing tab of the same document.
#[derive(Event)]
pub(super) struct OpenMaterial {
    pub(super) program: aestra_core::MaterialProgramId,
    pub(super) new_view: bool,
}

#[derive(Event)]
pub(super) struct OpenFunction {
    pub(super) function: aestra_core::MaterialFunctionId,
    pub(super) new_view: bool,
}

#[derive(Event)]
pub(super) struct OpenWeslSource {
    pub(super) relative: std::path::PathBuf,
    pub(super) new_view: bool,
}

/// Shared semantic locate route for graph, property, and source-reference controls.
#[derive(Component, Event, Clone, Copy)]
pub(crate) struct LocateInAssets(pub(crate) ProjectAssetId);

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserAction {
    Scope(SourceScope),
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
    NewFolder,
    Duplicate(ProjectSourceId, ProjectContentVersion),
    Rename(ProjectSourceId, ProjectContentVersion),
    Delete(ProjectSourceId, ProjectContentVersion),
    DeletedItems,
    OpenSelected,
    OpenSelectedInNewTab,
    LocateCurrentEffect,
    LocateSource(ProjectSourceId, ProjectContentVersion),
    InspectSource(ProjectSourceId, ProjectContentVersion, InspectionTab),
    InspectionTab(InspectionTab),
    InspectionPage(bool),
}

pub(super) fn activate_button(
    event: On<Activate>,
    actions: Query<&BrowserAction>,
    folders: Query<(), With<super::panel::BrowserFolderButton>>,
    mut commands: Commands,
) {
    if !folders.contains(event.entity)
        && let Ok(action) = actions.get(event.entity)
    {
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
    mut layout: Option<ResMut<WorkspaceLayout>>,
) {
    clicks.0 = None;
    let content = catalog.content();
    match *event {
        BrowserAction::Scope(scope) => {
            state.scope = scope;
            state.legacy = false;
            state.query.clear();
            state.page = 0;
            state.selected = None;
            session.ui_revision += 1;
        }
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
        BrowserAction::Refresh => {
            commands.trigger(crate::library::LibraryAction::RefreshProject);
            commands.trigger(super::relocation_recovery::CheckRecovery);
        }
        BrowserAction::NewFolder => {
            commands.trigger(super::operations::OpenFolderPrompt(None, false, None))
        }
        BrowserAction::Duplicate(source, version) => {
            if version == catalog.content_revision() {
                commands.trigger(super::operations::OpenFolderPrompt(
                    Some((source, version)),
                    false,
                    None,
                ));
            }
        }
        BrowserAction::Rename(source, version) => {
            if version == catalog.content_revision() {
                commands.trigger(super::operations::OpenFolderPrompt(
                    Some((source, version)),
                    true,
                    None,
                ));
            }
        }
        BrowserAction::Delete(source, version) => {
            if version == catalog.content_revision() {
                commands.trigger(super::deletion::Open(Some(source)));
            }
        }
        BrowserAction::DeletedItems => commands.trigger(super::deletion::Open(None)),
        BrowserAction::OpenSelected => {
            if let Some(id) = state.selected {
                open_source(id, false, &catalog, &mut state, &mut commands);
            }
        }
        BrowserAction::OpenSelectedInNewTab => {
            if let Some(id) = state.selected {
                open_source(id, true, &catalog, &mut state, &mut commands);
            }
        }
        BrowserAction::LocateCurrentEffect => {
            // A current document has an exact location even when semantic IDs are duplicated.
            let source = session.source_path.as_deref().and_then(|path| {
                content
                    .source_tree()
                    .entries()
                    .find(|entry| entry.path == path)
                    .map(|entry| entry.id)
            });
            if let Some(source) = source {
                state.locate(content, source);
            } else {
                commands.trigger(LocateInAssets(ProjectAssetId::Effect(session.effect.id)));
            }
        }
        BrowserAction::LocateSource(source, version) => {
            if catalog.content_revision() == version && state.locate(content, source) {
                reveal_browser(&mut state, &mut session, layout.as_deref_mut());
            }
        }
        BrowserAction::InspectSource(source, version, tab) => {
            if version == catalog.content_revision() && content.source(source).is_some() {
                state.inspected = Some(source);
                state.inspection_tab = tab;
                state.inspection_page = 0;
                if let Some(layout) = layout.as_deref_mut() {
                    reveal_dock_panel(layout, &mut session, ToolPanel::AssetInspector);
                }
            }
        }
        BrowserAction::InspectionTab(tab) => {
            state.inspection_tab = tab;
            state.inspection_page = 0;
        }
        BrowserAction::InspectionPage(next) => {
            state.inspection_page = if next {
                state.inspection_page.saturating_add(1)
            } else {
                state.inspection_page.saturating_sub(1)
            };
        }
    }
}

pub(super) fn activate_locate(
    event: On<Activate>,
    actions: Query<&LocateInAssets>,
    mut commands: Commands,
) {
    if let Ok(action) = actions.get(event.entity) {
        commands.trigger(*action);
    }
}

pub(super) fn locate_asset(
    event: On<LocateInAssets>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<AssetBrowserState>,
    mut session: ResMut<EditorSession>,
    mut layout: Option<ResMut<WorkspaceLayout>>,
    localizer: Res<Localizer>,
) {
    match catalog.content().unique_source_for_asset(event.0) {
        Ok(source) => {
            state.locate(catalog.content(), source.id);
            reveal_browser(&mut state, &mut session, layout.as_deref_mut());
            session.status = localizer.text("browser-located");
        }
        Err(error) => {
            let mut args = FluentArgs::new();
            args.set("error", error.to_string());
            session.status = localizer.text_with("browser-locate-failed", &args);
        }
    }
}

fn reveal_browser(
    state: &mut AssetBrowserState,
    session: &mut EditorSession,
    layout: Option<&mut WorkspaceLayout>,
) {
    if state.legacy || state.scope != SourceScope::Project {
        state.legacy = false;
        state.scope = SourceScope::Project;
        session.ui_revision += 1;
    }
    if let Some(layout) = layout {
        reveal_dock_panel(layout, session, ToolPanel::Assets);
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

#[allow(clippy::too_many_arguments)]
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
    editors: Query<(), With<super::operations::InlineRenameEditor>>,
    drag: Res<super::drag_drop::AssetDrag>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    if drag.suppress_click {
        clicks.0 = None;
        event.propagate(false);
        return;
    }
    // Resolve the row immediately, before another widget consumes the descendant click.
    let target = event.entity;
    if std::iter::once(target)
        .chain(parents.iter_ancestors(target))
        .any(|entity| editors.contains(entity))
    {
        event.propagate(false);
        return;
    }
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
            open_source(row.0, false, &catalog, &mut state, &mut commands);
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

/// Routes an open to a fresh view (split / "Open in New Tab") or to the shared default view (focus
/// an existing tab of the document), sharing one helper across the three per-kind open systems.
fn open_view_for(
    new_view: bool,
    documents: &mut crate::document::DocumentManager,
    views: &mut crate::editor_view::EditorViewManager,
    active: &mut crate::editor_view::ActiveEditorContext,
    key: crate::document::DocumentKey,
    kind: crate::editor_view::EditorViewKind,
) -> crate::docking::EditorViewId {
    if new_view {
        crate::editor_view::open_document_view_in_new_tab(documents, views, active, key, kind)
    } else {
        crate::editor_view::open_document_view(documents, views, active, key, kind)
    }
}

pub(super) fn open_source(
    id: ProjectSourceId,
    new_view: bool,
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
        commands.trigger(OpenMaterial { program, new_view });
    } else if let Some(ProjectAssetId::MaterialFunction(function)) = content.asset_for_source(id) {
        commands.trigger(OpenFunction { function, new_view });
    } else if let Some(relative) = wesl_source_relative_path(content, id) {
        commands.trigger(OpenWeslSource { relative, new_view });
    }
}

/// The project-relative path of a `.wesl`/`.wgsl` source at `id`, if that row is one. WESL modules
/// have no semantic asset id, so they are opened by path (Milestone 7).
fn wesl_source_relative_path(
    content: &aestra_project::ProjectContent,
    id: ProjectSourceId,
) -> Option<std::path::PathBuf> {
    use aestra_project::{ProjectFileClassification, ProjectSourceKind};
    let entry = content.source(id)?;
    let ProjectSourceKind::File(info) = &entry.kind else {
        return None;
    };
    if info.classification != ProjectFileClassification::Shader {
        return None;
    }
    let extension = entry
        .relative_path
        .extension()
        .and_then(|extension| extension.to_str())?;
    (extension.eq_ignore_ascii_case("wesl") || extension.eq_ignore_ascii_case("wgsl"))
        .then(|| entry.relative_path.clone())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn open_function(
    event: On<OpenFunction>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    catalog: Res<ProjectEffectCatalog>,
    io: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    protection: Option<Res<crate::persistence::DocumentProtectionState>>,
    mut documents: ResMut<crate::document::DocumentManager>,
    mut views: ResMut<crate::editor_view::EditorViewManager>,
    mut active: ResMut<crate::editor_view::ActiveEditorContext>,
) {
    if !crate::project_content::io::idle(io) || protection.is_some_and(|value| value.is_open()) {
        return;
    }
    match session.open_material_function(&catalog, event.function) {
        Ok(()) => {
            let view = open_view_for(
                event.new_view,
                &mut documents,
                &mut views,
                &mut active,
                crate::document::DocumentKey::MaterialFunction(event.function),
                crate::editor_view::EditorViewKind::MaterialFunctionGraph,
            );
            session.status = if session
                .graph_function(&catalog)
                .is_ok_and(|function| function.custom_wesl.is_some())
            {
                "Code function opened: custom WESL source is read-only".into()
            } else {
                "Graph function opened; edit its signature in Properties".into()
            };
            crate::shell::reveal_editor_tab(&mut layout, &mut session, view);
        }
        Err(error) => {
            session.status = format!("Cannot open function: {error}");
            session.ui_revision += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn open_material(
    event: On<OpenMaterial>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
    catalog: Res<ProjectEffectCatalog>,
    io: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    protection: Option<Res<crate::persistence::DocumentProtectionState>>,
    mut documents: ResMut<crate::document::DocumentManager>,
    mut views: ResMut<crate::editor_view::EditorViewManager>,
    mut active: ResMut<crate::editor_view::ActiveEditorContext>,
) {
    if !crate::project_content::io::idle(io)
        || protection.is_some_and(|protection| protection.is_open())
    {
        return;
    }
    // This is a non-destructive target switch: retain all drafts and effect state.
    // Destructive effect/project navigation still uses the document coordinator.
    if let Err(error) = session.open_material_program(&catalog, event.program) {
        session.status = format!("Cannot open material: {error}");
    } else {
        let view = open_view_for(
            event.new_view,
            &mut documents,
            &mut views,
            &mut active,
            crate::document::DocumentKey::MaterialProgram(event.program),
            crate::editor_view::EditorViewKind::MaterialGraph,
        );
        session.status = localizer.text("browser-material-opened");
        crate::shell::reveal_editor_tab(&mut layout, &mut session, view);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn open_wesl_source(
    event: On<OpenWeslSource>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    io: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    protection: Option<Res<crate::persistence::DocumentProtectionState>>,
    mut wesl_documents: ResMut<crate::wesl_document::WeslDocuments>,
    mut documents: ResMut<crate::document::DocumentManager>,
    mut views: ResMut<crate::editor_view::EditorViewManager>,
    mut active: ResMut<crate::editor_view::ActiveEditorContext>,
) {
    if !crate::project_content::io::idle(io) || protection.is_some_and(|value| value.is_open()) {
        return;
    }
    let relative = &event.relative;
    let absolute = catalog.root().join(relative);
    let text = match std::fs::read_to_string(&absolute) {
        Ok(text) => text,
        Err(error) => {
            session.status = format!("Cannot open WESL source: {error}");
            return;
        }
    };
    let id = wesl_documents.open(relative.clone(), text);
    let view = open_view_for(
        event.new_view,
        &mut documents,
        &mut views,
        &mut active,
        crate::document::DocumentKey::WeslSource(id),
        crate::editor_view::EditorViewKind::WeslSource,
    );
    session.status = localizer.text("browser-wesl-opened");
    crate::shell::reveal_editor_tab(&mut layout, &mut session, view);
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
        KeyCode::F2 => {
            if let Some(row) = active.0.and_then(|id| rows.get(id).ok()) {
                commands.trigger(BrowserAction::Rename(row.0, catalog.content_revision()));
            }
        }
        KeyCode::Delete => {
            if let Some(row) = active.0.and_then(|id| rows.get(id).ok()) {
                commands.trigger(BrowserAction::Delete(row.0, catalog.content_revision()));
            }
        }
        KeyCode::Enter => {
            if let Some(row) = active.0.and_then(|id| rows.get(id).ok()) {
                open_source(row.0, false, &catalog, &mut state, &mut commands);
            }
        }
        KeyCode::Escape => state.selected = None,
        KeyCode::Backspace => commands.trigger(BrowserAction::Up),
        _ => return,
    }
    clicks.0 = None;
    event.propagate(false);
}

pub(super) fn begin_resize_sources(
    mut event: On<Pointer<DragStart>>,
    mut splitters: Query<(&ChildOf, &mut SourcesSplitter)>,
    sources: Query<(&ChildOf, &ComputedNode), With<SourcesPane>>,
) {
    if event.button == PointerButton::Primary
        && let Ok((parent, mut splitter)) = splitters.get_mut(event.entity)
        && let Some((_, node)) = sources
            .iter()
            .find(|(source_parent, _)| *source_parent == parent)
    {
        event.propagate(false);
        // Preferences may be wider than the current panel's percentage cap.
        // Anchor at the rendered edge so reversing direction reacts immediately.
        splitter.drag_start_width = Some(node.size().x * node.inverse_scale_factor());
    }
}

pub(super) fn resize_sources(
    mut event: On<Pointer<Drag>>,
    splitters: Query<(&ComputedNode, &SourcesSplitter)>,
    mut state: ResMut<AssetBrowserState>,
) {
    if event.button == PointerButton::Primary
        && let Ok((node, splitter)) = splitters.get(event.entity)
        && let Some(start_width) = splitter.drag_start_width
    {
        event.propagate(false);
        state.sources_width =
            (start_width + event.distance.x * node.inverse_scale_factor()).clamp(90.0, 360.0);
    }
}

pub(super) fn end_resize_sources(
    mut event: On<Pointer<DragEnd>>,
    mut splitters: Query<&mut SourcesSplitter>,
) {
    if event.button == PointerButton::Primary
        && let Ok(mut splitter) = splitters.get_mut(event.entity)
    {
        event.propagate(false);
        splitter.drag_start_width = None;
    }
}
