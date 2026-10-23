//! Complete production host branches, not a replacement dispatch surface.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{icon::SvgPlugin, text_input::tests::app_with},
    material_document::MaterialEditingTarget,
    material_graph::{MaterialGraphViewport, asset_drop::GraphDropTarget},
};
use aestra_core::{
    MaterialFunctionId,
    material::{MaterialFunction, MaterialProgram},
};
use bevy::feathers::controls::FeathersTextInputContainer;

struct Fixture {
    root: tempfile::TempDir,
    graph: MaterialFunction,
    code: MaterialFunction,
    program: MaterialProgram,
    session: EditorSession,
    catalog: ProjectEffectCatalog,
    app: App,
    locale: Localizer,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let graph = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        let code = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        let program = MaterialProgram::additive_sprite("Host material").normalized();
        graph
            .save_ron(root.path().join("graph.aestra.material-function.ron"))
            .unwrap();
        code.save_ron(root.path().join("code.aestra.material-function.ron"))
            .unwrap();
        program
            .save_ron(root.path().join("program.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.open_material_function(&catalog, graph.id).unwrap();
        Self {
            root,
            graph,
            code,
            program,
            catalog,
            session,
            locale: Localizer::new("en-US").unwrap(),
            app: app_with(|app| {
                app.add_plugins(SvgPlugin);
            }),
        }
    }
    fn target(&self, id: MaterialFunctionId) -> MaterialEditingTarget {
        MaterialEditingTarget::Function {
            root: self.root.path().to_owned(),
            id,
        }
    }
    fn mount(
        &mut self,
        target: Option<&MaterialEditingTarget>,
        view: Option<EditorViewId>,
    ) -> Entity {
        let server = self.app.world().resource::<AssetServer>().clone();
        let mut root = Entity::PLACEHOLDER;
        self.app
            .world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                root = parent.target_entity();
                spawn_material_graph_workspace(
                    parent,
                    target,
                    view,
                    &self.session,
                    &self.catalog,
                    &default(),
                    &default(),
                    &default(),
                    &default(),
                    &default(),
                    &self.locale,
                    &server,
                );
            });
        self.app.world_mut().flush();
        self.app.update();
        root
    }
    fn count<T: Component>(&mut self, root: Entity) -> usize {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<T>>()
            .iter(w)
            .filter(|e| within(w, *e, root))
            .count()
    }
    fn function_views(&mut self, root: Entity) -> usize {
        let w = self.app.world_mut();
        w.query::<(
            Entity,
            &crate::feathers::node_graph::geometry::GraphGeometryView,
        )>()
        .iter(w)
        .filter(|(e, v)| {
            within(w, *e, root)
                && matches!(
                    v.key.document.asset,
                    crate::document::DocumentKey::MaterialFunction(_)
                )
        })
        .count()
    }
    fn inspection_scrolls(&mut self, root: Entity) -> usize {
        let w = self.app.world_mut();
        w.query::<(Entity, &crate::feathers::scroll::PersistedScroll)>()
            .iter(w)
            .filter(|(e, scroll)| {
                within(w, *e, root)
                    && matches!(scroll.0, ScrollMemoryKey::MaterialFunctionInspector)
            })
            .count()
    }
    fn text(&mut self, root: Entity) -> String {
        let w = self.app.world_mut();
        w.query::<(Entity, &Text)>()
            .iter(w)
            .filter(|(e, _)| within(w, *e, root))
            .map(|(_, t)| t.0.clone())
            .collect::<Vec<_>>()
            .join("\n")
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
fn unfocused_code_tab_inspects_its_explicit_target_not_the_active_editable_graph() {
    let mut f = Fixture::new();
    let target = f.target(f.code.id);
    let before = f.session.effect.clone();
    let active = f.session.material_target.clone();
    let serial = f.session.operation_order.edit_serial();
    let root = f.mount(Some(&target), Some(EditorViewId(8)));
    let text = f.text(root);
    assert!(text.contains(&f.code.name), "{text}");
    assert!(
        text.contains(&f.code.custom_wesl.as_ref().unwrap().source),
        "{text}"
    );
    assert!(!text.contains(&f.graph.name));
    assert_eq!(f.count::<FeathersTextInputContainer>(root), 0);
    assert_eq!(f.function_views(root), 0);
    assert_eq!(f.inspection_scrolls(root), 1);
    assert_eq!(f.count::<GraphDropTarget>(root), 0);
    assert_eq!(f.session.material_target, active);
    assert_eq!(f.session.effect, before);
    assert_eq!(f.session.operation_order.edit_serial(), serial);
}

#[test]
fn unavailable_or_wrong_project_tab_never_exposes_the_active_functions_controls_or_source() {
    let mut f = Fixture::new();
    for target in [
        f.target(MaterialFunctionId::new()),
        MaterialEditingTarget::Function {
            root: f.root.path().join("other"),
            id: f.code.id,
        },
    ] {
        let expected = f
            .session
            .graph_function_for(&target, &f.catalog)
            .unwrap_err();
        let root = f.mount(Some(&target), Some(EditorViewId(7)));
        let text = f.text(root);
        assert!(text.contains(&expected), "{text}");
        assert!(!text.contains(&f.graph.name));
        assert_eq!(f.count::<FeathersTextInputContainer>(root), 0);
        assert_eq!(f.function_views(root), 0);
    }
}

#[test]
fn complete_host_routes_graph_tabs_and_material_tabs_to_their_actual_canvas() {
    let mut f = Fixture::new();
    f.session
        .open_material_function(&f.catalog, f.code.id)
        .unwrap();
    let target = f.target(f.graph.id);
    let root = f.mount(Some(&target), Some(EditorViewId(3)));
    assert_eq!(f.function_views(root), 1);
    assert_eq!(f.count::<MaterialGraphViewport>(root), 0);
    assert_eq!(f.inspection_scrolls(root), 0);
    let target = MaterialEditingTarget::Program {
        root: f.root.path().to_owned(),
        id: f.program.id,
    };
    let root = f.mount(Some(&target), Some(EditorViewId(4)));
    assert_eq!(f.function_views(root), 0);
    assert_eq!(f.count::<MaterialGraphViewport>(root), 1);
    assert!(f.text(root).contains(&f.program.name));
    assert_eq!(f.session.material_target, f.target(f.code.id));
}

#[test]
fn tool_without_override_follows_current_target_and_keeps_read_only_code_and_status() {
    let mut f = Fixture::new();
    f.session
        .open_material_function(&f.catalog, f.code.id)
        .unwrap();
    f.session.status = "Host status retained".into();
    let root = f.mount(None, None);
    let text = f.text(root);
    assert!(text.contains("Custom WESL source (read-only)"));
    assert!(text.contains("Host status retained"));
    assert!(text.contains(&f.code.name));
    assert_eq!(f.count::<FeathersTextInputContainer>(root), 0);
    f.session
        .open_material_function(&f.catalog, f.graph.id)
        .unwrap();
    let root = f.mount(None, None);
    assert_eq!(f.function_views(root), 1);
}

#[test]
fn nonsemantic_effect_empty_state_and_code_fallback_keep_localized_back_action() {
    use crate::material_graph::menu_actions::MaterialGraphToolbarAction;
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new();
        f.locale = Localizer::new(locale).unwrap();
        let root = f.mount(Some(&MaterialEditingTarget::EffectInstance), None);
        assert!(
            f.text(root)
                .contains(&f.locale.text("material-graph-empty"))
        );
        assert_eq!(f.function_views(root), 0);
        assert_eq!(f.count::<MaterialGraphViewport>(root), 0);
        let root = f.mount(Some(&f.target(f.code.id)), Some(EditorViewId(2)));
        let back = f.locale.text("material-document-back");
        let world = f.app.world_mut();
        assert!(
            world
                .query::<(Entity, &MaterialGraphToolbarAction, &AccessibleLabel)>()
                .iter(world)
                .any(|(e, action, label)| within(world, e, root)
                    && matches!(action, MaterialGraphToolbarAction::EffectContext)
                    && label.0 == back)
        );
    }
}
