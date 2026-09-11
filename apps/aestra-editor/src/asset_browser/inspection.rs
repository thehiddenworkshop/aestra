//! Read-only inspection of the published project snapshot, independent of emitter selection.
use super::{actions::BrowserAction, panel, state::*};
use crate::*;
use aestra_project::{
    ProjectAssetId, ProjectContent, ProjectRelationStatus, ProjectRelationTarget, ProjectSourceId,
};

const RELATION_PAGE_SIZE: usize = 24;

#[cfg(test)]
#[test]
fn preflight_includes_unsaved_program_dependencies() {
    use aestra_core::material::*;
    let root = tempfile::tempdir().unwrap();
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    let program = MaterialProgram::additive_sprite("Caller").normalized();
    program
        .save_ron(root.path().join("caller.aestra.material.ron"))
        .unwrap();
    let mut catalog = ProjectEffectCatalog::scan(root.path());
    let session = crate::test_support::session_with_timing_slack();
    let target = catalog
        .content()
        .unique_source_for_asset(ProjectAssetId::MaterialFunction(function.id))
        .unwrap()
        .id;
    assert!(preflight(&catalog, &session, target).usages.is_empty());
    let mut draft = program.clone();
    draft.expressions[0].kind = MaterialExpressionKind::FunctionCall {
        function: MaterialFunctionRef::Project(function.id),
        output: function.outputs[0].id,
        arguments: Default::default(),
    };
    catalog.replace_material_program(&program, &draft).unwrap();
    assert_eq!(preflight(&catalog, &session, target).usages.len(), 1);
    assert_eq!(
        MaterialProgram::load_ron(root.path().join("caller.aestra.material.ron")).unwrap(),
        program
    );
}

type InspectionKey = (
    Option<ProjectSourceId>,
    Option<aestra_project::ProjectContentVersion>,
    InspectionTab,
    usize,
);
#[derive(Component, Default)]
pub(super) struct AssetInspectorUi(
    Option<InspectionKey>,
    Option<(
        aestra_core::EffectAsset,
        crate::material_drafts::MaterialDrafts,
        bool,
        bool,
    )>,
);

fn preflight(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    source: ProjectSourceId,
) -> aestra_project::content::operations::ReferencePreflight {
    let (drafts, complete) = draft_inventory(catalog, session);
    catalog
        .content()
        .reference_preflight(source, &drafts, complete)
}

pub(super) fn draft_inventory(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    collect_drafts(catalog, session, true)
}

/// Deletion checks the active effect's current references separately, including
/// untitled effects which have no indexed source to attach a draft to.
pub(super) fn deletion_draft_inventory(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    collect_drafts(catalog, session, false)
}

fn collect_drafts(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    include_effect: bool,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    use aestra_project::content::operations::DraftDocument;
    let content = catalog.content();
    let mut drafts = Vec::new();
    // The deletion coordinator checks both active and proposed effect references.
    let mut complete = !include_effect || session.pending_change.is_none();
    if include_effect && session.effect_is_dirty() {
        if let Ok(entry) =
            content.unique_source_for_asset(ProjectAssetId::Effect(session.effect.id))
        {
            drafts.push((
                entry.id,
                DraftDocument::Effect(Box::new(session.effect.clone())),
            ));
        } else {
            complete = false;
        }
    }
    for (id, draft) in &catalog.material_drafts.programs {
        if let (Ok(entry), Some(current)) = (
            content.unique_source_for_asset(ProjectAssetId::MaterialProgram(*id)),
            &draft.current,
        ) {
            drafts.push((entry.id, DraftDocument::Program(Box::new(current.clone()))));
        } else {
            complete = false;
        }
    }
    for (id, draft) in &catalog.material_drafts.functions {
        if let (Ok(entry), Some(current)) = (
            content.unique_source_for_asset(ProjectAssetId::MaterialFunction(*id)),
            &draft.current,
        ) {
            drafts.push((entry.id, DraftDocument::Function(Box::new(current.clone()))));
        } else {
            complete = false;
        }
    }
    if !catalog.material_drafts.is_empty()
        && catalog.material_drafts.root.as_deref() != Some(catalog.root())
    {
        complete = false;
    }
    (drafts, complete)
}

