use super::super::asset_drop::RendererDropTarget;
use super::*;
use aestra_core::material::{MaterialDomain, MaterialProgram};
use bevy::ecs::system::RunSystemOnce;

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    target: DropTarget,
}
impl Fixture {
    fn new(texture: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]))
            .save(root.path().join("albedo.png"))
            .unwrap();
        let program = MaterialProgram::additive_sprite("Compatible");
        program
            .save_ron(root.path().join("compatible.aestra.material.ron"))
            .unwrap();
        let mut wrong = MaterialProgram::additive_sprite("Wrong domain");
        wrong.domain = MaterialDomain::Mesh;
        wrong
            .save_ron(root.path().join("wrong.aestra.material.ron"))
            .unwrap();
        std::fs::write(
            root.path().join("cube.gltf"),
            include_str!("../../../../../assets/test/meshes/lab_cube.gltf"),
        )
        .unwrap();
        let session = crate::test_support::session_with_timing_slack();
        let renderer = &session.effect.emitters[0].renderers[0];
        let target = if texture {
            DropTarget::Texture(super::super::texture_drop::TextureDropTarget::sprite(
                &session,
                renderer.id,
                renderer.material,
            ))
        } else {
            DropTarget::Material(RendererDropTarget {
                effect: session.effect.id,
                renderer: renderer.id,
            })
        };
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ))
        .init_asset::<Font>();
        super::super::asset_drop::register(&mut app);
        app.insert_resource(session)
            .insert_resource(catalog)
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action);
        Self {
            _root: root,
            app,
            target,
        }
    }
    fn effect(&self) -> EffectAsset {
        self.app.world().resource::<EditorSession>().effect.clone()
    }
    fn open(&mut self) {
        let field = self.app.world_mut().spawn(PickerField(self.target)).id();
        self.app.world_mut().trigger(Activate { entity: field });
        self.app.world_mut().flush();
    }
    fn select_name(&mut self, name: &str) {
        let index = self
            .app
            .world()
            .resource::<Picker>()
            .0
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .position(|entry| entry.label.contains(name))
            .unwrap();
        select(self.app.world_mut(), index);
        self.app.world_mut().flush();
    }
}

#[test]
fn fields_filter_compatible_assets_and_search_without_editing() {
    for texture in [false, true] {
        let mut f = Fixture::new(texture);
        let before = f.effect();
        f.open();
        f.app.world_mut().run_system_once(sync).unwrap();
        let pending = f.app.world().resource::<Picker>().0.as_ref().unwrap();
        let labels: Vec<_> = pending.entries.iter().map(|e| e.label.as_str()).collect();
        assert!(
            !labels
                .iter()
                .any(|label| label.contains("wrong") || label.contains("cube.gltf"))
        );
        assert_eq!(
            labels.iter().any(|label| label.contains("albedo.png")),
            texture
        );
        assert_eq!(
            labels
                .iter()
                .any(|label| label.contains("compatible.aestra")),
            !texture
        );
        assert!(f.app.world().resource::<InputFocus>().get().is_some());
        let search = pending.search;
        f.app.world_mut().trigger(ValueChange::<String> {
            source: search,
            value: "NO SUCH ASSET".into(),
            is_final: false,
        });
        f.app.world_mut().flush();
        assert!(matching(f.app.world().resource::<Picker>().0.as_ref().unwrap()).is_empty());
        f.app.world_mut().trigger(ValueChange::<String> {
            source: search,
            value: if texture { "ALBEDO" } else { "compatible" }.into(),
            is_final: false,
        });
        f.app.world_mut().flush();
        assert_eq!(
            matching(f.app.world().resource::<Picker>().0.as_ref().unwrap()).len(),
            1
        );
        assert_eq!(f.effect(), before);
    }
}

#[test]
fn picker_source_assignment_is_undoable_and_repeat_is_noop() {
    for texture in [false, true] {
        let mut f = Fixture::new(texture);
        let before = f.effect();
        f.open();
        f.select_name(if texture {
            "albedo.png"
        } else {
            "compatible.aestra"
        });
        let after = f.effect();
        assert_ne!(
            before,
            after,
            "{}",
            f.app.world().resource::<EditorSession>().status
        );
        assert!(f.app.world().resource::<Picker>().0.is_none());
        assert!(
            !f.app
                .world()
                .resource::<DocumentProtectionState>()
                .is_open()
        );
        assert_eq!(
            after.emitters[0].renderers[0].properties,
            before.emitters[0].renderers[0].properties
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
        f.open();
        f.select_name(if texture {
            "albedo.png"
        } else {
            "compatible.aestra"
        });
        assert_eq!(f.effect(), after);
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Undo);
        assert_eq!(f.effect(), before, "repeat assignment must be a no-op");
    }
}

