use super::*;

fn fixture(scope: SourceScope) -> (tempfile::TempDir, App) {
    let root = tempfile::tempdir().unwrap();
    let mut app = super::super::tests::browser_app(root.path());
    app.world_mut().trigger(BrowserAction::Scope(scope));
    let state = app.world().resource::<AssetBrowserState>().clone();
    let localizer = Localizer::new("en-US").unwrap();
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| spawn(parent, &state, &localizer));
    app.world_mut().flush();
    app.update();
    (root, app)
}

#[test]
fn built_in_rows_restore_previews_and_metadata_search() {
    let (_root, mut app) = fixture(SourceScope::BuiltIns);
    let catalog = aestra_compiler::MaterialCompiler.material_preset_catalog();
    let count = app
        .world_mut()
        .query::<&crate::material_graph::MaterialPresetPreviewRaster>()
        .iter(app.world())
        .count();
    assert_eq!(count, catalog.iter().count());
    let preset = catalog
        .iter()
        .find(|preset| !preset.tags.is_empty())
        .unwrap();
    app.world_mut().resource_mut::<AssetBrowserState>().query = preset.tags[0].to_uppercase();
    app.update();
    assert!(
        app.world_mut()
            .query::<&VirtualRow>()
            .iter(app.world())
            .any(|row| row.0.asset == VirtualAsset::BuiltInPreset(preset.id))
    );
}

#[test]
fn legacy_flipbook_declarations_are_visible_but_do_not_start_assignment_drags() {
    use bevy::picking::{
        backend::HitData,
        pointer::{Location, PointerId},
    };
    let (_root, mut app) = fixture(SourceScope::CurrentDocument);
    let id = aestra_core::AssetId::new();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .assets
        .push(aestra_core::AssetDefinition {
            id,
            name: "Legacy atlas".into(),
            kind: AssetKind::Flipbook,
            path: "legacy.png".into(),
        });
    app.update();
    let (entity, row) = app
        .world_mut()
        .query::<(Entity, &VirtualRow)>()
        .iter(app.world())
        .find(|(_, row)| row.0.asset == VirtualAsset::FlipbookDeclaration(id))
        .unwrap();
    assert!(row.0.description.contains("no atlas metadata"));
    app.world_mut().trigger(Activate { entity });
    assert_eq!(
        app.world().resource::<Selection>().asset,
        Some(VirtualAsset::FlipbookDeclaration(id))
    );
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(100.0, 120.0),
        },
        DragStart {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        entity,
    ));
    app.world_mut().flush();
    assert!(app.world().resource::<Drag>().origin.is_none());
    assert!(app.world().get::<AssetPayload>(entity).is_none());
}

#[test]
fn virtual_sources_are_not_files_and_filter_without_edits() {
    for scope in [SourceScope::BuiltIns, SourceScope::CurrentDocument] {
        let (_root, mut app) = fixture(scope);
        let before = app.world().resource::<EditorSession>().effect.clone();
        let rows: Vec<_> = app
            .world_mut()
            .query::<(Entity, &VirtualRow)>()
            .iter(app.world())
            .map(|(entity, row)| (entity, row.0.clone()))
            .collect();
        assert!(!rows.is_empty());
        for (entity, _) in &rows {
            assert!(
                app.world()
                    .get::<super::super::panel::BrowserRow>(*entity)
                    .is_none()
            );
            assert!(
                app.world().get::<AssetPayload>(*entity).is_none(),
                "payload only exists during a drag"
            );
        }
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .content()
                .source_tree()
                .entries()
                .count(),
            1,
            "virtual entries must not enter the source tree"
        );
        app.world_mut().trigger(Activate { entity: rows[0].0 });
        assert_eq!(
            app.world().resource::<Selection>().asset,
            Some(rows[0].1.asset)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, before);
        app.world_mut().resource_mut::<AssetBrowserState>().query = "not-a-resource".into();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&VirtualRow>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut().resource_mut::<AssetBrowserState>().query = rows[0].1.name.to_uppercase();
        app.update();
        assert!(
            app.world_mut()
                .query::<&VirtualRow>()
                .iter(app.world())
                .any(|row| row.0.asset == rows[0].1.asset)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, before);
    }
}

#[test]
fn local_creation_uses_selected_texture_and_one_undo_step() {
    let (_root, mut app) = fixture(SourceScope::CurrentDocument);
    let texture = aestra_core::AssetDefinition {
        id: aestra_core::AssetId::new(),
        name: "Chosen texture".into(),
        kind: AssetKind::Texture,
        path: "chosen.png".into(),
    };
    let texture_id = texture.id;
    app.world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .assets
        .push(texture);
    app.update();
    let button = app
        .world_mut()
        .query::<(Entity, &Create)>()
        .iter(app.world())
        .find(|(_, action)| matches!(action, Create::Flipbook))
        .unwrap()
        .0;
    assert!(app.world().get::<InteractionDisabled>(button).is_some());
    let row = app
        .world_mut()
        .query::<(Entity, &VirtualRow)>()
        .iter(app.world())
        .find(|(_, row)| row.0.asset == VirtualAsset::Texture(texture_id))
        .unwrap()
        .0;
    app.world_mut().trigger(Activate { entity: row });
    app.update();
    assert!(app.world().get::<InteractionDisabled>(button).is_none());
    let before = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().trigger(Activate { entity: button });
    let after = app.world().resource::<EditorSession>().effect.clone();
    assert_eq!(after.flipbooks.len(), before.flipbooks.len() + 1);
    assert_eq!(after.flipbooks.last().unwrap().texture, texture_id);
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.undo();
        assert_eq!(session.effect, before);
        session.redo();
        assert_eq!(session.effect, after);
    }
    app.update();
    let button = app
        .world_mut()
        .query::<(Entity, &Create)>()
        .iter(app.world())
        .find(|(_, action)| matches!(action, Create::Material))
        .unwrap()
        .0;
    app.world_mut().trigger(Activate { entity: button });
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .effect
            .materials
            .len(),
        after.materials.len() + 1
    );
    app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(app.world().resource::<EditorSession>().effect, after);
}

