//! Plays the editor's Pixel Pulse flipbook among ordinary Bevy sprites.
//! Run with `cargo run --locked -p aestra-bevy --example pixel_pulse`.
//! Pass `-- --stateful` to use the persistent solver with the same authored look.
//! Pass `-- --dither` for Bayer coverage or `-- --blue-noise` for blue-noise coverage.
//! Pass `-- --lightning`, `-- --ribbon`, `-- --stamps`, or `-- --beam` to play an authored path.
//! P toggles snapping for the path.
//! Up/Down changes the left pulse's Z layer. Left/Right pans by quarter pixels.
//! Y switches to sorting by world Y, where Up/Down moves the pulse across the scene.
//! T toggles 12 FPS presentation. R resets the sort policy, position, and pan.

use std::{path::PathBuf, sync::Arc};

#[cfg(test)]
use aestra_bevy::PresentedEffect;
use aestra_bevy::{
    AestraPlugin, AestraSettings, AestraSort2d, CompiledEffectProject, EffectAsset, EffectCompiler,
    EffectPlayer, PixelGrid, PixelTrailGrid, PlaybackHistoryPolicy, PresentationCadence,
    PresentationMode, integer_pixel_viewport, pixel_render_target,
};
use aestra_core::{
    dither::{BayerPattern, DitherPattern},
    material::MaterialInput,
};
use aestra_project::ProjectAssetIndex;
#[cfg(test)]
use bevy::render::render_resource::TextureFormat;
use bevy::{
    camera::{CameraUpdateSystems, RenderTarget, visibility::RenderLayers},
    prelude::*,
    transform::TransformSystems,
};

const VIRTUAL_SIZE: UVec2 = UVec2::new(192, 128);
const OUTPUT_LAYER: usize = 1;
const BACKGROUND_Z: f32 = -4.0;
const FOREGROUND_Z: f32 = 4.0;
const PULSE_Z: f32 = 0.0;

#[derive(Resource)]
struct PulseProject(Arc<CompiledEffectProject>);

#[derive(Component)]
struct MovablePulse;

#[derive(Component)]
struct PixelCamera;

#[derive(Component)]
struct OutputCamera;

#[derive(Component)]
struct PixelComposite;

fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sample-project")
}

fn compile_pulse(stateful: bool) -> Arc<CompiledEffectProject> {
    let source = std::fs::read_to_string(asset_root().join("effects/pixel_pulse.aestra.ron"))
        .expect("read the sample project's Pixel Pulse");
    let mut effect = EffectAsset::from_ron(&source).expect("parse Pixel Pulse");
    if stateful {
        effect.emitters[0]
            .modules
            .push(aestra_bevy::ModuleInstance::persistent());
    }
    compile_sample_effect(&effect)
}

fn compile_sample_effect(effect: &EffectAsset) -> Arc<CompiledEffectProject> {
    let project = ProjectAssetIndex::scan(asset_root())
        .resolve_effect_project(effect)
        .expect("resolve the sample project's effect assets");
    Arc::new(
        EffectCompiler::default()
            .compile_resolved_project(&project)
            .expect("compile the sample effect"),
    )
}

fn compile_path_sample(file_name: &str) -> Arc<CompiledEffectProject> {
    let effect = EffectAsset::load_ron(asset_root().join("effects").join(file_name))
        .expect("load the sample project's path effect");
    compile_sample_effect(&effect)
}

fn enable_dither(
    project: &mut Arc<CompiledEffectProject>,
    pattern: DitherPattern,
    opacity: f32,
    anchor: MaterialInput,
) {
    use aestra_core::material::{MaterialExpressionKind, MaterialParameterValue, MaterialValue};
    let effect = Arc::get_mut(&mut Arc::get_mut(project).expect("new pulse project").root)
        .expect("new pulse effect");
    let program = &mut effect.material_programs[0];
    let coverage = program
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Dither Coverage")
        .expect("authored dither control")
        .id;
    effect.material_instances[0].values.insert(
        coverage,
        MaterialParameterValue::Constant(MaterialValue::Float(opacity)),
    );
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.kind.bayer_pattern().is_some())
        .expect("authored Bayer node");
    let MaterialExpressionKind::BayerDither4x4 {
        alpha,
        position,
        cell_size,
    } = node.kind
    else {
        panic!("expected authored 4×4 Bayer node");
    };
    node.kind = MaterialExpressionKind::dither(pattern, alpha, position, cell_size);
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == position)
        .expect("authored dither position")
        .kind = MaterialExpressionKind::Input(anchor);
}

