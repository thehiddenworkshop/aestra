//! Projectional material graph workspace backed by semantic material commands.

use crate::{
    feathers::{
        context_menu::{
            pointer_position_in_node, should_dismiss_pointer_context_menu,
            spawn_pointer_context_menu_custom_item, spawn_pointer_context_menu_item,
            spawn_pointer_context_menu_sized,
        },
        icon::load_svg_icon,
        node_graph::{
            FeathersGraphNavigationBlocker, FeathersGraphNode, FeathersGraphNodePreviewToggle,
            FeathersGraphViewport, FeathersGraphWireLayer, GraphFrameAction, GraphFrameTarget,
            GraphNodePreviewToggleProps, GraphNodeProps, GraphPortProps, GraphSocketSide,
            GraphViewportMemory, GraphViewportProps, GraphWireMaterial, NODE_HEADER_HEIGHT,
            NODE_PREVIEW_SIZE, NODE_WIDTH, PORT_ROW_HEIGHT, spawn_graph_frame_button,
            spawn_graph_node, spawn_graph_node_preview, spawn_graph_node_preview_toggle,
            spawn_graph_port, spawn_graph_port_with, spawn_graph_viewport,
        },
        number_input::ScrubbableNumber,
        panel::spawn_panel_empty_state,
        scenes,
    },
    *,
};
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommandExecutor, MaterialConnectionTarget,
    MaterialExpressionInput, MaterialOutputSocket, MaterialToolCommand, MaterialToolPlan,
    MaterialToolPlanner,
};
use aestra_compiler::{
    MaterialCompiler, MaterialGraphCreateKind, MaterialGraphEdgeTarget, MaterialGraphNode,
    MaterialGraphNodeDescriptor, MaterialGraphNodeKind, MaterialGraphOutput,
    MaterialGraphOutputKind, MaterialGraphProjection,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialId, MaterialProgramId,
    material::{
        MaterialExpressionDomain, MaterialExpressionKind, MaterialInput, MaterialInstance,
        MaterialParameterValue, MaterialProgram, MaterialValue, MaterialValueType,
        MaterialVectorComponent,
    },
};
use aestra_project::{MaterialGraphNodeLayout, MaterialGraphViewportLayout, ProjectEditorLayout};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    input_focus::{FocusCause, InputFocus},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    text::EditableText,
    ui::widget::TextScroll,
    ui_render::ui_material::MaterialNode,
    ui_widgets::Activate,
};
use bevy_resvg::prelude::{SvgColor, UiSvg};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

const COLUMN_WIDTH: f32 = 282.0;
const CANVAS_PADDING: f32 = 34.0;
const NODE_GAP: f32 = 22.0;
const SNAP_RADIUS: f32 = 38.0;
const MATERIAL_GRAPH_LAYOUT_SAVE_DELAY: Duration = Duration::from_millis(300);
const MATERIAL_GRAPH_OUTPUT_NODE_KEY: &str = "output";

pub(crate) struct EditorMaterialGraphPlugin;

impl Plugin for EditorMaterialGraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialGraphGesture>()
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphLayoutPersistence>()
            .add_observer(begin_material_connection_drag)
            .add_observer(update_material_connection_drag)
            .add_observer(finish_material_connection_drag)
            .add_observer(stop_material_socket_click)
            .add_observer(select_material_graph_node)
            .add_observer(open_material_graph_palette)
            .add_observer(open_material_graph_node_menu)
            .add_observer(select_material_graph_canvas)
            .add_observer(stop_material_graph_preview_toggle_click)
            .add_observer(focus_material_graph_number_input)
            .add_observer(handle_material_graph_default_number_change)
            .add_observer(handle_material_graph_default_toggle_change)
            .add_observer(update_material_graph_palette_search)
            .add_observer(queue_material_graph_menu_action_activation)
            .add_systems(
                Update,
                (
                    open_material_graph_palette_from_keyboard,
                    dismiss_material_graph_palette,
                    focus_material_graph_palette_search,
                    handle_material_graph_palette_actions,
                    handle_material_graph_context_actions,
                    material_graph_keyboard_input,
                    handle_material_graph_actions,
                    handle_material_graph_toolbar_actions,
                    handle_material_graph_preview_actions,
                    sync_material_graph_default_number_inputs,
                    rasterize_material_graph_previews,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    attach_material_graph_wire_materials,
                    ApplyDeferred,
                    update_material_graph_wires,
                )
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Startup, load_material_graph_layout)
            .add_systems(
                Last,
                (
                    persist_material_graph_layout,
                    flush_material_graph_layout_on_exit,
                )
                    .chain(),
            );
    }
}

