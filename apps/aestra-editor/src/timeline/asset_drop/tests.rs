use super::*;
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
    EffectAsset,
) {
    let root = tempfile::tempdir().unwrap();
    let effect = EffectAsset::new("Dragged effect", 0.6);
    effect
        .save_ron(root.path().join("child.aestra.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("child.aestra.ron")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(&catalog, source);
    (root, catalog, payload, effect)
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

fn drop_on(app: &mut App, source: Entity, target: Entity, button: PointerButton) {
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        DragDrop {
            button,
            dropped: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        target,
    ));
    app.world_mut().flush();
}

#[test]
fn browser_drop_on_canvas_or_header_is_one_undoable_transaction() {
    for canvas in [true, false] {
        let (root, catalog, payload, child) = fixture();
        let bytes = std::fs::read(root.path().join("child.aestra.ron")).unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        session.material_target = if canvas {
            crate::material_document::MaterialEditingTarget::Program {
                root: root.path().to_path_buf(),
                id: aestra_core::MaterialProgramId::new(),
            }
        } else {
            crate::material_document::MaterialEditingTarget::Function {
                root: root.path().to_path_buf(),
                id: aestra_core::MaterialFunctionId::new(),
            }
        };
        session.material_history_active = true;
        let graph = session.material_target.clone();
        let original = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(TimelineState::framed(2.0))
            .init_resource::<crate::history::EditorHistoryLedger>()
            .init_resource::<crate::history::MaterialProgramEditHistory>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action)
            .add_observer(reject_project_effect_drop);
        let source = app.world_mut().spawn(payload).id();
        // Picking may report a child label, not the item itself.
        let label = app.world_mut().spawn(ChildOf(source)).id();
        let target = if canvas {
            app.world_mut()
                .spawn((
                    TimelineCanvas,
                    RelativeCursorPosition {
                        normalized: Some(Vec2::new(-0.25, 0.0)),
                        ..default()
                    },
                ))
                .observe(drop_project_effect_on_timeline)
                .id()
        } else {
            app.world_mut()
                .spawn_empty()
                .observe(drop_project_effect_on_track_headers)
                .id()
        };
        drop_on(&mut app, label, target, PointerButton::Secondary);
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        drop_on(&mut app, label, target, PointerButton::Primary);
        let session = app.world().resource::<EditorSession>();
        assert!(!session.material_history_active);
        assert_eq!(session.material_target, graph);
        assert_eq!(
            session.effect.effect_clips.len(),
            original.effect_clips.len() + 1
        );
        let clip = session.effect.effect_clips.last().unwrap();
        assert_eq!(clip.source.id, child.id);
        assert!(
            session
                .effect
                .choreography_order
                .contains(&ChoreographyTrackId::EffectClip(clip.id))
        );
        if canvas {
            assert!((clip.start_time - 0.5).abs() < 0.001);
        }
        let inserted = session.effect.clone();
        app.world_mut().trigger(crate::history::HistoryAction::Undo);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original);
        assert!(!session.can_undo());
        app.world_mut().trigger(crate::history::HistoryAction::Redo);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, inserted);
        assert_eq!(session.material_target, graph);
        assert_eq!(
            std::fs::read(root.path().join("child.aestra.ron")).unwrap(),
            bytes
        );
    }
}

#[test]
fn escape_on_release_cancels_canvas_and_header_drops() {
    for canvas in [true, false] {
        let (_root, catalog, payload, _) = fixture();
        let session = crate::test_support::session_with_timing_slack();
        let original = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(TimelineState::framed(2.0))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_observer(reject_project_effect_drop);
        let source = app.world_mut().spawn(payload).id();
        let target = if canvas {
            app.world_mut()
                .spawn((
                    TimelineCanvas,
                    RelativeCursorPosition {
                        normalized: Some(Vec2::ZERO),
                        ..default()
                    },
                ))
                .observe(drop_project_effect_on_timeline)
                .id()
        } else {
            app.world_mut()
                .spawn_empty()
                .observe(drop_project_effect_on_track_headers)
                .id()
        };
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        drop_on(&mut app, source, target, PointerButton::Primary);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original);
        assert!(!session.can_undo());
        assert!(session.status.contains("cancelled"));
    }
}