pub(crate) fn spawn_asset_inspector(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        AssetInspectorUi::default(),
        super::BrowserSurface,
        crate::feathers::node_graph::FeathersGraphNavigationBlocker,
        RelativeCursorPosition::default(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            ..default()
        },
    ));
}

pub(super) fn sync_panel(
    mut commands: Commands,
    mut panels: Query<(Entity, &mut AssetInspectorUi)>,
    mut state: ResMut<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
    session: Res<EditorSession>,
) {
    for (entity, mut ui) in &mut panels {
        let key = (
            state.inspected,
            state.version,
            state.inspection_tab,
            state.inspection_page,
        );
        let draft_key = (state.inspection_tab == InspectionTab::Preflight).then(|| {
            (
                session.effect.clone(),
                catalog.material_drafts.clone(),
                session.pending_change.is_some(),
                session.effect_is_dirty(),
            )
        });
        if ui.0 == Some(key) && ui.1 == draft_key && !localizer.is_changed() {
            continue;
        }
        if ui.0.is_none_or(|old| old.0 != key.0 || old.1 != key.1) {
            state.inspection_page = 0;
        }
        sync(
            &mut commands,
            entity,
            &mut state,
            catalog.content(),
            &catalog,
            &localizer,
            &assets,
            &session,
        );
        ui.1 = draft_key;
        ui.0 = Some((
            state.inspected,
            state.version,
            state.inspection_tab,
            state.inspection_page,
        ));
    }
}

#[derive(Debug)]
struct RelationRow {
    label: String,
    status: ProjectRelationStatus,
    source: Option<ProjectSourceId>,
}

fn identity(asset: ProjectAssetId) -> String {
    match asset {
        ProjectAssetId::Effect(id) => id.to_string(),
        ProjectAssetId::MaterialProgram(id) => id.to_string(),
        ProjectAssetId::MaterialFunction(id) => id.to_string(),
        ProjectAssetId::MaterialPreset(id) => id.to_string(),
    }
}

fn relation_rows(
    content: &ProjectContent,
    selected: ProjectSourceId,
    tab: InspectionTab,
) -> (Vec<RelationRow>, bool) {
    let report = content.source_relations(selected);
    let known = if tab == InspectionTab::Dependencies {
        report.dependencies_known
    } else {
        report.usages_complete
    };
    let edges = if tab == InspectionTab::Dependencies {
        report.dependencies
    } else {
        report.usages
    };
    let mut rows = Vec::new();
    for edge in edges {
        let status = content.relation_status(&edge.target);
        let sources = if tab == InspectionTab::Usages {
            vec![edge.owner]
        } else {
            content.relation_sources(&edge.target)
        };
        if sources.is_empty() {
            let label = match edge.target {
                ProjectRelationTarget::Asset(id) | ProjectRelationTarget::BuiltIn(id) => {
                    identity(id)
                }
                ProjectRelationTarget::File(path) => path.display().to_string(),
                ProjectRelationTarget::ContextualResource(id) => id.to_string(),
            };
            rows.push(RelationRow {
                label,
                status,
                source: None,
            });
        } else {
            for id in sources {
                if let Some(source) = content.source(id) {
                    rows.push(RelationRow {
                        label: source.relative_path.display().to_string(),
                        status,
                        source: Some(id),
                    });
                }
            }
        }
    }
    (rows, known)
}

