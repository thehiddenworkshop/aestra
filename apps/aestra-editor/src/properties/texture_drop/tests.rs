use super::*;
use aestra_core::material::{
    MaterialEvaluationDomain, MaterialInstance, MaterialParameter, MaterialProgram,
    MaterialProgramRef, MaterialTextureColorSpace, MaterialTextureDescriptor,
};
use bevy::{
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        events::DragEnter,
        pointer::{Location, PointerId},
    },
};

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    source: Entity,
    target: Entity,
    marker: TextureDropTarget,
}
impl Fixture {
    fn new(semantic: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("textures")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]))
            .save(root.path().join("textures/new.png"))
            .unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        let renderer = session.effect.emitters[0].renderers[0].id;
        let marker = if semantic {
            let mut program = MaterialProgram::additive_sprite("Textured");
            let parameter = MaterialParameterId::new();
            program.parameters.push(MaterialParameter {
                id: parameter,
                name: "Albedo".into(),
                value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
                    color_space: MaterialTextureColorSpace::SrgbColor,
                    sampler: default(),
                }),
                evaluation_domain: MaterialEvaluationDomain::Instance,
                default: None,
            });
            program
                .save_ron(root.path().join("material.aestra.material.ron"))
                .unwrap();
            let instance = MaterialInstance {
                id: MaterialId::new(),
                program: MaterialProgramRef::Project(program.id),
                values: default(),
                render_state: program.render_state_policy.default,
            };
            let id = instance.id;
            session.effect.emitters[0].renderers[0].material = id;
            session.effect.material_instances.push(instance);
            TextureDropTarget::parameter(&session, id, parameter).unwrap()
        } else {
            TextureDropTarget::sprite(
                &session,
                renderer,
                session.effect.emitters[0].renderers[0].material,
            )
        };
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("textures/new.png")
                .unwrap()
                .id,
        );
        let mut app = App::new();
        asset_drop::register(&mut app);
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action);
        let source = app.world_mut().spawn(payload).id();
        // Proves the specific texture input wins over the enclosing material-drop card.
        let card = app
            .world_mut()
            .spawn(asset_drop::RendererDropTarget {
                effect: marker.effect,
                renderer,
            })
            .id();
        let target = app.world_mut().spawn((marker, ChildOf(card))).id();
        Self {
            root,
            app,
            source,
            target,
            marker,
        }
    }
    fn drop(&mut self) {
        let source = self.app.world_mut().spawn(ChildOf(self.source)).id();
        let target = self.app.world_mut().spawn(ChildOf(self.target)).id();
        self.app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location(),
            DragDrop {
                button: PointerButton::Primary,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            target,
        ));
        self.app.world_mut().flush();
    }
    fn effect(&self) -> EffectAsset {
        self.app.world().resource::<EditorSession>().effect.clone()
    }
    fn plan(&self) -> Result<Assignment, String> {
        plan(
            self.app.world().get::<AssetPayload>(self.source).unwrap(),
            self.marker,
            self.app.world().resource::<ProjectEffectCatalog>(),
            self.app.world().resource::<EditorSession>(),
        )
    }
    fn history(&mut self, undo: bool) {
        self.app.world_mut().trigger(if undo {
            crate::history::HistoryAction::Undo
        } else {
            crate::history::HistoryAction::Redo
        });
        self.app.world_mut().flush();
    }
}
fn location() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        },
        position: Vec2::ZERO,
    }
}

#[test]
fn sprite_and_semantic_texture_drops_register_and_assign_atomically() {
    for semantic in [false, true] {
        let mut fixture = Fixture::new(semantic);
        let before = fixture.effect();
        let bytes = std::fs::read(fixture.root.path().join("textures/new.png")).unwrap();
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .material_history_active = true;
        fixture.drop();
        let after = fixture.effect();
        assert_eq!(
            after.assets.len(),
            before.assets.len() + 1,
            "{}",
            fixture.app.world().resource::<EditorSession>().status
        );
        let asset = after.assets.last().unwrap();
        assert_eq!(asset.path, "textures/new.png");
        match fixture.marker.slot {
            Slot::Sprite { material, .. } => {
                let material = after
                    .materials
                    .iter()
                    .find(|value| value.id == material)
                    .unwrap();
                let MaterialProperties::Sprite { texture, .. } = material.properties;
                assert_eq!(texture, Some(asset.id));
            }
            Slot::Parameter {
                instance,
                parameter,
                ..
            } => {
                assert_eq!(
                    after
                        .material_instances
                        .iter()
                        .find(|value| value.id == instance)
                        .unwrap()
                        .values[&parameter],
                    MaterialParameterValue::Constant(MaterialValue::Texture2D(asset.id))
                );
                assert!(
                    fixture
                        .app
                        .world()
                        .resource::<ProjectEffectCatalog>()
                        .material_drafts
                        .is_empty()
                );
            }
        }
        assert!(
            !fixture
                .app
                .world()
                .resource::<EditorSession>()
                .material_history_active
        );
        fixture.drop();
        assert_eq!(fixture.effect(), after);
        assert!(fixture.plan().unwrap().transaction.is_none());
        fixture.history(true);
        assert_eq!(fixture.effect(), before);
        fixture.history(false);
        assert_eq!(fixture.effect(), after);
        assert_eq!(
            std::fs::read(fixture.root.path().join("textures/new.png")).unwrap(),
            bytes
        );
        let saved = fixture.root.path().join("saved.aestra.ron");
        after.save_ron(&saved).unwrap();
        assert_eq!(EffectAsset::load_ron(saved).unwrap(), after);
        assert!(after.flipbooks.is_empty());
    }
}

