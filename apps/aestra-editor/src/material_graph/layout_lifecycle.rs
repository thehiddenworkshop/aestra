//! Project-bound placement lifecycle. The active project's legacy material keys are reset as a
//! unit; generic graph widgets are untouched. Base placement survives semantic reloads, while
//! removed identities and temporary offsets do not.
use super::*;
use crate::{document::DocumentKey, feathers::node_graph::geometry::GraphGeometryRegistry};
use std::hash::{DefaultHasher, Hash, Hasher};

pub(super) fn is_material_graph(key: &str) -> bool {
    key.starts_with("material:") || key.starts_with("function:")
}

/// Include inactive/shared documents, not just the currently focused effect or function tab.
pub(super) fn programs(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Vec<MaterialProgram> {
    let mut programs = BTreeMap::new();
    for entry in catalog.content().asset_index().material_programs() {
        if let Some(reference) = entry.reference
            && let Ok(program) = catalog.material_program(reference.id())
        {
            programs.insert(program.id, program);
        }
    }
    for (id, draft) in &catalog.material_drafts.programs {
        if let Some(program) = &draft.current {
            programs.insert(*id, program.clone());
        } else {
            programs.remove(id);
        }
    }
    // Built-ins have no project source entry. Keep them even when another effect reference is
    // unresolved, and never let the active effect reintroduce a tombstoned shared draft.
    for instance in &session.effect.material_instances {
        if let Some(program) = MaterialProgram::built_in(instance.program) {
            programs.insert(program.id, program);
        }
    }
    programs.into_values().collect()
}

#[derive(Default)]
pub(super) struct Observed {
    root: Option<PathBuf>,
    fingerprints: BTreeMap<DocumentKey, u64>,
}

fn fingerprint(value: &impl std::fmt::Debug) -> u64 {
    let mut hash = DefaultHasher::new();
    format!("{value:?}").hash(&mut hash);
    hash.finish()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reconcile(
    mut commands: Commands,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
    mut memory: ResMut<GraphViewportMemory>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut registry: ResMut<GraphGeometryRegistry>,
    views: Query<(Entity, &GraphGeometryView)>,
    mut observed: Local<Observed>,
) {
    let switched = persistence.root.as_deref() != Some(catalog.root());
    let retry = std::mem::take(&mut persistence.bypass_change_detection().reload_requested);
    let reload = retry
        && persistence.write_blocked
        && match ProjectEditorLayout::load(catalog.root()) {
            Ok(_) => true,
            Err(error) => {
                persistence.last_error = Some(error.to_string());
                false
            }
        };
    if switched || reload {
        load_graph_layout(catalog.root(), &mut persistence, &mut memory, &mut previews);
        // Stale UI cannot write an old camera/drag back into the new project's memory.
        for (entity, _) in &views {
            commands.entity(entity).despawn();
        }
        *registry = default();
        observed.fingerprints.clear();
        commands.queue(clear_document_transients);
        session.ui_revision += 1;
    }
    if observed.root.as_deref() != Some(catalog.root()) {
        observed.root = Some(catalog.root().to_owned());
        observed.fingerprints.clear();
    } else if !catalog.is_changed() && !session.is_changed() {
        return;
    }

    let programs = programs(&catalog, &session);
    let functions = catalog.material_functions();
    // Absence is only authoritative in a complete, parseable inventory. A transient read/parse
    // failure must not erase a user's placement. Reconcile resolved documents independently.
    let ready = matches!(
        catalog.content().asset_index().availability(),
        aestra_project::ProjectAssetIndexAvailability::Ready
    ) && !catalog
        .content()
        .source_tree()
        .diagnostics()
        .iter()
        .any(|diagnostic| {
            diagnostic.code == aestra_project::ProjectAssetDiagnosticCode::SourceUnavailable
        });
    let materials_complete = ready
        && catalog
            .content()
            .asset_index()
            .material_programs()
            .iter()
            .all(|entry| entry.reference.is_some());
    let functions_complete = ready && functions.is_ok();
    let mut removed = Vec::new();
    for id in persistence.document.material_graphs.keys().copied() {
        if materials_complete
            && !programs.iter().any(|program| program.id == id)
            && catalog.material_program_missing(id)
            && MaterialProgram::built_in(aestra_core::material::MaterialProgramRef::BuiltIn(id))
                .is_none()
        {
            removed.push((
                DocumentKey::MaterialProgram(id),
                material_graph_view_key(id),
            ));
        }
    }
    for id in persistence.document.function_graphs.keys().copied() {
        if functions_complete
            && !functions
                .as_ref()
                .unwrap()
                .iter()
                .any(|function| function.id == id)
            && catalog.material_function_missing(id)
        {
            removed.push((
                DocumentKey::MaterialFunction(id),
                function_graph_memory_key(catalog.root(), id),
            ));
        }
    }
    for (document, key) in removed {
        let prefix = format!("{key}#view:");
        let tool = format!("{key}#tool");
        memory.retain_graphs(|candidate| {
            candidate != key && candidate != tool && !candidate.starts_with(&prefix)
        });
        observed.fingerprints.remove(&document);
        match document {
            DocumentKey::MaterialProgram(id) => {
                persistence.document.material_graphs.remove(&id);
                previews.visible.retain(|(program, _)| *program != id);
                previews.cache.retain(|(program, _), _| *program != id);
            }
            DocumentKey::MaterialFunction(id) => {
                persistence.document.function_graphs.remove(&id);
            }
            DocumentKey::WeslSource(_) => unreachable!(),
        }
    }
    for program in &programs {
        let document = DocumentKey::MaterialProgram(program.id);
        let stamp = fingerprint(program);
        if observed.fingerprints.insert(document, stamp) == Some(stamp) {
            continue;
        }
        let key = material_graph_view_key(program.id);
        let expressions = program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<BTreeSet<_>>();
        let keys = expressions
            .iter()
            .map(|id| material_graph_expression_node_key(*id))
            .collect::<BTreeSet<_>>();
        memory.retain_nodes(&key, |node| {
            node == MATERIAL_GRAPH_OUTPUT_NODE_KEY || keys.contains(node)
        });
        memory.clear_offsets(&key);
        let keep = |(id, target): &(MaterialProgramId, MaterialGraphPreviewTarget)| {
            *id != program.id
                || match target {
                    MaterialGraphPreviewTarget::Expression(id) => expressions.contains(id),
                    MaterialGraphPreviewTarget::Output => true,
                }
        };
        previews.visible.retain(keep);
        previews.cache.retain(|key, _| keep(key));
    }
    if let Ok(functions) = functions {
        for function in functions {
            let document = DocumentKey::MaterialFunction(function.id);
            let stamp = fingerprint(&function);
            if observed.fingerprints.insert(document, stamp) == Some(stamp) {
                continue;
            }
            let key = function_graph_memory_key(catalog.root(), function.id);
            let keys = function
                .expressions
                .iter()
                .map(|expression| expression.id.to_string())
                .collect::<BTreeSet<_>>();
            memory.retain_nodes(&key, |node| {
                function.custom_wesl.is_none() && (node == "outputs" || keys.contains(node))
            });
            memory.clear_offsets(&key);
            if function.custom_wesl.is_some() {
                persistence.document.function_graphs.remove(&function.id);
            }
        }
    }
    if persistence.document != persistence.persisted {
        persistence.changed_at.get_or_insert_with(Instant::now);
    }
}

#[cfg(test)]
mod tests;

#[derive(Component)]
struct LayoutNotice {
    error: String,
}

#[derive(Component)]
struct RetryLayout(PathBuf);

pub(super) fn register_notice(app: &mut App) {
    app.add_observer(retry_layout)
        .add_systems(Update, sync_notice.after(EditorSet::UiRebuild));
}

fn retry_layout(
    activate: On<Activate>,
    buttons: Query<&RetryLayout>,
    catalog: Res<ProjectEffectCatalog>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
) {
    if let Ok(button) = buttons.get(activate.entity)
        && button.0 == catalog.root()
        && persistence.write_blocked
    {
        persistence.reload_requested = true;
    }
}

fn sync_notice(
    mut commands: Commands,
    catalog: Res<ProjectEffectCatalog>,
    persistence: Res<MaterialGraphLayoutPersistence>,
    localizer: Res<Localizer>,
    views: Query<(Entity, &GraphGeometryView)>,
    notices: Query<(Entity, &ChildOf, &LayoutNotice)>,
) {
    let error = persistence
        .write_blocked
        .then(|| persistence.last_error.as_deref())
        .flatten();
    for (entity, _, notice) in &notices {
        if error != Some(notice.error.as_str()) || localizer.is_changed() {
            commands.entity(entity).despawn();
        }
    }
    let Some(error) = error else {
        return;
    };
    for (entity, view) in &views {
        if view.key.document.project != catalog.root()
            || notices.iter().any(|(_, parent, notice)| {
                parent.parent() == entity && notice.error == error && !localizer.is_changed()
            })
        {
            continue;
        }
        commands.entity(entity).with_children(|parent| {
            parent
                .spawn((
                    LayoutNotice {
                        error: error.to_owned(),
                    },
                    FeathersGraphNavigationBlocker,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(8.0),
                        right: Val::Px(8.0),
                        top: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                    ZIndex(50),
                    crate::feathers::tooltip::EditorTooltip::description(error),
                ))
                .with_children(|notice| {
                    notice.spawn((
                        Text::new(localizer.text("graph-layout-saving-blocked")),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    crate::feathers::button::spawn_action_button(
                        notice,
                        &localizer.text("graph-layout-reload-saved"),
                        RetryLayout(catalog.root().to_owned()),
                        false,
                    );
                });
        });
    }
}
