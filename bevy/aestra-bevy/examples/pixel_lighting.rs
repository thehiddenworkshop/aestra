//! One moving light on an Aestra sprite and flipbook in Bevy's Camera2d.
//! Run with `cargo run --locked -p aestra-bevy --example pixel_lighting`.
//! Press Space to cycle continuous falloff, four bands, and off.
//! Pass `-- --normals` to use the normal-mapped sprite and flipbook.
//! Pass `-- --palette` to shade the sprite and flipbook with an ordered palette ramp.

use std::{num::NonZeroU32, path::PathBuf, sync::Arc};

use aestra_bevy::{
    AestraLights2d, AestraPlugin, AestraPointLight2d, AestraSettings, EffectAsset, EffectCompiler,
    EffectPlayer, PlaybackHistoryPolicy, PointLightFalloff2d, PresentationMode,
};
use aestra_project::ProjectAssetIndex;
use bevy::{camera::ScalingMode, prelude::*};

fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sample-project")
}

#[derive(Clone, Copy)]
enum LightingSample {
    Flat,
    NormalMapped,
    Palette,
}

fn player(sample: LightingSample) -> EffectPlayer {
    let path = match sample {
        LightingSample::Flat => "effects/pixel_lighting.aestra.ron",
        LightingSample::NormalMapped => "effects/pixel_normal_lighting.aestra.ron",
        LightingSample::Palette => "effects/pixel_palette_lighting.aestra.ron",
    };
    let effect = EffectAsset::load_ron(asset_root().join(path)).expect("load Pixel Lighting");
    let resolved = ProjectAssetIndex::scan(asset_root())
        .resolve_effect_project(&effect)
        .expect("resolve Pixel Lighting's material and atlas");
    let project = EffectCompiler::default()
        .compile_resolved_project(&resolved)
        .expect("compile Pixel Lighting");
    EffectPlayer::from_project(Arc::new(project))
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly)
}

fn asset_plugin() -> AssetPlugin {
    AssetPlugin {
        file_path: asset_root().to_string_lossy().into_owned(),
        ..default()
    }
}

fn settings() -> AestraSettings {
    AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    }
}

fn point_light(position: Vec2, falloff: PointLightFalloff2d) -> AestraPointLight2d {
    AestraPointLight2d {
        position,
        radius: 64.0,
        color: Vec3::new(1.0, 0.75, 0.28),
        intensity: 1.0,
        falloff,
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Msaa::Off,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 112.0,
            },
            ..OrthographicProjection::default_2d()
        }),
        AestraLights2d::default(),
    ));
    let sample = std::env::args_os()
        .find_map(|arg| match arg.to_str()? {
            "--normals" => Some(LightingSample::NormalMapped),
            "--palette" => Some(LightingSample::Palette),
            _ => None,
        })
        .unwrap_or(LightingSample::Flat);
    commands.spawn((player(sample), Transform::IDENTITY));
}

#[derive(Clone, Copy, Default)]
enum LightingMode {
    #[default]
    Continuous,
    Stepped,
    Off,
}

fn move_light(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    mut mode: Local<LightingMode>,
    mut camera: Single<&mut AestraLights2d>,
    mut window: Single<&mut Window>,
) {
    if input.just_pressed(KeyCode::Space) {
        let (next, label) = match *mode {
            LightingMode::Continuous => (LightingMode::Stepped, "Four bands"),
            LightingMode::Stepped => (LightingMode::Off, "Off"),
            LightingMode::Off => (LightingMode::Continuous, "Continuous"),
        };
        *mode = next;
        window.title = format!("Aestra Pixel Lighting | {label} | Space to cycle");
    }
    let falloff = match *mode {
        LightingMode::Continuous => Some(PointLightFalloff2d::Continuous),
        LightingMode::Stepped => Some(PointLightFalloff2d::Stepped(NonZeroU32::new(4).unwrap())),
        LightingMode::Off => None,
    };
    let phase = time.elapsed_secs() * 1.5;
    let position = Vec2::new(phase.cos() * 46.0, phase.sin() * 18.0);
    camera.lights.clear();
    camera
        .lights
        .extend(falloff.map(|falloff| point_light(position, falloff)));
}

fn main() {
    App::new()
        .insert_resource(settings())
        .insert_resource(ClearColor(Color::srgb_u8(8, 10, 17)))
        .add_plugins((
            DefaultPlugins
                .set(asset_plugin())
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Aestra Pixel Lighting | Continuous | Space to cycle".into(),
                        resolution: (800, 500).into(),
                        ..default()
                    }),
                    ..default()
                }),
            AestraPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, move_light)
        .run();
}

#[cfg(test)]
#[path = "tests/pixel_lighting.rs"]
mod tests;
