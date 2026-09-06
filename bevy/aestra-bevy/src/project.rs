use super::{EffectPlayer, EffectRenderMode, PresentedEffect};
use aestra_core::EffectClipId;
use aestra_runtime::ScheduledEffectInstance;
use bevy::{camera::visibility::RenderLayers, prelude::*};
use std::{collections::BTreeMap, sync::Arc};

/// A plugin-owned presentation following a clip path in one root player.
/// All instances are direct children of the root entity: historical ancestry is
/// already included in their immutable transform context, not Bevy parenting.
#[derive(Component)]
pub struct EffectClipInstance {
    pub root: Entity,
    pub path: Vec<EffectClipId>,
    epoch: u32,
    revision: u64,
    overrides: Vec<aestra_runtime::CompiledParameterOverride>,
}

fn configure(
    presented: &mut PresentedEffect,
    scheduled: &ScheduledEffectInstance,
    mode: EffectRenderMode,
) {
    if presented.instance.seed() != scheduled.seed {
        presented.instance.set_seed(scheduled.seed);
    }
    presented
        .instance
        .set_inherited_host_transform(scheduled.inherited.clone());
    presented.instance.set_playback_time(scheduled.time);
    presented.set_render_mode(mode);
}

pub(super) fn sync_project_instances(
    mut commands: Commands,
    roots: Query<(Entity, &EffectPlayer, Option<&RenderLayers>)>,
    mut children: Query<
        (
            Entity,
            &mut EffectClipInstance,
            &mut PresentedEffect,
            Option<&RenderLayers>,
        ),
        Without<EffectPlayer>,
    >,
) {
    let mut wanted = BTreeMap::new();
    for (root, player, layers) in &roots {
        let Some(project) = player.project() else {
            continue;
        };
        // Instance time also supports hosts supplying an external simulation clock.
        for scheduled in project.instances_with(
            player.instance.time(),
            player.instance.seed(),
            player.instance.host_transform_context(),
            |_, clip| Some(clip.clone()),
        ) {
            if scheduled.path.is_empty() {
                continue;
            }
            wanted.insert(
                (root, scheduled.path.clone()),
                (scheduled, player, layers.cloned()),
            );
        }
    }
    for (entity, mut child, mut presented, layers) in &mut children {
        let key = (child.root, child.path.clone());
        let Some((scheduled, player, desired_layers)) = wanted.get(&key) else {
            commands.entity(entity).despawn();
            continue;
        };
        if !Arc::ptr_eq(presented.effect(), &scheduled.effect) {
            // Renderer preparation owns buffers/materials. Recreate on a different
            // compiled source rather than leaving stale prepared components behind.
            commands.entity(entity).despawn();
            continue;
        }
        if child.revision != player.instance.history_revision() {
            presented.instance.invalidate_history();
        } else if child.epoch != player.instance.history_epoch() {
            presented.instance.mark_history_discontinuity();
        }
        child.epoch = player.instance.history_epoch();
        child.revision = player.instance.history_revision();
        if child.overrides != scheduled.parameter_overrides {
            for old in &child.overrides {
                let _ = presented.instance.clear_parameter(old.source);
            }
            presented
                .instance
                .apply_compiled_parameter_overrides(&scheduled.parameter_overrides);
            child.overrides = scheduled.parameter_overrides.clone();
        }
        configure(&mut presented, scheduled, player.render_mode());
        if layers != desired_layers.as_ref() {
            if let Some(layers) = desired_layers {
                commands.entity(entity).insert(layers.clone());
            } else {
                commands.entity(entity).remove::<RenderLayers>();
            }
        }
        wanted.remove(&key);
    }
    for ((root, path), (scheduled, player, layers)) in wanted {
        let mut presented = PresentedEffect::new(scheduled.effect.clone());
        presented
            .instance
            .apply_compiled_parameter_overrides(&scheduled.parameter_overrides);
        configure(&mut presented, &scheduled, player.render_mode());
        let mut entity = commands.spawn((
            ChildOf(root),
            Transform::IDENTITY,
            Visibility::Inherited,
            EffectClipInstance {
                root,
                path,
                epoch: player.instance.history_epoch(),
                revision: player.instance.history_revision(),
                overrides: scheduled.parameter_overrides.clone(),
            },
            presented,
        ));
        if let Some(layers) = layers {
            entity.insert(layers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::*;
    use aestra_runtime::{CompiledEffectProject, CompiledParameterOverride, RuntimeValue};

    fn fixture() -> Arc<CompiledEffectProject> {
        let compiler = crate::EffectCompiler::default();
        let mut leaf = EffectAsset::new("Leaf", 2.0);
        leaf.playback_mode = EffectPlaybackMode::LoopContinuous;
        leaf.emitters.push(Emitter::basic_sprite("Particle", 2.0));
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Rate".into(),
            default: Value::Scalar(3.0),
            exposed: true,
        };
        let id = parameter.id;
        leaf.parameters.push(parameter);
        leaf.emitters[0]
            .modules
            .iter_mut()
            .find(|m| m.module_type.0 == MODULE_EMISSION)
            .unwrap()
            .bindings
            .insert("spawn_rate".into(), id);
        let leaf = Arc::new(compiler.compile(&leaf).unwrap());
        let mut parent = EffectAsset::new("Parent", 4.0);
        parent.playback_mode = EffectPlaybackMode::LoopContinuous;
        let mut clip = EffectClip::new(leaf.source, 0.25, 3.75);
        clip.source_offset = 0.5;
        clip.seed = EffectClipSeed::Fixed(47);
        parent.effect_clips.push(clip);
        let mut parent = compiler.compile(&parent).unwrap();
        parent.effect_clips[0]
            .parameter_overrides
            .push(CompiledParameterOverride {
                source: id,
                slot: leaf.parameter_slots[&id],
                value: RuntimeValue::Scalar(20.0),
            });
        let parent = Arc::new(parent);
        let mut root = EffectAsset::new("Root", 6.0);
        root.playback_mode = EffectPlaybackMode::LoopContinuous;
        let mut clip = EffectClip::new(parent.source, 0.5, 5.5);
        clip.source_offset = 0.75;
        root.effect_clips.push(clip);
        Arc::new(CompiledEffectProject {
            root: Arc::new(compiler.compile(&root).unwrap()),
            dependencies: BTreeMap::from([(parent.source, parent), (leaf.source, leaf)]),
        })
    }

    fn snapshots(
        app: &mut App,
        root: Entity,
    ) -> Vec<(Entity, Vec<EffectClipId>, aestra_runtime::EffectInstance)> {
        app.world_mut()
            .query::<(Entity, &EffectClipInstance, &PresentedEffect)>()
            .iter(app.world())
            .filter(|(_, child, _)| child.root == root)
            .map(|(entity, child, presented)| {
                (entity, child.path.clone(), presented.instance.clone())
            })
            .collect()
    }

    #[test]
    fn nested_children_follow_one_clock_seek_seed_and_parameter_contract() {
        let project = fixture();
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin)
            .add_systems(Update, sync_project_instances);
        let root = app
            .world_mut()
            .spawn((
                EffectPlayer::from_project(project.clone()),
                Transform::from_xyz(10.0, 2.0, 0.0),
                RenderLayers::layer(7),
            ))
            .id();
        app.update();
        assert!(snapshots(&mut app, root).is_empty());
        let mut previous = BTreeMap::new();
        for time in [1.0, 1.5, 3.5, 5.0, 7.0, 1.0] {
            {
                let mut player = app.world_mut().get_mut::<EffectPlayer>(root).unwrap();
                player.playing = false;
                player.seek_simulation_time(time);
                player.set_seed(23);
                player.set_render_mode(EffectRenderMode::Wireframe);
            }
            app.update();
            let expected = project.instances(time, 23);
            let children = snapshots(&mut app, root);
            assert_eq!(children.len(), 2);
            for (entity, path, instance) in children {
                let scheduled = expected.iter().find(|s| s.path == path).unwrap();
                assert_eq!(instance.time(), scheduled.time);
                assert_eq!(instance.seed(), scheduled.seed);
                assert_eq!(
                    instance.host_transform_context().inherited,
                    scheduled.inherited
                );
                if path.len() == 2 {
                    assert_eq!(
                        instance.parameter(scheduled.parameter_overrides[0].source),
                        Some(&RuntimeValue::Scalar(20.0))
                    );
                }
                if let Some(old) = previous.insert(path, entity) {
                    assert_eq!(old, entity, "seeks must reuse presentations");
                }
                assert_eq!(app.world().get::<ChildOf>(entity).unwrap().parent(), root);
                assert_eq!(
                    app.world().get::<RenderLayers>(entity),
                    Some(&RenderLayers::layer(7))
                );
                assert_eq!(
                    app.world()
                        .get::<PresentedEffect>(entity)
                        .unwrap()
                        .render_mode(),
                    EffectRenderMode::Wireframe
                );
                assert!(app.world().get::<EffectPlayer>(entity).is_none());
                let epoch = instance.history_epoch();
                let revision = instance.history_revision();
                app.update();
                let paused = &app.world().get::<PresentedEffect>(entity).unwrap().instance;
                assert_eq!(
                    (
                        paused.time(),
                        paused.history_epoch(),
                        paused.history_revision()
                    ),
                    (instance.time(), epoch, revision)
                );
            }
        }
        // Live advancement preserves compatible history, unlike an explicit seek.
        let before = snapshots(&mut app, root);
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .advance_clock(0.1);
        app.update();
        for (entity, _, instance) in before {
            let now = &app.world().get::<PresentedEffect>(entity).unwrap().instance;
            assert!(now.time() > instance.time());
            assert_eq!(now.history_epoch(), instance.history_epoch());
        }
        {
            let mut player = app.world_mut().get_mut::<EffectPlayer>(root).unwrap();
            let revision = player.instance.history_revision();
            player.set_playback_time(1.25);
            assert_eq!(player.simulation_time(), player.instance.time());
            assert_eq!(player.frame(), 75);
            assert_eq!(player.instance.history_revision(), revision);
        }
        app.update();
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .restart();
        app.update();
        assert!(snapshots(&mut app, root).is_empty());
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .seek_simulation_time(1.0);
        app.update();
        assert_eq!(snapshots(&mut app, root).len(), 2);
        app.world_mut().entity_mut(root).remove::<EffectPlayer>();
        app.update();
        assert!(snapshots(&mut app, root).is_empty());
    }

    #[test]
    fn changed_overrides_layers_and_dependencies_reconcile_without_stale_instances() {
        let project = fixture();
        let mut app = App::new();
        app.add_systems(Update, sync_project_instances);
        let mut player = EffectPlayer::from_project(project.clone());
        player.seek_simulation_time(1.0);
        let root = app.world_mut().spawn((player, RenderLayers::layer(7))).id();
        app.update();
        let first = snapshots(&mut app, root);
        let leaf_entity = first.iter().find(|s| s.1.len() == 2).unwrap().0;
        let mut edited = (*project).clone();
        let parent_id = edited.root.effect_clips[0].source.id;
        let parent = Arc::make_mut(edited.dependencies.get_mut(&parent_id).unwrap());
        parent.effect_clips[0].parameter_overrides[0].value = RuntimeValue::Scalar(8.0);
        let parameter = parent.effect_clips[0].parameter_overrides[0].source;
        let mut player = EffectPlayer::from_project(Arc::new(edited));
        player.seek_simulation_time(1.0);
        app.world_mut()
            .entity_mut(root)
            .insert(player)
            .remove::<RenderLayers>();
        app.update();
        assert_eq!(snapshots(&mut app, root).len(), 2);
        assert_eq!(
            app.world()
                .get::<PresentedEffect>(leaf_entity)
                .unwrap()
                .instance
                .parameter(parameter),
            Some(&RuntimeValue::Scalar(8.0))
        );
        for (entity, _, _) in snapshots(&mut app, root) {
            assert!(app.world().get::<RenderLayers>(entity).is_none());
        }
        // Replacing project playback with the ordinary root player removes children.
        app.world_mut()
            .entity_mut(root)
            .insert(EffectPlayer::from_compiled(project.root.clone()));
        app.update();
        assert!(snapshots(&mut app, root).is_empty());
    }

    #[test]
    fn two_roots_are_isolated_and_despawning_one_removes_its_children() {
        let project = fixture();
        let mut app = App::new();
        app.add_systems(Update, sync_project_instances);
        let roots = [23, 99].map(|seed| {
            let mut player = EffectPlayer::from_project(project.clone());
            player.set_seed(seed);
            player.seek_simulation_time(1.0);
            app.world_mut().spawn(player).id()
        });
        app.update();
        let a = snapshots(&mut app, roots[0]);
        let b = snapshots(&mut app, roots[1]);
        assert_eq!((a.len(), b.len()), (2, 2));
        assert_ne!(
            a.iter().find(|s| s.1.len() == 1).unwrap().2.seed(),
            b.iter().find(|s| s.1.len() == 1).unwrap().2.seed()
        );
        app.world_mut().entity_mut(roots[0]).despawn();
        app.update();
        assert!(snapshots(&mut app, roots[0]).is_empty());
        assert_eq!(snapshots(&mut app, roots[1]).len(), 2);
    }
}
