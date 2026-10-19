//! Actual material Add Node palette: lifecycle, typed options, filtering and Feathers composition.
use crate::material_graph::keys::material_graph_view_key;
use crate::{
    EditorSession, ProjectEffectCatalog, ScrollMemoryKey,
    feathers::{
        context_menu::{
            PointerContextSubmenuSurface, should_dismiss_pointer_context_menu,
            spawn_pointer_context_menu_custom_item, spawn_pointer_context_menu_sized,
        },
        node_graph::{FeathersGraphNavigationBlocker, FeathersGraphViewport},
        scroll::spawn_vertical_scroll_area,
        search_field::spawn_search_field,
    },
    localization::Localizer,
    material_graph::{
        MaterialGraphViewport, MaterialSelectionScope, edge_target,
        palette_state::{
            MaterialGraphPaletteConnection, MaterialGraphPaletteOpen, MaterialGraphPaletteState,
        },
    },
    theme,
};
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialConnectionTarget, MaterialToolCommand, MaterialToolPlanner,
};
use aestra_compiler::{
    MaterialCompiler, MaterialGraphCreateKind, MaterialGraphNodeDescriptor, MaterialGraphProjection,
};
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::ValueChange,
};

#[derive(Component)]
pub(super) struct MaterialGraphPalette;

#[derive(Component)]
pub(super) struct MaterialGraphNodeMenu;

#[derive(Component)]
pub(super) struct MaterialGraphConnectionMenu;

#[derive(Component)]
pub(super) struct MaterialGraphPaletteAnchor;

#[derive(Component)]
pub(super) struct MaterialGraphPaletteSearch;

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphPaletteCategory {
    pub(super) searchable: String,
}

#[derive(Component)]
pub(super) struct MaterialGraphPaletteEmptySearch;

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphPaletteAction {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: crate::material_document::MaterialEditingTarget,
    pub(super) scope: MaterialSelectionScope,
    pub(super) kind: MaterialGraphCreateKind,
    pub(super) source: Option<MaterialExpressionId>,
    pub(super) target: Option<MaterialConnectionTarget>,
    pub(super) label: String,
    pub(super) graph_position: Vec2,
    pub(super) graph_key: String,
    pub(super) searchable: String,
}

#[derive(Debug, Clone)]
pub(super) struct MaterialGraphPaletteOption {
    pub(super) descriptor: MaterialGraphNodeDescriptor,
    pub(super) source: Option<MaterialExpressionId>,
    pub(super) target: Option<MaterialConnectionTarget>,
}

pub(super) fn open_material_graph_palette_from_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &RelativeCursorPosition,
        &ComputedNode,
    )>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if !keys.just_pressed(KeyCode::Tab) || palette.open.is_some() {
        return;
    }
    let Some((marker, viewport, cursor, computed)) = viewports
        .iter()
        .find(|(_, _, cursor, _)| cursor.cursor_over())
    else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let menu_position = (normalized + Vec2::splat(0.5)) * computed.size();
    palette.open = Some(MaterialGraphPaletteOpen {
        program: marker.program,
        editing_target: marker.editing_target.clone(),
        scope: marker.scope,
        menu_position,
        graph_position: viewport.unproject_viewport_point(menu_position),
        graph_key: material_graph_view_key(marker.program),
        connection: None,
    });
    palette.node_menu = None;
    palette.connection_menu = None;
    palette.query.clear();
    session.ui_revision += 1;
}

