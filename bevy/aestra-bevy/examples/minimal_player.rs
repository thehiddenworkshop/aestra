//! Minimal reference host for embedding compiled Aestra effects in a Bevy app.
//!
//! This is the canonical, copy-paste starting point for an Aestra integration —
//! including engines beyond Bevy (`aestra-godot`, `aestra-unity`, …): the ECS
//! wiring differs per host, but the *pipeline* below is the same everywhere.
//!
//! The whole client story is four steps:
//!   1. Load an authored [`EffectAsset`] (here embedded at build time).
//!   2. Resolve it against a project asset root, pulling in referenced child
//!      clips and material programs, with [`ProjectAssetIndex`].
//!   3. Compile the resolved project into a [`CompiledEffectProject`] artifact.
//!   4. Hand the artifact to an [`EffectPlayer`] and let [`AestraPlugin`] drive
//!      simulation and presentation.
//!
//! Run it from anywhere in the workspace:
//!
//! ```sh
//! cargo run -p aestra-bevy --example minimal_player
//! ```
//!
//! Controls: `Space` play/pause · `R` restart · `←`/`→` step one frame.

use std::{path::PathBuf, sync::Arc};

use aestra_bevy::{
    AestraPlugin, AestraSettings, CompiledEffectProject, EffectAsset, EffectCompiler, EffectPlayer,
};
use aestra_project::ProjectAssetIndex;
use bevy::prelude::*;

/// The authored effect, embedded so the example is self-contained. A real host
/// would load its own `.aestra.ron` (or a pre-compiled artifact) from disk.
const EFFECT_SOURCE: &str = include_str!("../../../assets/test/effects/ember_sigil.aestra.ron");

/// The compiled project artifact, kept in a resource so a startup system can
/// spawn players from it. Cloning it is cheap — it is behind an `Arc`.
#[derive(Resource, Clone)]
struct SampleProject(Arc<CompiledEffectProject>);

fn main() {
    // Steps 1–3: produce the compiled artifact before the app starts. In a
    // shipping game this happens offline and you load the artifact directly.
    let project = compile_sample_project().expect("the bundled sample effect must compile");
    let asset_root = asset_root().to_string_lossy().into_owned();

    App::new()
        .insert_resource(SampleProject(project))
        // Backend selection + particle budget. `Auto` picks the GPU path when
        // available and falls back without panicking; the default suits most hosts.
        .insert_resource(AestraSettings::default())
        .add_plugins((
            // Point Bevy's asset loader at the project's asset root so material
            // textures (e.g. `textures/ember_spark.png`) resolve.
            DefaultPlugins.set(AssetPlugin {
                file_path: asset_root,
                ..default()
            }),
            // Installs the whole client runtime: playback advance, choreography
            // dispatch, and the rendering backend.
            AestraPlugin,
        ))
        .add_systems(Startup, spawn_scene)
        .add_systems(Update, controls)
        .run();
}

fn spawn_scene(mut commands: Commands, project: Res<SampleProject>) {
    commands.spawn(Camera2d);
    // Step 4: one player per project artifact. `AestraPlugin` inserts the
    // `PresentedEffect` and drives simulation/presentation automatically — you
    // never touch the render backend directly.
    commands.spawn(EffectPlayer::from_project(project.0.clone()));
}

/// Everything a host needs to drive playback goes through the `EffectPlayer`
/// handle — the same API a game's own input or timeline would call.
fn controls(keys: Res<ButtonInput<KeyCode>>, mut players: Query<&mut EffectPlayer>) {
    for mut player in &mut players {
        if keys.just_pressed(KeyCode::Space) {
            player.playing = !player.playing;
        }
        if keys.just_pressed(KeyCode::KeyR) {
            player.restart();
        }
        if keys.just_pressed(KeyCode::ArrowLeft) {
            player.step_back();
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            player.step_forward();
        }
    }
}

/// Resolve + compile the embedded effect into a project artifact.
fn compile_sample_project() -> Result<Arc<CompiledEffectProject>, String> {
    let effect = EffectAsset::from_ron(EFFECT_SOURCE).map_err(|error| error.to_string())?;
    let root = asset_root();
    // Scans the asset root once and resolves the effect's dependency graph
    // (child clips + material programs) into a self-contained project.
    let resolved = ProjectAssetIndex::scan(&root)
        .resolve_effect_project(&effect)
        .map_err(|error| error.to_string())?;
    let project = EffectCompiler::default()
        .compile_resolved_project(&resolved)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(project))
}

/// The sample project's asset root, resolved from this crate's location so the
/// example runs from any working directory.
fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/test")
        .canonicalize()
        .expect("workspace assets/test directory must exist")
}