fn queue_material_graph_menu_action_activation(
    activate: On<Activate>,
    actions: Query<
        (),
        (
            With<FeathersActionButton>,
            Or<(
                With<MaterialGraphPaletteAction>,
                With<MaterialGraphContextAction>,
                With<MaterialGraphPreviewToggle>,
                With<MaterialGraphToolbarAction>,
            )>,
        ),
    >,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialGraphAction {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphCanvas;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MaterialGraphViewport {
    program: MaterialProgramId,
}

#[derive(Debug, Clone)]
struct MaterialGraphPaletteOpen {
    program: MaterialProgramId,
    menu_position: Vec2,
    graph_position: Vec2,
    graph_key: String,
    connection: Option<MaterialGraphPaletteConnection>,
}

#[derive(Debug, Clone, Copy)]
enum MaterialGraphPaletteConnection {
    FromOutput(MaterialExpressionId),
    FromInput(MaterialConnectionTarget),
}

#[derive(Debug, Clone)]
struct MaterialGraphNodeMenuOpen {
    program: MaterialProgramId,
    menu_position: Vec2,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphPaletteState {
    open: Option<MaterialGraphPaletteOpen>,
    node_menu: Option<MaterialGraphNodeMenuOpen>,
    query: String,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphSelectionState {
    program: Option<MaterialProgramId>,
    expressions: BTreeSet<MaterialExpressionId>,
    connection: Option<MaterialGraphConnection>,
}

impl MaterialGraphSelectionState {
    fn select_expression(
        &mut self,
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        control: bool,
        shift: bool,
    ) -> Option<MaterialExpressionId> {
        if self.program != Some(program) {
            self.program = Some(program);
            self.expressions.clear();
            self.connection = None;
        }
        if control {
            if !self.expressions.insert(expression) {
                self.expressions.remove(&expression);
            }
        } else if shift {
            self.expressions.insert(expression);
        } else {
            self.expressions.clear();
            self.expressions.insert(expression);
        }
        self.connection = None;
        self.expressions
            .contains(&expression)
            .then_some(expression)
            .or_else(|| self.expressions.iter().next_back().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MaterialGraphPreviewTarget {
    Expression(MaterialExpressionId),
    Output,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphPreviewState {
    visible: BTreeSet<(MaterialProgramId, MaterialGraphPreviewTarget)>,
    cache: BTreeMap<(MaterialProgramId, MaterialGraphPreviewTarget), MaterialGraphPreviewCache>,
}

impl MaterialGraphPreviewState {
    fn is_visible(&self, program: MaterialProgramId, target: MaterialGraphPreviewTarget) -> bool {
        self.visible.contains(&(program, target))
    }

    fn toggle(&mut self, program: MaterialProgramId, target: MaterialGraphPreviewTarget) -> bool {
        let key = (program, target);
        if self.visible.remove(&key) {
            false
        } else {
            self.visible.insert(key);
            true
        }
    }

    fn set_visible(
        &mut self,
        program: MaterialProgramId,
        targets: impl IntoIterator<Item = MaterialGraphPreviewTarget>,
        visible: bool,
    ) {
        for target in targets {
            let key = (program, target);
            if visible {
                self.visible.insert(key);
            } else {
                self.visible.remove(&key);
            }
        }
    }

    fn visible_expressions(&self, program: MaterialProgramId) -> BTreeSet<MaterialExpressionId> {
        self.visible
            .iter()
            .filter_map(|(candidate, target)| {
                (*candidate == program)
                    .then_some(*target)
                    .and_then(|target| match target {
                        MaterialGraphPreviewTarget::Expression(expression) => Some(expression),
                        MaterialGraphPreviewTarget::Output => None,
                    })
            })
            .collect()
    }

    fn output_visible(&self, program: MaterialProgramId) -> bool {
        self.is_visible(program, MaterialGraphPreviewTarget::Output)
    }

    fn restore_layout(
        &mut self,
        program: MaterialProgramId,
        expressions: &BTreeSet<MaterialExpressionId>,
        output_visible: bool,
    ) {
        self.visible.retain(|(candidate, _)| *candidate != program);
        self.visible.extend(
            expressions
                .iter()
                .copied()
                .map(|expression| (program, MaterialGraphPreviewTarget::Expression(expression))),
        );
        if output_visible {
            self.visible
                .insert((program, MaterialGraphPreviewTarget::Output));
        }
    }
}

#[derive(Resource, Debug, Default)]
struct MaterialGraphLayoutPersistence {
    root: Option<PathBuf>,
    document: ProjectEditorLayout,
    persisted: ProjectEditorLayout,
    changed_at: Option<Instant>,
    last_error: Option<String>,
}

fn load_material_graph_layout(
    catalog: Res<ProjectEffectCatalog>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut previews: ResMut<MaterialGraphPreviewState>,
) {
    let root = catalog.root().to_owned();
    let document = match ProjectEditorLayout::load(&root) {
        Ok(document) => document,
        Err(error) => {
            warn!(
                "failed to load project material graph layout from {}: {error}",
                root.display()
            );
            ProjectEditorLayout::default()
        }
    };
    restore_material_graph_layouts(&document, &mut graph_memory, &mut previews);
    persistence.root = Some(root);
    persistence.persisted = document.clone();
    persistence.document = document;
    persistence.changed_at = None;
    persistence.last_error = None;
}

fn persist_material_graph_layout(
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    graph_memory: Res<GraphViewportMemory>,
    previews: Res<MaterialGraphPreviewState>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
) {
    let Ok(programs) = catalog.material_programs_for_effect(&session.effect) else {
        return;
    };
    update_material_graph_layout_document(
        &mut persistence.document,
        &programs,
        &graph_memory,
        &previews,
    );
    if persistence.document == persistence.persisted {
        persistence.changed_at = None;
        return;
    }
    let now = Instant::now();
    let changed_at = persistence.changed_at.get_or_insert(now);
    if now.duration_since(*changed_at) < MATERIAL_GRAPH_LAYOUT_SAVE_DELAY {
        return;
    }
    save_material_graph_layout(&mut persistence);
}

fn flush_material_graph_layout_on_exit(
    mut exits: MessageReader<AppExit>,
    mut persistence: ResMut<MaterialGraphLayoutPersistence>,
) {
    if exits.read().next().is_some() && persistence.document != persistence.persisted {
        save_material_graph_layout(&mut persistence);
    }
}

fn save_material_graph_layout(persistence: &mut MaterialGraphLayoutPersistence) {
    let Some(root) = persistence.root.clone() else {
        return;
    };
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

fn restore_material_graph_layouts(
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
        }
        if let Some(output) = layout.output {
            graph_memory.set_node(
                graph_key,
                MATERIAL_GRAPH_OUTPUT_NODE_KEY,
                Vec2::from_array(output.position),
                output.collapsed,
            );
        }
        previews.restore_layout(
            *program,
            &layout.visible_previews,
            layout.output_preview_visible,
        );
    }
}

fn update_material_graph_layout_document(
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

#[derive(Debug, Clone)]
struct MaterialGraphPreviewCache {
    instance: MaterialId,
    document_revision: u64,
    image: Handle<Image>,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphPreviewToggle {
    program: MaterialProgramId,
    target: MaterialGraphPreviewTarget,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialGraphToolbarAction {
    AddNode(MaterialProgramId),
    ToggleAllPreviews(MaterialProgramId),
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphPreviewRaster {
    program: MaterialProgramId,
    instance: MaterialId,
    target: MaterialGraphPreviewTarget,
    value_type: Option<MaterialValueType>,
}

#[derive(Component, Debug, Clone)]
struct MaterialGraphDefaultNumberControl {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    value: MaterialValue,
    component: u8,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphDefaultToggleControl {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialGraphConnection {
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
}

#[derive(Component)]
struct MaterialGraphPalette;

#[derive(Component)]
struct MaterialGraphNodeMenu;

#[derive(Component)]
struct MaterialGraphPaletteAnchor;

#[derive(Component)]
struct MaterialGraphPaletteSearch;

#[derive(Component, Debug, Clone)]
struct MaterialGraphPaletteCategory {
    searchable: String,
}

#[derive(Component)]
struct MaterialGraphPaletteEmptySearch;

#[derive(Component, Debug, Clone, Copy)]
enum MaterialGraphContextAction {
    ExtractFunction(MaterialProgramId),
    Duplicate(MaterialProgramId),
    Delete(MaterialProgramId),
}

#[derive(Debug, Clone, Copy)]
enum MaterialGraphSelectionEdit {
    ExtractFunction,
    Duplicate,
    Delete,
    Disconnect,
}

#[derive(Component, Debug, Clone)]
struct MaterialGraphPaletteAction {
    program: MaterialProgramId,
    kind: MaterialGraphCreateKind,
    source: Option<MaterialExpressionId>,
    target: Option<MaterialConnectionTarget>,
    label: String,
    graph_position: Vec2,
    graph_key: String,
    searchable: String,
}

#[derive(Debug, Clone)]
struct MaterialGraphPaletteOption {
    descriptor: MaterialGraphNodeDescriptor,
    source: Option<MaterialExpressionId>,
    target: Option<MaterialConnectionTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialGraphSocketKind {
    ExpressionOutput(MaterialExpressionId),
    ConnectionInput(MaterialConnectionTarget),
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphSocket {
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphSocketAnchor {
    node: Entity,
    offset: Option<Vec2>,
}

#[derive(Debug, Clone, Copy)]
struct MaterialGraphSocketPosition {
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
    graph: Vec2,
    world: Vec2,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphWire {
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
    color: Vec4,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphGhostWire {
    program: MaterialProgramId,
}

#[derive(Resource, Debug, Default)]
enum MaterialGraphGesture {
    #[default]
    Idle,
    Connecting {
        program: MaterialProgramId,
        origin: MaterialGraphSocketKind,
        cursor: Vec2,
        snap: Option<MaterialGraphSocketKind>,
    },
}

#[derive(Debug)]
struct MaterialGraphLayout {
    nodes: BTreeMap<MaterialExpressionId, Vec2>,
    output: Vec2,
    size: Vec2,
}

fn handle_material_graph_actions(
    mut actions: Query<
        (&Interaction, &MaterialGraphAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    selection: Res<MaterialGraphSelectionState>,
) {
    for (interaction, action, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = if selection.program == Some(action.program)
                    && selection.expressions.contains(&action.expression)
                {
                    theme::SELECTION
                } else {
                    theme::PANEL
                };
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
            }
        }
    }
}

fn select_material_graph_node(
    mut click: On<Pointer<Click>>,
    actions: Query<&MaterialGraphAction>,
    mut graph_nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    value_controls: Query<
        (),
        Or<(
            With<MaterialGraphDefaultNumberControl>,
            With<MaterialGraphDefaultToggleControl>,
        )>,
    >,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    if click.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if sockets.contains(entity) || value_controls.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if graph_nodes
        .get_mut(entity)
        .is_ok_and(|mut node| node.consume_suppressed_release_click())
    {
        click.propagate(false);
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    inspector.selected = selection
        .select_expression(action.program, action.expression, control, shift)
        .map(|expression| (action.program, expression));
    session.ui_revision += 1;
    reveal_dock_panel(&mut layout, &mut session, DockPanel::Properties);
    click.propagate(false);
}

fn open_material_graph_palette(
    mut click: On<Pointer<Click>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(), With<FeathersGraphNode>>,
    palette_surfaces: Query<(), With<MaterialGraphPalette>>,
    parents: Query<&ChildOf>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    loop {
        if graph_nodes.contains(entity) || palette_surfaces.contains(entity) {
            return;
        }
        if let Ok((marker, viewport, computed, transform)) = viewports.get(entity) {
            let menu_position =
                pointer_position_in_node(click.pointer_location.position, computed, transform);
            palette.open = Some(MaterialGraphPaletteOpen {
                program: marker.program,
                menu_position,
                graph_position: viewport.unproject_viewport_point(menu_position),
                graph_key: material_graph_view_key(marker.program),
                connection: None,
            });
            palette.node_menu = None;
            palette.query.clear();
            session.ui_revision += 1;
            click.propagate(false);
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

fn open_material_graph_node_menu(
    mut click: On<Pointer<Click>>,
    actions: Query<&MaterialGraphAction>,
    viewports: Query<(&MaterialGraphViewport, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if let Ok(action) = actions.get(entity) {
            break *action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let mut ancestor = entity;
    let (viewport, computed, transform) = loop {
        if let Ok(viewport) = viewports.get(ancestor) {
            break viewport;
        }
        let Ok(parent) = parents.get(ancestor) else {
            return;
        };
        ancestor = parent.parent();
    };
    if viewport.program != action.program {
        return;
    }
    if selection.program != Some(action.program)
        || !selection.expressions.contains(&action.expression)
    {
        selection.program = Some(action.program);
        selection.expressions.clear();
        selection.expressions.insert(action.expression);
        selection.connection = None;
    }
    inspector.selected = Some((action.program, action.expression));
    palette.open = None;
    palette.query.clear();
    palette.node_menu = Some(MaterialGraphNodeMenuOpen {
        program: action.program,
        menu_position: pointer_position_in_node(
            click.pointer_location.position,
            computed,
            transform,
        ),
    });
    session.ui_revision += 1;
    click.propagate(false);
}

fn select_material_graph_canvas(
    mut click: On<Pointer<Click>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    sockets: Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &MaterialGraphSocketAnchor,
    )>,
    wires: Query<&MaterialGraphWire>,
    parents: Query<&ChildOf>,
    controls: Query<
        (),
        Or<(
            With<FeathersGraphNode>,
            With<MaterialGraphPalette>,
            With<MaterialGraphNodeMenu>,
        )>,
    >,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.event_target();
    let (marker, viewport, computed, transform) = loop {
        if controls.contains(entity) {
            return;
        }
        if let Ok(viewport) = viewports.get(entity) {
            break viewport;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let cursor = pointer_position_in_node(click.pointer_location.position, computed, transform);
    let socket_positions = collect_socket_positions(&sockets, &graph_nodes);
    let selected_wire = wires
        .iter()
        .filter(|wire| wire.program == marker.program)
        .filter_map(|wire| {
            let start = socket_graph_position(
                &socket_positions,
                wire.program,
                MaterialGraphSocketKind::ExpressionOutput(wire.source),
            )?;
            let end = socket_graph_position(
                &socket_positions,
                wire.program,
                MaterialGraphSocketKind::ConnectionInput(wire.target),
            )?;
            let start = viewport.project_graph_point(start);
            let end = viewport.project_graph_point(end);
            let distance = distance_to_graph_wire(cursor, start, end);
            (distance <= 9.0).then_some((
                distance,
                MaterialGraphConnection {
                    program: wire.program,
                    source: wire.source,
                    target: wire.target,
                },
            ))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, connection)| connection);
    let changed = selection.program != Some(marker.program)
        || !selection.expressions.is_empty()
        || selection.connection != selected_wire
        || inspector.selected.is_some();
    selection.program = Some(marker.program);
    selection.expressions.clear();
    selection.connection = selected_wire;
    inspector.selected = None;
    if changed {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

fn stop_material_graph_preview_toggle_click(
    mut click: On<Pointer<Click>>,
    toggles: Query<(), With<MaterialGraphPreviewToggle>>,
) {
    if click.button == PointerButton::Primary && toggles.contains(click.event_target()) {
        click.propagate(false);
    }
}

fn handle_material_graph_preview_actions(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &MaterialGraphPreviewToggle,
            Option<&PendingFeathersActivation>,
        ),
        (
            Changed<Interaction>,
            With<FeathersActionButton>,
            With<FeathersGraphNodePreviewToggle>,
        ),
    >,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, toggle, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        previews.toggle(toggle.program, toggle.target);
        session.ui_revision += 1;
    }
}

fn handle_material_graph_toolbar_actions(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &MaterialGraphToolbarAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
    )>,
    graph_nodes: Query<&MaterialGraphAction>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        match *action {
            MaterialGraphToolbarAction::AddNode(program) => {
                let Some((_, viewport, computed)) = viewports
                    .iter()
                    .find(|(marker, _, _)| marker.program == program)
                else {
                    continue;
                };
                let menu_position = computed.size() * 0.5;
                palette.open = Some(MaterialGraphPaletteOpen {
                    program,
                    menu_position,
                    graph_position: viewport.unproject_viewport_point(menu_position),
                    graph_key: material_graph_view_key(program),
                    connection: None,
                });
                palette.node_menu = None;
                palette.query.clear();
            }
            MaterialGraphToolbarAction::ToggleAllPreviews(program) => {
                let targets = graph_nodes
                    .iter()
                    .filter(|node| node.program == program)
                    .map(|node| MaterialGraphPreviewTarget::Expression(node.expression))
                    .chain(std::iter::once(MaterialGraphPreviewTarget::Output))
                    .collect::<Vec<_>>();
                let show = targets
                    .iter()
                    .any(|target| !previews.is_visible(program, *target));
                previews.set_visible(program, targets, show);
            }
        }
        session.ui_revision += 1;
    }
}

fn open_material_graph_palette_from_keyboard(
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
        menu_position,
        graph_position: viewport.unproject_viewport_point(menu_position),
        graph_key: material_graph_view_key(marker.program),
        connection: None,
    });
    palette.node_menu = None;
    palette.query.clear();
    session.ui_revision += 1;
}

fn dismiss_material_graph_palette(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<
        &RelativeCursorPosition,
        Or<(With<MaterialGraphPalette>, With<MaterialGraphNodeMenu>)>,
    >,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if should_dismiss_pointer_context_menu(
        palette.open.is_some() || palette.node_menu.is_some(),
        buttons.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    ) {
        palette.open = None;
        palette.node_menu = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

fn update_material_graph_palette_search(
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

fn focus_material_graph_palette_search(
    searches: Query<Entity, Added<MaterialGraphPaletteSearch>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Some(entity) = searches.iter().next() {
        focus.set(entity, FocusCause::Navigated);
    }
}

fn begin_material_connection_drag(
    mut event: On<Pointer<DragStart>>,
    sockets: Query<(&MaterialGraphSocket, &ComputedNode)>,
    viewports: Query<(Entity, &MaterialGraphViewport)>,
    wire_layers: Query<(Entity, &FeathersGraphWireLayer)>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if event.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let Ok((socket, computed)) = sockets.get(event.event_target()) else {
        return;
    };
    let cursor = event.pointer_location.position / computed.inverse_scale_factor;
    *gesture = MaterialGraphGesture::Connecting {
        program: socket.program,
        origin: socket.kind,
        cursor,
        snap: None,
    };
    if let Some((viewport, _)) = viewports
        .iter()
        .find(|(_, viewport)| viewport.program == socket.program)
        && let Some((wire_layer, _)) = wire_layers
            .iter()
            .find(|(_, wire_layer)| wire_layer.viewport == viewport)
    {
        let material = materials.add(GraphWireMaterial::default());
        commands.spawn((
            MaterialGraphGhostWire {
                program: socket.program,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            MaterialNode(material),
            Pickable::IGNORE,
            ChildOf(wire_layer),
        ));
    }
    event.propagate(false);
}

fn update_material_connection_drag(
    mut event: On<Pointer<Drag>>,
    sockets: Query<&ComputedNode, With<MaterialGraphSocket>>,
    mut gesture: ResMut<MaterialGraphGesture>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Ok(computed) = sockets.get(event.event_target()) else {
        return;
    };
    if let MaterialGraphGesture::Connecting { cursor, .. } = &mut *gesture {
        *cursor = event.pointer_location.position / computed.inverse_scale_factor;
        event.propagate(false);
    }
}

fn finish_material_connection_drag(
    mut event: On<Pointer<DragEnd>>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    ghosts: Query<Entity, With<MaterialGraphGhostWire>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary || sockets.get(event.event_target()).is_err() {
        return;
    }
    let previous = std::mem::replace(&mut *gesture, MaterialGraphGesture::Idle);
    let MaterialGraphGesture::Connecting {
        program,
        origin,
        snap,
        ..
    } = previous
    else {
        return;
    };
    for ghost in &ghosts {
        if let Ok(mut entity) = commands.get_entity(ghost) {
            entity.despawn();
        }
    }
    if let Some((source, target)) = snap.and_then(|snap| connection_endpoints(origin, snap)) {
        apply_material_connection(
            &mut session,
            &mut catalog,
            &mut material_history,
            &mut history_ledger,
            program,
            source,
            target,
        );
    } else if let Some((_, viewport, computed, transform)) = viewports
        .iter()
        .find(|(marker, _, _, _)| marker.program == program)
    {
        let menu_position =
            pointer_position_in_node(event.pointer_location.position, computed, transform);
        palette.open = Some(MaterialGraphPaletteOpen {
            program,
            menu_position,
            graph_position: viewport.unproject_viewport_point(menu_position),
            graph_key: material_graph_view_key(program),
            connection: Some(match origin {
                MaterialGraphSocketKind::ExpressionOutput(source) => {
                    MaterialGraphPaletteConnection::FromOutput(source)
                }
                MaterialGraphSocketKind::ConnectionInput(target) => {
                    MaterialGraphPaletteConnection::FromInput(target)
                }
            }),
        });
        palette.node_menu = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
    event.propagate(false);
}

fn connection_endpoints(
    first: MaterialGraphSocketKind,
    second: MaterialGraphSocketKind,
) -> Option<(MaterialExpressionId, MaterialConnectionTarget)> {
    match (first, second) {
        (
            MaterialGraphSocketKind::ExpressionOutput(source),
            MaterialGraphSocketKind::ConnectionInput(target),
        )
        | (
            MaterialGraphSocketKind::ConnectionInput(target),
            MaterialGraphSocketKind::ExpressionOutput(source),
        ) => Some((source, target)),
        _ => None,
    }
}

fn stop_material_socket_click(
    mut event: On<Pointer<Click>>,
    sockets: Query<(), With<MaterialGraphSocket>>,
) {
    if sockets.get(event.event_target()).is_ok() {
        event.propagate(false);
    }
}

fn apply_material_connection(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
) {
    if current_material_connection_source(session, catalog, program, target) == Some(source) {
        session.status = "Material connection unchanged".into();
        session.ui_revision += 1;
        return;
    }
    let result = apply_material_tool_command(
        session,
        catalog,
        material_history,
        program,
        "Connect material nodes",
        MaterialToolCommand::ConnectMaterialExpression {
            program,
            source,
            target,
        },
    );
    match result {
        Ok(_) => {
            history_ledger.record_material_edit(session);
            session.status = "Connected material nodes".into();
        }
        Err(error) => session.status = format!("Material connection failed: {error}"),
    }
    session.ui_revision += 1;
}

fn current_material_connection_source(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    program: MaterialProgramId,
    target: MaterialConnectionTarget,
) -> Option<MaterialExpressionId> {
    let programs = catalog.material_programs_for_effect(&session.effect).ok()?;
    let program = programs.iter().find(|candidate| candidate.id == program)?;
    let functions = catalog.material_function_library().ok()?;
    MaterialCompiler
        .project_graph_with_functions(program, None, &functions)
        .edges
        .into_iter()
        .find(|edge| edge_target(&edge.target) == Some(target))
        .map(|edge| edge.source)
}

fn handle_material_graph_palette_actions(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &MaterialGraphPaletteAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        let command = MaterialToolCommand::CreateMaterialGraphNode {
            program: action.program,
            kind: action.kind,
            source: action.source,
            target: action.target,
        };
        let result = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut material_history,
            action.program,
            "Add material graph node",
            command,
        );
        match result {
            Ok(plan) => {
                history_ledger.record_material_edit(&mut session);
                let position =
                    action.graph_position - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5);
                for expression in &plan.created_expressions {
                    graph_memory.place_node(
                        action.graph_key.clone(),
                        material_graph_expression_node_key(*expression),
                        position,
                    );
                }
                if let Some(expression) = plan.created_expressions.last().copied() {
                    inspector.selected = Some((action.program, expression));
                    selection.program = Some(action.program);
                    selection.expressions.clear();
                    selection.expressions.insert(expression);
                    selection.connection = None;
                }
                session.status = format!("Added {} node", action.label);
            }
            Err(error) => session.status = format!("Could not add material node: {error}"),
        }
        palette.open = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

fn handle_material_graph_context_actions(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &MaterialGraphContextAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        let (program, edit) = match *action {
            MaterialGraphContextAction::ExtractFunction(program) => {
                (program, MaterialGraphSelectionEdit::ExtractFunction)
            }
            MaterialGraphContextAction::Duplicate(program) => {
                (program, MaterialGraphSelectionEdit::Duplicate)
            }
            MaterialGraphContextAction::Delete(program) => {
                (program, MaterialGraphSelectionEdit::Delete)
            }
        };
        apply_material_graph_selection_edit(
            edit,
            program,
            &graph_nodes,
            &mut session,
            &mut catalog,
            &mut material_history,
            &mut history_ledger,
            &mut graph_memory,
            &mut inspector,
            &mut selection,
        );
        palette.node_menu = None;
        session.ui_revision += 1;
    }
}

fn material_graph_keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    viewports: Query<(&MaterialGraphViewport, &RelativeCursorPosition)>,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    palette: Res<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
) {
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if editing_text || palette.open.is_some() || palette.node_menu.is_some() {
        return;
    }
    let Some(program) = viewports
        .iter()
        .find_map(|(viewport, cursor)| cursor.cursor_over().then_some(viewport.program))
    else {
        return;
    };
    if selection.program != Some(program) {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let edit = if control
        && shift
        && keys.just_pressed(KeyCode::KeyE)
        && !selection.expressions.is_empty()
    {
        Some(MaterialGraphSelectionEdit::ExtractFunction)
    } else if control && keys.just_pressed(KeyCode::KeyD) && !selection.expressions.is_empty() {
        Some(MaterialGraphSelectionEdit::Duplicate)
    } else if keys.just_pressed(KeyCode::Delete) {
        if !selection.expressions.is_empty() {
            Some(MaterialGraphSelectionEdit::Delete)
        } else if selection.connection.is_some() {
            Some(MaterialGraphSelectionEdit::Disconnect)
        } else {
            None
        }
    } else {
        None
    };
    let Some(edit) = edit else {
        return;
    };
    apply_material_graph_selection_edit(
        edit,
        program,
        &graph_nodes,
        &mut session,
        &mut catalog,
        &mut material_history,
        &mut history_ledger,
        &mut graph_memory,
        &mut inspector,
        &mut selection,
    );
    session.ui_revision += 1;
}

fn focus_material_graph_number_input(
    mut press: On<Pointer<Press>>,
    controls: Query<(), With<MaterialGraphDefaultNumberControl>>,
    editable_text: Query<(), With<EditableText>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let target = press.event_target();
    let mut candidate = target;
    for _ in 0..6 {
        if controls.contains(candidate) {
            let input = editable_text
                .contains(target)
                .then_some(target)
                .or_else(|| {
                    children
                        .iter_descendants(candidate)
                        .find(|descendant| editable_text.contains(*descendant))
                });
            if let Some(input) = input {
                focus.set(input, FocusCause::Pressed);
                press.propagate(false);
            }
            return;
        }
        let Ok(parent) = parents.get(candidate) else {
            return;
        };
        candidate = parent.parent();
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_material_graph_selection_edit(
    edit: MaterialGraphSelectionEdit,
    program: MaterialProgramId,
    graph_nodes: &Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    graph_memory: &mut GraphViewportMemory,
    inspector: &mut MaterialStackInspectorState,
    selection: &mut MaterialGraphSelectionState,
) {
    if selection.program != Some(program) {
        return;
    }
    let expressions = selection.expressions.iter().copied().collect::<Vec<_>>();
    let connection = selection.connection;
    let ordered = catalog
        .material_programs_for_effect(&session.effect)
        .ok()
        .and_then(|programs| {
            programs
                .iter()
                .find(|candidate| candidate.id == program)
                .map(|program| {
                    program
                        .expressions
                        .iter()
                        .filter(|expression| selection.expressions.contains(&expression.id))
                        .map(|expression| expression.id)
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default();
    let positions = graph_nodes
        .iter()
        .filter(|(action, _)| {
            action.program == program && selection.expressions.contains(&action.expression)
        })
        .map(|(action, node)| (action.expression, node.position()))
        .collect::<BTreeMap<_, _>>();
    let extraction_center = (!positions.is_empty())
        .then(|| positions.values().copied().sum::<Vec2>() / positions.len() as f32);
    let (label, command) = match edit {
        MaterialGraphSelectionEdit::ExtractFunction => (
            "Extract material function",
            MaterialToolCommand::ExtractMaterialFunction {
                program,
                function: MaterialFunctionId::new(),
                name: catalog.next_material_function_name("Extracted Function"),
                expressions: expressions.clone(),
            },
        ),
        MaterialGraphSelectionEdit::Duplicate => (
            "Duplicate material graph nodes",
            MaterialToolCommand::DuplicateMaterialExpressions {
                program,
                expressions: expressions.clone(),
            },
        ),
        MaterialGraphSelectionEdit::Delete => (
            "Delete material graph nodes",
            MaterialToolCommand::DeleteMaterialExpressions {
                program,
                expressions: expressions.clone(),
            },
        ),
        MaterialGraphSelectionEdit::Disconnect => {
            let Some(connection) = connection else {
                return;
            };
            (
                "Reset material graph connection",
                MaterialToolCommand::DisconnectMaterialConnection {
                    program,
                    target: connection.target,
                },
            )
        }
    };
    match apply_material_tool_command(session, catalog, material_history, program, label, command) {
        Ok(plan) => {
            history_ledger.record_material_edit(session);
            let graph_key = material_graph_view_key(program);
            match edit {
                MaterialGraphSelectionEdit::ExtractFunction => {
                    for expression in &expressions {
                        graph_memory.remove_node(
                            &graph_key,
                            &material_graph_expression_node_key(*expression),
                        );
                    }
                    selection.expressions.clear();
                    let center = extraction_center.unwrap_or(Vec2::ZERO);
                    let call_count = plan.created_expressions.len();
                    for (index, call) in plan.created_expressions.iter().copied().enumerate() {
                        let vertical_offset =
                            (index as f32 - (call_count.saturating_sub(1) as f32 * 0.5)) * 72.0;
                        graph_memory.place_node(
                            graph_key.clone(),
                            material_graph_expression_node_key(call),
                            center + Vec2::new(0.0, vertical_offset),
                        );
                        selection.expressions.insert(call);
                    }
                    selection.connection = None;
                    inspector.selected = plan
                        .created_expressions
                        .last()
                        .copied()
                        .map(|expression| (program, expression));
                    let function = plan
                        .created_function()
                        .expect("a successful extraction plan creates a function");
                    session.status = format!(
                        "Extracted {} node(s) as {}",
                        expressions.len(),
                        function.name
                    );
                }
                MaterialGraphSelectionEdit::Duplicate => {
                    selection.expressions.clear();
                    for (source, duplicate) in ordered.iter().zip(&plan.created_expressions) {
                        let position = positions
                            .get(source)
                            .copied()
                            .or_else(|| {
                                graph_memory.node_position(
                                    &graph_key,
                                    &material_graph_expression_node_key(*source),
                                )
                            })
                            .unwrap_or(Vec2::ZERO)
                            + Vec2::splat(24.0);
                        graph_memory.place_node(
                            graph_key.clone(),
                            material_graph_expression_node_key(*duplicate),
                            position,
                        );
                        selection.expressions.insert(*duplicate);
                    }
                    selection.connection = None;
                    inspector.selected = plan
                        .created_expressions
                        .last()
                        .copied()
                        .map(|expression| (program, expression));
                    session.status = format!(
                        "Duplicated {} material node(s)",
                        plan.created_expressions.len()
                    );
                }
                MaterialGraphSelectionEdit::Delete => {
                    for expression in &expressions {
                        graph_memory.remove_node(
                            &graph_key,
                            &material_graph_expression_node_key(*expression),
                        );
                    }
                    selection.expressions.clear();
                    selection.connection = None;
                    inspector.selected = None;
                    session.status = format!("Deleted {} material node(s)", expressions.len());
                }
                MaterialGraphSelectionEdit::Disconnect => {
                    selection.connection = None;
                    inspector.selected = None;
                    session.status = "Reset material connection to its typed default".into();
                }
            }
        }
        Err(error) => {
            session.status = match edit {
                MaterialGraphSelectionEdit::ExtractFunction => {
                    format!("Could not extract material function: {error}")
                }
                MaterialGraphSelectionEdit::Duplicate => {
                    format!("Could not duplicate material nodes: {error}")
                }
                MaterialGraphSelectionEdit::Delete => {
                    format!("Could not delete material nodes: {error}")
                }
                MaterialGraphSelectionEdit::Disconnect => {
                    format!("Could not reset material connection: {error}")
                }
            };
        }
    }
}

fn apply_material_tool_command(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    program: MaterialProgramId,
    label: &str,
    command: MaterialToolCommand,
) -> Result<MaterialToolPlan, String> {
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let current = programs
        .iter()
        .find(|candidate| candidate.id == program)
        .cloned()
        .ok_or_else(|| format!("Material program {program} is unavailable"))?;
    let functions = catalog.material_functions()?;
    let document = MaterialAuthoringDocument::new(session.effect.clone(), programs)
        .with_material_functions(functions);
    let plan = MaterialToolPlanner::plan(&document, command).map_err(|error| error.to_string())?;
    let mut preview = document;
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction)
        .map_err(|error| error.to_string())?;
    let replacement = preview
        .programs
        .into_iter()
        .find(|candidate| candidate.id == program)
        .ok_or_else(|| format!("material tool plan removed program {program}"))?;
    if let Some(function) = plan.created_function().cloned() {
        material_history.execute_extraction(
            session,
            catalog,
            label,
            current,
            replacement,
            function,
        )?;
    } else {
        material_history.execute_replacement(session, catalog, label, current, replacement)?;
    }
    Ok(plan)
}

fn attach_material_graph_wire_materials(
    wires: Query<(Entity, &MaterialGraphWire), Without<MaterialNode<GraphWireMaterial>>>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut commands: Commands,
) {
    for (entity, wire) in &wires {
        let material = materials.add(GraphWireMaterial {
            color: wire.color,
            ..default()
        });
        commands.entity(entity).insert(MaterialNode(material));
    }
}

fn update_material_graph_wires(
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    wires: Query<(&MaterialGraphWire, &MaterialNode<GraphWireMaterial>)>,
    ghosts: Query<(&MaterialGraphGhostWire, &MaterialNode<GraphWireMaterial>)>,
    mut sockets: Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &mut MaterialGraphSocketAnchor,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    selection: Res<MaterialGraphSelectionState>,
    mut gesture: ResMut<MaterialGraphGesture>,
) {
    // Socket offsets are graph-local and invariant under pan and zoom. Cache them after layout,
    // then project from graph state directly so a render frame can never mix UI transforms from
    // two adjacent zoom layouts.
    let mut socket_positions = Vec::with_capacity(sockets.iter().len());
    for (socket, transform, mut anchor) in &mut sockets {
        let Ok((node, computed, node_transform)) = graph_nodes.get(anchor.node) else {
            continue;
        };
        let (_, _, world) = transform.to_scale_angle_translation();
        let offset = *anchor
            .offset
            .get_or_insert_with(|| viewport_local_position(computed, node_transform, world));
        socket_positions.push(MaterialGraphSocketPosition {
            program: socket.program,
            kind: socket.kind,
            graph: node.position() + offset,
            world,
        });
    }

    let detached_target = match &*gesture {
        MaterialGraphGesture::Connecting {
            origin: MaterialGraphSocketKind::ConnectionInput(target),
            ..
        } => Some(*target),
        _ => None,
    };
    for (wire, material) in &wires {
        let Some(start_graph) = socket_graph_position(
            &socket_positions,
            wire.program,
            MaterialGraphSocketKind::ExpressionOutput(wire.source),
        ) else {
            continue;
        };
        let Some(end_graph) = socket_graph_position(
            &socket_positions,
            wire.program,
            MaterialGraphSocketKind::ConnectionInput(wire.target),
        ) else {
            continue;
        };
        let Some((_, viewport, _, _)) = viewports
            .iter()
            .find(|(marker, _, _, _)| marker.program == wire.program)
        else {
            continue;
        };
        let start = viewport.project_graph_point(start_graph);
        let end = viewport.project_graph_point(end_graph);
        let selected = selection.connection
            == Some(MaterialGraphConnection {
                program: wire.program,
                source: wire.source,
                target: wire.target,
            });
        let detached = detached_target == Some(wire.target);
        update_wire_material(
            &mut materials,
            &material.0,
            start,
            end,
            if detached {
                Vec4::ZERO
            } else if selected {
                Vec4::new(0.70, 0.50, 1.0, 1.0)
            } else {
                wire.color
            },
            if detached {
                0.0
            } else if selected {
                4.0
            } else {
                2.0
            },
        );
    }

    let MaterialGraphGesture::Connecting {
        program,
        origin,
        cursor,
        snap,
    } = &mut *gesture
    else {
        return;
    };
    let Some(origin_graph) = socket_graph_position(&socket_positions, *program, *origin) else {
        return;
    };

    let document = catalog
        .material_programs_for_effect(&session.effect)
        .ok()
        .and_then(|programs| {
            catalog.material_functions().ok().map(|functions| {
                MaterialAuthoringDocument::new(session.effect.clone(), programs)
                    .with_material_functions(functions)
            })
        });
    let mut nearest: Option<(f32, MaterialGraphSocketKind, Vec2)> = None;
    for socket in &socket_positions {
        if socket.program != *program {
            continue;
        }
        let Some((source, target)) = connection_endpoints(*origin, socket.kind) else {
            continue;
        };
        let distance = socket.world.distance(*cursor);
        if distance > SNAP_RADIUS
            || nearest
                .as_ref()
                .is_some_and(|(nearest_distance, _, _)| distance >= *nearest_distance)
        {
            continue;
        }
        let valid = document.as_ref().is_some_and(|document| {
            MaterialToolPlanner::plan(
                document,
                MaterialToolCommand::ConnectMaterialExpression {
                    program: *program,
                    source,
                    target,
                },
            )
            .is_ok()
        });
        if valid {
            nearest = Some((distance, socket.kind, socket.graph));
        }
    }
    *snap = nearest.map(|(_, endpoint, _)| endpoint);
    let Some((_, viewport, computed, transform)) = viewports
        .iter()
        .find(|(marker, _, _, _)| marker.program == *program)
    else {
        return;
    };
    let origin_position = viewport.project_graph_point(origin_graph);
    let cursor = viewport_local_position(computed, transform, *cursor);
    let snapped = nearest.map(|(_, _, graph)| viewport.project_graph_point(graph));
    let (start, end) = match *origin {
        MaterialGraphSocketKind::ExpressionOutput(_) => {
            (origin_position, snapped.unwrap_or(cursor))
        }
        MaterialGraphSocketKind::ConnectionInput(_) => (snapped.unwrap_or(cursor), origin_position),
    };
    for (ghost, material) in &ghosts {
        if ghost.program != *program {
            continue;
        }
        let color = if snap.is_some() {
            Vec4::new(0.45, 1.0, 0.72, 1.0)
        } else {
            Vec4::new(0.76, 0.70, 0.92, 0.72)
        };
        update_wire_material(&mut materials, &material.0, start, end, color, 2.5);
    }
}

fn viewport_local_position(
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    world_position: Vec2,
) -> Vec2 {
    transform.try_inverse().map_or(world_position, |inverse| {
        inverse.transform_point2(world_position) + computed.size() * 0.5
    })
}

fn socket_graph_position(
    sockets: &[MaterialGraphSocketPosition],
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
) -> Option<Vec2> {
    sockets
        .iter()
        .find(|socket| socket.program == program && socket.kind == kind)
        .map(|socket| socket.graph)
}

fn collect_socket_positions(
    sockets: &Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &MaterialGraphSocketAnchor,
    )>,
    graph_nodes: &Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
) -> Vec<MaterialGraphSocketPosition> {
    sockets
        .iter()
        .filter_map(|(socket, transform, anchor)| {
            let (node, computed, node_transform) = graph_nodes.get(anchor.node).ok()?;
            let (_, _, world) = transform.to_scale_angle_translation();
            let offset = anchor
                .offset
                .unwrap_or_else(|| viewport_local_position(computed, node_transform, world));
            Some(MaterialGraphSocketPosition {
                program: socket.program,
                kind: socket.kind,
                graph: node.position() + offset,
                world,
            })
        })
        .collect()
}

fn distance_to_graph_wire(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    let control_start = start + Vec2::new(control, 0.0);
    let control_end = end - Vec2::new(control, 0.0);
    let mut distance = f32::INFINITY;
    let mut previous = start;
    for index in 1..=32 {
        let t = index as f32 / 32.0;
        let inverse = 1.0 - t;
        let sample = start * inverse.powi(3)
            + control_start * (3.0 * inverse.powi(2) * t)
            + control_end * (3.0 * inverse * t.powi(2))
            + end * t.powi(3);
        distance = distance.min(distance_to_segment(point, previous, sample));
        previous = sample;
    }
    distance
}

fn distance_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn set_wire_points(material: &mut GraphWireMaterial, start: Vec2, end: Vec2) {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    material.start = start;
    material.control_start = start + Vec2::new(control, 0.0);
    material.control_end = end - Vec2::new(control, 0.0);
    material.end = end;
}

fn update_wire_material(
    materials: &mut Assets<GraphWireMaterial>,
    handle: &Handle<GraphWireMaterial>,
    start: Vec2,
    end: Vec2,
    color: Vec4,
    width: f32,
) {
    const POSITION_EPSILON_SQUARED: f32 = 0.0001;
    let unchanged = materials.get(handle).is_some_and(|material| {
        material.start.distance_squared(start) <= POSITION_EPSILON_SQUARED
            && material.end.distance_squared(end) <= POSITION_EPSILON_SQUARED
            && material.color == color
            && material.width == width
    });
    if unchanged {
        return;
    }
    if let Some(mut material) = materials.get_mut(handle) {
        set_wire_points(&mut material, start, end);
        material.color = color;
        material.width = width;
    }
}

const MATERIAL_PREVIEW_SIZE: u32 = NODE_PREVIEW_SIZE as u32;
const MATERIAL_PREVIEW_LAYOUT_HEIGHT: f32 = NODE_PREVIEW_SIZE + 8.0;

fn rasterize_material_graph_previews(
    mut commands: Commands,
    requests: Query<(Entity, &MaterialGraphPreviewRaster), Added<MaterialGraphPreviewRaster>>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut images: ResMut<Assets<Image>>,
) {
    if requests.is_empty() {
        return;
    }
    let Ok(programs) = catalog.material_programs_for_effect(&session.effect) else {
        return;
    };
    for (entity, request) in &requests {
        let key = (request.program, request.target);
        if let Some(cached) = previews.cache.get(&key)
            && cached.instance == request.instance
            && cached.document_revision == session.document_revision()
        {
            commands
                .entity(entity)
                .insert(ImageNode::new(cached.image.clone()).with_mode(NodeImageMode::Stretch));
            continue;
        }
        let Some(program) = programs
            .iter()
            .find(|program| program.id == request.program)
        else {
            continue;
        };
        let instance = session
            .effect
            .material_instances
            .iter()
            .find(|instance| instance.id == request.instance);
        let image = images.add(render_material_graph_preview(
            program,
            instance,
            request.target,
            request.value_type,
        ));
        previews.cache.insert(
            key,
            MaterialGraphPreviewCache {
                instance: request.instance,
                document_revision: session.document_revision(),
                image: image.clone(),
            },
        );
        commands
            .entity(entity)
            .insert(ImageNode::new(image).with_mode(NodeImageMode::Stretch));
    }
}

#[derive(Debug, Clone, Copy)]
enum PreviewValue {
    Numeric([f32; 4], usize),
    Bool(bool),
    Texture,
}

impl PreviewValue {
    fn scalar(value: f32) -> Self {
        Self::Numeric([value, value, value, value], 1)
    }

    fn numeric(self) -> Option<([f32; 4], usize)> {
        match self {
            Self::Numeric(value, lanes) => Some((value, lanes)),
            Self::Bool(value) => Some(([if value { 1.0 } else { 0.0 }; 4], 1)),
            Self::Texture => None,
        }
    }

    fn boolean(self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(value),
            Self::Numeric(value, _) => Some(value[0] >= 0.5),
            Self::Texture => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MaterialPreviewContext {
    uv: Vec2,
    normal: Vec3,
}

struct MaterialPreviewEvaluator<'a> {
    program: &'a MaterialProgram,
    instance: Option<&'a MaterialInstance>,
}

impl MaterialPreviewEvaluator<'_> {
    fn evaluate(
        &self,
        expression: MaterialExpressionId,
        context: MaterialPreviewContext,
    ) -> Option<PreviewValue> {
        self.evaluate_inner(
            expression,
            context,
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
        )
    }

    fn evaluate_inner(
        &self,
        expression: MaterialExpressionId,
        context: MaterialPreviewContext,
        memo: &mut BTreeMap<MaterialExpressionId, PreviewValue>,
        visiting: &mut BTreeSet<MaterialExpressionId>,
    ) -> Option<PreviewValue> {
        if let Some(value) = memo.get(&expression) {
            return Some(*value);
        }
        if !visiting.insert(expression) {
            return None;
        }
        let expression_value = self
            .program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)?;
        if self.program.disabled_expressions.contains(&expression)
            && let Some(source) = expression_value.kind.bypass_input()
        {
            visiting.remove(&expression);
            return self.evaluate_inner(source, context, memo, visiting);
        }
        let mut read = |source| self.evaluate_inner(source, context, memo, visiting);
        let value = match &expression_value.kind {
            MaterialExpressionKind::Constant(value) => preview_value(value),
            MaterialExpressionKind::Input(input) => Some(preview_input(*input, context)),
            MaterialExpressionKind::Parameter(parameter) => self.parameter(*parameter),
            MaterialExpressionKind::FunctionInput(_)
            | MaterialExpressionKind::FunctionCall { .. } => None,
            MaterialExpressionKind::Add(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left + right)
            }
            MaterialExpressionKind::Subtract(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left - right)
            }
            MaterialExpressionKind::Multiply(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left * right)
            }
            MaterialExpressionKind::Divide(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| {
                    if right.abs() <= f32::EPSILON {
                        0.0
                    } else {
                        left / right
                    }
                })
            }
            MaterialExpressionKind::Lerp { start, end, factor } => {
                let factor = read(*factor)?.numeric()?.0;
                preview_ternary_numeric(read(*start)?, read(*end)?, |start, end, index| {
                    start + (end - start) * factor[index]
                })
            }
            MaterialExpressionKind::Clamp { value, min, max } => {
                let min = read(*min)?.numeric()?.0;
                let max = read(*max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    value.clamp(min[index].min(max[index]), min[index].max(max[index]))
                })
            }
            MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => {
                let input_min = read(*input_min)?.numeric()?.0;
                let input_max = read(*input_max)?.numeric()?.0;
                let output_min = read(*output_min)?.numeric()?.0;
                let output_max = read(*output_max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    let denominator = input_max[index] - input_min[index];
                    if denominator.abs() <= f32::EPSILON {
                        output_min[index]
                    } else {
                        let t = (value - input_min[index]) / denominator;
                        output_min[index] + (output_max[index] - output_min[index]) * t
                    }
                })
            }
            MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => {
                let edge_min = read(*edge_min)?.numeric()?.0;
                let edge_max = read(*edge_max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    let denominator = edge_max[index] - edge_min[index];
                    if denominator.abs() <= f32::EPSILON {
                        if value >= edge_min[index] { 1.0 } else { 0.0 }
                    } else {
                        let t = ((value - edge_min[index]) / denominator).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    }
                })
            }
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            } => {
                let normal = preview_vec3(read(*normal)?)?.normalize_or_zero();
                let view = preview_vec3(read(*view)?)?.normalize_or_zero();
                let power = read(*power)?.numeric()?.0[0].max(0.0);
                Some(PreviewValue::scalar(
                    (1.0 - normal.dot(view).clamp(0.0, 1.0)).powf(power),
                ))
            }
            MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let radius = read(*radius)?.numeric()?.0[0].max(0.0);
                let softness = read(*softness)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let distance = uv.distance(center);
                let mask = if softness <= f32::EPSILON {
                    if distance <= radius { 1.0 } else { 0.0 }
                } else {
                    let t = ((radius - distance) / softness).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                Some(PreviewValue::scalar(if invert { 1.0 - mask } else { mask }))
            }
            MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                let source = read(*source)?.numeric()?.0[0];
                let threshold = read(*threshold)?.numeric()?.0[0];
                let edge = read(*edge_width)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let mask = if edge <= f32::EPSILON {
                    if source >= threshold { 1.0 } else { 0.0 }
                } else {
                    ((source - threshold) / edge + 0.5).clamp(0.0, 1.0)
                };
                Some(PreviewValue::scalar(if invert { 1.0 - mask } else { mask }))
            }
            MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                let source = read(*source)?.numeric()?.0[0];
                let threshold = read(*threshold)?.numeric()?.0[0];
                let edge = read(*edge_width)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let distance = if invert {
                    threshold - source
                } else {
                    source - threshold
                };
                Some(PreviewValue::scalar(if edge <= f32::EPSILON {
                    0.0
                } else {
                    (1.0 - distance.abs() / edge).clamp(0.0, 1.0)
                }))
            }
            MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => Some(PreviewValue::scalar(preview_depth_fade(
                read(*scene_depth)?,
                read(*pixel_depth)?,
                read(*fade_distance)?,
                read(*invert)?,
            )?)),
            MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => {
                let alpha = read(*alpha)?.numeric()?.0[0];
                Some(PreviewValue::scalar(
                    alpha
                        * preview_depth_fade(
                            read(*scene_depth)?,
                            read(*pixel_depth)?,
                            read(*fade_distance)?,
                            read(*invert)?,
                        )?,
                ))
            }
            MaterialExpressionKind::PanUv { uv, speed, time } => {
                let uv = preview_vec2(read(*uv)?)?;
                let speed = preview_vec2(read(*speed)?)?;
                let time = read(*time)?.numeric()?.0[0];
                Some(preview_vec2_value(uv + speed * time))
            }
            MaterialExpressionKind::RotateUv { uv, center, angle } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let angle = read(*angle)?.numeric()?.0[0];
                let (sin, cos) = angle.sin_cos();
                let offset = uv - center;
                Some(preview_vec2_value(
                    center
                        + Vec2::new(
                            offset.x * cos - offset.y * sin,
                            offset.x * sin + offset.y * cos,
                        ),
                ))
            }
            MaterialExpressionKind::ScaleUv { uv, center, scale } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let scale = preview_vec2(read(*scale)?)?;
                Some(preview_vec2_value(center + (uv - center) * scale))
            }
            MaterialExpressionKind::SampleTexture { texture, uv } => {
                let _texture = read(*texture)?;
                let uv = preview_vec2(read(*uv)?)?;
                Some(preview_checker_sample(uv))
            }
            MaterialExpressionKind::ExtractComponent { value, component } => {
                let value = read(*value)?.numeric()?.0;
                let index = match component {
                    MaterialVectorComponent::X => 0,
                    MaterialVectorComponent::Y => 1,
                    MaterialVectorComponent::Z => 2,
                    MaterialVectorComponent::W => 3,
                };
                Some(PreviewValue::scalar(value[index]))
            }
        }?;
        visiting.remove(&expression);
        memo.insert(expression, value);
        Some(value)
    }

    fn parameter(&self, parameter: aestra_core::MaterialParameterId) -> Option<PreviewValue> {
        let definition = self
            .program
            .parameters
            .iter()
            .find(|candidate| candidate.id == parameter)?;
        let authored = self
            .instance
            .and_then(|instance| instance.values.get(&parameter));
        match authored {
            Some(MaterialParameterValue::Constant(value)) => preview_value(value),
            Some(MaterialParameterValue::RandomRange { min, max, .. }) => {
                preview_binary(preview_value(min)?, preview_value(max)?, |min, max| {
                    (min + max) * 0.5
                })
            }
            Some(MaterialParameterValue::EffectParameter(_))
            | Some(MaterialParameterValue::EmitterParameter(_))
            | None => definition.default.as_ref().and_then(preview_value),
        }
    }
}

fn preview_value(value: &MaterialValue) -> Option<PreviewValue> {
    Some(match value {
        MaterialValue::Float(value) => PreviewValue::scalar(*value),
        MaterialValue::Vec2(value) => PreviewValue::Numeric([value[0], value[1], 0.0, 1.0], 2),
        MaterialValue::Vec3(value) => PreviewValue::Numeric([value[0], value[1], value[2], 1.0], 3),
        MaterialValue::Vec4(value) => PreviewValue::Numeric(*value, 4),
        MaterialValue::ColorSrgb(value) => PreviewValue::Numeric(
            [
                srgb_to_linear(value[0]),
                srgb_to_linear(value[1]),
                srgb_to_linear(value[2]),
                value[3],
            ],
            4,
        ),
        MaterialValue::Texture2D(_) => PreviewValue::Texture,
        MaterialValue::Bool(value) => PreviewValue::Bool(*value),
    })
}

fn preview_input(input: MaterialInput, context: MaterialPreviewContext) -> PreviewValue {
    match input {
        MaterialInput::Uv0 | MaterialInput::Uv1 | MaterialInput::ScreenUv => {
            preview_vec2_value(context.uv)
        }
        MaterialInput::Normal
        | MaterialInput::Tangent
        | MaterialInput::CameraDirection
        | MaterialInput::ViewDirection => PreviewValue::Numeric(
            if input == MaterialInput::ViewDirection {
                [0.0, 0.0, 1.0, 1.0]
            } else {
                [context.normal.x, context.normal.y, context.normal.z, 1.0]
            },
            3,
        ),
        MaterialInput::LocalPosition | MaterialInput::WorldPosition => PreviewValue::Numeric(
            [context.uv.x * 2.0 - 1.0, context.uv.y * 2.0 - 1.0, 0.0, 1.0],
            3,
        ),
        MaterialInput::ParticleColor => PreviewValue::Numeric(
            [
                context.uv.x,
                0.25 + context.uv.y * 0.55,
                1.0 - context.uv.x,
                1.0,
            ],
            4,
        ),
        MaterialInput::ParticleVelocity => PreviewValue::Numeric([1.0, 0.35, 0.0, 1.0], 3),
        MaterialInput::CameraPosition => PreviewValue::Numeric([0.0, 0.0, 3.0, 1.0], 3),
        MaterialInput::ParticleOpacity => PreviewValue::scalar(1.0),
        MaterialInput::ParticleNormalizedAge
        | MaterialInput::EffectNormalizedTime
        | MaterialInput::EmitterNormalizedTime => PreviewValue::scalar(context.uv.x),
        MaterialInput::SceneDepth => PreviewValue::scalar(1.0 + context.uv.x),
        MaterialInput::PixelDepth => PreviewValue::scalar(1.0),
        MaterialInput::ParticleRandom => PreviewValue::scalar(
            ((context.uv.x * 91.7).sin() * (context.uv.y * 43.1).cos() * 43758.547)
                .fract()
                .abs(),
        ),
        MaterialInput::ParticleAge | MaterialInput::EffectTime | MaterialInput::EmitterTime => {
            PreviewValue::scalar(1.0)
        }
        MaterialInput::ParticleLifetime => PreviewValue::scalar(2.0),
        MaterialInput::ParticleSpeed => PreviewValue::scalar(1.0),
        MaterialInput::ParticleId => PreviewValue::scalar((context.uv.x * 8.0).floor()),
        MaterialInput::ParticleSize => PreviewValue::scalar(1.0),
        MaterialInput::ParticleRotation => PreviewValue::scalar(0.0),
    }
}

fn preview_binary(
    left: PreviewValue,
    right: PreviewValue,
    operation: impl Fn(f32, f32) -> f32,
) -> Option<PreviewValue> {
    let (left, left_lanes) = left.numeric()?;
    let (right, right_lanes) = right.numeric()?;
    let lanes = left_lanes.max(right_lanes);
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| {
            operation(
                left[index.min(left_lanes - 1)],
                right[index.min(right_lanes - 1)],
            )
        }),
        lanes,
    ))
}

fn preview_unary_numeric(
    value: PreviewValue,
    operation: impl Fn(f32, usize) -> f32,
) -> Option<PreviewValue> {
    let (value, lanes) = value.numeric()?;
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| operation(value[index.min(lanes - 1)], index)),
        lanes,
    ))
}

fn preview_ternary_numeric(
    left: PreviewValue,
    right: PreviewValue,
    operation: impl Fn(f32, f32, usize) -> f32,
) -> Option<PreviewValue> {
    let (left, left_lanes) = left.numeric()?;
    let (right, right_lanes) = right.numeric()?;
    let lanes = left_lanes.max(right_lanes);
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| {
            operation(
                left[index.min(left_lanes - 1)],
                right[index.min(right_lanes - 1)],
                index,
            )
        }),
        lanes,
    ))
}

fn preview_vec2(value: PreviewValue) -> Option<Vec2> {
    let (value, lanes) = value.numeric()?;
    Some(Vec2::new(value[0], value[if lanes > 1 { 1 } else { 0 }]))
}

fn preview_vec3(value: PreviewValue) -> Option<Vec3> {
    let (value, lanes) = value.numeric()?;
    Some(Vec3::new(
        value[0],
        value[if lanes > 1 { 1 } else { 0 }],
        value[if lanes > 2 { 2 } else { 0 }],
    ))
}

fn preview_vec2_value(value: Vec2) -> PreviewValue {
    PreviewValue::Numeric([value.x, value.y, 0.0, 1.0], 2)
}

fn preview_checker_sample(uv: Vec2) -> PreviewValue {
    let cell = ((uv.x * 8.0).floor() as i32 + (uv.y * 8.0).floor() as i32) & 1;
    let color = if cell == 0 {
        [0.08, 0.20, 0.42, 1.0]
    } else {
        [0.88, 0.32, 0.08, 1.0]
    };
    PreviewValue::Numeric(color, 4)
}

fn preview_depth_fade(
    scene_depth: PreviewValue,
    pixel_depth: PreviewValue,
    fade_distance: PreviewValue,
    invert: PreviewValue,
) -> Option<f32> {
    let scene = scene_depth.numeric()?.0[0];
    let pixel = pixel_depth.numeric()?.0[0];
    let distance = fade_distance.numeric()?.0[0];
    let fade = if distance <= f32::EPSILON {
        if scene >= pixel { 1.0 } else { 0.0 }
    } else {
        ((scene - pixel) / distance).clamp(0.0, 1.0)
    };
    Some(if invert.boolean()? { 1.0 - fade } else { fade })
}

fn render_material_graph_preview(
    program: &MaterialProgram,
    instance: Option<&MaterialInstance>,
    target: MaterialGraphPreviewTarget,
    value_type: Option<MaterialValueType>,
) -> Image {
    let evaluator = MaterialPreviewEvaluator { program, instance };
    let uses_sphere = preview_uses_surface_normal(program, target);
    let mut rgba = Vec::with_capacity((MATERIAL_PREVIEW_SIZE * MATERIAL_PREVIEW_SIZE * 4) as usize);
    for y in 0..MATERIAL_PREVIEW_SIZE {
        for x in 0..MATERIAL_PREVIEW_SIZE {
            let uv = Vec2::new(
                (x as f32 + 0.5) / MATERIAL_PREVIEW_SIZE as f32,
                1.0 - (y as f32 + 0.5) / MATERIAL_PREVIEW_SIZE as f32,
            );
            let sphere = uv * 2.0 - Vec2::ONE;
            let radius_squared = sphere.length_squared();
            let inside = !uses_sphere || radius_squared <= 1.0;
            let normal = if radius_squared <= 1.0 {
                Vec3::new(sphere.x, sphere.y, (1.0 - radius_squared).sqrt())
            } else {
                Vec3::Z
            };
            let context = MaterialPreviewContext { uv, normal };
            let sample = if inside {
                match target {
                    MaterialGraphPreviewTarget::Expression(expression) => evaluator
                        .evaluate(expression, context)
                        .and_then(|value| preview_rgba(value, value_type)),
                    MaterialGraphPreviewTarget::Output => {
                        preview_output_rgba(&evaluator, program, context)
                    }
                }
            } else {
                None
            };
            let checker = if ((x / 10) + (y / 10)) & 1 == 0 {
                0.10
            } else {
                0.16
            };
            let color = sample.unwrap_or([checker, checker, checker, 1.0]);
            let alpha = color[3].clamp(0.0, 1.0);
            let composite = std::array::from_fn::<_, 3, _>(|channel| {
                color[channel].clamp(0.0, 1.0) * alpha + checker * (1.0 - alpha)
            });
            rgba.extend_from_slice(&[
                (composite[0] * 255.0).round() as u8,
                (composite[1] * 255.0).round() as u8,
                (composite[2] * 255.0).round() as u8,
                255,
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: MATERIAL_PREVIEW_SIZE,
            height: MATERIAL_PREVIEW_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

fn preview_rgba(value: PreviewValue, value_type: Option<MaterialValueType>) -> Option<[f32; 4]> {
    match value {
        PreviewValue::Bool(value) => {
            let value = if value { 1.0 } else { 0.0 };
            Some([value, value, value, 1.0])
        }
        PreviewValue::Texture => None,
        PreviewValue::Numeric(value, lanes) => {
            let linear = match value_type {
                Some(MaterialValueType::Float) | Some(MaterialValueType::Bool) => {
                    [value[0], value[0], value[0], 1.0]
                }
                Some(MaterialValueType::Vec2) => [value[0], value[1], 0.0, 1.0],
                Some(MaterialValueType::Vec3) | Some(MaterialValueType::Color) => {
                    [value[0], value[1], value[2], 1.0]
                }
                Some(MaterialValueType::Vec4) => value,
                Some(MaterialValueType::Texture2D(_)) | None => match lanes {
                    1 => [value[0], value[0], value[0], 1.0],
                    2 => [value[0], value[1], 0.0, 1.0],
                    3 => [value[0], value[1], value[2], 1.0],
                    _ => value,
                },
            };
            Some([
                linear_to_srgb(linear[0]),
                linear_to_srgb(linear[1]),
                linear_to_srgb(linear[2]),
                linear[3],
            ])
        }
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn preview_uses_surface_normal(
    program: &MaterialProgram,
    target: MaterialGraphPreviewTarget,
) -> bool {
    fn visits_normal(
        program: &MaterialProgram,
        expression: MaterialExpressionId,
        visited: &mut BTreeSet<MaterialExpressionId>,
    ) -> bool {
        if !visited.insert(expression) {
            return false;
        }
        let Some(expression) = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
        else {
            return false;
        };
        if matches!(
            expression.kind,
            MaterialExpressionKind::Input(MaterialInput::Normal | MaterialInput::ViewDirection)
                | MaterialExpressionKind::Fresnel { .. }
        ) {
            return true;
        }
        preview_dependencies(&expression.kind)
            .into_iter()
            .any(|dependency| visits_normal(program, dependency, visited))
    }

    match target {
        MaterialGraphPreviewTarget::Expression(expression) => {
            visits_normal(program, expression, &mut BTreeSet::new())
        }
        MaterialGraphPreviewTarget::Output => {
            visits_normal(program, program.outputs.color, &mut BTreeSet::new())
                || visits_normal(program, program.outputs.alpha, &mut BTreeSet::new())
        }
    }
}

fn preview_output_rgba(
    evaluator: &MaterialPreviewEvaluator<'_>,
    program: &MaterialProgram,
    context: MaterialPreviewContext,
) -> Option<[f32; 4]> {
    let color = evaluator.evaluate(program.outputs.color, context)?;
    let mut rgba = preview_rgba(color, Some(MaterialValueType::Color))?;
    rgba[3] = evaluator
        .evaluate(program.outputs.alpha, context)?
        .numeric()?
        .0[0]
        .clamp(0.0, 1.0);
    Some(rgba)
}

fn preview_dependencies(kind: &MaterialExpressionKind) -> Vec<MaterialExpressionId> {
    match kind {
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_) => Vec::new(),
        MaterialExpressionKind::FunctionCall { arguments, .. } => {
            arguments.values().copied().collect()
        }
        MaterialExpressionKind::Add(left, right)
        | MaterialExpressionKind::Subtract(left, right)
        | MaterialExpressionKind::Multiply(left, right)
        | MaterialExpressionKind::Divide(left, right) => vec![*left, *right],
        MaterialExpressionKind::Lerp { start, end, factor } => vec![*start, *end, *factor],
        MaterialExpressionKind::Clamp { value, min, max } => vec![*value, *min, *max],
        MaterialExpressionKind::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => vec![*value, *input_min, *input_max, *output_min, *output_max],
        MaterialExpressionKind::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => vec![*edge_min, *edge_max, *value],
        MaterialExpressionKind::Fresnel {
            normal,
            view,
            power,
        } => vec![*normal, *view, *power],
        MaterialExpressionKind::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => vec![*uv, *center, *radius, *softness, *invert],
        MaterialExpressionKind::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        }
        | MaterialExpressionKind::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => vec![*source, *threshold, *edge_width, *invert],
        MaterialExpressionKind::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![*scene_depth, *pixel_depth, *fade_distance, *invert],
        MaterialExpressionKind::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![*alpha, *scene_depth, *pixel_depth, *fade_distance, *invert],
        MaterialExpressionKind::PanUv { uv, speed, time } => vec![*uv, *speed, *time],
        MaterialExpressionKind::RotateUv { uv, center, angle } => vec![*uv, *center, *angle],
        MaterialExpressionKind::ScaleUv { uv, center, scale } => vec![*uv, *center, *scale],
        MaterialExpressionKind::SampleTexture { texture, uv } => vec![*texture, *uv],
        MaterialExpressionKind::ExtractComponent { value, .. } => vec![*value],
    }
}

pub(crate) fn spawn_material_graph_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    palette: &MaterialGraphPaletteState,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    graph_memory: &GraphViewportMemory,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            let projection = selected_projection(session, catalog);
            spawn_header(
                panel,
                projection
                    .as_ref()
                    .ok()
                    .map(|(name, graph, _, _)| (name.as_str(), graph)),
                previews,
                localizer,
                asset_server,
            );
            let Ok((_program_name, projection, instance, program_definition)) = projection else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("material-graph-empty"),
                    &localizer.text("material-graph-empty-description"),
                    theme::ACCENT,
                );
                return;
            };
            let layout = layout_graph(&projection, previews);
            let graph_key = material_graph_view_key(projection.program);
            let selection_bounds = selected_graph_node_bounds(
                &layout,
                &projection,
                selection,
                previews,
                graph_memory,
                &graph_key,
            );
            let viewport = spawn_graph_viewport(
                panel,
                GraphViewportProps {
                    key: graph_key.clone(),
                    content_size: layout.size,
                    selection_bounds,
                },
                MaterialGraphCanvas,
                |overlay| spawn_graph_wires(overlay, &projection),
                |canvas| {
                    for node in &projection.nodes {
                        let position = layout
                            .nodes
                            .get(&node.expression)
                            .copied()
                            .unwrap_or(Vec2::splat(CANVAS_PADDING));
                        spawn_expression_node(
                            canvas,
                            projection.program,
                            node,
                            &program_definition,
                            position,
                            selection,
                            previews,
                            instance,
                            localizer,
                            asset_server,
                            &graph_key,
                        );
                    }
                    spawn_output_node(
                        canvas,
                        projection.program,
                        &projection.outputs,
                        previews,
                        instance,
                        layout.output,
                        localizer,
                        asset_server,
                        &graph_key,
                    );
                },
            );
            panel
                .commands()
                .entity(viewport)
                .insert(MaterialGraphViewport {
                    program: projection.program,
                });
            if let Some(open) = palette
                .open
                .as_ref()
                .filter(|open| open.program == projection.program)
            {
                let options = material_graph_palette_options(
                    session,
                    catalog,
                    projection.program,
                    &projection,
                    open.connection,
                );
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_palette(
                        viewport,
                        open,
                        &options,
                        &palette.query,
                        localizer,
                    );
                });
            }
            if let Some(open) = palette
                .node_menu
                .as_ref()
                .filter(|open| open.program == projection.program)
            {
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_node_menu(viewport, open, localizer);
                });
            }
        });
}

fn material_graph_palette_options(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    program: MaterialProgramId,
    projection: &MaterialGraphProjection,
    connection: Option<MaterialGraphPaletteConnection>,
) -> Vec<MaterialGraphPaletteOption> {
    let Ok(programs) = catalog.material_programs_for_effect(&session.effect) else {
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
    let document = MaterialAuthoringDocument::new(session.effect.clone(), programs)
        .with_material_functions(functions);
    select_palette_operations(&document, program, projection, &descriptors, connection)
}

fn select_palette_operations(
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

fn projection_connection_source(
    projection: &MaterialGraphProjection,
    target: MaterialConnectionTarget,
) -> Option<MaterialExpressionId> {
    projection
        .edges
        .iter()
        .find(|edge| edge_target(&edge.target) == Some(target))
        .map(|edge| edge.source)
}

fn spawn_material_graph_palette(
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

fn material_graph_palette_categories(
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

fn spawn_material_graph_palette_option(
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

fn spawn_material_graph_node_menu(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphNodeMenuOpen,
    localizer: &Localizer,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.menu_position,
        190.0,
        MaterialGraphPaletteAnchor,
        (MaterialGraphNodeMenu, FeathersGraphNavigationBlocker),
        |menu| {
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-extract-function"),
                MaterialGraphContextAction::ExtractFunction(open.program),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-duplicate-nodes"),
                MaterialGraphContextAction::Duplicate(open.program),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-delete-nodes"),
                MaterialGraphContextAction::Delete(open.program),
            );
        },
    );
}

fn selected_projection(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<(String, MaterialGraphProjection, MaterialId, MaterialProgram), String> {
    let selected_renderer = match session.selection.primary {
        SemanticTarget::Renderer(id) => session
            .effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .find(|renderer| renderer.id == id),
        _ => session
            .selection
            .emitter(&session.effect)
            .and_then(|id| {
                session
                    .effect
                    .emitters
                    .iter()
                    .find(|emitter| emitter.id == id)
            })
            .and_then(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .find(|renderer| renderer.enabled)
                    .or_else(|| emitter.renderers.first())
            }),
    }
    .ok_or_else(|| "selected emitter has no renderer".to_owned())?;
    let instance = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == selected_renderer.material)
        .ok_or_else(|| "selected renderer does not use a semantic material".to_owned())?;
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let program = programs
        .iter()
        .find(|program| program.id == instance.program.id())
        .ok_or_else(|| "selected material program is unavailable".to_owned())?;
    let functions = catalog.material_function_library()?;
    let compiler = MaterialCompiler;
    let ir = compiler.compile_with_functions(program, &functions).ok();
    Ok((
        program.name.clone(),
        compiler.project_graph_with_functions(program, ir.as_ref(), &functions),
        instance.id,
        program.clone(),
    ))
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    projection: Option<(&str, &MaterialGraphProjection)>,
    previews: &MaterialGraphPreviewState,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(38.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                column_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
        ))
        .with_children(|header| {
            if let Some((name, graph)) = projection {
                let key = material_graph_view_key(graph.program);
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    "icons/plus.svg",
                    localizer.text("material-graph-add-node"),
                    MaterialGraphToolbarAction::AddNode(graph.program),
                );
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-all.svg",
                    localizer.text("material-graph-frame-all"),
                    GraphFrameAction::new(key.clone(), GraphFrameTarget::All),
                );
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-selection.svg",
                    localizer.text("material-graph-frame-selection"),
                    GraphFrameAction::new(key, GraphFrameTarget::Selection),
                );
                let all_previews_visible = graph_preview_targets(graph)
                    .all(|target| previews.is_visible(graph.program, target));
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    if all_previews_visible {
                        "icons/hide.svg"
                    } else {
                        "icons/show.svg"
                    },
                    localizer.text(if all_previews_visible {
                        "material-graph-hide-all-previews"
                    } else {
                        "material-graph-show-all-previews"
                    }),
                    MaterialGraphToolbarAction::ToggleAllPreviews(graph.program),
                );
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                header.spawn((
                    Text::new(format!(
                        "{name}  ·  {} NODES  ·  {} LINKS",
                        graph.nodes.len(),
                        graph.edges.len()
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

fn spawn_material_graph_toolbar_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: MaterialGraphToolbarAction,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(scenes::feathers_tool_button());
    let entity = button.id();
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, icon_path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
        ));
    entity
}

fn graph_preview_targets(
    graph: &MaterialGraphProjection,
) -> impl Iterator<Item = MaterialGraphPreviewTarget> + '_ {
    graph
        .nodes
        .iter()
        .map(|node| MaterialGraphPreviewTarget::Expression(node.expression))
        .chain(std::iter::once(MaterialGraphPreviewTarget::Output))
}

fn material_graph_view_key(program: MaterialProgramId) -> String {
    format!("material:{program}")
}

fn material_graph_expression_node_key(expression: MaterialExpressionId) -> String {
    format!("expression:{expression}")
}

fn selected_graph_node_bounds(
    layout: &MaterialGraphLayout,
    graph: &MaterialGraphProjection,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    graph_memory: &GraphViewportMemory,
    graph_key: &str,
) -> Option<Rect> {
    if selection.program != Some(graph.program) || selection.expressions.is_empty() {
        return None;
    }
    graph
        .nodes
        .iter()
        .filter(|node| selection.expressions.contains(&node.expression))
        .filter_map(|node| {
            let node_key = material_graph_expression_node_key(node.expression);
            let position = graph_memory
                .node_position(graph_key, &node_key)
                .or_else(|| layout.nodes.get(&node.expression).copied())?;
            Some(Rect::from_corners(
                position,
                position
                    + Vec2::new(
                        NODE_WIDTH,
                        node_height(
                            node.inputs.len(),
                            node.disabled || !node.reachable,
                            previews.is_visible(
                                graph.program,
                                MaterialGraphPreviewTarget::Expression(node.expression),
                            ),
                        ),
                    ),
            ))
        })
        .reduce(|bounds, node| {
            Rect::from_corners(bounds.min.min(node.min), bounds.max.max(node.max))
        })
}

fn spawn_graph_wires(parent: &mut ChildSpawnerCommands, graph: &MaterialGraphProjection) {
    for edge in &graph.edges {
        let Some(target) = edge_target(&edge.target) else {
            continue;
        };
        parent.spawn((
            MaterialGraphWire {
                program: graph.program,
                source: edge.source,
                target,
                color: wire_color(edge.value_type),
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
}

fn inline_material_graph_default(
    program: &MaterialProgram,
    source: MaterialExpressionId,
) -> Option<&MaterialValue> {
    if !program.inline_constants.contains(&source) {
        return None;
    }
    program
        .expressions
        .iter()
        .find(|expression| expression.id == source)
        .and_then(|expression| match &expression.kind {
            MaterialExpressionKind::Constant(value) => Some(value),
            _ => None,
        })
}

fn spawn_material_graph_default_control(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    value: &MaterialValue,
) {
    parent.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    match value {
        MaterialValue::Float(number) => {
            parent
                .spawn(Node {
                    width: Val::Px(66.0),
                    height: Val::Px(20.0),
                    ..default()
                })
                .with_children(|container| {
                    container
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            MaterialGraphDefaultNumberControl {
                                program,
                                expression,
                                value: value.clone(),
                                component: 0,
                            },
                            ScrubbableNumber::new(*number, -f32::MAX, f32::MAX, 0.01),
                            AccessibleLabel("Material input default".into()),
                        ));
                });
        }
        MaterialValue::Bool(enabled) => {
            let mut checkbox = parent.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                MaterialGraphDefaultToggleControl {
                    program,
                    expression,
                    value: *enabled,
                },
                AccessibleLabel("Material input default".into()),
            ));
            if *enabled {
                checkbox.insert(Checked);
            }
        }
        MaterialValue::Vec2(_)
        | MaterialValue::Vec3(_)
        | MaterialValue::Vec4(_)
        | MaterialValue::ColorSrgb(_)
        | MaterialValue::Texture2D(_) => {}
    }
}

fn spawn_material_graph_constant_editor(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    value: &MaterialValue,
) {
    let labels: &[&str] = match value {
        MaterialValue::Float(_) => &["Value"],
        MaterialValue::Vec2(_) => &["X", "Y"],
        MaterialValue::Vec3(_) => &["X", "Y", "Z"],
        MaterialValue::Vec4(_) => &["X", "Y", "Z", "W"],
        MaterialValue::ColorSrgb(_) => &["R", "G", "B", "A"],
        MaterialValue::Bool(enabled) => {
            parent
                .spawn(Node {
                    height: Val::Px(PORT_ROW_HEIGHT),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new("Value"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    row.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    let mut checkbox = row.spawn_empty();
                    checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                        MaterialGraphDefaultToggleControl {
                            program,
                            expression,
                            value: *enabled,
                        },
                        AccessibleLabel("Material constant value".into()),
                    ));
                    if *enabled {
                        checkbox.insert(Checked);
                    }
                });
            return;
        }
        MaterialValue::Texture2D(_) => return,
    };
    for (component, label) in labels.iter().enumerate() {
        let Some(number) = material_graph_value_component(value, component as u8) else {
            continue;
        };
        let (min, max, step) = if matches!(value, MaterialValue::ColorSrgb(_)) {
            (0.0, 1.0, 0.01)
        } else {
            (-f32::MAX, f32::MAX, 0.01)
        };
        parent
            .spawn(Node {
                height: Val::Px(PORT_ROW_HEIGHT),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(*label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn(Node {
                    width: Val::Px(88.0),
                    height: Val::Px(20.0),
                    ..default()
                })
                .with_children(|container| {
                    container
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            MaterialGraphDefaultNumberControl {
                                program,
                                expression,
                                value: value.clone(),
                                component: component as u8,
                            },
                            ScrubbableNumber::new(number, min, max, step),
                            AccessibleLabel(format!("Material constant {label}")),
                        ));
                });
            });
    }
}

