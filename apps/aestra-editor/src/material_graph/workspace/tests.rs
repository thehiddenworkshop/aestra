//! Mount the real material/effect workspace; function composition remains native-qualified.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{
        icon::SvgPlugin,
        node_graph::{FeathersGraphViewport, geometry::GraphGeometryView},
        text_input::tests::app_with,
    },
    material_graph::{
        MaterialGraphViewport,
        keys::{MATERIAL_GRAPH_OUTPUT_NODE_KEY, material_graph_view_key},
        menu_actions::MaterialGraphToolbarAction,
        palette::{MaterialGraphPalette, MaterialGraphPaletteAction},
        palette_state::{MaterialGraphConnection, MaterialGraphPaletteOpen},
        preview_surface::MaterialGraphPreviewRaster,
    },
};
use aestra_authoring::{MaterialConnectionTarget, MaterialOutputSocket, SemanticTarget};
use aestra_core::{
    MaterialExpressionId, MaterialId,
    material::{MaterialInstance, MaterialProgram, MaterialProgramRef, MaterialRenderState},
};
use std::collections::BTreeMap;

struct Fixture {
    root: tempfile::TempDir,
    program: MaterialProgram,
    session: EditorSession,
    catalog: ProjectEffectCatalog,
    palette: MaterialGraphPaletteState,
    selection: MaterialGraphSelectionState,
    previews: MaterialGraphPreviewState,
    memory: GraphViewportMemory,
    localizer: Localizer,
    app: App,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Workspace program").normalized();
        program
            .save_ron(root.path().join("workspace.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let app = app_with(|app| {
            app.add_plugins(SvgPlugin);
        });
        Self {
            root,
            program,
            catalog,
            app,
            session: crate::test_support::session_with_timing_slack(),
            palette: default(),
            selection: default(),
            previews: default(),
            memory: default(),
            localizer: Localizer::new("en-US").unwrap(),
        }
    }
    fn target(&self) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().to_owned(),
            id: self.program.id,
        }
    }
    fn mount(&mut self, target: &MaterialEditingTarget, view: Option<EditorViewId>) -> Entity {
        let server = self.app.world().resource::<AssetServer>().clone();
        let mut root = Entity::PLACEHOLDER;
        self.app
            .world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                root = parent.target_entity();
                spawn(
                    parent,
                    target,
                    view,
                    &self.session,
                    &self.catalog,
                    &self.palette,
                    &self.selection,
                    &self.previews,
                    &self.memory,
                    &self.localizer,
                    &server,
                );
            });
        self.app.world_mut().flush();
        self.app.update();
        root
    }
    fn count<T: Component>(&mut self, root: Entity) -> usize {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<T>>()
            .iter(world)
            .filter(|entity| within(world, *entity, root))
            .count()
    }
    fn viewport(&mut self, root: Entity) -> Entity {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<MaterialGraphViewport>>()
            .iter(world)
            .find(|entity| within(world, *entity, root))
            .unwrap()
    }
    fn text(&mut self, root: Entity) -> String {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Text)>()
            .iter(world)
            .filter(|(e, _)| within(world, *e, root))
            .map(|(_, t)| t.0.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn semantic_effect(&mut self) -> MaterialId {
        let id = MaterialId::new();
        self.session
            .effect
            .material_instances
            .push(MaterialInstance {
                id,
                program: MaterialProgramRef::Project(self.program.id),
                values: BTreeMap::new(),
                render_state: MaterialRenderState::additive_sprite(),
            });
        let renderer = &mut self.session.effect.emitters[0].renderers[0];
        renderer.material = id;
        self.session.selection.primary = SemanticTarget::Renderer(renderer.id);
        id
    }
}
fn within(world: &World, mut entity: Entity, root: Entity) -> bool {
    loop {
        if entity == root {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
}

#[test]
fn restored_standalone_workspace_uses_its_own_target_before_focus_without_editing() {
    let mut f = Fixture::new();
    let target = f.target();
    let effect = f.session.effect.clone();
    let root = f.mount(&target, Some(EditorViewId(7)));
    let v = f.viewport(root);
    let identity = f.app.world().get::<MaterialGraphViewport>(v).unwrap();
    assert_eq!(identity.program, f.program.id);
    assert_eq!(identity.editing_target, target);
    assert_eq!(identity.scope, Some(EditorViewId(7)));
    assert_eq!(
        f.app
            .world()
            .get::<GraphDropTarget>(v)
            .unwrap()
            .editing_target(),
        &target
    );
    let geometry = f.app.world().get::<GraphGeometryView>(v).unwrap();
    assert_eq!(geometry.key.document.project, f.root.path());
    assert_eq!(geometry.key.view, Some(EditorViewId(7)));
    assert!(f.text(root).contains(&f.program.name));
    assert_eq!(
        f.session.material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert_eq!(f.session.effect, effect);
    assert_eq!(f.session.operation_order.edit_serial(), 0);
}

#[test]
fn split_views_keep_independent_cameras_and_shared_authored_placement() {
    let mut f = Fixture::new();
    let target = f.target();
    let key = material_graph_view_key(f.program.id);
    f.memory.set_node(
        &key,
        MATERIAL_GRAPH_OUTPUT_NODE_KEY,
        Vec2::new(500.0, 200.0),
        false,
    );
    for (view, pan, zoom) in [
        (Some(EditorViewId(1)), Vec2::new(10.0, 20.0), 0.75),
        (Some(EditorViewId(2)), Vec2::new(90.0, 80.0), 1.5),
    ] {
        f.memory
            .set_view(material_graph_viewport_key(f.program.id, view), pan, zoom);
        let root = f.mount(&target, view);
        let v = f.viewport(root);
        let camera = f.app.world().get::<FeathersGraphViewport>(v).unwrap();
        assert_eq!(camera.project_graph_point(Vec2::ZERO), pan);
        assert_eq!(
            camera.project_graph_point(Vec2::ONE),
            pan + Vec2::splat(zoom)
        );
    }
    assert_eq!(
        f.memory.node(&key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
        Some((Vec2::new(500.0, 200.0), false))
    );
}

#[test]
fn effect_workspace_binds_selected_renderer_instance_and_does_not_offer_back_navigation() {
    let mut f = Fixture::new();
    let instance = f.semantic_effect();
    f.previews.toggle(
        f.program.id,
        crate::material_graph::MaterialGraphPreviewTarget::Output,
    );
    let root = f.mount(&MaterialEditingTarget::EffectInstance, None);
    let v = f.viewport(root);
    assert_eq!(
        f.app.world().get::<MaterialGraphViewport>(v).unwrap().scope,
        None
    );
    let world = f.app.world_mut();
    let raster = world
        .query::<&MaterialGraphPreviewRaster>()
        .single(world)
        .unwrap();
    assert_eq!(raster.instance, Some(instance));
    assert_eq!(raster.editing_target, MaterialEditingTarget::EffectInstance);
    assert!(
        !world
            .query::<&MaterialGraphToolbarAction>()
            .iter(world)
            .any(|action| matches!(action, MaterialGraphToolbarAction::EffectContext))
    );
}

#[test]
fn empty_effect_and_unavailable_standalone_target_mount_no_canvas_in_both_locales() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new();
        f.localizer = Localizer::new(locale).unwrap();
        let root = f.mount(&MaterialEditingTarget::EffectInstance, None);
        assert_eq!(f.count::<MaterialGraphViewport>(root), 0);
        assert_eq!(f.count::<GraphDropTarget>(root), 0);
        let title = f.localizer.text("material-graph-empty");
        let text = f.text(root);
        assert!(text.contains(&title));
        assert!(text.contains("selected renderer does not use a semantic material"));
        let target = MaterialEditingTarget::Program {
            root: f.root.path().join("other"),
            id: f.program.id,
        };
        let root = f.mount(&target, Some(EditorViewId(9)));
        assert_eq!(f.count::<MaterialGraphViewport>(root), 0);
        assert!(f.text(root).contains("belongs to another project"));
    }
}

#[test]
fn invalid_unsaved_program_still_mounts_diagnostic_graph_without_publication() {
    let mut f = Fixture::new();
    let mut draft = f.program.clone();
    draft.outputs.alpha = MaterialExpressionId::new();
    f.catalog
        .replace_material_program(&f.program, &draft)
        .unwrap();
    let draft = draft.normalized();
    let target = f.target();
    let (_, projection, _, _) = selected_projection_for(&target, &f.session, &f.catalog).unwrap();
    assert!(!projection.diagnostics.is_valid());
    let root = f.mount(&target, Some(EditorViewId(3)));
    assert_eq!(f.count::<MaterialGraphViewport>(root), 1);
    assert_eq!(f.catalog.material_program(f.program.id).unwrap(), draft);
    assert_eq!(f.session.operation_order.edit_serial(), 0);
}

#[test]
fn add_palette_requires_matching_program_view_and_editing_target() {
    let mut f = Fixture::new();
    let target = f.target();
    let view = Some(EditorViewId(5));
    f.palette.open = Some(MaterialGraphPaletteOpen {
        program: f.program.id,
        editing_target: target.clone(),
        scope: view,
        menu_position: Vec2::new(5.0, 6.0),
        graph_position: Vec2::new(25.0, 26.0),
        graph_key: material_graph_view_key(f.program.id),
        connection: None,
    });
    let root = f.mount(&target, view);
    assert_eq!(f.count::<MaterialGraphPalette>(root), 1);
    assert!(f.count::<MaterialGraphPaletteAction>(root) > 0);
    let root = f.mount(&target, Some(EditorViewId(6)));
    assert_eq!(f.count::<MaterialGraphPalette>(root), 0);
    f.palette.open.as_mut().unwrap().editing_target = MaterialEditingTarget::EffectInstance;
    let root = f.mount(&target, view);
    assert_eq!(f.count::<MaterialGraphPalette>(root), 0);
}

#[test]
fn node_and_connection_menus_only_mount_in_their_own_view() {
    let mut f = Fixture::new();
    let target = f.target();
    let view = Some(EditorViewId(5));
    f.palette.node_menu = Some(MaterialGraphNodeMenuOpen {
        program: f.program.id,
        scope: view,
        menu_position: Vec2::ONE,
    });
    f.palette.connection_menu = Some(MaterialGraphConnectionMenuOpen {
        program: f.program.id,
        scope: view,
        menu_position: Vec2::ONE,
        connections: vec![MaterialGraphConnection {
            program: f.program.id,
            source: f.program.outputs.color,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
        }],
    });
    let root = f.mount(&target, view);
    assert_eq!(f.count::<MaterialGraphNodeMenu>(root), 1);
    assert_eq!(f.count::<MaterialGraphConnectionMenu>(root), 1);
    let root = f.mount(&target, Some(EditorViewId(6)));
    assert_eq!(f.count::<MaterialGraphNodeMenu>(root), 0);
    assert_eq!(f.count::<MaterialGraphConnectionMenu>(root), 0);
    let root = f.mount(&target, None);
    assert_eq!(f.count::<MaterialGraphNodeMenu>(root), 0);
}
