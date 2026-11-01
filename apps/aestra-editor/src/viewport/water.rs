//! Optional host environment, using exactly the demo's shared receiver shader.
use crate::{session::EditorSession, settings::EditorSettings};
use aestra_bevy_render::preview::water::{self, SeaState, WaterMaterial};
use bevy::{camera::visibility::RenderLayers, prelude::*};

#[derive(Component)]
pub(super) struct PreviewWater;

#[allow(clippy::type_complexity)]
pub(super) fn sync_reflections(
    mut commands: Commands,
    settings: Option<Res<EditorSettings>>,
    water: Query<Entity, With<PreviewWater>>,
    cameras: Query<
        (
            Entity,
            Option<&aestra_bevy_render::preview::planar::PlanarReflection>,
        ),
        With<super::navigation::PreviewRenderCamera>,
    >,
) {
    use aestra_bevy_render::preview::planar::PlanarReflection;
    let receiver = settings
        .filter(|s| s.preview.water_enabled && s.preview.water_reflections)
        .and_then(|_| water.single().ok());
    for (entity, current) in &cameras {
        if let Some(receiver) = receiver {
            if current.is_none_or(|c| c.receiver != receiver) {
                commands
                    .entity(entity)
                    .insert(PlanarReflection::new(receiver));
            }
        } else if current.is_some() {
            commands.entity(entity).remove::<PlanarReflection>();
        }
    }
}

pub(super) fn sync_water(
    mut commands: Commands,
    settings: Option<Res<EditorSettings>>,
    session: Res<EditorSession>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut water: Query<(&mut Visibility, &MeshMaterial3d<WaterMaterial>), With<PreviewWater>>,
) {
    let enabled = settings.is_some_and(|s| s.preview.water_enabled);
    if water.is_empty() && enabled {
        let mut material = water::material(SeaState::Rough);
        material.extension.set_time(session.simulation_time());
        commands.spawn((
            PreviewWater,
            Visibility::Inherited,
            Mesh3d(meshes.add(Plane3d::default().mesh().size(4000.0, 4000.0))),
            MeshMaterial3d(materials.add(material)),
            Transform::from_xyz(0.0, -0.2, 0.0),
            RenderLayers::layer(0),
        ));
        return;
    }
    for (mut visibility, handle) in &mut water {
        visibility.set_if_neq(if enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if enabled
            && materials
                .get(&handle.0)
                .is_some_and(|m| m.extension.time() != session.simulation_time())
            && let Some(mut material) = materials.get_mut(&handle.0)
        {
            material.extension.set_time(session.simulation_time());
        }
    }
}

#[cfg(test)]
#[path = "tests/water.rs"]
mod tests;