#[test]
fn virtual_page_is_bounded_and_local_payload_rejects_document_changes() {
    let (_root, mut app) = fixture(SourceScope::CurrentDocument);
    for index in 0..120 {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .assets
            .push(aestra_core::AssetDefinition {
                id: aestra_core::AssetId::new(),
                name: format!("Texture {index:03}"),
                kind: AssetKind::Texture,
                path: format!("{index}.png"),
            });
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&VirtualRow>()
            .iter(app.world())
            .count(),
        PAGE_SIZE
    );
    let (entity, payload) = app
        .world_mut()
        .query::<(Entity, &VirtualRow)>()
        .iter(app.world())
        .next()
        .map(|(e, row)| (e, row.1.clone()))
        .unwrap();
    app.world_mut().resource_mut::<EditorSession>().effect.id = aestra_core::EffectId::new();
    assert!(
        payload
            .check_document(app.world().resource::<EditorSession>())
            .is_err()
    );
    app.world_mut().trigger(Activate { entity });
    assert!(app.world().resource::<Selection>().asset.is_none());
    app.update();
    let next = app
        .world_mut()
        .query::<(Entity, &Page)>()
        .iter(app.world())
        .find(|(_, page)| page.0)
        .unwrap()
        .0;
    app.world_mut().trigger(Activate { entity: next });
    app.update();
    assert!(
        app.world_mut()
            .query::<&VirtualRow>()
            .iter(app.world())
            .count()
            < PAGE_SIZE
    );
}

#[test]
fn virtual_drag_has_a_nonblocking_copy_and_cancel_restores_the_row() {
    use bevy::picking::{
        backend::HitData,
        pointer::{Location, PointerId},
    };
    for cancel in [false, true] {
        let (_root, mut app) = fixture(SourceScope::CurrentDocument);
        app.init_resource::<ButtonInput<KeyCode>>();
        let row = app
            .world_mut()
            .query_filtered::<Entity, With<VirtualRow>>()
            .iter(app.world())
            .next()
            .unwrap();
        let before = app.world().get::<Node>(row).unwrap().clone();
        let effect = app.world().resource::<EditorSession>().effect.clone();
        let location = Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(100.0, 120.0),
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            DragStart {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            row,
        ));
        app.world_mut().flush();
        let preview = app
            .world()
            .resource::<Drag>()
            .preview
            .expect("visible drag copy");
        assert!(app.world().get::<AssetPayload>(row).is_some());
        assert!(app.world().get::<OverrideClip>(preview).is_some());
        assert!(
            !app.world()
                .get::<Pickable>(preview)
                .unwrap()
                .should_block_lower
        );
        assert_eq!(*app.world().get::<Node>(row).unwrap(), before);
        if cancel {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                location,
                DragEnd {
                    button: PointerButton::Primary,
                    distance: Vec2::new(20.0, 10.0),
                },
                row,
            ));
        }
        app.update();
        assert!(app.world().get_entity(preview).is_err());
        assert!(app.world().get::<AssetPayload>(row).is_none());
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    }
}

#[test]
fn narrow_virtual_views_keep_a_scrollable_viewport() {
    for view in [ViewMode::List, ViewMode::Grid] {
        let root = tempfile::tempdir().unwrap();
        let mut app =
            super::super::tests::browser_layout_app(root.path(), UVec2::new(630, 780), 1.5);
        for i in 0..100 {
            app.world_mut()
                .resource_mut::<EditorSession>()
                .effect
                .assets
                .push(aestra_core::AssetDefinition::texture(
                    format!("Texture {i:03}"),
                    format!("{i}.png"),
                ));
        }
        let root_entity = app
            .world_mut()
            .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
            .single(app.world())
            .unwrap();
        let mut state = app.world_mut().resource_mut::<AssetBrowserState>();
        state.scope = SourceScope::CurrentDocument;
        state.view = view;
        let state = state.clone();
        let localizer = Localizer::new("en-US").unwrap();
        app.world_mut()
            .commands()
            .entity(root_entity)
            .despawn_children()
            .with_children(|parent| spawn(parent, &state, &localizer));
        app.world_mut().flush();
        for _ in 0..8 {
            app.update();
        }
        let ui = app
            .world_mut()
            .query::<&VirtualUi>()
            .single(app.world())
            .unwrap();
        let node = app.world().get::<Node>(ui.list).unwrap();
        assert_eq!(node.overflow, Overflow::scroll_y());
        let computed = app.world().get::<ComputedNode>(ui.list).unwrap();
        assert!(computed.size().y > 0.0);
        assert!(
            computed.content_size().y > computed.size().y,
            "{view:?}: rows must overflow vertically, not compress or escape the viewport"
        );
        assert!(computed.size().x <= 630.0);
    }
}
