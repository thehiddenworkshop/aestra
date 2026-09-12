use super::*;
use aestra_core::material::{
    MaterialDomain, MaterialInstance, MaterialProgram, MaterialProgramRef,
};
use bevy::ecs::system::RunSystemOnce;
use bevy::picking::{
    backend::HitData,
    pointer::{Location, PointerId},
};

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    source: Entity,
    input: Entity,
    card: Entity,
}

impl Fixture {
    fn new(multiple: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("meshes")).unwrap();
        let mut text =
            include_str!("../../../../../../assets/test/meshes/lab_cube.gltf").to_owned();
        if multiple {
            text = text.replace(
                "\"primitives\": [",
                "\"primitives\": [{\"attributes\":{\"POSITION\":0},\"mode\":4},",
            );
        }
        std::fs::write(root.path().join("meshes/cube.gltf"), text).unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        let mut program = MaterialProgram::additive_sprite("Mesh material");
        program.domain = MaterialDomain::Mesh;
        program
            .save_ron(root.path().join("mesh.aestra.material.ron"))
            .unwrap();
        let material = MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: default(),
            render_state: program.render_state_policy.default,
        };
        let asset = AssetDefinition {
            id: AssetId::new(),
            name: "Old".into(),
            kind: AssetKind::Mesh,
            path: "meshes/old.gltf#Mesh0/Primitive0".into(),
        };
        let renderer = &mut session.effect.emitters[0].renderers[0];
        renderer.properties = RendererProperties::Mesh { asset: asset.id };
        renderer.renderer_type = aestra_core::RendererTypeId::new(aestra_core::RENDERER_MESH);
        renderer.material = material.id;
        let marker = RendererDropTarget {
            effect: session.effect.id,
            renderer: renderer.id,
        };
        session.effect.material_instances.push(material);
        session.effect.assets.push(asset);
        let mesh_target = MeshDropTarget::capture(&session, marker).unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("meshes/cube.gltf")
                .unwrap()
                .id,
        );
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Font>();
        crate::properties::asset_drop::register(&mut app);
        app.insert_resource(catalog)
            .insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action);
        let source = app.world_mut().spawn(payload).id();
        let card = app.world_mut().spawn(marker).id();
        let input = app.world_mut().spawn((mesh_target, ChildOf(card))).id();
        Self {
            root,
            app,
            source,
            input,
            card,
        }
    }

    fn drop_on(&mut self, card: bool) {
        let source = self.app.world_mut().spawn(ChildOf(self.source)).id();
        let target = self
            .app
            .world_mut()
            .spawn(ChildOf(if card { self.card } else { self.input }))
            .id();
        self.app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            Location {
                target: bevy::camera::NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
            DragDrop {
                button: PointerButton::Primary,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            target,
        ));
        self.app.world_mut().flush();
    }
    fn finish(&mut self) {
        io::drain(self.app.world_mut());
    }
    fn effect(&self) -> EffectAsset {
        self.app.world().resource::<EditorSession>().effect.clone()
    }
}

#[test]
fn add_mesh_renderer_chooses_source_and_creates_one_undoable_edit() {
    for multiple in [false, true] {
        let mut f = Fixture::new(multiple);
        f.app
            .insert_resource(crate::test_support::session_with_timing_slack());
        let before = f.effect();
        assert!(
            !before.emitters[0]
                .renderers
                .iter()
                .any(|r| matches!(r.properties, RendererProperties::Mesh { .. }))
        );
        f.app.world_mut().trigger(OpenMeshRenderer);
        f.app.world_mut().flush();
        assert_eq!(f.effect(), before);
        let button = f
            .app
            .world_mut()
            .query::<(Entity, &Choice)>()
            .iter(f.app.world())
            .find(|(_, choice)| matches!(choice, Choice::Source(0)))
            .unwrap()
            .0;
        f.app.world_mut().trigger(Activate { entity: button });
        f.app.world_mut().flush();
        f.finish();
        if multiple {
            assert_eq!(f.effect(), before);
            assign(f.app.world_mut(), 1);
        }
        let after = f.effect();
        assert_eq!(
            after.emitters[0].renderers.len(),
            before.emitters[0].renderers.len() + 1,
            "{}",
            f.app.world().resource::<EditorSession>().status
        );
        let renderer = after.emitters[0].renderers.last().unwrap();
        aestra_compiler::EffectCompiler::default()
            .compile(&after)
            .unwrap();
        assert_eq!(renderer.renderer_type.0, aestra_core::RENDERER_MESH);
        assert_eq!(
            renderer.properties,
            RendererProperties::Mesh {
                asset: after.assets.last().unwrap().id
            }
        );
        assert_eq!(after.materials, before.materials);
        assert_eq!(
            after.material_instances.len(),
            before.material_instances.len() + 1
        );
        let catalog = f.app.world().resource::<ProjectEffectCatalog>();
        aestra_compiler::EffectCompiler::default()
            .compile_project(&after, catalog.content().asset_index())
            .unwrap();
        assert!(
            catalog
                .material_programs_for_effect(&after)
                .unwrap()
                .iter()
                .any(|p| p.id == MaterialProgram::DEFAULT_MESH_ID)
        );
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            SemanticTarget::Renderer(renderer.id)
        );
        assert_eq!(
            EffectAsset::from_ron(&ron::to_string(&after).unwrap()).unwrap(),
            after
        );
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Undo);
        assert_eq!(f.effect(), before);
        assert!(!f.app.world().resource::<EditorSession>().can_undo());
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Redo);
        assert_eq!(f.effect(), after);
    }
}

