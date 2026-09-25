//! Function socket-to-canvas creation. Semantic plans are function-native and transactional.
use super::*;
use crate::feathers::{
    combo_box::spawn_searchable_action_list, context_menu::spawn_pointer_context_menu_sized,
    search_field::spawn_search_field,
};
use crate::theme;
use bevy::{
    input_focus::{FocusCause, InputFocus},
    ui::RelativeCursorPosition,
};

#[derive(Clone)]
struct Choice {
    label: String,
    category: String,
    edits: Vec<Edit>,
    created: Vec<MaterialExpressionId>,
    sizes: Vec<Vec2>,
}

struct Open {
    view: GraphViewKey,
    viewport: Entity,
    anchor: Entity,
    before: MaterialFunction,
    origin: Option<SocketKind>,
    position: Vec2,
    choices: Vec<Choice>,
    generation: u64,
    input: Entity,
}

#[derive(Resource, Default)]
struct Palette(Option<Open>);
#[derive(Component)]
pub(super) struct Surface;
#[derive(Component, Clone, Copy)]
struct Choose(usize);

pub(super) fn register(app: &mut App) {
    app.init_resource::<Palette>()
        .add_observer(choose)
        .add_systems(Update, maintain);
}

fn validate(
    document: &MaterialAuthoringDocument,
    owner: MaterialFunctionId,
    edits: &[Edit],
) -> bool {
    let mut preview = document.clone();
    aestra_authoring::MaterialCommandExecutor::execute(
        &mut preview,
        &aestra_authoring::MaterialTransaction::new(
            "Create connected function node",
            edits
                .iter()
                .cloned()
                .map(
                    |edit| aestra_authoring::MaterialCommand::EditMaterialFunctionBody {
                        function: owner,
                        edit,
                    },
                )
                .collect(),
        ),
    )
    .is_ok()
}

fn choices(
    document: &MaterialAuthoringDocument,
    function: &MaterialFunction,
    origin: Option<SocketKind>,
) -> Vec<Choice> {
    let library =
        aestra_compiler::MaterialFunctionLibrary::new(document.material_functions.clone());
    let descriptors = MaterialCompiler.function_graph_node_catalog(function, &library);
    let actions = descriptors
        .into_iter()
        .map(|d| (d.label, d.category, BodyActionKind::Create(d.kind)))
        .chain(function.inputs.iter().map(|input| {
            (
                format!("Input: {}", input.name),
                "Function inputs".into(),
                BodyActionKind::Input(input.id),
            )
        }));
    let mut choices = Vec::new();
    for (label, category, action) in actions.take(512) {
        let Ok(edits) = create_edits(function, action, &library) else {
            continue;
        };
        let created = edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::Add { expression, .. } => Some(expression.id),
                _ => None,
            })
            .collect::<Vec<_>>();
        let Some(main) = created.last().copied() else {
            continue;
        };
        let mut preview = function.clone();
        preview
            .expressions
            .extend(edits.iter().filter_map(|edit| match edit {
                Edit::Add { expression, .. } => Some(expression.clone()),
                _ => None,
            }));
        let MaterialFunctionBodyProjection::Graph { nodes, edges } = MaterialCompiler
            .project_function_graph(&preview, &library)
            .body
        else {
            continue;
        };
        let sizes = created
            .iter()
            .map(|id| estimated_size(*id, &nodes, &edges))
            .collect::<Vec<_>>();
        if origin.is_none() {
            if validate(document, function.id, &edits) {
                choices.push(Choice {
                    label,
                    category,
                    edits,
                    created,
                    sizes,
                });
            }
            continue;
        }
        let connections = match origin {
            Some(SocketKind::Target(target)) => {
                vec![(String::new(), connection_edit(main, target))]
            }
            Some(SocketKind::Source(source)) => {
                let mut inputs = Vec::new();
                let mut names = Vec::new();
                for edge in edges {
                    if let Some(Target::Input(expression, input)) = target(&edge.target)
                        && expression == main
                        && !inputs.contains(&input)
                    {
                        inputs.push(input);
                        names.push(input_name(&edge.target, &preview, &library));
                    }
                }
                inputs
                    .into_iter()
                    .zip(names)
                    .take(16)
                    .map(|(input, name)| {
                        (
                            format!(" — {name}"),
                            connection_edit(source, Target::Input(main, input)),
                        )
                    })
                    .collect()
            }
            None => unreachable!(),
        };
        for (suffix, connection) in connections {
            let mut candidate = edits.clone();
            candidate.push(connection);
            if validate(document, function.id, &candidate) {
                choices.push(Choice {
                    label: format!("{label}{suffix}"),
                    category: category.clone(),
                    edits: candidate,
                    created: created.clone(),
                    sizes: sizes.clone(),
                });
            }
        }
    }
    choices
}