pub(super) fn dismiss_material_graph_palette(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<MaterialGraphPalette>,
            With<MaterialGraphNodeMenu>,
            With<MaterialGraphConnectionMenu>,
            With<PointerContextSubmenuSurface>,
        )>,
    >,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if should_dismiss_pointer_context_menu(
        palette.is_open(),
        buttons.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    ) {
        palette.open = None;
        palette.node_menu = None;
        palette.connection_menu = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

pub(super) fn update_material_graph_palette_search(
    change: On<ValueChange<String>>,
    searches: Query<(), With<MaterialGraphPaletteSearch>>,
    mut items: Query<(&MaterialGraphPaletteAction, &mut Node)>,
    mut categories: Query<
        (&MaterialGraphPaletteCategory, &mut Node),
        Without<MaterialGraphPaletteAction>,
    >,
    mut empty_search: Query<
        &mut Node,
        (
            With<MaterialGraphPaletteEmptySearch>,
            Without<MaterialGraphPaletteAction>,
            Without<MaterialGraphPaletteCategory>,
        ),
    >,
    mut palette: ResMut<MaterialGraphPaletteState>,
) {
    if !searches.contains(change.source) {
        return;
    }
    palette.query = change.value.trim().to_lowercase();
    let mut any_matches = false;
    for (action, mut node) in &mut items {
        let matches = palette.query.is_empty() || action.searchable.contains(&palette.query);
        any_matches |= matches;
        node.display = if matches {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (category, mut node) in &mut categories {
        node.display = if palette.query.is_empty() || category.searchable.contains(&palette.query) {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut empty_search {
        node.display = if palette.query.is_empty() || any_matches {
            Display::None
        } else {
            Display::Flex
        };
    }
}

pub(super) fn focus_material_graph_palette_search(
    searches: Query<Entity, Added<MaterialGraphPaletteSearch>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Some(entity) = searches.iter().next() {
        focus.set(entity, FocusCause::Navigated);
    }
}

pub(super) fn material_graph_palette_options(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    editing_target: &crate::material_document::MaterialEditingTarget,
    program: MaterialProgramId,
    projection: &MaterialGraphProjection,
    connection: Option<MaterialGraphPaletteConnection>,
) -> Vec<MaterialGraphPaletteOption> {
    let Ok(programs) = session.graph_material_programs_for(editing_target, catalog) else {
        return Vec::new();
    };
    let Some(program_definition) = programs.iter().find(|candidate| candidate.id == program) else {
        return Vec::new();
    };
    let Ok(functions) = catalog.material_functions() else {
        return Vec::new();
    };
    let function_library = aestra_compiler::MaterialFunctionLibrary::new(functions.clone());
    let descriptors =
        MaterialCompiler.graph_node_catalog_with_functions(program_definition, &function_library);
    let Ok(document) = session.graph_authoring_document_for(editing_target, catalog) else {
        return Vec::new();
    };
    select_palette_operations(&document, program, projection, &descriptors, connection)
}

pub(super) fn select_palette_operations(
    document: &MaterialAuthoringDocument,
    program: MaterialProgramId,
    projection: &MaterialGraphProjection,
    descriptors: &[MaterialGraphNodeDescriptor],
    connection: Option<MaterialGraphPaletteConnection>,
) -> Vec<MaterialGraphPaletteOption> {
    descriptors
        .iter()
        .filter_map(|descriptor| {
            let (source, target) = match connection {
                None => (None, None),
                Some(MaterialGraphPaletteConnection::FromOutput(source)) => {
                    if !descriptor.kind.consumes_source() {
                        return None;
                    }
                    (Some(source), None)
                }
                Some(MaterialGraphPaletteConnection::FromInput(target)) => {
                    let source = descriptor
                        .kind
                        .consumes_source()
                        .then(|| projection_connection_source(projection, target))
                        .flatten();
                    (source, Some(target))
                }
            };
            let command = MaterialToolCommand::CreateMaterialGraphNode {
                program,
                kind: descriptor.kind,
                source,
                target,
            };
            MaterialToolPlanner::plan(document, command)
                .is_ok()
                .then(|| MaterialGraphPaletteOption {
                    descriptor: descriptor.clone(),
                    source,
                    target,
                })
        })
        .collect()
}

pub(super) fn projection_connection_source(
    projection: &MaterialGraphProjection,
    target: MaterialConnectionTarget,
) -> Option<MaterialExpressionId> {
    projection
        .edges
        .iter()
        .find(|edge| edge_target(&edge.target) == Some(target))
        .map(|edge| edge.source)
}

pub(super) fn spawn_material_graph_palette(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphPaletteOpen,
    options: &[MaterialGraphPaletteOption],
    query: &str,
    localizer: &Localizer,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.menu_position,
        268.0,
        MaterialGraphPaletteAnchor,
        (MaterialGraphPalette, FeathersGraphNavigationBlocker),
        |menu| {
            spawn_search_field(
                menu,
                query,
                &localizer.text("material-graph-search-nodes"),
                &localizer.text("material-graph-clear-search"),
                MaterialGraphPaletteSearch,
            );
            if options.is_empty() {
                menu.spawn((
                    Text::new(localizer.text("material-graph-no-compatible-nodes")),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        padding: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                return;
            }
            menu.spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(360.0),
                min_height: Val::Px(0.0),
                align_items: AlignItems::Stretch,
                ..default()
            })
            .with_children(|body| {
                spawn_vertical_scroll_area(
                    body,
                    ScrollMemoryKey::MaterialGraphPalette,
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        height: Val::Percent(100.0),
                        padding: UiRect::right(Val::Px(2.0)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    |list| {
                        for (category, category_options) in
                            material_graph_palette_categories(options)
                        {
                            let searchable = category_options
                                .iter()
                                .map(|option| {
                                    format!(
                                        "{} {}",
                                        option.descriptor.category, option.descriptor.label
                                    )
                                    .to_lowercase()
                                })
                                .collect::<Vec<_>>()
                                .join(" ");
                            list.spawn((
                                MaterialGraphPaletteCategory { searchable },
                                Node {
                                    width: Val::Percent(100.0),
                                    min_height: Val::Px(24.0),
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_children(|header| {
                                header.spawn((
                                    Text::new(category.to_uppercase()),
                                    TextFont {
                                        font_size: FontSize::Px(8.0),
                                        ..default()
                                    },
                                    TextColor(theme::ACCENT),
                                    Pickable::IGNORE,
                                ));
                            });
                            for option in category_options {
                                spawn_material_graph_palette_option(list, open, option);
                            }
                        }
                        list.spawn((
                            MaterialGraphPaletteEmptySearch,
                            Text::new(localizer.text("material-graph-no-matching-nodes")),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                            Node {
                                display: Display::None,
                                padding: UiRect::all(Val::Px(10.0)),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    },
                );
            });
        },
    );
}

pub(super) fn material_graph_palette_categories(
    options: &[MaterialGraphPaletteOption],
) -> Vec<(&str, Vec<&MaterialGraphPaletteOption>)> {
    let mut categories = Vec::<(&str, Vec<&MaterialGraphPaletteOption>)>::new();
    for option in options {
        let category = option.descriptor.category.as_str();
        if let Some((_, category_options)) = categories
            .iter_mut()
            .find(|(candidate, _)| *candidate == category)
        {
            category_options.push(option);
        } else {
            categories.push((category, vec![option]));
        }
    }
    categories
}

pub(super) fn spawn_material_graph_palette_option(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphPaletteOpen,
    option: &MaterialGraphPaletteOption,
) {
    let label = option.descriptor.label.as_str();
    let category = option.descriptor.category.as_str();
    let searchable = format!("{category} {label}").to_lowercase();
    spawn_pointer_context_menu_custom_item(
        parent,
        label,
        MaterialGraphPaletteAction {
            program: open.program,
            editing_target: open.editing_target.clone(),
            scope: open.scope,
            kind: option.descriptor.kind,
            source: option.source,
            target: option.target,
            label: option.descriptor.label.clone(),
            graph_position: open.graph_position,
            graph_key: open.graph_key.clone(),
            searchable,
        },
        |item| {
            item.spawn(Node {
                min_height: Val::Px(32.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|content| {
                content.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
            });
        },
    );
}

#[cfg(test)]
#[path = "palette/tests.rs"]
mod tests;
