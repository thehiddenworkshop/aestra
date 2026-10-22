//! Production project-bound material layout persistence and registration.
use crate::feathers::node_graph::GraphViewportMemory;
use crate::material_graph::{
    MaterialGraphPreviewState, function_layout,
    keys::{
        MATERIAL_GRAPH_OUTPUT_NODE_KEY, material_graph_expression_node_key, material_graph_view_key,
    },
    layout_lifecycle,
};
use crate::{EditorSession, EditorSet, PersistenceSet, ProjectEffectCatalog};
use aestra_core::material::MaterialProgram;
use aestra_project::{MaterialGraphNodeLayout, MaterialGraphViewportLayout, ProjectEditorLayout};
use bevy::prelude::*;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) const MATERIAL_GRAPH_LAYOUT_SAVE_DELAY: Duration = Duration::from_millis(300);

pub(super) fn register(app: &mut App) {
    layout_lifecycle::register_notice(app);
    app.init_resource::<MaterialGraphLayoutPersistence>()
        .init_resource::<MaterialGraphPreviewState>()
        .add_systems(Startup, load_material_graph_layout)
        .add_systems(
            Update,
            layout_lifecycle::reconcile
                .after(PersistenceSet::Lifecycle)
                .before(EditorSet::UiRebuild),
        )
        .add_systems(
            Last,
            (
                function_layout::mirror_graph_camera,
                persist_material_graph_layout,
                flush_material_graph_layout_on_exit,
            )
                .chain(),
        );
}

#[derive(Resource, Debug, Default)]
pub(super) struct MaterialGraphLayoutPersistence {
    pub(super) root: Option<PathBuf>,
    pub(super) document: ProjectEditorLayout,
    pub(super) persisted: ProjectEditorLayout,
    pub(super) changed_at: Option<Instant>,
    pub(super) last_error: Option<String>,
    /// A failed load must not turn a future/corrupt file into a writable empty layout.
    pub(super) write_blocked: bool,
    pub(super) reload_requested: bool,
}

pub(super) fn load_material_graph_layout(
    catalog: Res<ProjectEffectCatalog>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut previews: ResMut<MaterialGraphPreviewState>,
) {
    load_graph_layout(
        catalog.root(),
        &mut persistence,
        &mut graph_memory,
        &mut previews,
    );
}

pub(super) fn load_graph_layout(
    root: &std::path::Path,
    persistence: &mut MaterialGraphLayoutPersistence,
    graph_memory: &mut GraphViewportMemory,
    previews: &mut MaterialGraphPreviewState,
) {
    let root = root.to_owned();
    // Capture the old project's last complete layout before replacing its namespace. Never
    // combine the new catalog with the previous project's graph memory.
    if persistence.root.as_deref().is_some_and(|old| old != root)
        && persistence.document != persistence.persisted
    {
        save_material_graph_layout(persistence);
    }
    graph_memory.retain_graphs(|key| !layout_lifecycle::is_material_graph(key));
    *previews = default();
    let (document, load_error) = match ProjectEditorLayout::load(&root) {
        Ok(document) => (document, None),
        Err(error) => {
            warn!(
                "failed to load project material graph layout from {}: {error}",
                root.display()
            );
            (ProjectEditorLayout::default(), Some(error.to_string()))
        }
    };
    restore_material_graph_layouts(&document, graph_memory, previews);
    function_layout::restore(&root, &document, graph_memory);
    persistence.root = Some(root);
    persistence.persisted = document.clone();
    persistence.document = document;
    persistence.changed_at = None;
    persistence.write_blocked = load_error.is_some();
    persistence.last_error = load_error;
}

pub(super) fn persist_material_graph_layout(
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    graph_memory: Res<GraphViewportMemory>,
    previews: Res<MaterialGraphPreviewState>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
) {
    // Do not combine another project's catalog/drafts with this session's loaded layout.
    // The lifecycle system reloads before UI rebuild; keep this guard for late root changes.
    if persistence.write_blocked || persistence.root.as_deref() != Some(catalog.root()) {
        return;
    }
    // Rebuild the layout document only when an input that feeds it changed. An idle graph (no edit,
    // pan, or node move) leaves all of these unchanged, so it does no per-frame work here.
    if graph_memory.is_changed()
        || session.is_changed()
        || catalog.is_changed()
        || previews.is_changed()
    {
        let programs = layout_lifecycle::programs(&catalog, &session);
        update_material_graph_layout_document(
            &mut persistence.document,
            &programs,
            &graph_memory,
            &previews,
        );
        if let Ok(functions) = catalog.material_functions() {
            function_layout::update(
                catalog.root(),
                &mut persistence.document,
                &functions,
                &graph_memory,
            );
        }
        if persistence.document == persistence.persisted {
            persistence.changed_at = None;
        } else {
            persistence.changed_at.get_or_insert_with(Instant::now);
        }
    }
    // Fire the debounced save once a pending change has settled, whether or not this frame rebuilt.
    if let Some(changed_at) = persistence.changed_at
        && Instant::now().duration_since(changed_at) >= MATERIAL_GRAPH_LAYOUT_SAVE_DELAY
    {
        save_material_graph_layout(&mut persistence);
    }
}