fn sync_material_graph_default_number_inputs(
    mut commands: Commands,
    controls: Query<
        (Entity, &MaterialGraphDefaultNumberControl),
        Added<MaterialGraphDefaultNumberControl>,
    >,
    children: Query<&Children>,
    editable_text: Query<(), With<EditableText>>,
    mut nodes: Query<&mut Node>,
    mut text_layouts: Query<&mut TextLayout>,
) {
    for (entity, control) in &controls {
        constrain_graph_number_input(
            &mut commands,
            entity,
            &children,
            &editable_text,
            &mut nodes,
            &mut text_layouts,
        );
        if let Some(value) = material_graph_value_component(&control.value, control.component) {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(value),
            });
        }
    }
}

fn constrain_graph_number_input(
    commands: &mut Commands,
    entity: Entity,
    children: &Query<&Children>,
    editable_text: &Query<(), With<EditableText>>,
    nodes: &mut Query<&mut Node>,
    text_layouts: &mut Query<&mut TextLayout>,
) {
    if let Ok(mut node) = nodes.get_mut(entity) {
        node.width = Val::Percent(100.0);
        node.min_width = Val::Px(0.0);
        node.max_width = Val::Percent(100.0);
        node.flex_shrink = 1.0;
        node.overflow = Overflow::clip();
    }
    for descendant in children.iter_descendants(entity) {
        if !editable_text.contains(descendant) {
            continue;
        }
        if let Ok(mut node) = nodes.get_mut(descendant) {
            node.width = Val::Percent(100.0);
            node.min_width = Val::Px(0.0);
            node.max_width = Val::Percent(100.0);
            node.flex_shrink = 1.0;
            node.overflow = Overflow::clip();
        }
        if let Ok(mut layout) = text_layouts.get_mut(descendant) {
            *layout = TextLayout::justify(Justify::Center);
        }
        // Bevy 0.19 derives an editable text node's private clip from its untransformed
        // content size. That clip shears glyphs when a graph canvas is zoomed with UiTransform.
        // The containing Feathers input and graph viewport already provide the desired clipping.
        commands.entity(descendant).remove::<TextScroll>();
    }
}

