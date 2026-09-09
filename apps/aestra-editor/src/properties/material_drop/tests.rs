use super::*;
use aestra_core::material::MaterialProgram;
use bevy::{
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        pointer::{Location, PointerId},
    },
};

fn fixture() -> (
    tempfile::TempDir,
    ProjectEffectCatalog,
    AssetPayload,
    EditorSession,
    RendererDropTarget,
) {
    let root = tempfile::tempdir().unwrap();
    let mut program = MaterialProgram::additive_sprite("Dropped material");
    program
        .parameters
        .push(aestra_core::material::MaterialParameter {
            id: MaterialParameterId::new(),
            name: "Strength".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: aestra_core::material::MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Float(1.0)),
        });
    program
        .save_ron(root.path().join("drop.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("drop.aestra.material.ron")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(&catalog, source);
    let session = crate::test_support::session_with_timing_slack();
    let target = RendererDropTarget {
        effect: session.effect.id,
        renderer: session.effect.emitters[0].renderers[0].id,
    };
    (root, catalog, payload, session, target)
}

fn pointer_location() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        },
        position: Vec2::ZERO,
    }
}

#[test]
fn renderer_compatibility_uses_domains_not_asset_labels() {
    assert_eq!(
        renderer_domain(&RendererProperties::Sprite),
        Some(MaterialDomain::Sprite)
    );
    let flipbook =
        aestra_core::RendererInstance::flipbook(MaterialId::new(), aestra_core::AssetId::new());
    assert_eq!(
        renderer_domain(&flipbook.properties),
        Some(MaterialDomain::Sprite)
    );
    assert_eq!(
        renderer_domain(&RendererProperties::Ribbon {
            width: 1.0,
            strand_count: 1
        }),
        Some(MaterialDomain::Ribbon)
    );
    let trail = RendererProperties::Trail {
        width: 1.0,
        sample_interval: 0.02,
        lifetime: 1.0,
        max_points: 16,
        max_trails: 0,
        sampling: default(),
        sample_distance: 0.1,
        curve_tolerance: 0.1,
        uv_mode: default(),
        tile_length: 1.0,
        end_cap: default(),
    };
    assert_eq!(renderer_domain(&trail), Some(MaterialDomain::Ribbon));
    assert_eq!(
        renderer_domain(&RendererProperties::Mesh {
            asset: aestra_core::AssetId::new()
        }),
        Some(MaterialDomain::Mesh)
    );
    assert_eq!(
        renderer_domain(&RendererProperties::Custom(default())),
        None
    );
}

#[test]
fn assignment_uses_but_does_not_modify_shared_material_draft() {
    let (root, mut catalog, payload, session, target) = fixture();
    let Some(ProjectAssetId::MaterialProgram(id)) = payload.resolve(&catalog).unwrap() else {
        panic!()
    };
    let original = catalog.material_program(id).unwrap();
    let mut draft = original.clone();
    draft.name = "Unsaved material name".into();
    catalog.replace_material_program(&original, &draft).unwrap();
    let assignment = plan(&payload, target, &catalog, &session).unwrap();
    assert_eq!(assignment.label, "Assign Unsaved material name");
    assert_eq!(catalog.material_program(id).unwrap(), draft);
    assert_eq!(
        MaterialProgram::load_ron(root.path().join("drop.aestra.material.ron")).unwrap(),
        original
    );
}

