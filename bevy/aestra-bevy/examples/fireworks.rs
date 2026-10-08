//! Public host for the editor-authored fireworks show. See docs/examples/fireworks.md.
//! No viewer/benchmark implementation is imported.
#[path = "fireworks/audio.rs"]
mod audio;
#[path = "fireworks/project.rs"]
mod project;
#[path = "fireworks/readiness.rs"]
mod readiness;

use aestra_bevy::{
    AestraParticleLightPlugin, AestraParticleLightSettings, AestraPlugin, AestraRuntimeStatus,
    AestraSet, AestraSettings, AestraTransientLightPlugin, EffectPlayer, ParticleLightGpuSettings,
    ParticleLightMode, PresentationMode, SpriteSampling, TrailRasterSampling,
    TransientLightSettings, TransientLightStatistics, preview::PhotographicPreview,
};
use bevy::{
    asset::{AssetApp, io::AssetSourceBuilder},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use project::{Options, ShowProject};

#[derive(Component)]
struct AudienceCamera;
#[derive(Component)]
struct StatusText;
#[derive(Resource, Default)]
struct CaptureState {
    requested: bool,
    completed: bool,
}

fn main() -> AppExit {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", project::USAGE);
        return AppExit::Success;
    }
    let options = Options::parse(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}\n{}", project::USAGE);
        std::process::exit(2);
    });
    if options.smoke_lighting && !options.particle_smoke_lighting {
        aestra_fluid::link();
    }
    let project = options.compile().unwrap_or_else(|error| {
        eprintln!("Cannot load fireworks project: {error}");
        std::process::exit(1);
    });
    let mut app = App::new();
    // Audio is a separate source: material textures still resolve against the VFX project.
    if let Some(root) = &options.audio_root {
        app.register_asset_source(
            "fireworks-audio",
            AssetSourceBuilder::platform_default(&root.to_string_lossy(), None),
        );
    }
    app.insert_resource(ShowProject(project))
        .insert_resource(SpriteSampling {
            minimum_pixels: 2.0,
        })
        .insert_resource(TrailRasterSampling {
            minimum_pixels: 1.0,
        })
        .insert_resource(AestraSettings {
            presentation: PresentationMode::Gpu,
            transparent_order: if options.particle_smoke_lighting {
                aestra_bevy::TransparentOrderMode::DepthBackToFront
            } else {
                aestra_bevy::TransparentOrderMode::Fast
            },
            ..default()
        })
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: options.project_root.to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Aestra — Fireworks Showcase".into(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                }),
            AestraPlugin,
            AestraTransientLightPlugin,
            audio::FireworksAudioPlugin,
        ));
    let lighting = options.lighting_policy();
    // The full show remains representative-only. The saved smoke fixture authors
    // bounded star outputs; native injection never copies selected positions to CPU.
    if options.smoke_lighting {
        app.add_plugins(AestraParticleLightPlugin)
            .insert_resource(ParticleLightMode::SameFrameGpu);
    }
    lighting
        .apply(app.world_mut())
        .expect("valid lighting preset");
    readiness::install(&mut app);
    app.insert_resource(options)
        .init_resource::<CaptureState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (readiness::start, controls)
                .chain()
                .before(AestraSet::Playback),
        )
        .add_systems(Update, (status, capture).after(AestraSet::Playback))
        .run()
}