#[test]
fn mesh_creation_source_picker_cancels_and_rechecks_document() {
    for changed in [false, true] {
        let mut f = Fixture::new(false);
        f.app.world_mut().trigger(OpenMeshRenderer);
        f.app.world_mut().flush();
        if changed {
            f.app
                .world_mut()
                .resource_mut::<EditorSession>()
                .effect
                .name = "Keep this edit".into();
        } else {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        }
        let before = f.effect();
        select_source(f.app.world_mut(), 0);
        f.app.world_mut().flush();
        assert_eq!(f.effect(), before);
        assert!(f.app.world().resource::<Prompt>().0.is_none());
        assert!(
            !f.app
                .world()
                .resource::<DocumentProtectionState>()
                .is_open()
        );
    }
}

#[test]
fn sample_project_exposes_a_valid_mesh_effect() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    let effect = EffectAsset::from_ron(include_str!(
        "../../../../../../sample-project/effects/mesh_material_lab.aestra.ron"
    ))
    .unwrap();
    let catalog = ProjectEffectCatalog::scan(&root);
    let programs = catalog.material_programs_for_effect(&effect).unwrap();
    MaterialAuthoringDocument::new(effect.clone(), programs)
        .with_material_functions(catalog.material_functions().unwrap())
        .validate()
        .unwrap();
    assert!(
        effect.emitters[0]
            .renderers
            .iter()
            .any(|r| matches!(r.properties, RendererProperties::Mesh { .. }))
    );
    for asset in &effect.assets {
        let file = asset.path.split('#').next().unwrap();
        assert!(root.join(file).is_file(), "Missing {file}");
    }
}

#[test]
fn single_mesh_drop_on_card_or_input_is_one_undoable_assignment() {
    for card in [false, true] {
        let mut f = Fixture::new(false);
        let before = f.effect();
        let path = f.root.path().join("meshes/cube.gltf");
        let bytes = std::fs::read(&path).unwrap();
        f.drop_on(card);
        assert_eq!(f.effect(), before, "inspection must not edit");
        f.finish();
        let after = f.effect();
        assert_eq!(
            after.assets.len(),
            before.assets.len() + 1,
            "{}",
            f.app.world().resource::<EditorSession>().status
        );
        assert_eq!(after.material_instances, before.material_instances);
        assert_eq!(after.materials, before.materials);
        assert_eq!(
            after.assets.last().unwrap().path,
            "meshes/cube.gltf#Mesh0/Primitive0"
        );
        assert_eq!(
            after.emitters[0].renderers[0].properties,
            RendererProperties::Mesh {
                asset: after.assets.last().unwrap().id
            }
        );
        assert!(f.app.world().resource::<Prompt>().0.is_none());
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Undo);
        assert_eq!(f.effect(), before);
        assert!(!f.app.world().resource::<EditorSession>().can_undo());
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Redo);
        assert_eq!(f.effect(), after);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        let text = ron::to_string(&after).unwrap();
        assert_eq!(EffectAsset::from_ron(&text).unwrap(), after);
    }
}

#[test]
fn multiple_primitives_wait_for_explicit_selection() {
    let mut f = Fixture::new(true);
    let before = f.effect();
    f.drop_on(false);
    f.finish();
    assert_eq!(f.effect(), before);
    f.app.world_mut().run_system_once(sync).unwrap();
    assert_eq!(
        f.app
            .world()
            .resource::<Prompt>()
            .0
            .as_ref()
            .unwrap()
            .choices
            .len(),
        2
    );
    let button = f
        .app
        .world_mut()
        .query::<(Entity, &Choice)>()
        .iter(f.app.world())
        .find(|(_, choice)| matches!(choice, Choice::Assign(1)))
        .unwrap()
        .0;
    f.app.world_mut().trigger(Activate { entity: button });
    f.app.world_mut().flush();
    assert_eq!(
        f.effect().assets.last().unwrap().path,
        "meshes/cube.gltf#Mesh0/Primitive1"
    );
}