#[test]
fn browser_hover_previews_effect_and_cancel_removes_gap_without_editing() {
    let (_root, catalog, payload, child) = fixture();
    let session = crate::test_support::session_with_timing_slack();
    let original = session.effect.clone();
    let mut app = App::new();
    app.insert_resource(catalog)
        .insert_resource(session)
        .init_resource::<TimelineState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, clear_cancelled_preview);
    let source = app.world_mut().spawn(payload).id();
    let target = app
        .world_mut()
        .spawn_empty()
        .observe(show_invalid_timeline_drop_feedback)
        .id();
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        DragEnter {
            button: PointerButton::Primary,
            dragged: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        target,
    ));
    assert_eq!(
        app.world()
            .resource::<TimelineState>()
            .effect_drop_preview
            .as_ref()
            .unwrap()
            .source_duration,
        child.duration
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(
        app.world()
            .resource::<TimelineState>()
            .effect_drop_preview
            .is_none()
    );
    assert!(
        app.world()
            .resource::<TimelineState>()
            .effect_drop_insertion
            .is_none()
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    assert!(!app.world().resource::<EditorSession>().can_undo());
}

#[test]
fn document_protection_and_pending_proposal_reject_drop_without_discarding_work() {
    for pending in [false, true] {
        let (_root, catalog, payload, _) = fixture();
        let mut session = crate::test_support::session_with_timing_slack();
        let original = session.effect.clone();
        if pending {
            assert!(session.preview_transaction(EffectTransaction::single(
                "Pending rename",
                EffectCommand::SetEffectName {
                    name: "Keep proposal".into()
                },
            )));
        }
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<TimelineState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<crate::DocumentProtectionState>()
            .add_observer(reject_project_effect_drop);
        app.world_mut()
            .resource_mut::<crate::DocumentProtectionState>()
            .asset_delete_open = !pending;
        let source = app.world_mut().spawn(payload).id();
        let target = app
            .world_mut()
            .spawn_empty()
            .observe(drop_project_effect_on_track_headers)
            .id();
        drop_on(&mut app, source, target, PointerButton::Primary);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original);
        assert_eq!(session.pending_change.is_some(), pending);
        assert!(!session.can_undo());
        assert!(session.status.starts_with("Effect clip was not added:"));
    }
}

#[test]
fn incompatible_stale_ambiguous_and_cyclic_sources_are_feedback_only() {
    for case in 0..7 {
        let (root, mut catalog, mut payload, mut child) = fixture();
        let mut session = crate::test_support::session_with_timing_slack();
        let mut other_root = None;
        match case {
            0 => {
                std::fs::write(root.path().join("image.png"), b"not executable").unwrap();
                catalog.refresh();
                payload = AssetPayload::capture(
                    &catalog,
                    catalog
                        .content()
                        .source_tree()
                        .at_relative_path("image.png")
                        .unwrap()
                        .id,
                );
            }
            1 => {
                child.name = "Changed after drag".into();
                child
                    .save_ron(root.path().join("child.aestra.ron"))
                    .unwrap();
                catalog.refresh();
            }
            2 => {
                let other = tempfile::tempdir().unwrap();
                child
                    .save_ron(other.path().join("child.aestra.ron"))
                    .unwrap();
                catalog = ProjectEffectCatalog::scan(other.path());
                other_root = Some(other);
            }
            3 => {
                child
                    .save_ron(root.path().join("duplicate.aestra.ron"))
                    .unwrap();
                catalog.refresh();
                payload = AssetPayload::capture(
                    &catalog,
                    catalog
                        .content()
                        .source_tree()
                        .at_relative_path("child.aestra.ron")
                        .unwrap()
                        .id,
                );
            }
            4 => {
                session.effect.id = child.id;
            }
            5 => {
                session
                    .effect
                    .save_ron(root.path().join("owner.aestra.ron"))
                    .unwrap();
                child
                    .effect_clips
                    .push(EffectClip::new(session.effect.id, 0.0, 0.5));
                child
                    .save_ron(root.path().join("child.aestra.ron"))
                    .unwrap();
                catalog.refresh();
                payload = AssetPayload::capture(
                    &catalog,
                    catalog
                        .content()
                        .source_tree()
                        .at_relative_path("child.aestra.ron")
                        .unwrap()
                        .id,
                );
            }
            _ => {
                std::fs::remove_file(root.path().join("child.aestra.ron")).unwrap();
                catalog.refresh();
            }
        }
        let original = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<TimelineState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(reject_project_effect_drop);
        let source = app.world_mut().spawn(payload).id();
        let target = app
            .world_mut()
            .spawn_empty()
            .observe(drop_project_effect_on_track_headers)
            .id();
        drop_on(&mut app, source, target, PointerButton::Primary);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original, "case {case}");
        assert!(!session.can_undo(), "case {case}");
        assert!(
            session.status.starts_with("Effect clip was not added:"),
            "case {case}: {}",
            session.status
        );
        drop(other_root);
    }
}