fn setup(
    mut commands: Commands,
    options: Res<Options>,
    project: Res<ShowProject>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera_transform = if options.smoke_lighting {
        Transform::from_xyz(0.0, 4.0, 16.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y)
    } else {
        Transform::from_xyz(0.0, 48.0, 180.0).looking_at(Vec3::new(0.0, 28.0, 0.0), Vec3::Y)
    };
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.001, 0.002, 0.006)),
                ..default()
            },
            camera_transform,
            AudienceCamera,
            SpatialListener::new(0.2),
        ))
        .id();
    // Same public HDR response as editor/viewer; fixed exposure, no auto adaptation.
    if !options.smoke_lighting {
        PhotographicPreview::default().apply(&mut commands.entity(camera));
    }
    if !options.smoke_lighting {
        commands.spawn((
            DirectionalLight {
                illuminance: 25.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(320.0, 320.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.035, 0.045, 0.065),
                perceptual_roughness: 0.75,
                ..default()
            })),
            Transform::from_xyz(0.0, -1.0, 0.0),
        ));
        // Non-emissive host receivers distinguish actual point lights from bloom.
        let receiver = materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.18, 0.24),
            ..default()
        });
        for x in [-65.0, 65.0] {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(8.0, 35.0, 12.0))),
                MeshMaterial3d(receiver.clone()),
                Transform::from_xyz(x, 16.5, -15.0),
            ));
        }
    }
    let mut player =
        EffectPlayer::from_project(project.0.clone()).with_history_policy(options.history);
    player.set_seed(project::SHOW_SEED);
    player.playing = false;
    commands.spawn((
        player,
        Transform::from_scale(Vec3::splat(if options.smoke_lighting { 0.1 } else { 1.0 })),
    ));
    commands.spawn((
        Text::new("AESTRA / FIREWORKS"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.82, 0.85, 0.93)),
        Node {
            position_type: PositionType::Absolute,
            left: px(18),
            top: px(16),
            ..default()
        },
        StatusText,
    ));
    commands.spawn((
        Text::new(if options.smoke_lighting {
            "Space: pause   R: restart   L: flashes + selected lights   Esc: exit"
        } else {
            "Space: pause   R: restart   1/2/3: camera   L: burst lights   M: mute   Esc: exit"
        }),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(18),
            bottom: px(16),
            ..default()
        },
    ));
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut EffectPlayer>,
    mut cameras: Query<&mut Transform, With<AudienceCamera>>,
    mut lights: ResMut<TransientLightSettings>,
    mut selection: ResMut<AestraParticleLightSettings>,
    mut gpu_lights: ResMut<ParticleLightGpuSettings>,
    options: Res<Options>,
    mut exit: MessageWriter<AppExit>,
    readiness: Res<readiness::Readiness>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    if !readiness.started {
        return;
    }
    for mut player in &mut players {
        if keys.just_pressed(KeyCode::Space) {
            player.playing = !player.playing;
        }
        if keys.just_pressed(KeyCode::KeyR) {
            player.restart();
            player.playing = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyL) {
        lights.enabled = !lights.enabled;
        let cap = if options.smoke_lighting && lights.enabled {
            options.lighting_policy().particle.max_lights
        } else {
            0
        };
        selection.max_lights = cap;
        gpu_lights.max_lights = cap;
    }
    if options.smoke_lighting {
        return;
    }
    for (key, eye, target) in [
        (
            KeyCode::Digit1,
            Vec3::new(0.0, 42.0, 145.0),
            Vec3::new(0.0, 24.0, 0.0),
        ),
        (
            KeyCode::Digit2,
            Vec3::new(0.0, 48.0, 180.0),
            Vec3::new(0.0, 28.0, 0.0),
        ),
        (
            KeyCode::Digit3,
            Vec3::new(0.0, 60.0, 235.0),
            Vec3::new(0.0, 26.0, 0.0),
        ),
    ] {
        if keys.just_pressed(key) {
            for mut camera in &mut cameras {
                *camera = Transform::from_translation(eye).looking_at(target, Vec3::Y);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn status(
    options: Res<Options>,
    players: Query<&EffectPlayer>,
    lights: Res<TransientLightStatistics>,
    mut labels: Query<&mut Text, With<StatusText>>,
    runtime: Res<AestraRuntimeStatus>,
    readiness: Res<readiness::Readiness>,
    time: Res<Time<Real>>,
    mut frame_ms: Local<f32>,
) {
    let Some(player) = players.iter().next() else {
        return;
    };
    *frame_ms += (time.delta_secs() * 1000.0 - *frame_ms) * 0.05;
    let title = if options.smoke_cohorts {
        "AESTRA / SHELL-BORN SMOKE COHORTS"
    } else if options.smoke_persistence {
        "AESTRA / SMOKE PERSISTENCE PROTOTYPE"
    } else if options.particle_smoke_lighting {
        "AESTRA / PARTICLE SMOKE LIGHTING LAB"
    } else if options.smoke_lighting {
        "AESTRA / SMOKE LIGHTING LAB"
    } else {
        "AESTRA / FIREWORKS"
    };
    for mut label in &mut labels {
        label.0 = format!(
            "{title}\n{} | {:.1} / {:.1}s | {} | {:?}\n{:?} | {:.1}ms frame (includes vsync) | burst lights: {} active / {} peak",
            options.tier,
            player.elapsed(),
            player.effect().duration,
            if !readiness.started {
                "preparing shaders"
            } else if player.playing {
                "playing"
            } else {
                "paused / finished"
            },
            options.history,
            runtime.active,
            *frame_ms,
            lights.active,
            lights.peak_active
        );
    }
}

// Existing capture hook, now waits for actual asynchronous image delivery before exiting.
#[allow(clippy::too_many_arguments)]
fn capture(
    time: Res<Time>,
    players: Query<&EffectPlayer>,
    mut commands: Commands,
    mut state: ResMut<CaptureState>,
    mut exit: MessageWriter<AppExit>,
    lights: Res<TransientLightStatistics>,
    runtime: Res<AestraRuntimeStatus>,
    gpu_lights: Option<Res<aestra_bevy::ParticleLightGpuStatistics>>,
) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    if state.completed {
        exit.write(AppExit::Success);
        return;
    }
    let seconds = std::env::var("AESTRA_EXAMPLE_CAPTURE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(5.0);
    if !state.requested
        && players
            .iter()
            .any(|p| p.elapsed() >= seconds.min(p.effect().duration))
    {
        info!(
            "Fireworks capture at {seconds}s: backend {:?}; {} authored flashes admitted, {} peak, {} invalid, {} stale",
            runtime.active,
            lights.accepted,
            lights.peak_active,
            lights.binding_invalid,
            lights.stale
        );
        if let Some(gpu_lights) = gpu_lights {
            let gpu = gpu_lights.snapshot();
            info!(
                "Smoke lighting native selection: capacity bound {} (not active count), {} adapter bytes, rejection {:?}",
                gpu.written_capacity, gpu.buffer_bytes, gpu.rejection
            );
        }
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(
                |_: On<ScreenshotCaptured>, mut state: ResMut<CaptureState>| {
                    state.completed = true;
                },
            );
        state.requested = true;
    }
    if time.elapsed_secs() > 90.0 {
        error!("Fireworks capture timed out");
        exit.write(AppExit::error());
    }
}
