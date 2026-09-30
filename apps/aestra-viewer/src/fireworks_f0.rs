//! Small, deterministic fireworks workload for F0 validation.
//!
//! This is deliberately a baseline, not a production-quality shell. Keep the
//! authored workload in one effect so later scale tests cannot pass by silently
//! splitting an over-limit event link or trail emitter into smaller pieces.

use aestra_bevy::{
    Collider, ColliderShape, ColorKey, Curve, CurveId, CurveKey, EffectAsset, EffectId,
    EffectPlaybackMode, Emitter, EmitterId, EmitterShape, EventId, EventLink, EventTrigger,
    Gradient, GradientId, ModuleId, ModuleInstance, ModuleParameters, RendererId,
    RendererProperties, ScalarRange,
};

pub const SEED: u64 = 0xf1e0_0000_0000_0001;

fn fix_emitter_ids(emitter: &mut Emitter, base: u128) {
    emitter.id = EmitterId::from_u128(base);
    for (index, module) in emitter.modules.iter_mut().enumerate() {
        module.id = ModuleId::from_u128(base + 10 + index as u128);
        if let ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } = &mut module.parameters
        {
            size.id = CurveId::from_u128(base + 40);
            opacity.id = CurveId::from_u128(base + 41);
            color.id = GradientId::from_u128(base + 42);
        }
    }
    for (index, renderer) in emitter.renderers.iter_mut().enumerate() {
        renderer.id = RendererId::from_u128(base + 60 + index as u128);
    }
}

#[allow(clippy::too_many_arguments)]
fn emitter(
    name: &str,
    capacity: u32,
    rate: f32,
    lifetime: (f32, f32),
    speed: (f32, f32),
    spread: f32,
    motion: ModuleInstance,
    size: f32,
    colors: [[f32; 4]; 3],
) -> Emitter {
    let mut emitter = Emitter::basic_sprite(name, 6.0);
    emitter.max_particles = capacity;
    emitter.modules = vec![
        ModuleInstance::emission(rate, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(lifetime.0, lifetime.1),
            ScalarRange::new(speed.0, speed.1),
            [0.0, 1.0, 0.0],
            spread,
            ScalarRange::new(0.0, 0.0),
        ),
        motion,
        ModuleInstance::appearance(
            Curve::new(vec![
                CurveKey::new(0.0, size),
                CurveKey::new(1.0, size * 0.4),
            ]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.7, 0.8),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, colors[0]),
                ColorKey::new(0.4, colors[1]),
                ColorKey::new(1.0, colors[2]),
            ]),
        ),
    ];
    emitter
}

pub fn effect() -> EffectAsset {
    let mut effect = EffectAsset::new("Fireworks F0 Validation", 6.0);
    effect.id = EffectId::from_u128(10);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut rockets = emitter(
        "Rockets",
        64,
        3.0,
        (1.1, 1.5),
        (38.0, 46.0),
        10.0,
        ModuleInstance::motion([0.0, -25.0, 0.0], 0.0, 0.0),
        1.4,
        [
            [1.0, 0.95, 0.8, 1.0],
            [1.0, 0.7, 0.3, 1.0],
            [1.0, 0.4, 0.1, 1.0],
        ],
    );
    let mut stars = emitter(
        "Stars",
        4096,
        0.0,
        (2.2, 2.8),
        (10.0, 18.0),
        180.0,
        ModuleInstance::motion([0.0, -20.0, 0.0], 0.4, 1.5),
        1.1,
        [
            [1.0, 1.0, 0.9, 1.0],
            [0.3, 0.8, 1.0, 1.0],
            [0.7, 0.2, 1.0, 1.0],
        ],
    );
    stars.modules.push(ModuleInstance::collision(vec![Collider {
        shape: ColliderShape::Plane {
            normal: [0.0, 1.0, 0.0],
            distance: 0.0,
        },
        restitution: 0.3,
        friction: 0.5,
        kill: false,
    }]));
    let mut glints = emitter(
        "Glints",
        2048,
        0.0,
        (0.3, 0.6),
        (2.0, 5.0),
        60.0,
        ModuleInstance::motion([0.0, -6.0, 0.0], 1.0, 0.0),
        0.6,
        [
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.9, 0.5, 1.0],
            [1.0, 0.6, 0.2, 1.0],
        ],
    );
    fix_emitter_ids(&mut rockets, 100);
    fix_emitter_ids(&mut stars, 200);
    fix_emitter_ids(&mut glints, 300);
    let mut burst = EventLink::new(rockets.id, EventTrigger::OnDeath, stars.id);
    burst.id = EventId::from_u128(400);
    burst.count = 48;
    burst.inherit_velocity = 0.2;
    let mut ground = EventLink::new(stars.id, EventTrigger::OnCollision, glints.id);
    ground.id = EventId::from_u128(401);
    ground.count = 2;
    effect.events = vec![burst, ground];
    effect.emitters = vec![rockets, stars, glints];
    effect
}