#[test]
fn cancel_or_late_document_change_never_applies_inspection() {
    for case in 0..5 {
        let mut f = Fixture::new(false);
        f.drop_on(false);
        let mut completion = io::prepared_completion(f.app.world_mut());
        match case {
            0 => close(f.app.world_mut(), "Cancelled".into()),
            1 => {
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .effect
                    .name = "Keep edit".into()
            }
            2 => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            3 => f
                .app
                .world_mut()
                .resource_mut::<ProjectEffectCatalog>()
                .refresh(),
            _ => {
                let id = f.effect().emitters[0].id;
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .locks
                    .lock(SemanticTarget::Emitter(id));
            }
        }
        let before = f.effect();
        completion.apply(f.app.world_mut());
        f.app.world_mut().flush();
        assert_eq!(f.effect(), before, "case {case}");
        assert!(!f.app.world().resource::<EditorSession>().can_undo());
        assert!(f.app.world().resource::<Prompt>().0.is_none());
    }
}

#[test]
fn chooser_cancel_and_changed_file_preserve_previous_binding() {
    for cancel in [true, false] {
        let mut f = Fixture::new(true);
        let before = f.effect();
        f.drop_on(false);
        f.finish();
        if cancel {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            f.app.world_mut().run_system_once(sync).unwrap();
        } else {
            std::fs::write(f.root.path().join("meshes/cube.gltf"), b"changed").unwrap();
            assign(f.app.world_mut(), 0);
        }
        f.app.world_mut().flush();
        assert_eq!(f.effect(), before);
        assert!(f.app.world().resource::<Prompt>().0.is_none());
        assert!(
            !f.app
                .world()
                .resource::<DocumentProtectionState>()
                .is_open()
        );
    }
}

#[test]
fn existing_mesh_identity_is_reused_and_repeated_assignment_is_noop() {
    let mut f = Fixture::new(false);
    let asset = AssetDefinition {
        id: AssetId::new(),
        name: "Registered".into(),
        kind: AssetKind::Mesh,
        path: "./meshes\\cube.gltf#Mesh0/Primitive0".into(),
    };
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .assets
        .push(asset.clone());
    let before = f.effect();
    f.drop_on(true);
    f.finish();
    assert_eq!(f.effect().assets, before.assets);
    assert_eq!(
        f.effect().emitters[0].renderers[0].properties,
        RendererProperties::Mesh { asset: asset.id }
    );
    let assigned = f.effect();
    f.drop_on(true);
    f.finish();
    assert_eq!(f.effect(), assigned);
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    assert_eq!(f.effect(), before);
}

#[test]
fn unsupported_or_invalid_sources_and_locked_targets_do_not_edit() {
    for case in 0..6 {
        let mut f = Fixture::new(false);
        match case {
            0 => {
                std::fs::remove_file(f.root.path().join("meshes/cube.gltf")).unwrap();
            }
            1 => {
                let id = f.effect().emitters[0].renderers[0].id;
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .locks
                    .lock(SemanticTarget::Renderer(id));
            }
            2 => {
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .effect
                    .emitters[0]
                    .renderers[0]
                    .properties = RendererProperties::Sprite
            }
            3..=5 => {
                let name = match case {
                    3 => "bad.obj",
                    4 => "bad.gltf",
                    _ => "texture.png",
                };
                std::fs::write(f.root.path().join(name), b"invalid file").unwrap();
                f.app
                    .world_mut()
                    .resource_mut::<ProjectEffectCatalog>()
                    .refresh();
                let catalog = f.app.world().resource::<ProjectEffectCatalog>();
                let payload = AssetPayload::capture(
                    catalog,
                    catalog
                        .content()
                        .source_tree()
                        .at_relative_path(name)
                        .unwrap()
                        .id,
                );
                f.app.world_mut().entity_mut(f.source).insert(payload);
            }
            _ => unreachable!(),
        }
        let before = f.effect();
        f.drop_on(false);
        f.finish();
        assert_eq!(f.effect(), before, "case {case}");
        assert!(!f.app.world().resource::<EditorSession>().can_undo());
        assert!(f.app.world().resource::<Prompt>().0.is_none());
    }
}
