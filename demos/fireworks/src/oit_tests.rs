use super::*;
use aestra_bevy::{AestraRuntimeStatus, PresentationMode, RendererPlanKind};

#[test]
fn reservation_is_explicit_and_does_not_raise_the_camera_memory_ceiling() {
    for value in ["NaN", "inf", "0", "-1"] {
        assert!(
            Options::parse([
                "--experimental-oit".into(),
                "--oit-fragments-per-pixel".into(),
                value.into()
            ])
            .is_err()
        );
    }
    assert!(Options::parse(["--oit-fragments-per-pixel".into(), "8".into()]).is_err());
    let options = Options::parse([
        "--experimental-oit".into(),
        "--oit-fragments-per-pixel".into(),
        "8".into(),
    ])
    .unwrap();
    let configured = settings(&options);
    assert_eq!(configured.fragments_per_pixel_average, 8.0);
    assert_eq!(configured.max_storage_bytes, 128 * 1024 * 1024);
    assert_eq!(configured.sorted_fragment_max_count, 32);
    assert_eq!(options.max_owners, None);
}

#[test]
fn global_loss_is_a_failure_but_long_list_work_is_not_a_capacity_rejection() {
    let mut sample = aestra_bevy::ExperimentalOitDiagnosticSample {
        camera: Entity::PLACEHOLDER,
        sequence: 1,
        target_size: UVec2::new(8, 8),
        sorted_fragment_max_count: 32,
        attempted_fragments: 1_000_000,
        node_capacity: 1_000_000,
        dropped_fragments: 0,
        over_sort_pixels: 32,
        invalid_links: 0,
        capped_max_depth: 33,
    };
    // Long accepted lists are sorted by the existing merge pass and stay valid.
    assert_eq!(capacity_failure(&sample), None);
    sample.dropped_fragments = 1;
    assert!(capacity_failure(&sample).is_some());
    sample.dropped_fragments = 0;
    sample.invalid_links = 1;
    assert!(capacity_failure(&sample).is_some());
}

#[test]
fn guarded_camera_keeps_the_public_hdr_profile_and_explicit_budget() {
    let options = Options::parse(["--smoke-art-show".into(), "--experimental-oit".into()]).unwrap();
    let project = options.compile().unwrap();
    let mut app = App::new();
    app.insert_resource(audit(&project).unwrap())
        .insert_resource(options)
        .insert_resource(crate::project::ShowProject(project))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<aestra_bevy::EffectPresentationCache>()
        .add_systems(Startup, crate::setup);
    app.update();
    let mut query = app.world_mut().query::<(
        &Msaa,
        &ExperimentalOitSettings,
        &bevy::camera::Hdr,
        &bevy::post_process::bloom::Bloom,
    )>();
    let (msaa, settings, _, bloom) = query.single(app.world()).unwrap();
    assert_eq!(*msaa, Msaa::Off);
    assert_eq!(settings.max_storage_bytes, 128 * 1024 * 1024);
    assert!(bloom.intensity > 0.0);
}

#[test]
fn opt_in_audits_both_saved_candidates_at_every_tier_without_altering_authored_work() {
    for show in ["--smoke-art-show", "--persistent-smoke-show"] {
        for tier in ["high", "medium", "low"] {
            let baseline = Options::parse([show.into(), "--tier".into(), tier.into()])
                .unwrap()
                .compile()
                .unwrap();
            let options = Options::parse([
                show.into(),
                "--experimental-oit".into(),
                "--tier".into(),
                tier.into(),
            ])
            .unwrap();
            let project = options.compile().unwrap();
            assert_eq!(project.root, baseline.root);
            assert_eq!(project.dependencies, baseline.dependencies);
            let result = audit(&project).unwrap();
            assert!(result.sprites > 0 && result.trails > 0);
            assert_eq!(result.renderers, result.sprites + result.trails);
        }
    }
    for args in [
        vec!["--experimental-oit", "--cinematic"],
        vec!["--experimental-oit", "--smoke-cohorts"],
        vec!["--smoke-lighting", "--experimental-oit"],
    ] {
        assert!(Options::parse(args.into_iter().map(String::from)).is_err());
    }
}

#[test]
fn independent_host_catalog_is_admitted_without_changing_effects_or_budgets() {
    for tier in ["high", "medium", "low"] {
        for look in [None, Some("--star-lights"), Some("--tuned-smoke")] {
            let mut args = vec!["--tier".into(), tier.into()];
            args.extend(look.map(String::from));
            let baseline = Options::parse(args.clone()).unwrap().compile().unwrap();
            args.push("--experimental-oit".into());
            let options = Options::parse(args).unwrap();
            assert!(options.host_driven);
            assert_eq!(options.max_owners, None);
            let candidate = options.compile().unwrap();
            // The host creates fresh root/binding identities on each load. Compare
            // every compiled field after normalizing only those transient IDs.
            let mut normalized = (*candidate).clone();
            std::sync::Arc::make_mut(&mut normalized.root).source = baseline.root.source;
            for (id, candidate) in &mut normalized.dependencies {
                let expected = &baseline.dependencies[id];
                let candidate = std::sync::Arc::make_mut(candidate);
                assert_eq!(candidate.bindings.len(), expected.bindings.len());
                for (binding, source) in candidate.bindings.iter_mut().zip(&expected.bindings) {
                    binding.source = source.source;
                }
                candidate.binding_slots = expected.binding_slots.clone();
            }
            assert_eq!(baseline.root, normalized.root);
            assert_eq!(baseline.dependencies, normalized.dependencies);
            let admission = audit(&candidate).unwrap();
            assert!(admission.sprites > 0 && admission.trails > 0);
            assert!(candidate.root.effect_clips.is_empty());
        }
    }
}

#[test]
fn custom_project_with_nonparticipating_geometry_is_rejected_before_window_start() {
    let project = Options::parse(["--smoke-art-show".into()])
        .unwrap()
        .compile()
        .unwrap();
    let mut project = (*project).clone();
    let child = std::sync::Arc::make_mut(project.dependencies.values_mut().next().unwrap());
    let renderer = child
        .emitters
        .iter_mut()
        .flat_map(|e| &mut e.renderers)
        .next()
        .unwrap();
    renderer.kind = RendererPlanKind::Ribbon {
        width: 1.0,
        strand_count: 1,
    };
    assert!(
        audit(&project)
            .unwrap_err()
            .contains("unsupported renderer")
    );
}

#[test]
fn runtime_admission_waits_for_native_camera_and_exposes_budget_rejection() {
    let options = Options::parse(["--smoke-art-show".into(), "--experimental-oit".into()]).unwrap();
    let mut app = App::new();
    app.insert_resource(options)
        .init_resource::<Admission>()
        .insert_resource(AestraRuntimeStatus {
            requested: PresentationMode::Gpu,
            active: ActiveBackend::Gpu,
            reason: String::new(),
        })
        .add_message::<AppExit>()
        .add_systems(Update, guard);
    let camera = app.world_mut().spawn(AudienceCamera).id();
    app.update();
    assert!(!app.world().resource::<Admission>().ready);
    app.world_mut()
        .entity_mut(camera)
        .insert(ExperimentalOitStatus {
            estimated_storage_bytes: 47_923_200,
            rejection: None,
        });
    app.update();
    assert!(app.world().resource::<Admission>().ready);
    app.world_mut()
        .entity_mut(camera)
        .insert(ExperimentalOitStatus {
            estimated_storage_bytes: 200_000_000,
            rejection: Some("shared OIT storage exceeds camera/device budget"),
        });
    app.update();
    assert!(!app.world().resource::<Admission>().ready);
    assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
}
