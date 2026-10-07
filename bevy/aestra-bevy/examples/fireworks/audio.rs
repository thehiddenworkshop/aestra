//! Host-owned sound binding. Never turns event magnitude into a voice count.
use bevy::prelude::*;

pub struct FireworksAudioPlugin;
impl Plugin for FireworksAudioPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "fireworks-audio")]
        app.init_resource::<enabled::SoundState>()
            .add_systems(Startup, enabled::load)
            .add_systems(
                Update,
                enabled::play.after(aestra_bevy::AestraSet::Playback),
            );
        #[cfg(not(feature = "fireworks-audio"))]
        app.add_systems(Startup, || {
            info!("Sound off: build with --features fireworks-audio to bind local WAVs")
        });
    }
}

#[cfg(feature = "fireworks-audio")]
mod enabled {
    use super::*;
    use crate::{AudienceCamera, project::Options};
    use aestra_bevy::{AestraOutputEvent, EffectClipId, EffectPlayer, EventOrigin};
    use bevy::audio::{SpatialScale, Volume};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    const MAX_PENDING: usize = 64;
    const MAX_VOICES: usize = 24;
    const MAX_SEEN: usize = 4096;
    const MAX_PACKETS: usize = 256;
    const MAX_VOICE_SECONDS: f32 = 10.0;
    type CueKey = (Vec<EffectClipId>, String, usize, u64);

    #[derive(Resource, Default)]
    pub struct SoundState {
        bank: BTreeMap<&'static str, Vec<Handle<AudioSource>>>,
        epoch: Option<(Entity, u32)>,
        seen: BTreeSet<CueKey>,
        pending: VecDeque<Pending>,
        muted: bool,
        dropped: u64,
        played: u64,
        peak_sinks: usize,
    }

    struct Pending {
        delay: f32,
        position: Vec3,
        sound: Handle<AudioSource>,
    }
    #[derive(Component)]
    pub(super) struct Voice {
        epoch: u32,
        remaining: f32,
    }

    pub fn load(options: Res<Options>, server: Res<AssetServer>, mut state: ResMut<SoundState>) {
        let Some(root) = &options.audio_root else {
            return;
        };
        // Existing one-shots are illustrative break sounds, not an invented launch recording.
        for size in ["Large", "Medium", "Small"] {
            let mut handles = Vec::new();
            for variant in 1..=5 {
                let name = format!("Firework_{size}_{variant:02}.wav");
                if root.join(&name).is_file() {
                    handles.push(server.load(format!("fireworks-audio://{name}")));
                }
            }
            if handles.is_empty() {
                warn!(
                    "No {size} WAVs in {}; that sound route is silent",
                    root.display()
                );
            }
            state.bank.insert(size, handles);
        }
    }