/// Supported-side high-fan-out probe: a full 64-particle cohort dies on one
/// tick and requests 4,096 children, beyond the fixed 1,024-entry list.
pub fn event_probe() -> EffectAsset {
    let mut effect = effect();
    effect.name = "Fireworks F0 Event Fan-Out Probe".into();
    effect.events.truncate(1);
    effect.emitters.truncate(2);
    let rockets = &mut effect.emitters[0];
    if let ModuleParameters::Emission {
        spawn_rate,
        burst_count,
    } = &mut rockets.modules[0].parameters
    {
        *spawn_rate = 3840.0;
        *burst_count = 0;
    }
    if let ModuleParameters::Initialize { lifetime, .. } = &mut rockets.modules[2].parameters {
        *lifetime = ScalarRange::new(0.5, 0.5);
    }
    effect.events[0].count = 64;
    effect
}

/// One rocket launches an authored 800-child hero burst through a single event link.
pub fn hero_event_probe() -> EffectAsset {
    let mut effect = event_probe();
    effect.name = "Fireworks F1 Hero Event Probe".into();
    effect.emitters[0].max_particles = 1;
    effect.emitters[1].max_particles = 800;
    if let ModuleParameters::Emission {
        spawn_rate,
        burst_count,
    } = &mut effect.emitters[0].modules[0].parameters
    {
        *spawn_rate = 60.0;
        *burst_count = 0;
    }
    effect.events[0].count = 800;
    effect
}

/// Original 256-parent trail benchmark, retained for before/after comparisons.
pub fn trail_probe() -> EffectAsset {
    let mut effect = EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/trail_lab.aestra.ron"
    ))
    .expect("bundled Trail Lab fixture must parse");
    effect.id = EffectId::from_u128(11);
    effect.name = "Fireworks F0 Trail Capacity Probe".into();
    effect.duration = 6.0;
    let emitter = &mut effect.emitters[0];
    emitter.duration = 6.0;
    if let ModuleParameters::Emission {
        spawn_rate,
        burst_count,
    } = &mut emitter.modules[0].parameters
    {
        *spawn_rate = 128.0;
        *burst_count = 0;
    }
    if let ModuleParameters::Initialize { lifetime, .. } = &mut emitter.modules[2].parameters {
        *lifetime = ScalarRange::new(4.0, 4.0);
    }
    emitter.max_particles = 256;
    emitter.transform.translation = [0.0, 24.0, 0.0];
    if let RendererProperties::Trail {
        max_points,
        max_trails,
        ..
    } = &mut emitter.renderers[0].properties
    {
        *max_points = 32;
        *max_trails = 256;
    }
    effect
}