#[test]
fn registered_texture_is_reused_and_survives_undo() {
    for semantic in [false, true] {
        let mut fixture = Fixture::new(semantic);
        let asset = AssetDefinition::texture("Existing", "./textures\\new.png");
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .assets
            .push(asset);
        let before = fixture.effect();
        fixture.drop();
        assert_eq!(fixture.effect().assets, before.assets);
        assert_ne!(fixture.effect(), before);
        fixture.history(true);
        assert_eq!(fixture.effect(), before);
    }
}

#[test]
fn invalid_stale_protected_and_cancelled_drops_leave_the_effect_unchanged() {
    for case in 0..8 {
        let mut fixture = Fixture::new(false);
        match case {
            0 => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<ProjectEffectCatalog>()
                    .refresh();
            }
            1 => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .effect
                    .id = aestra_core::EffectId::new();
            }
            2 => {
                let id = fixture.effect().emitters[0].renderers[0].id;
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .locks
                    .lock(SemanticTarget::Renderer(id));
            }
            3 => {
                let mut protection = DocumentProtectionState::default();
                protection.asset_create_open = true;
                fixture.app.insert_resource(protection);
            }
            4 => {
                let mut keys = ButtonInput::<KeyCode>::default();
                keys.press(KeyCode::Escape);
                fixture.app.insert_resource(keys);
            }
            5 => {
                std::fs::remove_file(fixture.root.path().join("textures/new.png")).unwrap();
            }
            6 => {
                std::fs::write(
                    fixture.root.path().join("textures/new.png"),
                    b"changed on disk",
                )
                .unwrap();
            }
            _ => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .preview_transaction(EffectTransaction::single(
                        "Pending",
                        EffectCommand::SetEffectName {
                            name: "Keep".into(),
                        },
                    ));
            }
        }
        let before = fixture.effect();
        fixture.drop();
        assert_eq!(fixture.effect(), before, "case {case}");
        assert!(
            fixture
                .app
                .world()
                .resource::<EditorSession>()
                .status
                .starts_with("Material drop rejected:"),
            "case {case}"
        );
        assert!(!fixture.app.world().resource::<EditorSession>().can_undo());
    }
}

#[test]
fn wrong_input_type_and_changed_binding_are_rejected() {
    for numeric in [false, true] {
        let fixture = Fixture::new(true);
        let mut target = fixture.marker;
        let Slot::Parameter {
            program, parameter, ..
        } = target.slot
        else {
            panic!()
        };
        if numeric {
            let mut app = fixture.app;
            let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
            let before = catalog.material_program(program).unwrap();
            let mut after = before.clone();
            after
                .parameters
                .iter_mut()
                .find(|value| value.id == parameter)
                .unwrap()
                .value_type = MaterialValueType::Float;
            catalog.replace_material_program(&before, &after).unwrap();
            assert!(
                plan(
                    app.world().get::<AssetPayload>(fixture.source).unwrap(),
                    target,
                    app.world().resource::<ProjectEffectCatalog>(),
                    app.world().resource::<EditorSession>()
                )
                .is_err()
            );
        } else {
            if let Slot::Parameter { program, .. } = &mut target.slot {
                *program = MaterialProgramId::new();
            }
            assert!(
                plan(
                    fixture
                        .app
                        .world()
                        .get::<AssetPayload>(fixture.source)
                        .unwrap(),
                    target,
                    fixture.app.world().resource::<ProjectEffectCatalog>(),
                    fixture.app.world().resource::<EditorSession>()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn hover_uses_texture_target_feedback_without_registering_an_asset() {
    let mut fixture = Fixture::new(false);
    let before = fixture.effect();
    fixture.app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        DragEnter {
            button: PointerButton::Primary,
            dragged: fixture.source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        fixture.target,
    ));
    fixture.app.world_mut().flush();
    assert!(
        fixture
            .app
            .world_mut()
            .query::<&Text>()
            .iter(fixture.app.world())
            .any(|text| text.0 == "Release to Assign texture new")
    );
    assert_eq!(fixture.effect(), before);
}

#[test]
fn duplicate_registry_entries_keep_the_current_binding_as_a_noop() {
    let mut fixture = Fixture::new(false);
    let first = AssetDefinition::texture("First", "textures/new.png");
    let current = AssetDefinition::texture("Current", "textures/new.png");
    let mut session = fixture.app.world_mut().resource_mut::<EditorSession>();
    let Slot::Sprite { material, .. } = fixture.marker.slot else {
        panic!()
    };
    let material = session
        .effect
        .materials
        .iter_mut()
        .find(|value| value.id == material)
        .unwrap();
    let MaterialProperties::Sprite { texture, .. } = &mut material.properties;
    *texture = Some(current.id);
    session.effect.assets.extend([first, current]);
    let before = fixture.effect();
    fixture.drop();
    assert_eq!(fixture.effect(), before);
    assert!(fixture.plan().unwrap().transaction.is_none());
    assert!(!fixture.app.world().resource::<EditorSession>().can_undo());
}

#[test]
fn wrong_source_kinds_and_reserved_loader_paths_are_rejected() {
    for filename in [
        "mesh.obj",
        "shader.wesl",
        "sheet.aestra.ron",
        "image#layer.png",
    ] {
        let mut fixture = Fixture::new(false);
        std::fs::write(fixture.root.path().join(filename), b"not a texture").unwrap();
        let catalog = ProjectEffectCatalog::scan(fixture.root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path(filename)
                .unwrap()
                .id,
        );
        fixture.app.insert_resource(catalog);
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.source)
            .insert(payload);
        assert!(fixture.plan().is_err(), "{filename}");
        let before = fixture.effect();
        fixture.drop();
        assert_eq!(fixture.effect(), before);
    }
}