fn handle_material_graph_default_number_change(
    change: On<ValueChange<f32>>,
    controls: Query<&MaterialGraphDefaultNumberControl>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let mut value = control.value.clone();
    let Some(current) = material_graph_value_component(&value, control.component) else {
        return;
    };
    if (current - change.value).abs() <= f32::EPSILON
        || !set_material_graph_value_component(&mut value, control.component, change.value)
    {
        return;
    }
    commit_material_graph_default(
        &mut session,
        &mut catalog,
        &mut material_history,
        &mut history_ledger,
        control.program,
        control.expression,
        value,
    );
}

fn handle_material_graph_default_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&MaterialGraphDefaultToggleControl>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.value == change.value {
        return;
    }
    commit_material_graph_default(
        &mut session,
        &mut catalog,
        &mut material_history,
        &mut history_ledger,
        control.program,
        control.expression,
        MaterialValue::Bool(change.value),
    );
}

fn commit_material_graph_default(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    program: MaterialProgramId,
    expression: MaterialExpressionId,
    value: MaterialValue,
) {
    match apply_material_tool_command(
        session,
        catalog,
        material_history,
        program,
        "Edit material input default",
        MaterialToolCommand::ReplaceMaterialExpression {
            program,
            expression,
            replacement: MaterialExpressionKind::Constant(value),
        },
    ) {
        Ok(_) => {
            history_ledger.record_material_edit(session);
            session.status = "Edited material input default".into();
        }
        Err(error) => session.status = format!("Could not edit material input default: {error}"),
    }
    session.ui_revision += 1;
}