#[test]
fn effects_are_rejected_by_renderer_targets_without_authoring_changes() {
    let (root, _catalog, _payload, session, target) = fixture();
    let effect = EffectAsset::new("Wrong type", 1.0);
    effect
        .save_ron(root.path().join("effect.aestra.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("effect.aestra.ron")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(&catalog, source);
    assert!(plan(&payload, target, &catalog, &session).is_err());
    assert!(!session.can_undo());
}

fn drop_on(app: &mut App, source: Entity, target: Entity) {
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        pointer_location(),
        DragDrop {
            button: PointerButton::Primary,
            dropped: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        target,
    ));
    app.world_mut().flush();
}

#[test]
fn material_drop_through_child_labels_is_one_undoable_assignment_and_preserves_source() {
    let (root, catalog, payload, mut session, target) = fixture();
    session.material_history_active = true;
    let before = session.effect.clone();
    let bytes = std::fs::read(root.path().join("drop.aestra.material.ron")).unwrap();
    let mut app = App::new();
    register(&mut app);
    app.insert_resource(catalog)
        .insert_resource(session)
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<crate::history::EditorHistoryLedger>()
        .init_resource::<crate::history::MaterialProgramEditHistory>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .add_observer(crate::history::execute_history_action);
    let source = app.world_mut().spawn(payload).id();
    let source_label = app.world_mut().spawn(ChildOf(source)).id();
    let card = app.world_mut().spawn(target).id();
    let label = app.world_mut().spawn(ChildOf(card)).id();
    drop_on(&mut app, source_label, label);
    let session = app.world().resource::<EditorSession>();
    assert!(!session.material_history_active);
    assert_eq!(
        session.effect.material_instances.len(),
        before.material_instances.len() + 1
    );
    let instance = session.effect.material_instances.last().unwrap();
    assert_eq!(
        session.effect.emitters[0].renderers[0].material,
        instance.id
    );
    let after = session.effect.clone();
    app.world_mut().trigger(crate::history::HistoryAction::Undo);
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert!(!app.world().resource::<EditorSession>().can_undo());
    app.world_mut().trigger(crate::history::HistoryAction::Redo);
    assert_eq!(app.world().resource::<EditorSession>().effect, after);
    assert_eq!(
        std::fs::read(root.path().join("drop.aestra.material.ron")).unwrap(),
        bytes
    );
    let saved = root.path().join("assigned.aestra.ron");
    after.save_ron(&saved).unwrap();
    assert_eq!(EffectAsset::load_ron(saved).unwrap(), after);
}

#[test]
fn repeat_drop_is_noop_and_other_renderers_reuse_only_default_instances() {
    for customized in [false, true] {
        let (_root, catalog, payload, mut session, target) = fixture();
        let transaction = plan(&payload, target, &catalog, &session)
            .unwrap()
            .transaction
            .unwrap();
        assert!(session.execute_transaction(transaction, true));
        let id = session.effect.material_instances[0].id;
        if customized {
            let Some(ProjectAssetId::MaterialProgram(program)) = payload.resolve(&catalog).unwrap()
            else {
                panic!()
            };
            let parameter = catalog.material_program(program).unwrap().parameters[0].id;
            session.effect.material_instances[0].values.insert(
                parameter,
                aestra_core::material::MaterialParameterValue::Constant(MaterialValue::Float(0.25)),
            );
        }
        let before = session.effect.clone();
        assert!(
            plan(&payload, target, &catalog, &session)
                .unwrap()
                .transaction
                .is_none()
        );
        assert_eq!(session.effect, before);
        let other = RendererDropTarget {
            renderer: session.effect.emitters[1].renderers[0].id,
            ..target
        };
        let transaction = plan(&payload, other, &catalog, &session)
            .unwrap()
            .transaction
            .unwrap();
        assert!(session.execute_transaction(transaction, true));
        assert_eq!(
            session.effect.material_instances.len(),
            if customized { 2 } else { 1 }
        );
        assert_eq!(
            session.effect.emitters[1].renderers[0].material == id,
            !customized
        );
        assert_eq!(
            session.effect.material_instances[0],
            before.material_instances[0]
        );
        session.undo();
        assert_eq!(session.effect, before);
    }
}

#[test]
fn invalid_targets_locks_pending_and_stale_sources_do_not_mutate() {
    for case in 0..6 {
        let (_root, mut catalog, payload, mut session, mut target) = fixture();
        match case {
            0 => target.effect = aestra_core::EffectId::new(),
            1 => target.renderer = RendererId::new(),
            2 => session
                .locks
                .lock(SemanticTarget::Renderer(target.renderer)),
            3 => {
                session.preview_transaction(EffectTransaction::single(
                    "Pending",
                    EffectCommand::SetEffectName {
                        name: "Keep".into(),
                    },
                ));
            }
            4 => {
                session.effect.emitters[0].renderers[0].properties = RendererProperties::Ribbon {
                    width: 1.0,
                    strand_count: 1,
                }
            }
            _ => {
                let elsewhere = tempfile::tempdir().unwrap();
                catalog = ProjectEffectCatalog::scan(elsewhere.path());
            }
        }
        let before = session.effect.clone();
        assert!(
            plan(&payload, target, &catalog, &session).is_err(),
            "case {case}"
        );
        assert_eq!(session.effect, before);
        assert!(!session.can_undo());
    }
}

#[test]
fn hover_is_non_mutating_feedback_and_escape_or_drag_end_cleans_up() {
    for cancelled in [false, true] {
        let (_root, catalog, payload, session, target) = fixture();
        let before = session.effect.clone();
        let mut app = App::new();
        register(&mut app);
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<ButtonInput<KeyCode>>();
        let source = app.world_mut().spawn(payload).id();
        let card = app.world_mut().spawn(target).id();
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            pointer_location(),
            DragEnter {
                button: PointerButton::Primary,
                dragged: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            card,
        ));
        app.world_mut().flush();
        assert_eq!(
            app.world_mut()
                .query::<&DropFeedback>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, before);
        if cancelled {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            app.world_mut().entity_mut(source).remove::<AssetPayload>();
        }
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&DropFeedback>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, before);
        assert!(!app.world().resource::<EditorSession>().can_undo());
    }
}

#[test]
fn document_operation_blocks_drop_without_mutation() {
    let (_root, catalog, payload, session, target) = fixture();
    let before = session.effect.clone();
    let mut app = App::new();
    register(&mut app);
    app.insert_resource(catalog)
        .insert_resource(session)
        .init_resource::<crate::DocumentProtectionState>();
    app.world_mut()
        .resource_mut::<crate::DocumentProtectionState>()
        .asset_delete_open = true;
    let source = app.world_mut().spawn(payload).id();
    let card = app.world_mut().spawn(target).id();
    drop_on(&mut app, source, card);
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, before);
    assert!(!session.can_undo());
    assert!(session.status.starts_with("Material drop rejected:"));
}