/// Hero-sized single-emitter burst: no splitting into several small trail pools.
pub fn hero_trail_probe() -> EffectAsset {
    let mut effect = trail_probe();
    effect.id = EffectId::from_u128(12);
    effect.name = "Fireworks F1B Hero Trail Probe".into();
    let emitter = &mut effect.emitters[0];
    emitter.max_particles = 800;
    // Keep the burst inside the close camera for draw/overdraw measurements.
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Initialize {
                speed,
                spread_degrees,
                ..
            } => {
                *speed = ScalarRange::new(4.0, 5.0);
                *spread_degrees = 180.0;
            }
            ModuleParameters::Motion {
                gravity,
                turbulence,
                ..
            } => {
                *gravity = [0.0; 3];
                *turbulence = 0.05;
            }
            ModuleParameters::Appearance { size, .. } => {
                for key in &mut size.keys {
                    key.value = 0.3;
                }
            }
            _ => {}
        }
    }
    if let ModuleParameters::Emission {
        spawn_rate,
        burst_count,
    } = &mut emitter.modules[0].parameters
    {
        *spawn_rate = 0.0;
        *burst_count = 800;
    }
    if let RendererProperties::Trail {
        max_trails,
        sample_distance,
        ..
    } = &mut emitter.renderers[0].properties
    {
        *max_trails = 1024;
        *sample_distance = 0.15;
    }
    effect
}

/// One actual death event produces 800 stateful stars, each with its own history.
/// There is no analytic star emission and no CPU upload of manufactured heads.
pub fn event_trail_probe() -> EffectAsset {
    let mut effect = hero_event_probe();
    effect.id = EffectId::from_u128(13);
    effect.name = "Fireworks F1B Event-Born Trail Probe".into();
    effect.emitters[0].transform.translation = [0.0, 24.0, 0.0];
    let trail = hero_trail_probe();
    effect.assets = trail.assets;
    effect.material_instances = trail.material_instances;
    effect.emitters[1]
        .renderers
        .extend(trail.emitters[0].renderers.clone());
    // The capacity-one rocket emitter periodically requests another cohort.
    // Inspect the first cohort separately from later destination/pool pressure.
    for emitter in &mut effect.emitters {
        for module in &mut emitter.modules {
            if let ModuleParameters::Motion {
                gravity,
                turbulence,
                ..
            } = &mut module.parameters
            {
                *gravity = [0.0, -2.0, 0.0];
                *turbulence = 0.05;
            }
            if let ModuleParameters::Initialize { speed, .. } = &mut module.parameters {
                *speed = ScalarRange::new(4.0, 5.0);
            }
        }
    }
    effect
}

