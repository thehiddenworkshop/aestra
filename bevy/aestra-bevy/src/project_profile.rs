use super::*;
use aestra_runtime::{ProjectInstanceProfile, ProjectProfile};
use bevy::prelude::*;

/// Active root + child costs. `EffectProfiler` remains the root-only API.
#[derive(Component, Debug, Clone, Default)]
pub struct ProjectProfiler(pub ProjectProfile);

#[allow(clippy::type_complexity)]
pub(super) fn update_project_profiles(
    mut commands: Commands,
    mut roots: Query<(
        Entity,
        &EffectPlayer,
        &PresentedEffect,
        &EffectRuntimeStatus,
        &mut EffectProfiler,
        Option<&mut ProjectProfiler>,
        Option<&gpu::GpuTrailStatistics>,
    )>,
    children: Query<
        (
            &EffectClipInstance,
            &PresentedEffect,
            Option<&EffectRuntimeStatus>,
            Option<&gpu::GpuTrailStatistics>,
        ),
        Without<EffectPlayer>,
    >,
    capabilities: Res<GpuCapabilities>,
    orphaned: Query<Entity, (With<ProjectProfiler>, Without<EffectPlayer>)>,
) {
    for entity in &orphaned {
        commands.entity(entity).remove::<ProjectProfiler>();
    }
    let mut by_root: BTreeMap<Entity, Vec<ProjectInstanceProfile>> = BTreeMap::new();
    for (child, presented, runtime, trails) in &children {
        let mut profile = runtime.map_or_else(
            || EffectProfile::from_compiled(presented.effect()),
            |runtime| bevy_profile(presented.effect(), &capabilities, runtime),
        );
        if let Some(runtime) = runtime {
            record_presented_profile(
                &mut profile,
                presented.effect(),
                presented.samples(),
                presented.cpu_evaluation_time(),
                runtime.active,
            );
        }
        profile.record_trail_usage(trails.and_then(|s| s.usage(&presented.instance)));
        by_root
            .entry(child.root)
            .or_default()
            .push(ProjectInstanceProfile {
                path: child.path.clone(),
                effect: presented.effect().source,
                name: presented.effect().name.clone(),
                profile,
            });
    }
    for (entity, player, presented, runtime, mut root_profile, project, trails) in &mut roots {
        record_presented_profile(
            &mut root_profile.0,
            presented.effect(),
            presented.samples(),
            presented.cpu_evaluation_time(),
            runtime.active,
        );
        root_profile
            .0
            .record_trail_usage(trails.and_then(|s| s.usage(&presented.instance)));
        let mut entries = vec![ProjectInstanceProfile {
            path: Vec::new(),
            effect: player.effect().source,
            name: player.effect().name.clone(),
            profile: root_profile.0.clone(),
        }];
        if player.project().is_some() {
            entries.extend(by_root.remove(&entity).unwrap_or_default());
        }
        if let Some(mut project) = project {
            project.0.update(entries);
        } else {
            let mut project = ProjectProfiler::default();
            project.0.update(entries);
            commands.entity(entity).insert(project);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_profiles_follow_nested_lifecycle_and_isolate_roots() {
        let compiler = EffectCompiler::default();
        let mut leaf = EffectAsset::new("Leaf", 1.0);
        leaf.emitters.push(Emitter::basic_sprite("Particles", 1.0));
        let leaf = Arc::new(compiler.compile(&leaf).unwrap());
        let mut carrier = EffectAsset::new("Carrier", 2.0);
        carrier
            .effect_clips
            .push(EffectClip::new(leaf.source, 0.0, 1.0));
        let carrier = Arc::new(compiler.compile(&carrier).unwrap());
        let mut root = EffectAsset::new("Root", 3.0);
        root.playback_mode = EffectPlaybackMode::LoopContinuous;
        root.effect_clips
            .push(EffectClip::new(carrier.source, 0.5, 1.0));
        root.effect_clips
            .push(EffectClip::new(leaf.source, 0.5, 1.0));
        let project = Arc::new(CompiledEffectProject {
            root: Arc::new(compiler.compile(&root).unwrap()),
            dependencies: BTreeMap::from([(carrier.source, carrier), (leaf.source, leaf.clone())]),
        });
        let mut app = App::new();
        app.init_resource::<GpuCapabilities>().add_systems(
            Update,
            (
                prepare_player_presentations,
                prepare_effect_profiles,
                project::sync_project_instances,
                sync_player_presentations,
                update_project_profiles,
            )
                .chain(),
        );
        let spawn = |app: &mut App| {
            app.world_mut()
                .spawn((
                    EffectPlayer::from_project(project.clone()),
                    EffectRuntimeStatus {
                        active: ActiveBackend::Gpu,
                        reason: "test".into(),
                        compatibility: CompatibilityReport::compatible(
                            CompatibilityTarget::NativeGpu,
                        ),
                    },
                ))
                .id()
        };
        let first = spawn(&mut app);
        let second = spawn(&mut app);
        for (time, count) in [(0.0, 1), (1.0, 4), (2.0, 1), (4.0, 4), (0.0, 1)] {
            app.world_mut()
                .get_mut::<EffectPlayer>(first)
                .unwrap()
                .seek_simulation_time(time);
            app.update();
            let profile = &app.world().get::<ProjectProfiler>(first).unwrap().0;
            assert_eq!(profile.instances.len(), count);
            assert_eq!(
                profile.total.particle_capacity.value(),
                Some(if count == 4 {
                    2 * leaf.max_particles as u32
                } else {
                    0
                })
            );
            assert_eq!(
                app.world()
                    .get::<ProjectProfiler>(second)
                    .unwrap()
                    .0
                    .instances
                    .len(),
                1
            );
            if count == 4 {
                assert_eq!(profile.total.alive_particles, ProfileValue::Unavailable);
                assert_eq!(
                    profile
                        .instances
                        .iter()
                        .filter(|i| i.effect == leaf.source)
                        .count(),
                    2
                );
                assert_eq!(
                    app.world()
                        .get::<EffectProfiler>(first)
                        .unwrap()
                        .0
                        .particle_capacity
                        .value(),
                    Some(0)
                );
            }
        }
        app.world_mut().entity_mut(first).despawn();
        app.update();
        assert_eq!(
            app.world()
                .get::<ProjectProfiler>(second)
                .unwrap()
                .0
                .instances
                .len(),
            1
        );
        app.world_mut().entity_mut(second).remove::<EffectPlayer>();
        app.update();
        assert!(app.world().get::<ProjectProfiler>(second).is_none());
    }
}