fn asset_plugin() -> AssetPlugin {
    AssetPlugin {
        file_path: asset_root().to_string_lossy().into_owned(),
        ..default()
    }
}

fn settings() -> AestraSettings {
    // The authored semantic sampler needs native GPU presentation.
    AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    }
}

fn main() {
    let stateful = std::env::args_os().any(|arg| arg == "--stateful");
    let path_sample = if std::env::args_os().any(|arg| arg == "--beam") {
        Some((
            "pixel_beam.aestra.ron",
            "Aestra Pixel Beam | P toggles endpoint snapping",
        ))
    } else if std::env::args_os().any(|arg| arg == "--stamps") {
        Some((
            "pixel_stamp_trail.aestra.ron",
            "Aestra Pixel Stamp Trail | P toggles stamp snapping",
        ))
    } else if std::env::args_os().any(|arg| arg == "--ribbon") {
        Some((
            "pixel_ribbon.aestra.ron",
            "Aestra Pixel Ribbon | P toggles joint snapping",
        ))
    } else if std::env::args_os().any(|arg| arg == "--lightning") {
        Some((
            "pixel_lightning.aestra.ron",
            "Aestra Pixel Lightning | P toggles joint snapping",
        ))
    } else {
        None
    };
    let mut project = path_sample.map_or_else(
        || compile_pulse(stateful),
        |(file_name, _)| compile_path_sample(file_name),
    );
    if path_sample.is_none() && std::env::args_os().any(|arg| arg == "--blue-noise") {
        enable_dither(
            &mut project,
            DitherPattern::BlueNoise,
            0.5,
            MaterialInput::EffectPositionXY,
        );
    } else if path_sample.is_none() && std::env::args_os().any(|arg| arg == "--dither") {
        enable_dither(
            &mut project,
            DitherPattern::Bayer(BayerPattern::FourByFour),
            0.5,
            MaterialInput::EffectPositionXY,
        );
    }
    let mut app = App::new();
    app.insert_resource(PulseProject(project))
        .insert_resource(settings())
        .add_plugins((
            DefaultPlugins.set(asset_plugin()).set(WindowPlugin {
                primary_window: Some(Window {
                    title: path_sample
                        .map_or(
                            "Aestra Pixel Pulse | ↑/↓ layer · ←/→ pan · T cadence · R reset",
                            |(_, title)| title,
                        )
                        .into(),
                    resolution: (768, 512).into(),
                    ..default()
                }),
                ..default()
            }),
            AestraPlugin,
        ))
        .add_systems(
            PostUpdate,
            fit_output
                .after(CameraUpdateSystems)
                .before(TransformSystems::Propagate),
        );
    if path_sample.is_some() {
        app.add_systems(Startup, (spawn_cameras, spawn_path_effect).chain())
            .add_systems(Update, toggle_path_grid);
    } else {
        app.add_systems(Startup, (spawn_cameras, spawn_layers))
            .add_systems(Update, move_effect);
    }
    app.run();
}

fn virtual_image() -> Image {
    pixel_render_target(VIRTUAL_SIZE).expect("valid virtual viewport size")
}

fn spawn_cameras(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(virtual_image());
    // Orthographic WindowSize scaling maps one world unit to one target pixel.
    commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Msaa::Off,
        PixelGrid,
        PixelCamera,
    ));
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Msaa::Off,
        RenderLayers::layer(OUTPUT_LAYER),
        OutputCamera,
    ));
    commands.spawn((
        Sprite::from_image(image),
        Visibility::Hidden,
        RenderLayers::layer(OUTPUT_LAYER),
        PixelComposite,
    ));
}