fn material_graph_value_component(value: &MaterialValue, component: u8) -> Option<f32> {
    let index = usize::from(component);
    match value {
        MaterialValue::Float(value) => (index == 0).then_some(*value),
        MaterialValue::Vec2(value) => value.get(index).copied(),
        MaterialValue::Vec3(value) => value.get(index).copied(),
        MaterialValue::Vec4(value) | MaterialValue::ColorSrgb(value) => value.get(index).copied(),
        MaterialValue::Texture2D(_) | MaterialValue::Bool(_) => None,
    }
}

fn set_material_graph_value_component(
    value: &mut MaterialValue,
    component: u8,
    replacement: f32,
) -> bool {
    let index = usize::from(component);
    match value {
        MaterialValue::Float(value) if index == 0 => *value = replacement,
        MaterialValue::Vec2(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Vec3(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Vec4(value) | MaterialValue::ColorSrgb(value) => {
            let Some(value) = value.get_mut(index) else {
                return false;
            };
            *value = replacement;
        }
        MaterialValue::Float(_) | MaterialValue::Texture2D(_) | MaterialValue::Bool(_) => {
            return false;
        }
    }
    true
}

fn spawn_expression_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    node: &MaterialGraphNode,
    program_definition: &MaterialProgram,
    position: Vec2,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    instance: MaterialId,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
) {
    let selected =
        selection.program == Some(program) && selection.expressions.contains(&node.expression);
    let target = MaterialGraphPreviewTarget::Expression(node.expression);
    let preview_visible = previews.is_visible(program, target);
    let graph_node = spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: material_graph_expression_node_key(node.expression),
            title: if node.kind == MaterialGraphNodeKind::Constant {
                "Constant".to_owned()
            } else {
                node.label.clone()
            },
            position,
            selected,
            muted: !node.reachable || node.disabled || node.validation_message.is_some(),
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        (
            Button,
            MaterialGraphAction {
                program,
                expression: node.expression,
            },
        ),
        |graph_node, body| {
            if node.kind == MaterialGraphNodeKind::Constant
                && let Some(value) = program_definition
                    .expressions
                    .iter()
                    .find(|expression| expression.id == node.expression)
                    .and_then(|expression| match &expression.kind {
                        MaterialExpressionKind::Constant(value) => Some(value),
                        _ => None,
                    })
            {
                spawn_material_graph_constant_editor(body, program, node.expression, value);
            }
            if node.disabled || !node.reachable || node.validation_message.is_some() {
                let state = node.validation_message.clone().unwrap_or_else(|| {
                    match (node.disabled, node.reachable) {
                        (true, false) => format!(
                            "{} · {}",
                            localizer.text("material-graph-disabled"),
                            localizer.text("material-graph-unreachable")
                        ),
                        (true, true) => localizer.text("material-graph-disabled"),
                        (false, false) => localizer.text("material-graph-unreachable"),
                        (false, true) => String::new(),
                    }
                });
                body.spawn((
                    Text::new(state),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.58, 0.30)),
                    Node {
                        margin: UiRect::horizontal(Val::Px(9.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            for port in &node.inputs {
                let target = port.function_input.map_or_else(
                    || input_target(node.expression, &port.name),
                    |input| {
                        Some(MaterialConnectionTarget::ExpressionInput {
                            expression: node.expression,
                            input: MaterialExpressionInput::FunctionArgument(input),
                        })
                    },
                );
                let Some(target) = target else {
                    continue;
                };
                let presentation = input_port_presentation(&port.name);
                let inline_default = inline_material_graph_default(program_definition, port.source);
                spawn_graph_port_with(
                    body,
                    GraphPortProps {
                        label: Some(presentation.label.to_owned()),
                        tooltip_title: material_slot_title(
                            &format!("{} input", presentation.label),
                            port.value_type,
                        ),
                        tooltip_description: material_slot_description(
                            &presentation.description,
                            port.evaluation_domain,
                        ),
                        side: GraphSocketSide::Input,
                        color: socket_color(port.value_type),
                    },
                    (
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                    |row| {
                        if let Some(value) = inline_default {
                            spawn_material_graph_default_control(row, program, port.source, value);
                        }
                    },
                );
            }
            spawn_graph_port(
                body,
                GraphPortProps {
                    label: None,
                    tooltip_title: material_slot_title("Output", node.value_type),
                    tooltip_description: material_slot_description(
                        &format!("Value produced by the {} node.", node.label),
                        node.evaluation_domain,
                    ),
                    side: GraphSocketSide::Output,
                    color: socket_color(node.value_type),
                },
                (
                    MaterialGraphSocket {
                        program,
                        kind: MaterialGraphSocketKind::ExpressionOutput(node.expression),
                    },
                    MaterialGraphSocketAnchor {
                        node: graph_node,
                        offset: None,
                    },
                ),
            );
            if preview_visible {
                spawn_graph_node_preview(
                    body,
                    MaterialGraphPreviewRaster {
                        program,
                        instance,
                        target,
                        value_type: node.value_type,
                    },
                );
            }
        },
    );
    parent
        .commands()
        .entity(graph_node)
        .with_children(|graph_node| {
            spawn_graph_node_preview_toggle(
                graph_node,
                preview_toggle_props(preview_visible, localizer, asset_server),
                MaterialGraphPreviewToggle { program, target },
            );
        });
}

fn spawn_output_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    outputs: &[MaterialGraphOutput],
    previews: &MaterialGraphPreviewState,
    instance: MaterialId,
    position: Vec2,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
) {
    let target = MaterialGraphPreviewTarget::Output;
    let preview_visible = previews.is_visible(program, target);
    let graph_node = spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: MATERIAL_GRAPH_OUTPUT_NODE_KEY.to_owned(),
            title: localizer.text("material-graph-outputs"),
            position,
            selected: false,
            muted: false,
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        (),
        |graph_node, body| {
            for output in outputs {
                let (label, description) = match output.kind {
                    MaterialGraphOutputKind::Color => {
                        ("Color", "Final color written by the material.")
                    }
                    MaterialGraphOutputKind::Alpha => {
                        ("Alpha", "Final opacity written by the material.")
                    }
                };
                let target = MaterialConnectionTarget::ProgramOutput(match output.kind {
                    MaterialGraphOutputKind::Color => MaterialOutputSocket::Color,
                    MaterialGraphOutputKind::Alpha => MaterialOutputSocket::Alpha,
                });
                spawn_graph_port(
                    body,
                    GraphPortProps {
                        label: Some(label.into()),
                        tooltip_title: material_slot_title(
                            &format!("{label} input"),
                            output.value_type,
                        ),
                        tooltip_description: material_slot_description(
                            description,
                            output.evaluation_domain,
                        ),
                        side: GraphSocketSide::Input,
                        color: socket_color(output.value_type),
                    },
                    (
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                );
            }
            if preview_visible {
                spawn_graph_node_preview(
                    body,
                    MaterialGraphPreviewRaster {
                        program,
                        instance,
                        target,
                        value_type: Some(MaterialValueType::Color),
                    },
                );
            }
        },
    );
    parent
        .commands()
        .entity(graph_node)
        .with_children(|graph_node| {
            spawn_graph_node_preview_toggle(
                graph_node,
                preview_toggle_props(preview_visible, localizer, asset_server),
                MaterialGraphPreviewToggle { program, target },
            );
        });
}

fn preview_toggle_props(
    visible: bool,
    localizer: &Localizer,
    asset_server: &AssetServer,
) -> GraphNodePreviewToggleProps {
    GraphNodePreviewToggleProps {
        visible,
        show_icon: load_svg_icon(asset_server, "icons/show.svg"),
        hide_icon: load_svg_icon(asset_server, "icons/hide.svg"),
        show_label: localizer.text("material-graph-show-preview"),
        hide_label: localizer.text("material-graph-hide-preview"),
    }
}

fn layout_graph(
    graph: &MaterialGraphProjection,
    previews: &MaterialGraphPreviewState,
) -> MaterialGraphLayout {
    let inputs = graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.expression,
                node.inputs
                    .iter()
                    .map(|port| port.source)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut depths = BTreeMap::new();
    for node in &graph.nodes {
        expression_depth(node.expression, &inputs, &mut depths, &mut BTreeSet::new());
    }
    let mut columns = BTreeMap::<usize, Vec<&MaterialGraphNode>>::new();
    for node in &graph.nodes {
        columns
            .entry(depths.get(&node.expression).copied().unwrap_or_default())
            .or_default()
            .push(node);
    }
    let mut positions = BTreeMap::new();
    let mut maximum_y = CANVAS_PADDING;
    for (depth, nodes) in columns {
        let mut y = CANVAS_PADDING + 34.0;
        for node in nodes {
            positions.insert(
                node.expression,
                Vec2::new(CANVAS_PADDING + depth as f32 * COLUMN_WIDTH, y),
            );
            y += node_height(
                material_graph_node_row_count(node),
                node.disabled || !node.reachable,
                previews.is_visible(
                    graph.program,
                    MaterialGraphPreviewTarget::Expression(node.expression),
                ),
            ) + NODE_GAP;
        }
        maximum_y = maximum_y.max(y);
    }
    let output_depth = depths.values().copied().max().unwrap_or_default() + 1;
    let output = Vec2::new(
        CANVAS_PADDING + output_depth as f32 * COLUMN_WIDTH,
        CANVAS_PADDING + 88.0,
    );
    let width = output.x + NODE_WIDTH + CANVAS_PADDING;
    let output_height = 130.0
        + if previews.is_visible(graph.program, MaterialGraphPreviewTarget::Output) {
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        } else {
            0.0
        };
    let height = maximum_y.max(output.y + output_height) + CANVAS_PADDING;
    MaterialGraphLayout {
        nodes: positions,
        output,
        size: Vec2::new(width.max(720.0), height.max(420.0)),
    }
}

fn material_graph_node_row_count(node: &MaterialGraphNode) -> usize {
    if node.kind != MaterialGraphNodeKind::Constant {
        return node.inputs.len();
    }
    match node.value_type {
        Some(MaterialValueType::Vec2) => 2,
        Some(MaterialValueType::Vec3) => 3,
        Some(MaterialValueType::Vec4 | MaterialValueType::Color) => 4,
        Some(MaterialValueType::Float | MaterialValueType::Bool)
        | Some(MaterialValueType::Texture2D(_))
        | None => 1,
    }
}

fn expression_depth(
    expression: MaterialExpressionId,
    inputs: &BTreeMap<MaterialExpressionId, Vec<MaterialExpressionId>>,
    memo: &mut BTreeMap<MaterialExpressionId, usize>,
    visiting: &mut BTreeSet<MaterialExpressionId>,
) -> usize {
    if let Some(depth) = memo.get(&expression) {
        return *depth;
    }
    if !visiting.insert(expression) {
        return 0;
    }
    let depth = inputs
        .get(&expression)
        .into_iter()
        .flatten()
        .filter(|source| inputs.contains_key(source))
        .map(|source| expression_depth(*source, inputs, memo, visiting) + 1)
        .max()
        .unwrap_or_default();
    visiting.remove(&expression);
    memo.insert(expression, depth);
    depth
}

fn node_height(input_count: usize, has_state: bool, has_preview: bool) -> f32 {
    40.0 + input_count.max(1) as f32 * PORT_ROW_HEIGHT
        + if has_state { 18.0 } else { 0.0 }
        + if has_preview {
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        } else {
            0.0
        }
}

fn input_target(expression: MaterialExpressionId, name: &str) -> Option<MaterialConnectionTarget> {
    let input = match name {
        "left" => MaterialExpressionInput::Left,
        "right" => MaterialExpressionInput::Right,
        "start" => MaterialExpressionInput::Start,
        "end" => MaterialExpressionInput::End,
        "factor" => MaterialExpressionInput::Factor,
        "value" => MaterialExpressionInput::Value,
        "min" => MaterialExpressionInput::Minimum,
        "max" => MaterialExpressionInput::Maximum,
        "input_min" => MaterialExpressionInput::InputMinimum,
        "input_max" => MaterialExpressionInput::InputMaximum,
        "output_min" => MaterialExpressionInput::OutputMinimum,
        "output_max" => MaterialExpressionInput::OutputMaximum,
        "edge_min" => MaterialExpressionInput::EdgeMinimum,
        "edge_max" => MaterialExpressionInput::EdgeMaximum,
        "normal" => MaterialExpressionInput::Normal,
        "view" => MaterialExpressionInput::View,
        "power" => MaterialExpressionInput::Power,
        "radius" => MaterialExpressionInput::Radius,
        "softness" => MaterialExpressionInput::Softness,
        "threshold" => MaterialExpressionInput::Threshold,
        "edge_width" => MaterialExpressionInput::EdgeWidth,
        "scene_depth" => MaterialExpressionInput::SceneDepth,
        "pixel_depth" => MaterialExpressionInput::PixelDepth,
        "fade_distance" => MaterialExpressionInput::FadeDistance,
        "invert" => MaterialExpressionInput::Invert,
        "speed" => MaterialExpressionInput::Speed,
        "time" => MaterialExpressionInput::Time,
        "center" => MaterialExpressionInput::Center,
        "angle" => MaterialExpressionInput::Angle,
        "scale" => MaterialExpressionInput::Scale,
        "texture" => MaterialExpressionInput::Texture,
        "uv" => MaterialExpressionInput::Uv,
        "source" => MaterialExpressionInput::Source,
        "alpha" => MaterialExpressionInput::SourceAlpha,
        _ => return None,
    };
    Some(MaterialConnectionTarget::ExpressionInput { expression, input })
}

fn edge_target(target: &MaterialGraphEdgeTarget) -> Option<MaterialConnectionTarget> {
    match target {
        MaterialGraphEdgeTarget::Input { expression, port } => input_target(*expression, port),
        MaterialGraphEdgeTarget::FunctionInput { expression, input } => {
            Some(MaterialConnectionTarget::ExpressionInput {
                expression: *expression,
                input: MaterialExpressionInput::FunctionArgument(*input),
            })
        }
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Color) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
        ),
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Alpha) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        ),
    }
}