#[test]
fn local_texture_and_procedural_remain_available() {
    let mut f = Fixture::new(true);
    f.open();
    f.select_name("albedo.png");
    let textured = f.effect();
    f.open();
    f.select_name("Procedural");
    let procedural = f.effect();
    assert_ne!(textured, procedural);
    f.open();
    f.select_name("albedo · Local");
    assert_eq!(f.effect(), textured);
    assert_eq!(f.effect().assets.len(), textured.assets.len());
}

#[test]
fn virtual_picker_entries_share_drop_validation_and_undo() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .add_sprite_material();
    let local = f.effect().materials.last().unwrap().id;
    f.open();
    let pending = f.app.world().resource::<Picker>().0.as_ref().unwrap();
    assert!(pending.entries.iter().any(|entry| matches!(&entry.selection, Selection::Source(payload) if matches!(payload.virtual_asset(), Some(crate::asset_drop::VirtualAsset::BuiltInPreset(_))))));
    let (index, payload) = pending
        .entries
        .iter()
        .enumerate()
        .find_map(|(i, entry)| match &entry.selection {
            Selection::Source(payload)
                if payload.virtual_asset()
                    == Some(crate::asset_drop::VirtualAsset::Material(local)) =>
            {
                Some((i, payload.clone()))
            }
            _ => None,
        })
        .unwrap();
    assert!(
        super::super::asset_drop::plan_target(
            &payload,
            f.target,
            f.app.world().resource::<ProjectEffectCatalog>(),
            f.app.world().resource::<EditorSession>()
        )
        .is_ok()
    );
    let before = f.effect();
    select(f.app.world_mut(), index);
    f.app.world_mut().flush();
    assert_eq!(f.effect().emitters[0].renderers[0].material, local);
    f.app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(f.effect(), before);
    assert!(
        payload
            .check_document(f.app.world().resource::<EditorSession>())
            .is_err(),
        "old drag must not survive a history change"
    );
}

#[test]
fn cancel_stale_document_and_locks_never_assign() {
    for case in 0..3 {
        let mut f = Fixture::new(true);
        f.open();
        match case {
            0 => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            1 => {
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .effect
                    .name = "Keep edited name".into()
            }
            _ => {
                let emitter = f.effect().emitters[0].id;
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .locks
                    .lock(SemanticTarget::Emitter(emitter));
            }
        }
        let before = f.effect();
        f.select_name("albedo.png");
        assert_eq!(f.effect(), before);
        assert!(f.app.world().resource::<Picker>().0.is_none());
    }
}

#[test]
fn mesh_picker_reuses_background_primitive_assignment() {
    let mut f = Fixture::new(false);
    let session = &mut *f.app.world_mut().resource_mut::<EditorSession>();
    let renderer = &mut session.effect.emitters[0].renderers[0];
    let material = aestra_core::material::MaterialInstance {
        id: MaterialId::new(),
        program: aestra_core::material::MaterialProgramRef::BuiltIn(
            MaterialProgram::DEFAULT_MESH_ID,
        ),
        values: default(),
        render_state: MaterialProgram::built_in(
            aestra_core::material::MaterialProgramRef::BuiltIn(MaterialProgram::DEFAULT_MESH_ID),
        )
        .unwrap()
        .render_state_policy
        .default,
    };
    let asset = aestra_core::AssetDefinition {
        id: aestra_core::AssetId::new(),
        name: "Old mesh".into(),
        kind: AssetKind::Mesh,
        path: "old.gltf#Mesh0/Primitive0".into(),
    };
    renderer.properties = RendererProperties::Mesh { asset: asset.id };
    renderer.renderer_type = aestra_core::RendererTypeId::new(aestra_core::RENDERER_MESH);
    renderer.material = material.id;
    let target = RendererDropTarget {
        effect: session.effect.id,
        renderer: renderer.id,
    };
    session.effect.assets.push(asset);
    session.effect.material_instances.push(material);
    f.target = DropTarget::Mesh(
        super::super::mesh_drop::MeshDropTarget::capture(session, target).unwrap(),
    );
    let before = f.effect();
    f.open();
    f.select_name("cube.gltf");
    assert_eq!(f.effect(), before);
    io::drain(f.app.world_mut());
    assert_ne!(
        f.effect(),
        before,
        "{}",
        f.app.world().resource::<EditorSession>().status
    );
    assert_eq!(f.effect().material_instances, before.material_instances);
}

#[test]
fn clicks_inside_picker_do_not_dismiss_but_backdrop_does() {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
    };
    let mut f = Fixture::new(true);
    f.open();
    let before = f.effect();
    let pending = f.app.world().resource::<Picker>().0.as_ref().unwrap();
    let overlay = pending.overlay;
    let search = pending.search;
    for target in [search, overlay] {
        f.app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
            Click {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            target,
        ));
        f.app.world_mut().flush();
        assert_eq!(
            f.app.world().resource::<Picker>().0.is_some(),
            target == search
        );
        assert_eq!(f.effect(), before);
    }
}
