//! Project-root-scoped asset resolution shared by renderer and qualification.
use bevy::prelude::*;
use std::{collections::BTreeMap, path::PathBuf};

/// Optional filesystem root for authored effect textures and meshes. Engine/UI assets keep their own source.
/// Applications that change this root must also replace their presented effect instances.
/// Filesystem roots require `AssetPlugin::unapproved_path_mode` to be `Deny`, allowing the
/// renderer's explicit path override while ordinary asset loads remain restricted.
#[derive(Resource, Default, Clone, PartialEq, Eq)]
pub struct AestraTextureRoot(pub Option<PathBuf>);

#[derive(Resource, Default)]
pub(crate) struct ProjectAssetCache {
    handles: BTreeMap<String, bevy::prelude::Handle<Image>>,
    meshes: BTreeMap<String, bevy::prelude::Handle<bevy::prelude::Mesh>>,
    root: Option<PathBuf>,
}

pub(crate) fn sync_texture_root(
    root: Res<AestraTextureRoot>,
    mut cache: bevy::prelude::ResMut<ProjectAssetCache>,
) {
    if cache.root != root.0 {
        cache.handles.clear();
        cache.meshes.clear();
        cache.root.clone_from(&root.0);
    }
}

impl ProjectAssetCache {
    pub(crate) fn load(
        &mut self,
        asset_server: &AssetServer,
        path: &str,
    ) -> bevy::prelude::Handle<Image> {
        self.handles
            .entry(path.to_owned())
            .or_insert_with(|| load_project_asset(asset_server, self.root.as_deref(), path))
            .clone()
    }

    pub(crate) fn load_mesh(
        &mut self,
        asset_server: &AssetServer,
        path: &str,
    ) -> bevy::prelude::Handle<bevy::prelude::Mesh> {
        self.meshes
            .entry(path.to_owned())
            .or_insert_with(|| load_project_asset(asset_server, self.root.as_deref(), path))
            .clone()
    }
}

fn load_project_asset<A: bevy::asset::Asset>(
    asset_server: &AssetServer,
    root: Option<&std::path::Path>,
    path: &str,
) -> bevy::prelude::Handle<A> {
    let (file, label) = path
        .split_once('#')
        .map_or((path, None), |(file, label)| (file, Some(label)));
    let relative = std::path::Path::new(file);
    if let Some(root) = root
        && !file.contains("://")
        && relative.components().all(|part| {
            matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        let mut resolved = bevy::asset::AssetPath::from_path_buf(root.join(relative));
        if let Some(label) = label {
            resolved = resolved.with_label(label.to_owned());
        }
        return asset_server
            .load_builder()
            .override_unapproved()
            .load(resolved);
    }
    asset_server.load(path.to_owned())
}

#[cfg(test)]
mod texture_root_tests {
    use super::*;
    use bevy::asset::{AssetApp, AssetPlugin};

    #[test]
    fn switching_projects_keeps_texture_and_labeled_mesh_handles_root_scoped() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                unapproved_path_mode: bevy::asset::UnapprovedPathMode::Deny,
                ..default()
            },
        ))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_resource::<ProjectAssetCache>()
        .init_resource::<AestraTextureRoot>()
        .add_systems(Update, sync_texture_root);
        let first = std::env::current_dir().unwrap().join("project-one");
        let second = std::env::current_dir().unwrap().join("project-two");
        let server = app.world().resource::<AssetServer>().clone();
        app.world_mut().resource_mut::<AestraTextureRoot>().0 = Some(first.clone());
        app.update();
        let a = app
            .world_mut()
            .resource_mut::<ProjectAssetCache>()
            .load(&server, "textures/sprite.png");
        let mesh_a = app
            .world_mut()
            .resource_mut::<ProjectAssetCache>()
            .load_mesh(&server, "meshes/cube.gltf#Mesh0/Primitive0");
        let mesh_path = server.get_path(mesh_a.id()).unwrap();
        assert_eq!(mesh_path.path(), first.join("meshes/cube.gltf"));
        assert_eq!(mesh_path.label(), Some("Mesh0/Primitive0"));
        assert_eq!(
            server.get_path(a.id()).unwrap().path(),
            first.join("textures/sprite.png")
        );
        app.world_mut().resource_mut::<AestraTextureRoot>().0 = Some(second.clone());
        app.update();
        let b = app
            .world_mut()
            .resource_mut::<ProjectAssetCache>()
            .load(&server, "textures/sprite.png");
        let mesh_b = app
            .world_mut()
            .resource_mut::<ProjectAssetCache>()
            .load_mesh(&server, "meshes/cube.gltf#Mesh0/Primitive0");
        assert_ne!(mesh_a.id(), mesh_b.id());
        let mesh_path = server.get_path(mesh_b.id()).unwrap();
        assert_eq!(mesh_path.path(), second.join("meshes/cube.gltf"));
        assert_eq!(mesh_path.label(), Some("Mesh0/Primitive0"));
        assert_ne!(a.id(), b.id());
        assert_eq!(
            server.get_path(b.id()).unwrap().path(),
            second.join("textures/sprite.png")
        );
    }
}
