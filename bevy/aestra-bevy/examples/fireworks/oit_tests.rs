use super::*;
use aestra_bevy::{AestraRuntimeStatus, PresentationMode, RendererPlanKind};

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
        vec!["--experimental-oit"],
        vec!["--experimental-oit", "--smoke-cohorts"],
        vec!["--smoke-lighting", "--experimental-oit"],
    ] {
        assert!(Options::parse(args.into_iter().map(String::from)).is_err());
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
