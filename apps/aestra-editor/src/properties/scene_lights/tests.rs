use super::*;
use aestra_core::{EventDefinition, ParticleOutputRoute};

fn fixture() -> (EditorSession, Vec<EventRouteId>, Localizer) {
    let mut effect = crate::test_support::effect_with_timing_slack();
    let mut routes = Vec::new();
    for name in ["break", "secondary", "each"] {
        let output = EventDefinition::new(name);
        let mut route =
            ParticleOutputRoute::new(effect.emitters[0].id, EventTrigger::OnDeath, output.id);
        if name == "each" {
            route.aggregation = EventAggregation::EachEvent { limit: 4 };
        }
        routes.push(route.id);
        effect.event_outputs.push(output);
        effect.particle_outputs.push(route);
    }
    effect.parameters.push(EffectParameter {
        id: ParameterId::new(),
        name: "Star Color".into(),
        default: Value::Gradient(Gradient::new(vec![
            ColorKey::new(0.0, [1.0; 4]),
            ColorKey::new(1.0, [0.2, 0.4, 1.0, 1.0]),
        ])),
        exposed: true,
    });
    (
        EditorSession::from_test_effect(effect),
        routes,
        Localizer::new("en-US").unwrap(),
    )
}

#[test]
fn picker_excludes_each_ambiguous_and_bound_routes_without_changing_existing_binding() {
    let (mut session, routes, localizer) = fixture();
    assert_eq!(compatible_routes(&session.effect, None), routes[..2]);
    let mut clipboard = LightClipboard::default();
    assert!(apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    let id = session.effect.point_lights[0].id;
    assert_eq!(session.selection.primary, SemanticTarget::PointLight(id));
    assert_eq!(compatible_routes(&session.effect, None), vec![routes[1]]);
    assert_eq!(compatible_routes(&session.effect, Some(id)), routes[..2]);
    let original = session.effect.clone();
    assert!(!apply(
        LightAction::Route(id, routes[2]),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    assert_eq!(session.effect, original);
    let mut ambiguous = session.effect.particle_outputs[1].clone();
    ambiguous.id = EventRouteId::new();
    session.effect.particle_outputs.push(ambiguous);
    assert!(compatible_routes(&session.effect, None).is_empty());
}

#[test]
fn controls_reproduce_a_saved_shell_binding_and_light_only_edits_are_undoable() {
    let (mut session, routes, localizer) = fixture();
    let mut clipboard = LightClipboard::default();
    assert!(apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    let id = session.effect.point_lights[0].id;
    let parameter = session.effect.parameters[0].id;
    assert!(apply(
        LightAction::ColorSource(id, Some(parameter)),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    assert!(set_number(
        &mut session,
        LightNumber::Age(id),
        0.25,
        "Color sample age".into()
    ));
    let binding = &session.effect.point_lights[0];
    let expected = PointLightPulse::flash([1.0; 3], 500_000.0, 80.0, 0.6);
    assert_eq!(
        binding.pulse.intensity_lumens.keys,
        expected.intensity_lumens.keys
    );
    assert_eq!(binding.pulse.range.keys, expected.range.keys);
    assert_eq!(binding.pulse.linear_color, expected.linear_color);
    assert_eq!(binding.pulse.radius, expected.radius);
    assert_eq!(binding.pulse.duration_seconds, expected.duration_seconds);
    assert!(!session.last_diff.is_empty());
    let serialized = session.effect.to_pretty_ron().unwrap();
    assert_eq!(
        EffectAsset::from_ron(&serialized).unwrap().point_lights,
        session.effect.point_lights
    );
    let before = session.effect.clone();
    assert!(set_number(
        &mut session,
        LightNumber::Radius(id),
        0.5,
        "Radius".into()
    ));
    session.undo();
    assert_eq!(session.effect, before);
    session.redo();
    assert_eq!(session.effect.point_lights[0].pulse.radius, 0.5);
    assert!(apply(
        LightAction::Remove(id),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    session.undo();
    assert_eq!(session.effect.point_lights[0].id, id);
}

#[test]
fn copy_paste_preserves_destination_ids_and_route_and_refuses_foreign_parameters() {
    let (mut session, routes, localizer) = fixture();
    let mut clipboard = LightClipboard::default();
    apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    apply(
        LightAction::Add(routes[1]),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    let source = session.effect.point_lights[0].id;
    let target = session.effect.point_lights[1].clone();
    set_number(
        &mut session,
        LightNumber::Duration(source),
        0.9,
        "Duration".into(),
    );
    apply(
        LightAction::Copy(source),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    assert!(apply(
        LightAction::Paste(target.id),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    let pasted = &session.effect.point_lights[1];
    assert_eq!(pasted.id, target.id);
    assert_eq!(pasted.route, target.route);
    assert_eq!(
        pasted.pulse.intensity_lumens.id,
        target.pulse.intensity_lumens.id
    );
    assert_eq!(pasted.pulse.range.id, target.pulse.range.id);
    assert_eq!(pasted.pulse.duration_seconds, 0.9);
    assert!(session.effect.validation_report().is_valid());
    let before = session.effect.clone();
    clipboard.0.as_mut().unwrap().color_parameter = Some(LightColorParameter {
        parameter: ParameterId::new(),
        normalized_age: 0.5,
    });
    assert!(!apply(
        LightAction::Paste(target.id),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    assert_eq!(session.effect, before);
}

#[test]
fn shared_gradient_edits_retain_identity_and_are_undone_with_the_light_edits() {
    let (mut session, routes, localizer) = fixture();
    let mut clipboard = LightClipboard::default();
    apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    let id = session.effect.point_lights[0].id;
    let parameter = session.effect.parameters[0].id;
    apply(
        LightAction::ColorSource(id, Some(parameter)),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    let original = session.effect.clone();
    assert!(set_number(
        &mut session,
        LightNumber::GradientChannel(parameter, 0, 0),
        0.1,
        "Color".into()
    ));
    assert!(apply(
        LightAction::AddColorKey(parameter),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    assert!(apply(
        LightAction::RemoveColorKey(parameter, 1),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    session.undo();
    session.undo();
    session.undo();
    assert_eq!(session.effect, original);
}

#[test]
fn normalized_curve_controls_use_output_units_and_invalid_values_never_commit() {
    let (mut session, routes, localizer) = fixture();
    let mut clipboard = LightClipboard::default();
    apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut clipboard,
        &localizer,
    );
    let id = session.effect.point_lights[0].id;
    edit_binding(&mut session, id, "Normalize".into(), |binding| {
        let curve_id = binding.pulse.range.id;
        binding.pulse.range = Curve::normalized(
            vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
            aestra_core::ScalarRange::new(10.0, 100.0),
        );
        binding.pulse.range.id = curve_id;
    });
    let value = LightNumber::CurveValue(id, LightCurve::Range, 0);
    assert_eq!(number_value(&session.effect, value), Some(10.0));
    assert!(set_number(&mut session, value, 55.0, "Range".into()));
    assert_eq!(
        session.effect.point_lights[0].pulse.range.keys[0].value,
        0.5
    );
    assert!(apply(
        LightAction::AddKey(id, LightCurve::Range),
        &mut session,
        &mut clipboard,
        &localizer
    ));
    assert_eq!(
        session.effect.point_lights[0].pulse.range.keys[1].value,
        0.75
    );
    let original = session.effect.clone();
    for (control, value) in [
        (LightNumber::Duration(id), -1.0),
        (LightNumber::CurveValue(id, LightCurve::Range, 0), 0.0),
        (LightNumber::CurveTime(id, LightCurve::Range, 1), 0.0),
        (LightNumber::Color(id, 0), 2.0),
    ] {
        assert!(!set_number(&mut session, control, value, "Invalid".into()));
        assert_eq!(session.effect, original);
    }
}

#[test]
fn numeric_observer_ignores_scrub_previews_and_commits_once_on_release() {
    let (mut session, routes, localizer) = fixture();
    apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut LightClipboard::default(),
        &localizer,
    );
    let id = session.effect.point_lights[0].id;
    let original = session.effect.clone();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(session)
        .insert_resource(localizer);
    register(&mut app);
    app.world_mut().flush();
    let control = app.world_mut().spawn(LightNumber::Duration(id)).id();
    app.world_mut().trigger(ValueChange {
        source: control,
        value: 0.9_f32,
        is_final: false,
    });
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    app.world_mut().trigger(ValueChange {
        source: control,
        value: 0.9_f32,
        is_final: true,
    });
    assert_eq!(
        app.world().resource::<EditorSession>().effect.point_lights[0]
            .pulse
            .duration_seconds,
        0.9
    );
    app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
}

#[test]
fn inspector_renders_shared_curve_widgets_real_controls_and_visible_invalid_route() {
    let (mut session, routes, localizer) = fixture();
    apply(
        LightAction::Add(routes[0]),
        &mut session,
        &mut LightClipboard::default(),
        &localizer,
    );
    session.effect.point_lights[0].route = EventRouteId::new();
    session.diagnostics = session.effect.validation_report();
    for locale in ["en-US", "fr-FR"] {
        let localizer = Localizer::new(locale).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            bevy::text::TextPlugin,
        ));
        app.init_asset::<Image>();
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| spawn(parent, &session, &localizer));
        app.world_mut().flush();
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&automation_curve::AutomationCurveRaster>()
                .iter(world)
                .count(),
            2
        );
        assert_eq!(world.query::<&LightNumber>().iter(world).count(), 15);
        assert!(
            world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0 == localizer.text("scene-lights-invalid-route"))
        );
        assert!(
            world
                .query::<&PropertiesSemanticTarget>()
                .iter(world)
                .any(|target| target.target == session.selection.primary)
        );
        assert!(
            world
                .query::<&LightAction>()
                .iter(world)
                .any(|action| matches!(action, LightAction::Paste(_)))
        );
    }
}

#[test]
fn saved_f7b1_shell_light_settings_are_reproducible_through_editor_controls() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test/effects");
    for name in [
        "fireworks_multi_break",
        "fireworks_crackle",
        "fireworks_crossette",
        "fireworks_strobe",
        "fireworks_secondary_volley",
    ] {
        let saved = EffectAsset::load_ron(root.join(format!("{name}.aestra.ron"))).unwrap();
        let expected = &saved.point_lights[0];
        // Use the source's shared color parameter with a minimal compileable editor fixture;
        // material-program/project resolution is orthogonal to this inspector contract.
        let (mut session, routes, localizer) = fixture();
        let parameter = saved
            .parameters
            .iter()
            .find(|parameter| {
                expected
                    .color_parameter
                    .as_ref()
                    .is_some_and(|color| color.parameter == parameter.id)
            })
            .unwrap()
            .clone();
        session.effect.parameters.push(parameter.clone());
        let mut clipboard = LightClipboard::default();
        assert!(apply(
            LightAction::Add(routes[0]),
            &mut session,
            &mut clipboard,
            &localizer
        ));
        let id = session.effect.point_lights[0].id;
        assert!(apply(
            LightAction::ColorSource(id, Some(parameter.id)),
            &mut session,
            &mut clipboard,
            &localizer
        ));
        let authored = &session.effect.point_lights[0];
        assert_eq!(authored.color_parameter, expected.color_parameter, "{name}");
        assert_eq!(
            authored.pulse.intensity_lumens.keys, expected.pulse.intensity_lumens.keys,
            "{name}"
        );
        assert_eq!(
            authored.pulse.range.keys, expected.pulse.range.keys,
            "{name}"
        );
        assert_eq!(authored.pulse.radius, expected.pulse.radius, "{name}");
        assert_eq!(
            authored.pulse.duration_seconds, expected.pulse.duration_seconds,
            "{name}"
        );
        assert_eq!(
            EffectAsset::from_ron(&session.effect.to_pretty_ron().unwrap())
                .unwrap()
                .point_lights,
            session.effect.point_lights
        );
    }
}
