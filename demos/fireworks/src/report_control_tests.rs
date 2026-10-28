use super::*;

#[test]
fn matched_transparency_can_explicitly_disable_msaa_without_changing_default_views() {
    assert_eq!(msaa_control(None), Ok(false));
    assert_eq!(msaa_control(Some("off")), Ok(true));
    for value in ["", "0", "4", "auto"] {
        assert!(msaa_control(Some(value)).is_err());
    }
}

#[test]
fn source_output_audit_has_a_fixed_key_bound_and_aggregates_delivery_age() {
    let mut audit = outputs::OutputAudit::default();
    let mut event = aestra_bevy::AestraOutputEvent::root(
        Entity::PLACEHOLDER,
        aestra_runtime::EffectOutputEvent::new(
            "first",
            aestra_runtime::EventOrigin::Emitter(0),
            "",
            vec![0.0; 3],
            1.0,
            60,
        ),
    );
    event.particle = Some(aestra_bevy::ParticleOutputContext {
        source_effect: aestra_core::EffectId::new(),
        seed: 1,
        root_time_seconds: 1.0,
        world_position: Some([0.0; 3]),
    });
    for index in 0..300 {
        event.event.kind = format!("route_{index}");
        audit.observe(&event, 2.5);
    }
    event.event.kind = "route_0".into();
    audit.observe(&event, 4.0);
    let summary = audit.summary();
    let routes = summary["routes"].as_object().unwrap();
    assert_eq!(routes.len(), 256);
    assert_eq!(summary["omitted_route_messages"], 44);
    let first = routes.values().find(|r| r["kind"] == "route_0").unwrap();
    assert_eq!(first["messages"], 2);
    assert_eq!(first["max_delivery_age_seconds"], 3.0);
}

#[test]
fn muted_flashes_are_an_explicit_image_only_control() {
    for images in [false, true] {
        assert_eq!(flash_control(None, images), Ok(true));
        assert_eq!(flash_control(Some("1"), images), Ok(true));
        for invalid in ["", "false", "2"] {
            assert!(flash_control(Some(invalid), images).is_err());
        }
    }
    assert!(flash_control(Some("0"), false).is_err());
    assert_eq!(flash_control(Some("0"), true), Ok(false));
}

#[test]
fn flash_mute_keeps_admission_policy_enabled_and_does_not_change_caps() {
    for tier in ["high", "medium", "low"] {
        let options =
            Options::parse(["--smoke-art-show".into(), "--tier".into(), tier.into()]).unwrap();
        let mut app = App::new();
        options.lighting_policy().apply(app.world_mut()).unwrap();
        assert!(!options.lighting_policy().particle.enabled);
        let mut settings = app
            .world_mut()
            .resource_mut::<aestra_bevy::TransientLightSettings>();
        let original = settings.clone();
        apply_flash_control(&mut settings, true);
        assert_eq!(settings.max_lumens, original.max_lumens);
        apply_flash_control(&mut settings, false);
        assert_eq!(settings.max_lumens, 0.0);
        assert!(settings.enabled);
        assert_eq!(settings.authored_bindings, original.authored_bindings);
        assert_eq!(settings.max_lights, original.max_lights);
        assert_eq!(
            settings.max_requests_per_frame,
            original.max_requests_per_frame
        );
        assert_eq!(settings.max_range, original.max_range);
    }
}