struct MaterialInputPortPresentation {
    label: String,
    description: String,
}

fn input_port_presentation(name: &str) -> MaterialInputPortPresentation {
    let (label, description) = match name {
        "left" => ("A", "First value used by this operation."),
        "right" => ("B", "Second value used by this operation."),
        "start" => ("Start", "Value returned when the blend factor is zero."),
        "end" => ("End", "Value returned when the blend factor is one."),
        "factor" => (
            "Factor",
            "Amount used to blend between the start and end values.",
        ),
        "value" => ("Value", "Value evaluated or transformed by this node."),
        "min" => ("Minimum", "Lowest value allowed by this node."),
        "max" => ("Maximum", "Highest value allowed by this node."),
        "input_min" => ("Source minimum", "Lower bound of the source range."),
        "input_max" => ("Source maximum", "Upper bound of the source range."),
        "output_min" => ("Target minimum", "Value mapped from the source minimum."),
        "output_max" => ("Target maximum", "Value mapped from the source maximum."),
        "edge_min" => ("Lower edge", "Value where the smooth transition begins."),
        "edge_max" => ("Upper edge", "Value where the smooth transition ends."),
        "normal" => ("Normal", "Surface normal used by the calculation."),
        "view" => (
            "View direction",
            "Direction from the surface toward the camera.",
        ),
        "power" => ("Power", "Exponent controlling the response curve."),
        "radius" => ("Radius", "Radius of the generated mask."),
        "softness" => ("Softness", "Width of the mask's feathered edge."),
        "threshold" => ("Threshold", "Cutoff used to evaluate the source value."),
        "edge_width" => (
            "Edge width",
            "Width of the transition around the threshold.",
        ),
        "scene_depth" => (
            "Scene depth",
            "Depth sampled from previously rendered geometry.",
        ),
        "pixel_depth" => ("Particle depth", "Depth of the current particle fragment."),
        "fade_distance" => (
            "Fade distance",
            "Distance over which the depth fade occurs.",
        ),
        "invert" => ("Invert", "Reverses the generated mask when enabled."),
        "speed" => ("Speed", "Rate of change applied over time."),
        "time" => ("Time", "Time value used to animate this operation."),
        "center" => ("Center", "Center point of the transformation or mask."),
        "angle" => ("Angle", "Rotation angle applied to the coordinates."),
        "scale" => ("Scale", "Scale applied to the coordinates."),
        "texture" => ("Texture", "Texture sampled by this node."),
        "uv" => (
            "UV coordinates",
            "Texture coordinates evaluated by this node.",
        ),
        "source" => ("Source", "Source value evaluated by this node."),
        "alpha" => ("Alpha", "Opacity used to combine the values."),
        _ => (name, "Value consumed by this node."),
    };
    MaterialInputPortPresentation {
        label: label.to_owned(),
        description: description.to_owned(),
    }
}