pub(super) fn flush_material_graph_layout_on_exit(
    mut exits: MessageReader<AppExit>,
    catalog: Res<ProjectEffectCatalog>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
) {
    if exits.read().next().is_some()
        && persistence.root.as_deref() == Some(catalog.root())
        && persistence.document != persistence.persisted
    {
        save_material_graph_layout(&mut persistence);
    }
}

pub(super) fn save_material_graph_layout(persistence: &mut MaterialGraphLayoutPersistence) {
    if persistence.write_blocked {
        return;
    }
    let Some(root) = persistence.root.clone() else {
        return;
    };
    // Recheck format/readability in case another editor replaced the file since startup.
    if let Err(error) = ProjectEditorLayout::load(&root) {
        let message = format!("layout saving disabled: {error}");
        warn!("{}: {message}", root.display());
        persistence.last_error = Some(message);
        persistence.write_blocked = true;
        persistence.changed_at = None;
        return;
    }
    match persistence.document.save(&root) {
        Ok(()) => {
            persistence.persisted = persistence.document.clone();
            persistence.changed_at = None;
            persistence.last_error = None;
        }
        Err(error) => {
            let message = error.to_string();
            if persistence.last_error.as_deref() != Some(&message) {
                warn!(
                    "failed to save project material graph layout to {}: {message}",
                    root.display()
                );
            }
            persistence.last_error = Some(message);
            persistence.changed_at = Some(Instant::now());
        }
    }
}

pub(super) fn restore_material_graph_layouts(
    document: &ProjectEditorLayout,
    graph_memory: &mut GraphViewportMemory,
    previews: &mut MaterialGraphPreviewState,
) {
    for (program, layout) in &document.material_graphs {
        let graph_key = material_graph_view_key(*program);
        if let Some(viewport) = layout.viewport {
            graph_memory.set_view(
                graph_key.clone(),
                Vec2::from_array(viewport.pan),
                viewport.zoom,
            );
        }
        for (expression, node) in &layout.nodes {
            graph_memory.set_node(
                graph_key.clone(),
                material_graph_expression_node_key(*expression),
                Vec2::from_array(node.position),
                node.collapsed,
            );
            graph_memory.set_pinned(
                &graph_key,
                &material_graph_expression_node_key(*expression),
                node.pinned,
            );
        }
        if let Some(output) = layout.output {
            graph_memory.set_node(
                graph_key.clone(),
                MATERIAL_GRAPH_OUTPUT_NODE_KEY,
                Vec2::from_array(output.position),
                output.collapsed,
            );
            graph_memory.set_pinned(&graph_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY, output.pinned);
        }
        previews.restore_layout(
            *program,
            &layout.visible_previews,
            layout.output_preview_visible,
        );
    }
}

pub(super) fn update_material_graph_layout_document(
    document: &mut ProjectEditorLayout,
    programs: &[MaterialProgram],
    graph_memory: &GraphViewportMemory,
    previews: &MaterialGraphPreviewState,
) {
    for program in programs {
        let graph_key = material_graph_view_key(program.id);
        let expressions = program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<BTreeSet<_>>();
        let mut layout = document
            .material_graphs
            .remove(&program.id)
            .unwrap_or_default();
        layout.retain_expressions(&expressions);
        layout.viewport =
            graph_memory
                .view(&graph_key)
                .map(|(pan, zoom)| MaterialGraphViewportLayout {
                    pan: pan.to_array(),
                    zoom,
                });
        layout.nodes = expressions
            .iter()
            .filter_map(|expression| {
                graph_memory
                    .node(&graph_key, &material_graph_expression_node_key(*expression))
                    .map(|(position, collapsed)| {
                        (
                            *expression,
                            MaterialGraphNodeLayout {
                                position: position.to_array(),
                                collapsed,
                                pinned: graph_memory.is_pinned(
                                    &graph_key,
                                    &material_graph_expression_node_key(*expression),
                                ),
                            },
                        )
                    })
            })
            .collect();
        layout.output = graph_memory
            .node(&graph_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY)
            .map(|(position, collapsed)| MaterialGraphNodeLayout {
                position: position.to_array(),
                collapsed,
                pinned: graph_memory.is_pinned(&graph_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
            });
        layout.visible_previews = previews
            .visible_expressions(program.id)
            .intersection(&expressions)
            .copied()
            .collect();
        layout.output_preview_visible = previews.output_visible(program.id);
        document.material_graphs.insert(program.id, layout);
    }
}

#[cfg(test)]
#[path = "layout_persistence/tests.rs"]
mod tests;