fn fit_output(
    camera: Single<&Camera, With<OutputCamera>>,
    composite: Single<(&mut Sprite, &mut Transform, &mut Visibility), With<PixelComposite>>,
) {
    let (sprite, transform, mut visibility) = composite.into_inner();
    let Some((output, viewport)) = camera.physical_target_size().and_then(|output| {
        integer_pixel_viewport(VIRTUAL_SIZE, output).map(|viewport| (output, viewport))
    }) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let size = viewport.physical_size.as_vec2();
    let center = viewport.physical_position.as_vec2() + (size - output.as_vec2()) * 0.5;
    // WindowSize projection uses logical units, while the fitted rectangle uses physical pixels.
    let scale_factor = camera.target_scaling_factor().unwrap_or(1.0);
    sprite
        .map_unchanged(|sprite| &mut sprite.custom_size)
        .set_if_neq(Some(size / scale_factor));
    transform
        .map_unchanged(|transform| &mut transform.translation)
        .set_if_neq(Vec3::new(center.x, -center.y, 0.0) / scale_factor);
    visibility.set_if_neq(Visibility::Inherited);
}

fn spawn_layers(mut commands: Commands, project: Res<PulseProject>) {
    // Normal Bevy sprites and Aestra draws share the same 2D phase.
    commands.spawn((
        Sprite::from_color(Color::srgb_u8(21, 50, 105), Vec2::new(160.0, 108.0)),
        Transform::from_xyz(0.0, 0.0, BACKGROUND_Z),
    ));
    commands.spawn((
        Sprite::from_color(Color::srgb_u8(39, 148, 110), Vec2::new(20.0, 86.0)),
        Transform::from_xyz(14.0, 0.0, FOREGROUND_Z),
    ));

    let pulse = || {
        EffectPlayer::from_project(project.0.clone())
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly)
            .with_presentation_cadence(PresentationCadence::FPS_12)
    };
    commands.spawn((
        pulse(),
        MovablePulse,
        AestraSort2d::WorldZ,
        Transform::from_xyz(-12.0, 0.0, PULSE_Z),
    ));
    let mut offset = pulse();
    offset.seek(0.25);
    commands.spawn((offset, Transform::from_xyz(18.0, 0.0, 1.0)));
}

fn spawn_path_effect(
    mut commands: Commands,
    project: Res<PulseProject>,
    camera: Single<Entity, With<PixelCamera>>,
) {
    commands.entity(*camera).insert(PixelTrailGrid);
    commands.spawn(
        EffectPlayer::from_project(project.0.clone())
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly),
    );
}

fn toggle_path_grid(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(Entity, Has<PixelTrailGrid>), With<PixelCamera>>,
) {
    if keys.just_pressed(KeyCode::KeyP) {
        let (entity, snapped) = *camera;
        if snapped {
            commands
                .entity(entity)
                .remove::<(PixelGrid, PixelTrailGrid)>();
        } else {
            commands.entity(entity).insert((PixelGrid, PixelTrailGrid));
        }
    }
}

fn move_effect(
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut EffectPlayer>,
    pulse: Single<(&mut Transform, &mut AestraSort2d), With<MovablePulse>>,
    mut camera: Single<&mut Transform, (With<PixelCamera>, Without<MovablePulse>)>,
) {
    let (mut pulse, mut sort) = pulse.into_inner();
    if keys.just_pressed(KeyCode::KeyY) {
        *sort = if matches!(*sort, AestraSort2d::WorldY { .. }) {
            AestraSort2d::WorldZ
        } else {
            AestraSort2d::WorldY { units_per_y: 3.0 }
        };
        pulse.translation.y = 0.0;
        pulse.translation.z = PULSE_Z;
    }
    if matches!(*sort, AestraSort2d::WorldY { .. }) {
        if keys.just_pressed(KeyCode::ArrowUp) {
            pulse.translation.y += 2.0;
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            pulse.translation.y -= 2.0;
        }
    } else {
        if keys.just_pressed(KeyCode::ArrowUp) {
            pulse.translation.z = FOREGROUND_Z + 1.0;
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            pulse.translation.z = BACKGROUND_Z - 1.0;
        }
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        camera.translation.x -= 0.25;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        camera.translation.x += 0.25;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        *sort = AestraSort2d::WorldZ;
        pulse.translation.y = 0.0;
        pulse.translation.z = PULSE_Z;
        camera.translation.x = 0.0;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        for mut player in &mut players {
            let cadence = player
                .presentation_cadence()
                .is_none()
                .then_some(PresentationCadence::FPS_12);
            player.set_presentation_cadence(cadence);
        }
    }
}

#[cfg(test)]
#[path = "tests/pixel_pulse.rs"]
mod tests;