fn material_slot_description(
    description: &str,
    domain: Option<MaterialExpressionDomain>,
) -> String {
    format!(
        "{description}\nEvaluation: {}",
        material_domain_name(domain)
    )
}

fn material_slot_title(label: &str, value_type: Option<MaterialValueType>) -> String {
    format!("{label} [{}]", material_value_type_name(value_type))
}

fn material_value_type_name(value_type: Option<MaterialValueType>) -> &'static str {
    match value_type {
        Some(MaterialValueType::Float) => "Float",
        Some(MaterialValueType::Vec2) => "Vector 2",
        Some(MaterialValueType::Vec3) => "Vector 3",
        Some(MaterialValueType::Vec4) => "Vector 4",
        Some(MaterialValueType::Color) => "Color",
        Some(MaterialValueType::Texture2D(_)) => "Texture 2D",
        Some(MaterialValueType::Bool) => "Boolean",
        None => "Unresolved",
    }
}

fn material_domain_name(domain: Option<MaterialExpressionDomain>) -> &'static str {
    match domain {
        Some(MaterialExpressionDomain::ShaderStatic) => "Shader static",
        Some(MaterialExpressionDomain::Instance) => "Material instance",
        Some(MaterialExpressionDomain::Effect) => "Effect",
        Some(MaterialExpressionDomain::Emitter) => "Emitter",
        Some(MaterialExpressionDomain::Particle) => "Particle",
        Some(MaterialExpressionDomain::Vertex) => "Vertex",
        Some(MaterialExpressionDomain::Fragment) => "Fragment",
        None => "Unresolved",
    }
}

fn socket_color(value_type: Option<MaterialValueType>) -> Color {
    match value_type {
        Some(MaterialValueType::Float) => Color::srgb(0.45, 0.78, 1.0),
        Some(MaterialValueType::Vec2) => Color::srgb(0.30, 0.86, 0.72),
        Some(MaterialValueType::Vec3) => Color::srgb(0.48, 0.86, 0.38),
        Some(MaterialValueType::Vec4) => Color::srgb(0.81, 0.55, 1.0),
        Some(MaterialValueType::Color) => Color::srgb(1.0, 0.60, 0.34),
        Some(MaterialValueType::Texture2D(_)) => Color::srgb(0.92, 0.42, 0.77),
        Some(MaterialValueType::Bool) => Color::srgb(0.96, 0.38, 0.42),
        None => theme::TEXT_FAINT,
    }
}