/// Sixteen simultaneous source deaths fan out into one 8,192-star trail pool.
/// No per-event count or source capture ceiling is bypassed by this scale probe.
pub fn large_event_trail_probe() -> EffectAsset {
    let mut effect = event_trail_probe();
    effect.id = EffectId::from_u128(14);
    effect.name = "Fireworks F1B Paged Event Trail Probe".into();
    effect.emitters[0].max_particles = 16;
    effect.emitters[1].max_particles = 8192;
    effect.events[0].count = 512;
    if let ModuleParameters::Emission { spawn_rate, .. } =
        &mut effect.emitters[0].modules[0].parameters
    {
        *spawn_rate = 960.0;
    }
    for renderer in &mut effect.emitters[1].renderers {
        if let RendererProperties::Trail { max_trails, .. } = &mut renderer.properties {
            *max_trails = 16384;
        }
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::EffectCompiler;

    #[test]
    fn baseline_compiles_and_is_one_effect_with_two_chained_links() {
        let fixture = effect();
        assert_eq!(
            fixture.to_pretty_ron().unwrap(),
            effect().to_pretty_ron().unwrap()
        );
        let compiled = EffectCompiler::default().compile(&fixture).unwrap();
        assert_eq!(compiled.emitters.len(), 3);
        assert_eq!(compiled.event_links.len(), 2);
        assert_eq!(compiled.event_links[0].count, 48);
    }

    #[test]
    fn hero_event_count_compiles_without_splitting_links() {
        let mut fixture = effect();
        fixture.events[0].count = 64;
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
        fixture.events[0].count = 65;
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
        fixture.events[0].count = 800;
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
        fixture.events[0].count = 801;
        assert!(EffectCompiler::default().compile(&fixture).is_err());
    }

    #[test]
    fn supported_side_fan_out_probe_fits_the_planned_expansion_list() {
        let fixture = event_probe();
        assert_eq!(fixture.events[0].count, 64);
        assert_eq!(fixture.emitters[0].max_particles, 64);
        assert_eq!(
            fixture.events[0].count * fixture.emitters[0].max_particles,
            4096
        );
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
    }

    #[test]
    fn single_link_hero_probe_compiles_at_800_children() {
        let fixture = hero_event_probe();
        assert_eq!(fixture.events.len(), 1);
        assert_eq!(fixture.events[0].count, 800);
        assert_eq!(fixture.emitters[1].max_particles, 800);
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
    }

    #[test]
    fn event_born_trail_probe_compiles_one_800_star_stateful_target() {
        let source = event_trail_probe();
        let index = aestra_project::ProjectAssetIndex::scan(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test"),
        );
        let resolved = index.resolve_effect_project(&source).unwrap();
        let compiled = EffectCompiler::default()
            .compile_resolved_project(&resolved)
            .unwrap();
        assert_eq!(compiled.root.event_links.len(), 1);
        assert_eq!(compiled.root.event_links[0].count, 800);
        assert_eq!(compiled.root.emitters[1].max_particles, 800);
        assert!(
            compiled.root.emitters[1]
                .simulation_state_layout()
                .requires_state_buffer()
        );
    }

    #[test]
    fn paged_event_probe_compiles_one_large_pool_without_bypassing_event_limits() {
        let source = large_event_trail_probe();
        let index = aestra_project::ProjectAssetIndex::scan(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test"),
        );
        let resolved = index.resolve_effect_project(&source).unwrap();
        let compiled = EffectCompiler::default()
            .compile_resolved_project(&resolved)
            .unwrap();
        assert_eq!(compiled.root.event_links.len(), 1);
        assert_eq!(
            compiled.root.event_links[0].count * compiled.root.emitters[0].max_particles,
            8192
        );
        assert_eq!(compiled.root.emitters[1].max_particles, 8192);
    }

    #[test]
    fn aggregate_event_list_budget_rejects_an_overcommitted_effect() {
        let mut fixture = effect();
        fixture.emitters[0].max_particles = 1024;
        fixture.events.clear();
        for index in 0..6 {
            let mut link = EventLink::new(
                fixture.emitters[0].id,
                EventTrigger::OnDeath,
                fixture.emitters[1].id,
            );
            link.id = EventId::from_u128(400 + index);
            link.count = 800;
            fixture.events.push(link);
        }
        fixture.events.truncate(5);
        assert!(EffectCompiler::default().compile(&fixture).is_ok());
        fixture.events.push({
            let mut link = fixture.events[0].clone();
            link.id = EventId::from_u128(405);
            link
        });
        assert!(EffectCompiler::default().compile(&fixture).is_err());
    }

    #[test]
    fn current_trail_parent_boundary_is_recorded_on_one_emitter() {
        let index = aestra_project::ProjectAssetIndex::scan(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test"),
        );
        let compiles = |effect: &EffectAsset| {
            let resolved = index.resolve_effect_project(effect).unwrap();
            EffectCompiler::default()
                .compile_resolved_project(&resolved)
                .is_ok()
        };
        let mut fixture = trail_probe();
        assert!(compiles(&fixture));
        if let RendererProperties::Trail { max_trails, .. } =
            &mut fixture.emitters[0].renderers[0].properties
        {
            *max_trails = 0;
        }
        fixture.emitters[0].max_particles = 257;
        assert!(compiles(&fixture));
        fixture.emitters[0].max_particles = 800;
        assert!(compiles(&fixture));
        fixture.emitters[0].max_particles = 1025;
        assert!(compiles(&fixture));
        fixture.emitters[0].max_particles = 8192;
        assert!(compiles(&fixture));
        let hero = hero_trail_probe();
        assert!(compiles(&hero));
        assert_eq!(hero.emitters.len(), 1);
        assert_eq!(hero.emitters[0].max_particles, 800);
    }
}
