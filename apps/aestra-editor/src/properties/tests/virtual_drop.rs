use super::*;
use aestra_core::{AssetDefinition, AssetId, FlipbookDefinition, RendererInstance};

#[test]
fn local_geometry_and_flipbook_assignments_preserve_materials_and_playback() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    for mesh in [false, true] {
        let mut session = crate::test_support::session_with_texture();
        let target_asset;
        if mesh {
            let first = AssetDefinition {
                id: AssetId::new(),
                name: "First".into(),
                kind: AssetKind::Mesh,
                path: "first.gltf#Mesh0/Primitive0".into(),
            };
            let second = AssetDefinition {
                id: AssetId::new(),
                name: "Second".into(),
                kind: AssetKind::Mesh,
                path: "second.gltf#Mesh0/Primitive1".into(),
            };
            target_asset = VirtualAsset::Mesh(second.id);
            let material = aestra_core::material::MaterialInstance {
                id: MaterialId::new(),
                program: aestra_core::material::MaterialProgramRef::BuiltIn(
                    aestra_core::material::MaterialProgram::DEFAULT_MESH_ID,
                ),
                values: default(),
                render_state: aestra_core::material::MaterialProgram::built_in(
                    aestra_core::material::MaterialProgramRef::BuiltIn(
                        aestra_core::material::MaterialProgram::DEFAULT_MESH_ID,
                    ),
                )
                .unwrap()
                .render_state_policy
                .default,
            };
            let renderer = &mut session.effect.emitters[0].renderers[0];
            renderer.properties = RendererProperties::Mesh { asset: first.id };
            renderer.renderer_type = aestra_core::RendererTypeId::new(aestra_core::RENDERER_MESH);
            renderer.material = material.id;
            session.effect.assets.extend([first, second]);
            session.effect.material_instances.push(material);
        } else {
            let texture = session.effect.assets[0].id;
            let first = FlipbookDefinition::grid("First", texture, 4, 1, 12.0);
            let second = FlipbookDefinition::grid("Second", texture, 2, 2, 24.0);
            target_asset = VirtualAsset::Flipbook(second.id);
            session.effect.emitters[0].renderers[0] =
                RendererInstance::flipbook(session.effect.materials[0].id, first.id);
            if let RendererProperties::Flipbook { random_start, .. } =
                &mut session.effect.emitters[0].renderers[0].properties
            {
                *random_start = true;
            }
            session.effect.flipbooks.extend([first, second]);
        }
        let target = RendererDropTarget {
            effect: session.effect.id,
            renderer: session.effect.emitters[0].renderers[0].id,
        };
        let before = session.effect.clone();
        assert!(
            plan(
                target_asset,
                DropTarget::Material(target),
                &catalog,
                &session
            )
            .is_err()
        );
        let assignment = plan(
            target_asset,
            DropTarget::Renderer(target),
            &catalog,
            &session,
        )
        .unwrap();
        assert!(session.execute_transaction(assignment.transaction.unwrap(), true));
        let after = session.effect.clone();
        assert_eq!(
            after.emitters[0].renderers[0].material,
            before.emitters[0].renderers[0].material
        );
        assert_eq!(after.assets, before.assets);
        assert_eq!(after.flipbooks, before.flipbooks);
        if let RendererProperties::Flipbook { random_start, .. } =
            after.emitters[0].renderers[0].properties
        {
            assert!(random_start);
        }
        assert!(
            plan(
                target_asset,
                DropTarget::Renderer(target),
                &catalog,
                &session
            )
            .unwrap()
            .transaction
            .is_none()
        );
        session.undo();
        assert_eq!(session.effect, before);
        session.redo();
        assert_eq!(session.effect, after);
        session
            .locks
            .lock(SemanticTarget::Renderer(target.renderer));
        assert!(
            plan(
                target_asset,
                DropTarget::Renderer(target),
                &catalog,
                &session
            )
            .is_err()
        );
    }
}