fn line(parent: &mut ChildSpawnerCommands, value: impl Into<String>) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: BrowserAction,
    selected: bool,
) -> Entity {
    let entity = mini_button(parent, label, action);
    // Shared tool buttons default to an icon-sized width; text actions must size to their label.
    parent.commands().entity(entity).insert(Node {
        height: Val::Px(24.0),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(Val::Px(6.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    });
    if selected {
        parent
            .commands()
            .entity(entity)
            .insert(bevy::feathers::controls::ButtonVariant::Primary);
    }
    entity
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    commands: &mut Commands,
    entity: Entity,
    state: &mut AssetBrowserState,
    content: &ProjectContent,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    assets: &AssetServer,
    session: &EditorSession,
) {
    commands.entity(entity).despawn_children();
    let Some(selected) = state.inspected.filter(|id| content.source(*id).is_some()) else {
        commands
            .entity(entity)
            .insert(Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            })
            .with_children(|parent| {
                line(
                    parent,
                    localizer.text(if state.inspected.is_some() {
                        "browser-inspected-missing"
                    } else {
                        "browser-inspector-empty"
                    }),
                );
            });
        return;
    };
    let (rows, known) = relation_rows(content, selected, state.inspection_tab);
    let preflight = (state.inspection_tab == InspectionTab::Preflight)
        .then(|| preflight(catalog, session, selected));
    let pages = rows.len().max(1).div_ceil(RELATION_PAGE_SIZE);
    state.inspection_page = state.inspection_page.min(pages - 1);
    let mut details_host = None;
    commands
        .entity(entity)
        .insert((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|inspector| {
            inspector
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|tabs| {
                    for (tab, key) in [
                        (InspectionTab::Details, "browser-details"),
                        (InspectionTab::Dependencies, "browser-dependencies"),
                        (InspectionTab::Usages, "browser-usages"),
                        (InspectionTab::Preflight, "browser-preflight"),
                    ] {
                        button(
                            tabs,
                            &localizer.text(key),
                            BrowserAction::InspectionTab(tab),
                            state.inspection_tab == tab,
                        );
                    }
                    tabs.spawn((
                        EditorTooltip::description(localizer.text("browser-snapshot-scope")),
                        AccessibleLabel(localizer.text("browser-snapshot-scope")),
                        Node {
                            width: Val::Px(24.0),
                            height: Val::Px(24.0),
                            flex_shrink: 0.0,
                            margin: UiRect::left(Val::Auto),
                            padding: UiRect::all(Val::Px(4.0)),
                            ..default()
                        },
                    ))
                    .with_child((
                        Node {
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            ..default()
                        },
                        bevy_resvg::prelude::UiSvg(crate::feathers::icon::load_svg_icon(
                            assets,
                            "icons/info.svg",
                        )),
                        bevy_resvg::prelude::SvgColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                });
            inspector
                .spawn(Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|body| {
                    let scroller = body
                        .spawn((
                            Node {
                                flex_grow: 1.0,
                                flex_basis: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                overflow: Overflow::scroll_y(),
                                scrollbar_width: 0.0,
                                ..default()
                            },
                            bevy::ui_widgets::ScrollArea,
                        ))
                        .with_children(|scroll| {
                            line(
                                scroll,
                                content
                                    .source(selected)
                                    .unwrap()
                                    .relative_path
                                    .display()
                                    .to_string(),
                            );
                            if let Some(report) = &preflight {
                                line(scroll, localizer.text("browser-preflight-scope"));
                                line(
                                    scroll,
                                    format!(
                                        "{} affected sources · {} known usages",
                                        report.affected_sources.len(),
                                        report.usages.len()
                                    ),
                                );
                                for reason in report.incomplete.iter().take(24) {
                                    line(scroll, reason.clone());
                                }
                                if report.incomplete.len() > 24 {
                                    line(
                                        scroll,
                                        format!(
                                            "{} additional incomplete-analysis reasons",
                                            report.incomplete.len() - 24
                                        ),
                                    );
                                }
                                for usage in report.usages.iter().take(24) {
                                    if let Some(owner) = content.source(usage.owner) {
                                        line(scroll, owner.relative_path.display().to_string());
                                    }
                                }
                            } else if state.inspection_tab == InspectionTab::Details {
                                if let Some(asset) = content.asset_for_source(selected) {
                                    line(scroll, format!("ID: {}", identity(asset)));
                                }
                                details_host = Some(scroll.spawn_empty().id());
                            } else {
                                if !known {
                                    line(
                                        scroll,
                                        localizer.text(
                                            if state.inspection_tab == InspectionTab::Dependencies {
                                                "browser-relations-unknown"
                                            } else {
                                                "browser-usages-incomplete"
                                            },
                                        ),
                                    );
                                }
                                if rows.is_empty() && known {
                                    line(scroll, localizer.text("browser-relations-empty"));
                                }
                                for row in rows
                                    .iter()
                                    .skip(state.inspection_page * RELATION_PAGE_SIZE)
                                    .take(RELATION_PAGE_SIZE)
                                {
                                    scroll
                                        .spawn(Node {
                                            width: Val::Percent(100.0),
                                            flex_shrink: 0.0,
                                            flex_direction: FlexDirection::Row,
                                            align_items: AlignItems::Center,
                                            min_height: Val::Px(28.0),
                                            padding: UiRect::all(Val::Px(3.0)),
                                            ..default()
                                        })
                                        .with_children(|item| {
                                            let key = match row.status {
                                                ProjectRelationStatus::Available => {
                                                    "browser-relation-available"
                                                }
                                                ProjectRelationStatus::Missing => {
                                                    "browser-relation-missing"
                                                }
                                                ProjectRelationStatus::Ambiguous => {
                                                    "browser-relation-ambiguous"
                                                }
                                                ProjectRelationStatus::Unavailable => {
                                                    "browser-relation-unavailable"
                                                }
                                                ProjectRelationStatus::BuiltIn => {
                                                    "browser-relation-builtin"
                                                }
                                                ProjectRelationStatus::ContextRequired => {
                                                    "browser-relation-context"
                                                }
                                            };
                                            item.spawn(Node {
                                                flex_grow: 1.0,
                                                flex_basis: Val::Px(0.0),
                                                min_width: Val::Px(0.0),
                                                flex_direction: FlexDirection::Column,
                                                overflow: Overflow::clip(),
                                                ..default()
                                            })
                                            .with_children(|label| {
                                                label.spawn((
                                                    Text::new(&row.label),
                                                    TextLayout::no_wrap(),
                                                    TextFont {
                                                        font_size: FontSize::Px(11.0),
                                                        ..default()
                                                    },
                                                    TextColor(theme::TEXT_MUTED),
                                                    EditorTooltip::description(format!(
                                                        "{}\n{}",
                                                        row.label,
                                                        localizer.text(key)
                                                    )),
                                                ));
                                                if row.status != ProjectRelationStatus::Available {
                                                    line(label, localizer.text(key));
                                                }
                                            });
                                            if let Some(source) = row.source {
                                                panel::tool(
                                                    item,
                                                    assets,
                                                    localizer.text("browser-locate"),
                                                    "icons/center-focus.svg",
                                                    BrowserAction::LocateSource(
                                                        source,
                                                        catalog.content_revision(),
                                                    ),
                                                    false,
                                                    false,
                                                );
                                            }
                                        });
                                }
                            }
                        })
                        .id();
                    crate::feathers::scroll::spawn_vertical_scrollbar(body, scroller);
                });
            if matches!(
                state.inspection_tab,
                InspectionTab::Dependencies | InspectionTab::Usages
            ) && pages > 1
            {
                inspector
                    .spawn(Node {
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|footer| {
                        for (next, label, disabled) in [
                            (false, "‹", state.inspection_page == 0),
                            (true, "›", state.inspection_page + 1 == pages),
                        ] {
                            let id =
                                button(footer, label, BrowserAction::InspectionPage(next), false);
                            footer
                                .commands()
                                .entity(id)
                                .insert(AccessibleLabel(localizer.text(if next {
                                    "browser-next-page"
                                } else {
                                    "browser-previous-page"
                                })));
                            if disabled {
                                footer.commands().entity(id).insert(InteractionDisabled);
                            }
                        }
                        line(footer, format!("{} / {}", state.inspection_page + 1, pages));
                    });
            }
        });
    if let Some(host) = details_host {
        panel::sync_details(commands, host, Some(selected), content, catalog, localizer);
    }
}
