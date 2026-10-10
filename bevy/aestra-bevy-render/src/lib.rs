//! Shared Bevy/WGPU presentation backend for Aestra effects.
//!
//! This crate owns rendering infrastructure, not playback lifecycle. Applications feed it
//! [`PresentedEffect`] components; `aestra-bevy` and `aestra-editor` remain independent owners of
//! runtime and preview behavior.

mod capabilities;
mod cpu;
pub mod execution;
pub mod gpu;
mod host_transform;
pub mod material;
mod material_layout;
mod presented_effect;
pub use presented_effect::{EffectRenderMode, PresentedEffect};
pub mod preview;
mod render_settings;
pub mod sampling;
pub use render_settings::{AestraRenderSettings, PresentationMode, TransparentOrderMode};

pub use aestra_runtime::{
    BackendCapabilities, CompatibilityIssue, CompatibilityIssueCode, CompatibilityReport,
    CompatibilityTarget, EffectRequirements, PlaybackHistoryPolicy, RendererCapability,
};
pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{App, AssetServer, Component, Entity, Image, Plugin, Res, Resource, Update, Without},
};
use std::{collections::BTreeMap, path::PathBuf};

/// Scheduling point applications use to update [`PresentedEffect`] before rendering consumes it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AestraRenderSet {
    Prepare,
}

#[derive(Default)]
pub struct AestraRenderPlugin;

impl Plugin for AestraRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            bevy::prelude::PostUpdate,
            cpu::sync_particle_globals
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
        app.init_resource::<AestraRenderSettings>()
            .init_resource::<sampling::SpriteSampling>()
            .init_resource::<sampling::TrailRasterSampling>()
            .init_resource::<AestraTextureRoot>()
            .init_resource::<GpuCapabilities>()
            .init_resource::<AestraRuntimeStatus>()
            .init_resource::<ProjectAssetCache>()
            .add_observer(gpu::receive_readback);
        gpu::install(app);
        app.add_systems(
            Update,
            (
                ensure_aestra_depth_prepass,
                sync_texture_root,
                assign_effect_backends,
                cpu::prepare_cpu_effects,
                gpu::prepare_gpu_effects,
                cpu::present_cpu_effects,
            )
                .chain()
                .in_set(AestraRenderSet::Prepare),
        );
    }
}

/// Aestra's semantic scene-depth inputs sample Bevy's separate 3D prepass
/// texture. Enabling it on 3D cameras avoids the invalid feedback loop that
/// would result from sampling the active main-pass depth attachment.
fn ensure_aestra_depth_prepass(
    mut commands: bevy::prelude::Commands,
    cameras: bevy::prelude::Query<
        bevy::prelude::Entity,
        (
            bevy::prelude::With<bevy::prelude::Camera3d>,
            bevy::prelude::Without<bevy::core_pipeline::prepass::DepthPrepass>,
        ),
    >,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(bevy::core_pipeline::prepass::DepthPrepass);
    }
}

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

fn sync_texture_root(
    root: Res<AestraTextureRoot>,
    mut cache: bevy::prelude::ResMut<ProjectAssetCache>,
) {
    if cache.root != root.0 {
        cache.handles.clear();
        cache.meshes.clear();
        cache.root.clone_from(&root.0);
    }
}

#[cfg(test)]
mod texture_root_tests {
    use super::*;
    use bevy::{
        asset::{AssetApp, AssetPlugin},
        prelude::*,
    };

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

#[derive(Component)]
pub(crate) struct GpuPresentationPrepared;

fn assign_effect_backends(
    mut commands: bevy::prelude::Commands,
    settings: Res<AestraRenderSettings>,
    runtime: Res<AestraRuntimeStatus>,
    capabilities: Res<GpuCapabilities>,
    effects: bevy::prelude::Query<(Entity, &PresentedEffect), Without<EffectRuntimeStatus>>,
) {
    if runtime.active == ActiveBackend::Pending {
        return;
    }
    let backend = capabilities.backend_capabilities(settings.max_gpu_particles);
    for (entity, effect) in &effects {
        commands
            .entity(entity)
            .insert(capabilities::select_effect_backend(
                &runtime,
                &effect.effect().requirements,
                &backend,
            ));
    }
}
