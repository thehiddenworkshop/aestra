//! Bevy host warm-up: don't spend the beginning of a play-once show compiling pipelines.
use aestra_bevy::EffectPlayer;
use bevy::{
    prelude::*,
    render::{
        ExtractSchedule, MainWorld, RenderApp,
        render_resource::{CachedPipelineState, PipelineCache},
    },
    shader::ShaderCacheError,
};

#[derive(Resource, Default)]
pub struct Readiness {
    pub started: bool,
    ready: bool,
    settled: u8,
    error: Option<String>,
}

pub fn install(app: &mut App) {
    app.init_resource::<Readiness>();
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(ExtractSchedule, publish);
    }
}

fn publish(cache: Res<PipelineCache>, mut main: ResMut<MainWorld>) {
    let mut ready = false;
    let mut error = None;
    for pipeline in cache.pipelines() {
        ready = true;
        if let CachedPipelineState::Err(reason) = &pipeline.state
            && fatal(reason)
        {
            error = Some(reason.to_string());
        }
    }
    ready &= cache
        .pipelines()
        .all(|p| matches!(p.state, CachedPipelineState::Ok(_)));
    let mut state = main.resource_mut::<Readiness>();
    state.ready = ready;
    state.error = error;
}

fn fatal(reason: &ShaderCacheError) -> bool {
    // Match Bevy's retry policy: lazy shader/import arrival is preparation, not failure.
    !matches!(
        reason,
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable
    )
}

pub fn start(
    mut state: ResMut<Readiness>,
    mut players: Query<&mut EffectPlayer>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
) {
    if state.started {
        return;
    }
    if let Some(error) = &state.error {
        error!("Fireworks shader preparation failed: {error}");
        exit.write(AppExit::error());
        return;
    }
    state.settled = if state.ready {
        state.settled.saturating_add(1)
    } else {
        0
    };
    if state.settled >= 3 {
        for mut player in &mut players {
            player.playing = true;
        }
        state.started = true;
    } else if time.elapsed_secs() > 60.0 {
        error!("Fireworks shader preparation timed out");
        exit.write(AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_shader_loading_retries_but_validation_failures_are_fatal() {
        assert!(!fatal(&ShaderCacheError::ShaderNotLoaded(
            Handle::<Shader>::default().id()
        )));
        assert!(!fatal(&ShaderCacheError::ShaderImportNotYetAvailable));
        assert!(fatal(&ShaderCacheError::CreateShaderModule(
            "invalid WGSL".into()
        )));
    }

    #[test]
    fn initial_clock_stays_paused_until_three_ready_frames_and_does_not_override_later_pause() {
        let mut app = App::new();
        app.init_resource::<Readiness>()
            .init_resource::<Time>()
            .add_message::<AppExit>()
            .add_systems(Update, start);
        let mut player = EffectPlayer::new(&aestra_bevy::EffectAsset::new("test", 26.0));
        player.playing = false;
        let root = app.world_mut().spawn(player).id();
        app.update();
        assert!(!app.world().get::<EffectPlayer>(root).unwrap().playing);
        app.world_mut().resource_mut::<Readiness>().ready = true;
        app.update();
        app.update();
        assert!(!app.world().get::<EffectPlayer>(root).unwrap().playing);
        app.update();
        assert!(app.world().get::<EffectPlayer>(root).unwrap().playing);
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .playing = false;
        app.update();
        assert!(!app.world().get::<EffectPlayer>(root).unwrap().playing);
    }
}