fn wire_color(value_type: Option<MaterialValueType>) -> Vec4 {
    match value_type {
        Some(MaterialValueType::Float) => Vec4::new(0.45, 0.78, 1.0, 0.82),
        Some(MaterialValueType::Vec2) => Vec4::new(0.30, 0.86, 0.72, 0.82),
        Some(MaterialValueType::Vec3) => Vec4::new(0.48, 0.86, 0.38, 0.82),
        Some(MaterialValueType::Vec4) => Vec4::new(0.81, 0.55, 1.0, 0.82),
        Some(MaterialValueType::Color) => Vec4::new(1.0, 0.60, 0.34, 0.82),
        Some(MaterialValueType::Texture2D(_)) => Vec4::new(0.92, 0.42, 0.77, 0.82),
        Some(MaterialValueType::Bool) => Vec4::new(0.96, 0.38, 0.42, 0.82),
        None => Vec4::new(0.59, 0.62, 0.70, 0.68),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn material_graph_canvas_bundle_spawns_without_duplicate_components() {
        let mut world = World::new();
        let viewport = world.spawn_empty().id();
        let entity = world
            .spawn(crate::feathers::node_graph::graph_canvas_bundle(
                viewport,
                Vec2::new(720.0, 420.0),
                MaterialGraphCanvas,
            ))
            .id();

        assert!(world.get::<Node>(entity).is_some());
        assert!(world.get::<MaterialGraphCanvas>(entity).is_some());
    }

    #[test]
    fn feathers_activation_queues_material_graph_menu_actions() {
        let mut app = App::new();
        app.add_observer(queue_material_graph_menu_action_activation);
        let context_action = app
            .world_mut()
            .spawn((
                MaterialGraphContextAction::Delete(MaterialProgramId::new()),
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let palette_action = app
            .world_mut()
            .spawn((
                MaterialGraphPaletteAction {
                    program: MaterialProgramId::new(),
                    kind: MaterialGraphCreateKind::Function(
                        aestra_compiler::MaterialGraphFunction::Remap,
                    ),
                    source: None,
                    target: Some(MaterialConnectionTarget::ProgramOutput(
                        MaterialOutputSocket::Color,
                    )),
                    label: "Remap".into(),
                    graph_position: Vec2::ZERO,
                    graph_key: "test".into(),
                    searchable: "remap".into(),
                },
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let preview_action = app
            .world_mut()
            .spawn((
                MaterialGraphPreviewToggle {
                    program: MaterialProgramId::new(),
                    target: MaterialGraphPreviewTarget::Output,
                },
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let toolbar_action = app
            .world_mut()
            .spawn((
                MaterialGraphToolbarAction::AddNode(MaterialProgramId::new()),
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate {
            entity: context_action,
        });
        app.world_mut().trigger(Activate {
            entity: palette_action,
        });
        app.world_mut().trigger(Activate {
            entity: preview_action,
        });
        app.world_mut().trigger(Activate {
            entity: toolbar_action,
        });
        app.update();

        for entity in [
            context_action,
            palette_action,
            preview_action,
            toolbar_action,
        ] {
            let action = app.world().entity(entity);
            assert!(action.contains::<PendingFeathersActivation>());
            assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
        }
    }

    #[test]
    fn material_graph_preview_visibility_is_remembered_per_node() {
        let program = MaterialProgramId::new();
        let first = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let second = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let mut previews = MaterialGraphPreviewState::default();

        assert!(!previews.is_visible(program, first));
        assert!(previews.toggle(program, first));
        assert!(previews.is_visible(program, first));
        assert!(!previews.is_visible(program, second));
        assert!(!previews.toggle(program, first));
        assert!(!previews.is_visible(program, first));
    }

    #[test]
    fn material_graph_preview_visibility_can_be_changed_for_the_whole_graph() {
        let program = MaterialProgramId::new();
        let first = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let second = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let output = MaterialGraphPreviewTarget::Output;
        let targets = [first, second, output];
        let mut previews = MaterialGraphPreviewState::default();

        previews.set_visible(program, targets, true);
        assert!(
            targets
                .into_iter()
                .all(|target| previews.is_visible(program, target))
        );

        previews.set_visible(program, targets, false);
        assert!(
            targets
                .into_iter()
                .all(|target| !previews.is_visible(program, target))
        );
    }

    #[test]
    fn material_graph_layout_round_trip_does_not_mutate_semantic_programs() {
        let program = MaterialProgram::additive_sprite("Layout contract");
        let expression = program.expressions[0].id;
        let semantic_source = program.to_pretty_ron().unwrap();
        let graph_key = material_graph_view_key(program.id);
        let mut memory = GraphViewportMemory::default();
        memory.set_view(graph_key.clone(), Vec2::new(24.0, -12.0), 1.25);
        memory.set_node(
            graph_key.clone(),
            material_graph_expression_node_key(expression),
            Vec2::new(180.0, 96.0),
            true,
        );
        memory.set_node(
            graph_key.clone(),
            MATERIAL_GRAPH_OUTPUT_NODE_KEY,
            Vec2::new(520.0, 110.0),
            false,
        );
        let mut previews = MaterialGraphPreviewState::default();
        previews.toggle(
            program.id,
            MaterialGraphPreviewTarget::Expression(expression),
        );
        previews.toggle(program.id, MaterialGraphPreviewTarget::Output);
        let mut document = ProjectEditorLayout::default();

        update_material_graph_layout_document(
            &mut document,
            std::slice::from_ref(&program),
            &memory,
            &previews,
        );

        assert_eq!(program.to_pretty_ron().unwrap(), semantic_source);
        let mut restored_memory = GraphViewportMemory::default();
        let mut restored_previews = MaterialGraphPreviewState::default();
        restore_material_graph_layouts(&document, &mut restored_memory, &mut restored_previews);
        assert_eq!(
            restored_memory.view(&graph_key),
            Some((Vec2::new(24.0, -12.0), 1.25))
        );
        assert_eq!(
            restored_memory.node(&graph_key, &material_graph_expression_node_key(expression)),
            Some((Vec2::new(180.0, 96.0), true))
        );
        assert!(restored_previews.is_visible(
            program.id,
            MaterialGraphPreviewTarget::Expression(expression)
        ));
        assert!(restored_previews.is_visible(program.id, MaterialGraphPreviewTarget::Output));
    }

    #[test]
    fn material_graph_preview_raster_has_expected_size_and_visible_content() {
        let program = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE)
            .expect("material graph lab program should parse");
        let image = render_material_graph_preview(
            &program,
            None,
            MaterialGraphPreviewTarget::Output,
            Some(MaterialValueType::Color),
        );

        assert_eq!(image.texture_descriptor.size.width, MATERIAL_PREVIEW_SIZE);
        assert_eq!(image.texture_descriptor.size.height, MATERIAL_PREVIEW_SIZE);
        let pixels = image.data.as_ref().expect("preview should be CPU-backed");
        assert_eq!(
            pixels.len(),
            (MATERIAL_PREVIEW_SIZE * MATERIAL_PREVIEW_SIZE * 4) as usize
        );
        assert!(pixels.windows(4).any(|pixel| pixel[0] != pixel[1]));
    }

    #[test]
    fn material_graph_preview_expands_only_its_node_height() {
        assert_eq!(
            node_height(3, false, true) - node_height(3, false, false),
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        );
        assert_eq!(
            node_height(3, true, false) - node_height(3, false, false),
            18.0
        );
    }

    #[test]
    fn graph_input_names_cover_every_current_semantic_socket() {
        let expression = MaterialExpressionId::new();
        for name in [
            "left",
            "right",
            "start",
            "end",
            "factor",
            "value",
            "min",
            "max",
            "input_min",
            "input_max",
            "output_min",
            "output_max",
            "edge_min",
            "edge_max",
            "normal",
            "view",
            "power",
            "radius",
            "softness",
            "threshold",
            "edge_width",
            "scene_depth",
            "pixel_depth",
            "fade_distance",
            "invert",
            "speed",
            "time",
            "center",
            "angle",
            "scale",
            "texture",
            "uv",
            "source",
            "alpha",
        ] {
            assert!(input_target(expression, name).is_some(), "missing {name}");
        }
        assert!(input_target(expression, "unknown").is_none());
    }

    #[test]
    fn graph_ports_use_presentation_labels_instead_of_storage_names() {
        assert_eq!(input_port_presentation("input_min").label, "Source minimum");
        assert_eq!(
            input_port_presentation("output_max").label,
            "Target maximum"
        );
        assert_eq!(input_port_presentation("edge_min").label, "Lower edge");
        assert_eq!(input_port_presentation("view").label, "View direction");
    }

    #[test]
    fn graph_slot_tooltips_report_type_and_evaluation_domain() {
        let title = material_slot_title("Target minimum input", Some(MaterialValueType::Color));
        let description = material_slot_description(
            "Value produced by this node.",
            Some(MaterialExpressionDomain::Fragment),
        );

        assert_eq!(title, "Target minimum input [Color]");
        assert!(!description.contains("Type:"));
        assert!(description.contains("Evaluation: Fragment"));
    }

    #[test]
    fn only_annotated_generated_constants_render_as_inline_defaults() {
        let mut program = MaterialProgram::additive_sprite("Inline defaults");
        let inline = program.outputs.alpha;
        let explicit = program.outputs.color;

        program.inline_constants.push(inline);

        assert!(inline_material_graph_default(&program, inline).is_some());
        assert!(inline_material_graph_default(&program, explicit).is_none());
    }

    #[test]
    fn graph_depth_places_consumers_after_their_sources() {
        let source = MaterialExpressionId::new();
        let middle = MaterialExpressionId::new();
        let output = MaterialExpressionId::new();
        let inputs = BTreeMap::from([
            (source, vec![]),
            (middle, vec![source]),
            (output, vec![middle]),
        ]);
        let mut memo = BTreeMap::new();
        assert_eq!(
            expression_depth(output, &inputs, &mut memo, &mut BTreeSet::new()),
            2
        );
        assert_eq!(memo[&source], 0);
        assert_eq!(memo[&middle], 1);
    }

    #[test]
    fn add_node_palette_uses_the_typed_compiler_catalog() {
        let effect = aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE)
            .expect("material graph lab effect should parse");
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .expect("material graph lab program should parse");
        let document = MaterialAuthoringDocument::new(effect, vec![program.clone()]);
        let compiler = MaterialCompiler;
        let ir = compiler.compile(&program).unwrap();
        let projection = compiler.project_graph(&program, Some(&ir));
        let descriptors = compiler.graph_node_catalog(&program);

        let options =
            select_palette_operations(&document, program.id, &projection, &descriptors, None);

        assert!(!options.is_empty());
        assert!(options.iter().all(|option| option.source.is_none()));
        assert!(options.iter().all(|option| option.target.is_none()));
        assert!(options.iter().any(|option| matches!(
            option.descriptor.kind,
            MaterialGraphCreateKind::Constant(MaterialValueType::Float)
        )));
        assert!(options.iter().any(|option| matches!(
            option.descriptor.kind,
            MaterialGraphCreateKind::Function(aestra_compiler::MaterialGraphFunction::Add)
        )));

        let source = program.outputs.color;
        let from_output = select_palette_operations(
            &document,
            program.id,
            &projection,
            &descriptors,
            Some(MaterialGraphPaletteConnection::FromOutput(source)),
        );
        assert!(!from_output.is_empty());
        assert!(
            from_output
                .iter()
                .all(|option| option.source == Some(source))
        );
        assert!(from_output.iter().all(|option| option.target.is_none()));
        assert!(
            from_output
                .iter()
                .all(|option| option.descriptor.kind.consumes_source())
        );

        let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
        let from_input = select_palette_operations(
            &document,
            program.id,
            &projection,
            &descriptors,
            Some(MaterialGraphPaletteConnection::FromInput(target)),
        );
        assert!(!from_input.is_empty());
        assert!(
            from_input
                .iter()
                .all(|option| option.target == Some(target))
        );
        assert!(
            from_input
                .iter()
                .any(|option| !option.descriptor.kind.consumes_source() && option.source.is_none())
        );
        assert!(from_input.iter().any(
            |option| option.descriptor.kind.consumes_source() && option.source == Some(source)
        ));
    }

    #[test]
    fn add_node_palette_groups_options_by_category_in_catalog_order() {
        let option = |label: &str, category: &str| MaterialGraphPaletteOption {
            descriptor: MaterialGraphNodeDescriptor {
                kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
                label: label.to_owned(),
                category: category.to_owned(),
            },
            source: None,
            target: None,
        };
        let options = vec![
            option("Float", "Constants"),
            option("UV", "Inputs"),
            option("Color", "Constants"),
            option("Add", "Math"),
        ];

        let categories = material_graph_palette_categories(&options);

        assert_eq!(
            categories
                .iter()
                .map(|(category, _)| *category)
                .collect::<Vec<_>>(),
            vec!["Constants", "Inputs", "Math"]
        );
        assert_eq!(
            categories[0]
                .1
                .iter()
                .map(|option| option.descriptor.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Float", "Color"]
        );
    }

    #[test]
    fn connection_endpoints_accept_either_drag_direction() {
        let source = MaterialExpressionId::new();
        let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
        let output = MaterialGraphSocketKind::ExpressionOutput(source);
        let input = MaterialGraphSocketKind::ConnectionInput(target);

        assert_eq!(connection_endpoints(output, input), Some((source, target)));
        assert_eq!(connection_endpoints(input, output), Some((source, target)));
        assert_eq!(connection_endpoints(output, output), None);
        assert_eq!(connection_endpoints(input, input), None);
    }

    #[test]
    fn graph_selection_supports_replace_add_and_toggle() {
        let program = MaterialProgramId::new();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let mut selection = MaterialGraphSelectionState::default();

        assert_eq!(
            selection.select_expression(program, first, false, false),
            Some(first)
        );
        assert_eq!(
            selection.select_expression(program, second, false, true),
            Some(second)
        );
        assert_eq!(selection.expressions, BTreeSet::from([first, second]));
        assert_eq!(
            selection.select_expression(program, first, true, false),
            Some(second)
        );
        assert_eq!(selection.expressions, BTreeSet::from([second]));
        selection.select_expression(program, second, true, false);
        assert!(selection.expressions.is_empty());
    }

    #[test]
    fn graph_wire_hit_distance_follows_the_rendered_bezier() {
        let start = Vec2::new(20.0, 40.0);
        let end = Vec2::new(360.0, 180.0);
        assert!(distance_to_graph_wire(start, start, end) < 0.001);
        assert!(distance_to_graph_wire(end, start, end) < 0.001);
        assert!(distance_to_graph_wire(Vec2::new(180.0, 430.0), start, end) > 100.0);
    }

    #[test]
    fn graph_add_and_delete_recompile_the_project_preview() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("graph.aestra.material.ron");
        let effect =
            aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        program.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let compiled = catalog.compile_project(&effect).unwrap().root;
        let mut session = test_support::session_with_timing_slack();
        session.open_compiled_effect("material_graph_lab.aestra.ron", effect, compiled);
        let mut history = MaterialProgramEditHistory::default();

        let added = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Add graph node",
            MaterialToolCommand::CreateMaterialGraphNode {
                program: program.id,
                kind: MaterialGraphCreateKind::Function(
                    aestra_compiler::MaterialGraphFunction::Remap,
                ),
                source: Some(program.outputs.color),
                target: Some(MaterialConnectionTarget::ProgramOutput(
                    MaterialOutputSocket::Color,
                )),
            },
        )
        .unwrap();
        let wrapper = *added.created_expressions.last().unwrap();
        assert!(
            session
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(program.id)
                .unwrap()
                .expressions
                .iter()
                .any(|expression| expression.id == wrapper)
        );

        apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Delete graph node",
            MaterialToolCommand::DeleteMaterialExpressions {
                program: program.id,
                expressions: vec![wrapper],
            },
        )
        .unwrap();
        assert!(
            !session
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(program.id)
                .unwrap()
                .expressions
                .iter()
                .any(|expression| expression.id == wrapper)
        );
        assert!(session.diagnostics.is_valid());
    }

    #[test]
    fn graph_extraction_persists_a_function_and_recompiles_the_call() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("extract.aestra.material.ron");
        let effect =
            aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        program.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let compiled = catalog.compile_project(&effect).unwrap().root;
        let mut session = test_support::session_with_timing_slack();
        session.open_compiled_effect("material_graph_lab.aestra.ron", effect, compiled);
        let mut history = MaterialProgramEditHistory::default();
        let function = MaterialFunctionId::from_u128(0xE871);

        let plan = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Extract material function",
            MaterialToolCommand::ExtractMaterialFunction {
                program: program.id,
                function,
                name: "Extracted Alpha".into(),
                expressions: vec![program.outputs.alpha],
            },
        )
        .unwrap();

        assert_eq!(plan.created_function().unwrap().id, function);
        assert!(
            catalog
                .material_functions()
                .unwrap()
                .iter()
                .any(|candidate| candidate.id == function)
        );
        let replacement = MaterialProgram::load_ron(&path).unwrap();
        assert!(matches!(
            replacement
                .expressions
                .iter()
                .find(|expression| expression.id == replacement.outputs.alpha)
                .map(|expression| &expression.kind),
            Some(MaterialExpressionKind::FunctionCall {
                function: aestra_core::material::MaterialFunctionRef::Project(id),
                ..
            }) if *id == function
        ));
        assert!(session.diagnostics.is_valid());
    }
}
