use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, Emitter, EventLink, EventTrigger, ParticleBudgetProfile, RendererProperties,
};
use aestra_runtime::QualityTier;
use std::collections::BTreeMap;

fn fixture() -> EffectAsset {
    let mut source = EffectAsset::new("Tier budgets", 3.0);
    let mut parent = Emitter::basic_sprite("Parent", 3.0);
    parent.max_particles = 4;
    let mut child = Emitter::basic_sprite("Child", 3.0);
    child.max_particles = 64;
    child.renderers[0].renderer_type =
        aestra_core::RendererTypeId(aestra_core::RENDERER_TRAIL.into());
    child.renderers[0].properties = RendererProperties::Trail {
        max_points: 32,
        max_trails: 64,
        sample_interval: 1.0 / 30.0,
        lifetime: 0.5,
        width: 0.1,
        sampling: Default::default(),
        sample_distance: 0.01,
        curve_tolerance: 0.01,
        uv_mode: Default::default(),
        tile_length: 1.0,
        end_cap: Default::default(),
    };
    let mut link = EventLink::new(parent.id, EventTrigger::OnDeath, child.id);
    link.count = 16;
    source.particle_budgets.insert(
        "low".into(),
        ParticleBudgetProfile {
            emitter_capacity: BTreeMap::from([(child.id, 16)]),
            event_count: BTreeMap::from([(link.id, 4)]),
            trail_capacity: BTreeMap::from([(child.renderers[0].id, 16)]),
        },
    );
    source.emitters = vec![parent, child];
    source.events.push(link);
    source
}

#[test]
fn explicit_profiles_round_trip_lower_together_and_never_rewrite_the_source() {
    let source = fixture();
    let original = source.clone();
    assert_eq!(
        EffectAsset::from_ron(&source.to_pretty_ron().unwrap()).unwrap(),
        source
    );
    let high = EffectCompiler::default().compile(&source).unwrap();
    let low = EffectCompiler::default()
        .with_tier(QualityTier::low())
        .compile(&source)
        .unwrap();
    assert_eq!(high.max_particles, 68);
    assert_eq!(low.max_particles, 20);
    assert_eq!(low.requirements.max_particles, 20);
    assert_eq!(high.event_links[0].count, 16);
    assert_eq!(low.event_links[0].count, 4);
    assert_eq!(source, original);
    assert_eq!(source.with_particle_budget_profile("medium"), source);
    let mut legacy = source.clone();
    legacy.particle_budgets.clear();
    assert_eq!(high, EffectCompiler::default().compile(&legacy).unwrap());
    assert!(matches!(
        low.emitters[1].renderers[0].kind,
        aestra_runtime::RendererPlanKind::Trail { max_trails: 16, .. }
    ));
}

#[test]
fn explicit_budget_is_checked_before_event_list_resource_lowering() {
    let mut source = fixture();
    source.emitters[0].max_particles = 1024;
    source.emitters[1].max_particles = 800;
    source.events[0].count = 800;
    source.events = (0..6)
        .map(|_| {
            let mut link = source.events[0].clone();
            link.id = aestra_core::EventId::new();
            link
        })
        .collect();
    source.particle_budgets.get_mut("low").unwrap().event_count =
        source.events.iter().map(|e| (e.id, 4)).collect();
    // Six worst-case expanded lists exceed 128 MiB at authored quality.
    assert!(EffectCompiler::default().compile(&source).is_err());
    let low = EffectCompiler::default()
        .with_tier(QualityTier::low())
        .compile(&source)
        .unwrap();
    assert_eq!(low.max_particles, 1040);
    assert!(low.event_links.iter().all(|e| e.count == 4));
    // A tier profile cannot waive resource checks: omitting count reductions fails.
    source
        .particle_budgets
        .get_mut("low")
        .unwrap()
        .event_count
        .clear();
    assert!(
        EffectCompiler::default()
            .with_tier(QualityTier::low())
            .compile(&source)
            .is_err()
    );
}

#[test]
fn profiles_reject_missing_wrong_kind_zero_increased_and_high_targets() {
    let source = fixture();
    for (name, profile) in [
        ("high", ParticleBudgetProfile::default()),
        (" bad ", ParticleBudgetProfile::default()),
        (
            "low",
            ParticleBudgetProfile {
                event_count: BTreeMap::from([(aestra_core::EventId::new(), 1)]),
                ..Default::default()
            },
        ),
        (
            "low",
            ParticleBudgetProfile {
                emitter_capacity: BTreeMap::from([(source.emitters[1].id, 0)]),
                ..Default::default()
            },
        ),
        (
            "low",
            ParticleBudgetProfile {
                event_count: BTreeMap::from([(source.events[0].id, 17)]),
                ..Default::default()
            },
        ),
        (
            "low",
            ParticleBudgetProfile {
                trail_capacity: BTreeMap::from([(source.emitters[0].renderers[0].id, 1)]),
                ..Default::default()
            },
        ),
    ] {
        let mut invalid = source.clone();
        invalid.particle_budgets = BTreeMap::from([(name.into(), profile)]);
        assert!(EffectCompiler::default().compile(&invalid).is_err());
        assert!(
            EffectCompiler::default()
                .with_tier(QualityTier::low())
                .compile(&invalid)
                .is_err()
        );
    }
}
