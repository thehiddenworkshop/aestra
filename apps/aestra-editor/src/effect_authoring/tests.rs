use super::*;
use crate::test_support;
use aestra_core::EventTrigger;

#[test]
fn reusable_effect_extraction_replaces_selected_emitters_and_is_undoable() {
    let temporary = tempfile::tempdir().unwrap();
    let mut owner = EffectAsset::new("Owner", 4.0);
    owner.playback_mode = EffectPlaybackMode::Once;
    let mut first = Emitter::basic_sprite("First", 1.0);
    first.start_time = 0.5;
    let first_id = first.id;
    let mut second = Emitter::basic_sprite("Second", 0.75);
    second.start_time = 1.25;
    let second_id = second.id;
    let mut untouched = Emitter::basic_sprite("Untouched", 1.0);
    untouched.start_time = 2.5;
    let untouched_id = untouched.id;
    owner.emitters = vec![first, second, untouched];
    owner.events.push(EventLink {
        id: EventId::new(),
        source: first_id,
        trigger: EventTrigger::OnDeath,
        target: second_id,
    });
    owner.choreography_order = vec![
        ChoreographyTrackId::Emitter(first_id),
        ChoreographyTrackId::Emitter(second_id),
        ChoreographyTrackId::Emitter(untouched_id),
    ];
    let mut session = test_support::session_from_effect_with_source_path(
        owner,
        temporary.path().join("owner.aestra.ron"),
    );
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let localizer = Localizer::new("en-US").unwrap();
    create_reusable_effect_from_emitters(
        &[first_id, second_id],
        "Prismatic Burst",
        true,
        &mut catalog,
        &mut session,
        &localizer,
    )
    .unwrap();

    let created_path = temporary.path().join("prismatic_burst.aestra.ron");
    let created = EffectAsset::load_ron(&created_path).unwrap();
    assert_eq!(created.name, "Prismatic Burst");
    assert_eq!(created.duration, 1.5);
    assert_eq!(created.playback_mode, EffectPlaybackMode::Once);
    assert_eq!(created.emitters.len(), 2);
    assert_eq!(created.emitters[0].start_time, 0.0);
    assert_eq!(created.emitters[1].start_time, 0.75);
    assert_eq!(created.events.len(), 1);
    assert_eq!(session.effect.emitters.len(), 1);
    assert_eq!(session.effect.emitters[0].id, untouched_id);
    assert_eq!(session.effect.effect_clips.len(), 1);
    let clip = &session.effect.effect_clips[0];
    assert_eq!(clip.source, EffectAssetRef::new(created.id));
    assert_eq!(clip.start_time, 0.5);
    assert_eq!(clip.duration, 1.5);
    assert_eq!(
        session.effect.choreography_order,
        vec![
            ChoreographyTrackId::EffectClip(clip.id),
            ChoreographyTrackId::Emitter(untouched_id),
        ]
    );
    assert!(session.can_undo());

    session.undo();
    assert_eq!(session.effect.emitters.len(), 3);
    assert!(session.effect.effect_clips.is_empty());
    assert_eq!(session.effect.events.len(), 1);
    assert!(
        created_path.exists(),
        "undo keeps the reusable asset available"
    );
}

#[test]
fn reusable_effect_extraction_rejects_cross_boundary_event_links() {
    let mut owner = EffectAsset::new("Owner", 2.0);
    let first = Emitter::basic_sprite("First", 1.0);
    let first_id = first.id;
    let second = Emitter::basic_sprite("Second", 1.0);
    let second_id = second.id;
    owner.emitters = vec![first, second];
    owner.events.push(EventLink {
        id: EventId::new(),
        source: first_id,
        trigger: EventTrigger::OnSpawn,
        target: second_id,
    });

    let error = reusable_effect_plan(&owner, &[first_id], "Partial").unwrap_err();

    assert!(error.contains("crosses the selection boundary"));
}

#[test]
fn baking_rejects_invalid_overrides_instead_of_silently_discarding_them() {
    let mut child = EffectAsset::new("Child", 1.0);
    let parameter = aestra_core::ParameterId::new();
    child.parameters.push(aestra_core::EffectParameter {
        id: parameter,
        name: "Count".into(),
        default: Value::U32(2),
        exposed: true,
    });
    let mut clip = aestra_core::EffectClip::new(child.id, 0.0, 1.0);
    clip.parameter_overrides
        .insert(parameter, Value::Scalar(2.0));

    let error = bake_parameter_overrides(&mut child, &clip.parameter_overrides).unwrap_err();

    assert!(error.contains("expects U32, found Scalar"));
    assert_eq!(child.parameters[0].default, Value::U32(2));
}

#[test]
fn exploding_recursively_materializes_nested_clip_emitters() {
    let temporary = tempfile::tempdir().unwrap();
    let mut grandchild = EffectAsset::new("Grandchild", 1.0);
    let mut nested_emitter = Emitter::basic_sprite("Nested", 1.0);
    nested_emitter.transform.translation = [1.0, 0.0, 0.0];
    grandchild.emitters.push(nested_emitter);
    grandchild
        .save_ron(temporary.path().join("grandchild.aestra.ron"))
        .unwrap();

    let mut child = EffectAsset::new("Child", 2.0);
    let mut nested_clip = aestra_core::EffectClip::new(grandchild.id, 0.5, 1.0);
    nested_clip.transform.translation = [2.0, 0.0, 0.0];
    child.effect_clips.push(nested_clip);
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();

    let catalog = ProjectEffectCatalog::scan(temporary.path());
    let mut output = ExplodedEffectContent::default();
    let parent_transform = EmitterTransform {
        translation: [3.0, 0.0, 0.0],
        ..default()
    };
    flatten_effect_window(
        &catalog,
        &child,
        &BTreeMap::new(),
        0.0,
        2.0,
        0.25,
        parent_transform,
        &mut BTreeSet::new(),
        &mut output,
    )
    .unwrap();

    assert_eq!(output.emitters.len(), 1);
    assert_eq!(output.emitters[0].name, "Nested");
    assert_eq!(output.emitters[0].start_time, 0.75);
    assert_eq!(output.emitters[0].duration, 1.0);
    assert_eq!(output.emitters[0].transform.translation, [6.0, 0.0, 0.0]);
}