fn input_name(
    port: &MaterialFunctionGraphTarget,
    function: &MaterialFunction,
    library: &aestra_compiler::MaterialFunctionLibrary,
) -> String {
    // Use the same port labels as the function canvas, including named call arguments.
    if let MaterialFunctionGraphTarget::Input { port, .. } = port {
        return crate::material_graph::input_port_presentation(port).label;
    }
    if let MaterialFunctionGraphTarget::Argument { input: id, .. } = port {
        for expression in &function.expressions {
            if let MaterialExpressionKind::FunctionCall { function, .. } = &expression.kind
                && let Some(definition) = library.get(*function)
                && let Some(input) = definition.inputs.iter().find(|input| input.id == *id)
            {
                return input.name.clone();
            }
        }
    }
    "Input".into()
}

fn viewport(world: &World, mut entity: Entity) -> Option<Entity> {
    loop {
        if world.get::<View>(entity).is_some() {
            return Some(entity);
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

fn close(world: &mut World) {
    if let Some(open) = world.resource_mut::<Palette>().0.take()
        && let Ok(entity) = world.get_entity_mut(open.anchor)
    {
        entity.despawn();
    }
}

pub(super) fn open(world: &mut World, socket: Entity, pointer: Vec2) {
    if !world.contains_resource::<Palette>() {
        return;
    }
    close(world);
    let Some(origin) = world.get::<Socket>(socket).copied() else {
        return;
    };
    let Some(viewport) = viewport(world, socket) else {
        return;
    };
    open_at(world, viewport, pointer, origin.owner, Some(origin.kind));
}

pub(super) fn open_canvas(world: &mut World, viewport: Entity, pointer: Vec2) {
    if !world.contains_resource::<Palette>() {
        return;
    }
    close(world);
    let Some(owner) = world.get::<View>(viewport).map(|view| view.0) else {
        return;
    };
    open_at(world, viewport, pointer, owner, None);
}

fn open_at(
    world: &mut World,
    viewport: Entity,
    pointer: Vec2,
    owner: MaterialFunctionId,
    origin: Option<SocketKind>,
) {
    if world
        .get::<View>(viewport)
        .is_none_or(|view| view.0 != owner)
        || world
            .get_resource::<ButtonInput<KeyCode>>()
            .is_some_and(|keys| keys.pressed(KeyCode::Escape))
    {
        return;
    }
    let Some(meta) = world.get::<GraphGeometryView>(viewport) else {
        return;
    };
    let view = meta.key.clone();
    let Some(computed) = world.get::<ComputedNode>(viewport) else {
        return;
    };
    let Some(transform) = world.get::<UiGlobalTransform>(viewport) else {
        return;
    };
    let local = crate::material_graph::viewport_local_position(
        computed,
        transform,
        pointer / computed.inverse_scale_factor,
    );
    if !local.is_finite()
        || !Rect::from_corners(Vec2::ZERO, computed.size() * computed.inverse_scale_factor)
            .contains(local)
    {
        return;
    }
    let Some(graph) = world.get::<FeathersGraphViewport>(viewport) else {
        return;
    };
    let position = graph.unproject_viewport_point(local);
    // Releasing on an incompatible node is not an empty-canvas creation gesture.
    let occupied = world
        .query::<(Entity, &FeathersGraphNode, &ComputedNode)>()
        .iter(world)
        .any(|(entity, node, computed)| {
            self::viewport(world, entity) == Some(viewport)
                && Rect::from_corners(
                    node.position(),
                    node.position() + computed.size() * computed.inverse_scale_factor,
                )
                .contains(position)
        });
    if occupied {
        return;
    }
    let Some(session) = world.get_resource::<EditorSession>() else {
        return;
    };
    let Some(catalog) = world.get_resource::<ProjectEffectCatalog>() else {
        return;
    };
    if catalog.root() != view.document.project {
        return;
    }
    let target = crate::material_document::MaterialEditingTarget::Function {
        root: catalog.root().to_owned(),
        id: owner,
    };
    let Ok(before) = session.graph_function_for(&target, catalog) else {
        return;
    };
    let Ok(document) = session.graph_authoring_document_for(&target, catalog) else {
        return;
    };
    let choices = choices(&document, &before, origin);
    let generation = catalog.content_revision().generation;
    let options = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| ComboOption {
            label: choice.label.clone(),
            selected: false,
            action: Choose(index),
        })
        .collect::<Vec<_>>();
    let categories = choices
        .iter()
        .map(|choice| choice.category.clone())
        .collect::<Vec<_>>();
    let mut anchor = Entity::PLACEHOLDER;
    let mut input = Entity::PLACEHOLDER;
    world.commands().entity(viewport).with_children(|parent| {
        spawn_pointer_context_menu_sized(
            parent,
            local,
            280.0,
            (),
            (Surface, FeathersGraphNavigationBlocker),
            |menu| {
                anchor = menu.target_entity();
                menu.commands()
                    .entity(anchor)
                    .insert(bevy::ui_widgets::MenuFocusState::Open);
                input = spawn_search_field(menu, "", "Search compatible nodes", "Clear search", ());
                if options.is_empty() {
                    menu.spawn((
                        Text::new("No compatible nodes"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                } else {
                    spawn_searchable_action_list(menu, input, &options, &categories);
                }
            },
        );
    });
    world.flush();
    // Own the anchoring parent as well, so dismissal does not leak empty entities.
    anchor = world.get::<ChildOf>(anchor).unwrap().parent();
    world.resource_mut::<Palette>().0 = Some(Open {
        view,
        viewport,
        anchor,
        before,
        origin,
        position,
        choices,
        generation,
        input,
    });
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.set(input, FocusCause::Navigated);
    }
}

fn maintain(world: &mut World) {
    let escape = world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    if escape {
        *world.resource_mut::<ConnectionPreview>() = default();
    }
    let Some(open) = world.resource::<Palette>().0.as_ref() else {
        return;
    };
    let valid = world
        .get::<GraphGeometryView>(open.viewport)
        .is_some_and(|meta| meta.key == open.view)
        && world.get_entity(open.anchor).is_ok()
        && world
            .get_resource::<ProjectEffectCatalog>()
            .is_some_and(|catalog| {
                catalog.root() == open.view.document.project
                    && catalog.content_revision().generation == open.generation
            })
        && world
            .get_resource::<EditorSession>()
            .zip(world.get_resource::<ProjectEffectCatalog>())
            .is_some_and(|(session, catalog)| {
                session
                    .graph_function_for(
                        &crate::material_document::MaterialEditingTarget::Function {
                            root: open.view.document.project.clone(),
                            id: open.before.id,
                        },
                        catalog,
                    )
                    .is_ok()
            });
    let anchor = open.anchor;
    let input = open.input;
    let pressed = world
        .get_resource::<ButtonInput<MouseButton>>()
        .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    let over = world
        .query_filtered::<&RelativeCursorPosition, With<Surface>>()
        .iter(world)
        .any(RelativeCursorPosition::cursor_over);
    if escape || !valid || (pressed && !over) {
        close(world);
        return;
    }
    if world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Enter))
        && world
            .get_resource::<InputFocus>()
            .and_then(|focus| focus.get())
            == Some(input)
    {
        let first = world
            .query::<(Entity, &Choose)>()
            .iter(world)
            .filter(|(entity, _)| visible_in(world, *entity, anchor))
            .min_by_key(|(_, choice)| choice.0)
            .map(|(entity, _)| entity);
        if let Some(entity) = first {
            world.trigger(Activate { entity });
        }
    }
}

fn visible_in(world: &World, mut entity: Entity, anchor: Entity) -> bool {
    while entity != anchor {
        if world
            .get::<Node>(entity)
            .is_some_and(|node| node.display == Display::None)
        {
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
    true
}

fn choose(
    event: On<Activate>,
    actions: Query<&Choose>,
    parents: Query<&ChildOf>,
    mut palette: ResMut<Palette>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    memory: Option<ResMut<GraphViewportMemory>>,
    placement: placement::Context,
    views: Query<&GraphGeometryView>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    let Some(mut memory) = memory else {
        return;
    };
    let Some(open) = palette.0.as_ref() else {
        return;
    };
    if !parents
        .iter_ancestors(event.entity)
        .any(|id| id == open.anchor)
    {
        return;
    }
    let open = palette.0.take().unwrap();
    commands.entity(open.anchor).despawn();
    let result: Result<(), String> = (|| {
        if catalog.root() != open.view.document.project
            || catalog.content_revision().generation != open.generation
            || !views
                .get(open.viewport)
                .is_ok_and(|view| view.key == open.view)
            || session
                .graph_function_for(
                    &crate::material_document::MaterialEditingTarget::Function {
                        root: open.view.document.project.clone(),
                        id: open.before.id,
                    },
                    &catalog,
                )?
                .normalized()
                != open.before.normalized()
        {
            return Err("Function or view changed; open the node menu again".into());
        }
        session.open_material_function(&catalog, open.before.id)?;
        let choice = open
            .choices
            .get(action.0)
            .ok_or("Node choice disappeared")?;
        let graph =
            crate::material_graph::function_graph_memory_key(catalog.root(), open.before.id);
        let before = crate::material_graph::presentation::Snapshot::capture(
            &graph, &catalog, &session, &memory,
        )
        .ok_or("Graph history unavailable")?;
        let mut area = placement.capture(&open.view, &memory);
        editor.edit_body(&mut session, &mut catalog, choice.edits.clone())?;
        placement.preserve_existing(&open.view, &mut memory);
        let neighborhood = match open.origin {
            Some(SocketKind::Source(source)) => {
                placement::Neighborhood::After(GraphNodeKey::Expression(source))
            }
            Some(SocketKind::Target(Target::Input(target, _))) => {
                placement::Neighborhood::Before(GraphNodeKey::Expression(target))
            }
            Some(SocketKind::Target(Target::Output(_))) => {
                placement::Neighborhood::Before(GraphNodeKey::FunctionOutputs)
            }
            None => placement::Neighborhood::Cursor,
        };
        let mut unassisted = false;
        for (id, size) in choice.created.iter().zip(&choice.sizes) {
            let placed = area.place(
                open.position - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5),
                *size,
                neighborhood,
            );
            memory.place_node(&graph, id.to_string(), placed.position);
            unassisted |= !placed.assisted;
        }
        before.attach(&catalog, &mut session, &mut memory);
        session.status = if open.origin.is_some() {
            format!("Added and connected {}", choice.label)
        } else {
            format!("Added {}", choice.label)
        };
        if unassisted {
            session
                .status
                .push_str(&format!(" · {}", placement.notice()));
        }
        Ok(())
    })();
    if let Err(error) = result {
        session.status = format!("Node creation failed: {error}");
    }
    session.ui_revision += 1;
}

#[cfg(test)]
mod tests;
