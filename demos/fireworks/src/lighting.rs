//! Independent host policy; toggles never alter compiled art or gameplay.
use super::{project::Options, readiness::Readiness, report::Report};
use aestra_bevy::{AestraSet, LightingQualityPolicy};
use bevy::prelude::*;

#[derive(Resource)]
pub struct Lighting(pub LightingQualityPolicy, bool);

pub fn install(app: &mut App, policy: LightingQualityPolicy) {
    app.insert_resource(Lighting(policy, false)).add_systems(
        Update,
        (keyboard, apply)
            .chain()
            .after(super::controls)
            .before(AestraSet::ResolveHostInputs),
    );
}
fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    options: Res<Options>,
    readiness: Res<Readiness>,
    report: Option<Res<Report>>,
    mut state: ResMut<Lighting>,
) {
    if !readiness.started || report.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        state.0.representative.enabled = !state.0.representative.enabled;
        state.1 = true;
    }
    if keys.just_pressed(KeyCode::KeyP) && (options.star_lights || options.smoke_lighting) {
        state.0.particle.enabled = !state.0.particle.enabled;
        state.1 = true;
    }
}
fn apply(world: &mut World) {
    if world.resource::<Lighting>().1 {
        let policy = world.resource::<Lighting>().0;
        policy.apply(world).expect("validated lighting policy");
        world.resource_mut::<Lighting>().1 = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{
        AestraParticleLightSettings, ParticleLightGpuSettings, ParticleLightReadbackSettings,
        TransientLightSettings,
    };
    #[test]
    fn families_toggle_independently_and_all_particle_paths_share_the_cap() {
        let mut app = App::new();
        let options = Options::parse(["--star-lights".into()]).unwrap();
        let policy = options.lighting_policy();
        policy.apply(app.world_mut()).unwrap();
        app.insert_resource(options)
            .init_resource::<Readiness>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Lighting(policy, false))
            .add_systems(Update, (keyboard, apply).chain());
        app.world_mut().resource_mut::<Readiness>().started = true;
        app.update();
        for key in [KeyCode::KeyL, KeyCode::KeyP, KeyCode::KeyL, KeyCode::KeyP] {
            let before = app.world().resource::<Lighting>().0;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let world = app.world();
            let after = world.resource::<Lighting>().0;
            assert_eq!(
                after.representative.enabled,
                if key == KeyCode::KeyL {
                    !before.representative.enabled
                } else {
                    before.representative.enabled
                }
            );
            assert_eq!(
                after.particle.enabled,
                if key == KeyCode::KeyP {
                    !before.particle.enabled
                } else {
                    before.particle.enabled
                }
            );
            let cap = if after.particle.enabled {
                after.particle.max_lights
            } else {
                0
            };
            assert_eq!(
                world.resource::<AestraParticleLightSettings>().max_lights,
                cap
            );
            assert_eq!(world.resource::<ParticleLightGpuSettings>().max_lights, cap);
            assert_eq!(
                world.resource::<ParticleLightReadbackSettings>().max_lights,
                cap
            );
            assert_eq!(
                world.resource::<TransientLightSettings>().enabled,
                after.representative.enabled
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
    }
}