    fn route(kind: &str) -> Option<&'static str> {
        match kind {
            "main_break" => Some("Large"),
            "secondary_break" => Some("Medium"),
            "crackle" | "crossette_split" => Some("Small"),
            _ => None,
        }
    }

    fn cue(
        output: &AestraOutputEvent,
        epoch: u32,
        now: f32,
        listener: Vec3,
    ) -> Option<(CueKey, &'static str, Vec3, f32, u64)> {
        if output.playback_epoch != Some(epoch) {
            return None;
        }
        let size = route(&output.event.kind)?;
        let spatial = output.particle.as_ref()?;
        let EventOrigin::Emitter(emitter) = output.event.origin else {
            return None;
        };
        let position = Vec3::from_array(spatial.world_position?);
        let age = now - spatial.root_time_seconds;
        if !position.is_finite()
            || !listener.is_finite()
            || !age.is_finite()
            || !(-0.05..=1.0).contains(&age)
        {
            return None;
        }
        // Occurrence time, not GPU delivery time. Approximate meters at 343m/s.
        let delay = (position.distance(listener) / 343.0 - age).max(0.0);
        if !delay.is_finite() || delay > 8.0 {
            return None;
        }
        Some((
            (
                output.clip_path.clone(),
                output.event.kind.clone(),
                emitter,
                output.event.tick,
            ),
            size,
            position,
            delay,
            spatial.seed ^ output.event.tick,
        ))
    }

    impl SoundState {
        fn synchronize(&mut self, root: Entity, epoch: u32) -> bool {
            let changed = self.epoch != Some((root, epoch));
            if changed {
                self.epoch = Some((root, epoch));
                self.seen.clear();
                self.pending.clear();
            }
            changed
        }

        fn enqueue(&mut self, output: &AestraOutputEvent, epoch: u32, now: f32, listener: Vec3) {
            let Some((key, size, position, delay, variant)) = cue(output, epoch, now, listener)
            else {
                return;
            };
            if self.seen.contains(&key) {
                return;
            }
            if self.seen.len() >= MAX_SEEN || self.pending.len() >= MAX_PENDING {
                self.dropped += 1;
                return;
            }
            self.seen.insert(key);
            let Some(sounds) = self.bank.get(size).filter(|s| !s.is_empty()) else {
                return;
            };
            self.pending.push_back(Pending {
                delay,
                position,
                sound: sounds[variant as usize % sounds.len()].clone(),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn play(
        mut commands: Commands,
        time: Res<Time>,
        keys: Res<ButtonInput<KeyCode>>,
        players: Query<(Entity, &EffectPlayer)>,
        listeners: Query<&Transform, With<AudienceCamera>>,
        mut outputs: MessageReader<AestraOutputEvent>,
        assets: Res<Assets<AudioSource>>,
        mut state: ResMut<SoundState>,
        mut voices: Query<(Entity, &mut Voice, Option<&mut SpatialAudioSink>)>,
    ) {
        let Some((root, player)) = players.iter().next() else {
            outputs.clear();
            state.pending.clear();
            state.seen.clear();
            state.epoch = None;
            for (entity, _, _) in &mut voices {
                commands.entity(entity).despawn();
            }
            return;
        };
        let epoch = player.instance().history_epoch();
        let changed = state.synchronize(root, epoch);
        if keys.just_pressed(KeyCode::KeyM) {
            state.muted = !state.muted;
        }
        // Let propagation/tails finish after Once reaches its end, but freeze during a user pause.
        let running = player.playing || player.elapsed() >= player.effect().duration;
        let dt = if running { time.delta_secs() } else { 0.0 };
        let live_sinks = voices.iter().filter(|(_, _, sink)| sink.is_some()).count();
        state.peak_sinks = state.peak_sinks.max(live_sinks);
        let mut active = 0;
        for (entity, mut voice, sink) in &mut voices {
            voice.remaining -= dt;
            if changed || voice.epoch != epoch || voice.remaining <= 0.0 {
                commands.entity(entity).despawn();
                continue;
            }
            active += 1;
            if let Some(mut sink) = sink {
                if running {
                    sink.play();
                } else {
                    sink.pause();
                }
                if state.muted {
                    sink.mute();
                } else {
                    sink.unmute();
                }
            }
        }
        for pending in &mut state.pending {
            pending.delay -= dt;
        }
        state.dropped += outputs.len().saturating_sub(MAX_PACKETS) as u64;
        if let Some(listener) = listeners.iter().next() {
            for output in outputs.read().take(MAX_PACKETS) {
                if output.effect == root {
                    if output.event.kind == aestra_bevy::EVENT_FINISHED && output.particle.is_none()
                    {
                        info!(
                            "Fireworks sound: {} voices admitted, {} dropped, {} pending; {} peak sinks",
                            state.played,
                            state.dropped,
                            state.pending.len(),
                            state.peak_sinks
                        );
                    }
                    state.enqueue(output, epoch, player.elapsed(), listener.translation);
                }
            }
        }
        outputs.clear();
        if !running {
            return;
        }
        let mut waiting = VecDeque::new();
        while let Some(pending) = state.pending.pop_front() {
            if pending.delay > 0.0 {
                waiting.push_back(pending);
                continue;
            }
            // Don't accumulate late sounds waiting for failed assets / an unavailable audio device.
            if active >= MAX_VOICES || assets.get(&pending.sound).is_none() {
                state.dropped += 1;
                continue;
            }
            commands.spawn((
                AudioPlayer::new(pending.sound),
                PlaybackSettings {
                    muted: state.muted,
                    ..PlaybackSettings::DESPAWN
                        .with_spatial(true)
                        .with_spatial_scale(SpatialScale::new(0.01))
                        .with_volume(Volume::Linear(0.6))
                },
                Transform::from_translation(pending.position),
                Voice {
                    epoch,
                    remaining: MAX_VOICE_SECONDS,
                },
            ));
            active += 1;
            state.played += 1;
        }
        state.pending = waiting;
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use aestra_bevy::{EffectId, EffectOutputEvent, ParticleOutputContext};
        fn packet(root: Entity) -> AestraOutputEvent {
            let mut output = AestraOutputEvent::root(
                root,
                EffectOutputEvent::new(
                    "main_break",
                    EventOrigin::Emitter(0),
                    "",
                    vec![0.0; 3],
                    1000.0,
                    60,
                ),
            )
            .in_epoch(7);
            output.particle = Some(ParticleOutputContext {
                source_effect: EffectId::new(),
                seed: 42,
                root_time_seconds: 1.0,
                world_position: Some([343.0, 0.0, 0.0]),
            });
            output
        }
        #[test]
        fn cue_uses_occurrence_time_and_real_position_not_delivery_time_or_particle_count() {
            let root = World::new().spawn_empty().id();
            let output = packet(root);
            let (_, size, position, delay, _) = cue(&output, 7, 1.25, Vec3::ZERO).unwrap();
            assert_eq!(size, "Large");
            assert_eq!(position.x, 343.0);
            assert_eq!(delay, 0.75);
            assert!(cue(&output, 8, 1.25, Vec3::ZERO).is_none());
            assert!(cue(&output, 7, 4.0, Vec3::ZERO).is_none());
            let mut invalid = output;
            invalid.particle.as_mut().unwrap().world_position = None;
            assert!(cue(&invalid, 7, 1.25, Vec3::ZERO).is_none());
            invalid.particle.as_mut().unwrap().world_position = Some([f32::MAX; 3]);
            assert!(cue(&invalid, 7, 1.25, Vec3::ZERO).is_none());
        }
        #[test]
        fn duplicates_are_one_voice_and_restart_clears_queued_and_seen_cues() {
            let root = World::new().spawn_empty().id();
            let output = packet(root);
            let mut state = SoundState::default();
            state.bank.insert("Large", vec![Handle::default()]);
            state.synchronize(root, 7);
            for _ in 0..100 {
                state.enqueue(&output, 7, 1.0, Vec3::ZERO);
            }
            assert_eq!(state.pending.len(), 1);
            assert_eq!(state.seen.len(), 1);
            assert!(state.synchronize(root, 8));
            assert!(state.pending.is_empty() && state.seen.is_empty());
            state.enqueue(&output, 8, 1.0, Vec3::ZERO);
            assert!(state.pending.is_empty());
        }
        #[test]
        fn admission_is_bounded_and_launch_is_explicitly_unbound() {
            let root = World::new().spawn_empty().id();
            let mut state = SoundState::default();
            state.bank.insert("Large", vec![Handle::default()]);
            for tick in 0..500 {
                let mut output = packet(root);
                output.event.tick = tick;
                state.enqueue(&output, 7, 1.0, Vec3::ZERO);
            }
            assert_eq!(state.pending.len(), MAX_PENDING);
            assert_eq!(state.dropped, 500 - MAX_PENDING as u64);
            assert_eq!(route("launch"), None);
        }

        #[test]
        fn host_system_caps_voices_mutes_at_creation_and_cleans_restart_and_despawn() {
            // No audio device or GPU: test the ECS host binding, not a simulated mixer.
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Assets<AudioSource>>()
                .init_resource::<SoundState>()
                .add_message::<AestraOutputEvent>()
                .add_systems(Update, play);
            let sound = app
                .world_mut()
                .resource_mut::<Assets<AudioSource>>()
                .add(AudioSource {
                    bytes: Vec::new().into(),
                });
            let player = EffectPlayer::new(&aestra_bevy::EffectAsset::new("test", 26.0));
            let epoch = player.instance().history_epoch();
            let root = app.world_mut().spawn(player).id();
            app.world_mut()
                .spawn((AudienceCamera, Transform::default()));
            {
                let mut state = app.world_mut().resource_mut::<SoundState>();
                state.bank.insert("Large", vec![sound]);
                state.muted = true;
            }
            for tick in 0..100 {
                let mut output = packet(root);
                output.playback_epoch = Some(epoch);
                output.event.tick = tick;
                let spatial = output.particle.as_mut().unwrap();
                spatial.root_time_seconds = 0.0;
                spatial.world_position = Some([0.0; 3]);
                app.world_mut().write_message(output);
            }
            app.update();
            let world = app.world_mut();
            assert_eq!(world.query::<&Voice>().iter(world).count(), MAX_VOICES);
            assert!(
                world
                    .query::<(&Voice, &PlaybackSettings)>()
                    .iter(world)
                    .all(|(_, s)| s.muted)
            );
            world.get_mut::<EffectPlayer>(root).unwrap().restart();
            app.update();
            let world = app.world_mut();
            assert_eq!(world.query::<&Voice>().iter(world).count(), 0);
            assert!(world.resource::<SoundState>().seen.is_empty());
            world.spawn(Voice {
                epoch,
                remaining: 1.0,
            });
            world.despawn(root);
            app.update();
            assert!(app.world().resource::<SoundState>().epoch.is_none());
            let world = app.world_mut();
            assert_eq!(world.query::<&Voice>().iter(world).count(), 0);
        }
    }
}
