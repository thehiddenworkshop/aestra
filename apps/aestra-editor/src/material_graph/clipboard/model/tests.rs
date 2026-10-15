use super::*;
use aestra_core::material::MaterialValue;

#[test]
fn capture_rejects_empty_stale_and_partially_missing_selections() {
    let (program, selected) = program();
    for selection in [
        BTreeSet::new(),
        BTreeSet::from([MaterialExpressionId::new()]),
        selected
            .into_iter()
            .chain([MaterialExpressionId::new()])
            .collect(),
    ] {
        assert!(
            Fragment::capture(&program.expressions, &selection, BTreeMap::new(), &[], &[]).is_err()
        );
    }
}

#[test]
fn capture_and_instantiation_reject_nonfinite_positions_and_overflow() {
    let (program, selected) = program();
    let id = *selected.first().unwrap();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            Fragment::capture(
                &program.expressions,
                &selected,
                BTreeMap::from([(id, Vec2::splat(value))]),
                &[],
                &[]
            )
            .is_err()
        );
    }
    let fragment = Fragment::capture(
        &program.expressions,
        &selected,
        BTreeMap::from([(id, Vec2::splat(f32::MAX))]),
        &[],
        &[],
    )
    .unwrap();
    for offset in [
        Vec2::splat(f32::NAN),
        Vec2::splat(f32::INFINITY),
        Vec2::splat(f32::MAX),
    ] {
        assert!(fragment.instantiate(&BTreeSet::new(), offset).is_err());
    }
}

#[test]
fn clipboard_snapshot_is_immutable_and_each_paste_gets_disjoint_identities() {
    let (mut program, selected) = program();
    let fragment =
        Fragment::capture(&program.expressions, &selected, BTreeMap::new(), &[], &[]).unwrap();
    program.expressions.clear();
    let first = fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).unwrap();
    let second = fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).unwrap();
    let first_ids: BTreeSet<_> = first.expressions.iter().map(|node| node.id).collect();
    let second_ids: BTreeSet<_> = second.expressions.iter().map(|node| node.id).collect();
    assert_eq!(first_ids.len(), 2);
    assert_eq!(second_ids.len(), 2);
    assert!(first_ids.is_disjoint(&second_ids));
    assert!(first_ids.is_disjoint(&selected));
}

#[test]
fn only_selected_metadata_is_retained_and_selected_constants_remain_visible_nodes() {
    let (program, selected) = program();
    let other = MaterialExpressionId::new();
    let metadata: Vec<_> = selected.iter().copied().chain([other]).collect();
    let fragment = Fragment::capture(
        &program.expressions,
        &selected,
        BTreeMap::new(),
        &metadata,
        &metadata,
    )
    .unwrap();
    let insert = fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).unwrap();
    assert_eq!(insert.disabled.len(), 2);
    assert_eq!(insert.constants.len(), 2);
    assert!(
        insert
            .expressions
            .iter()
            .all(|node| insert.disabled.contains(&node.id))
    );
    let id = MaterialExpressionId::new();
    let fragment = Fragment::capture(
        &[MaterialExpression {
            id,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.2)),
        }],
        &BTreeSet::from([id]),
        BTreeMap::new(),
        &[],
        &[],
    )
    .unwrap();
    let insert = fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).unwrap();
    assert!(insert.constants.contains(&insert.expressions[0].id));
}

#[test]
fn missing_positions_use_zero_and_group_anchor_preserves_negative_offsets() {
    let (program, selected) = program();
    let id = *selected.first().unwrap();
    let fragment = Fragment::capture(
        &program.expressions,
        &selected,
        BTreeMap::from([(id, Vec2::new(-40.0, 20.0))]),
        &[],
        &[],
    )
    .unwrap();
    let insert = fragment
        .instantiate(&BTreeSet::new(), fragment.offset_to(Vec2::new(100.0, 80.0)))
        .unwrap();
    let positions: Vec<_> = insert.positions.values().copied().collect();
    assert!(positions.contains(&Vec2::new(100.0, 100.0)));
    assert!(positions.contains(&Vec2::new(140.0, 80.0)));
}

#[test]
fn shared_shortcuts_require_control_and_keep_existing_priority() {
    let mut keys = ButtonInput::<KeyCode>::default();
    for (key, expected) in [
        (KeyCode::KeyC, Shortcut::Copy),
        (KeyCode::KeyX, Shortcut::Cut),
        (KeyCode::KeyV, Shortcut::Paste),
        (KeyCode::KeyD, Shortcut::Duplicate),
    ] {
        for control in [KeyCode::ControlLeft, KeyCode::ControlRight] {
            keys.reset_all();
            keys.press(key);
            assert_eq!(shortcut(&keys), None);
            keys.press(control);
            assert_eq!(shortcut(&keys), Some(expected));
            keys.clear();
            assert_eq!(shortcut(&keys), None);
        }
    }
    keys.reset_all();
    keys.press(KeyCode::ControlLeft);
    for key in [KeyCode::KeyD, KeyCode::KeyV, KeyCode::KeyX, KeyCode::KeyC] {
        keys.press(key);
    }
    assert_eq!(shortcut(&keys), Some(Shortcut::Copy));
}

#[test]
fn inline_defaults_are_cloned_independently_and_not_exposed_as_explicit_nodes() {
    let mut program = MaterialProgram::additive_sprite("Defaults").normalized();
    let default = MaterialExpressionId::new();
    let second = MaterialExpressionId::new();
    let expression = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: default,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: second,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Multiply(default, second),
        },
    ]);
    assert!(program.inline_constants().contains(&default));
    program.disabled_expressions.push(default);
    let fragment = Fragment::capture(
        &program.expressions,
        &BTreeSet::from([expression]),
        BTreeMap::new(),
        &[],
        &[],
    )
    .unwrap()
    .with_inline_defaults(&program);
    let insert = fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).unwrap();
    assert_eq!(fragment.len(), 1);
    assert_eq!(insert.expressions.len(), 3);
    assert_eq!(insert.positions.len(), 1);
    let cloned = insert
        .expressions
        .iter()
        .find(|node| node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.25)))
        .unwrap()
        .id;
    assert_ne!(cloned, default);
    assert!(insert.disabled.contains(&cloned));
    assert!(!insert.constants.contains(&cloned));
    assert!(
        insert
            .expressions
            .iter()
            .any(|node| matches!(node.kind, MaterialExpressionKind::Multiply(a, b) if a == cloned && b != second))
    );
}

fn program() -> (MaterialProgram, BTreeSet<MaterialExpressionId>) {
    let mut program = MaterialProgram::additive_sprite("Clipboard").normalized();
    let constant = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: constant,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: multiply,
            kind: MaterialExpressionKind::Multiply(constant, constant),
        },
    ]);
    program.node_constants.push(constant);
    (program.normalized(), BTreeSet::from([constant, multiply]))
}

#[test]
fn clipboard_remaps_internal_links_and_retains_metadata_and_relative_positions() {
    let (mut program, selected) = program();
    let a = program
        .expressions
        .iter()
        .find(|node| node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.75)))
        .unwrap()
        .id;
    let b = program
        .expressions
        .iter()
        .find(|node| matches!(node.kind, MaterialExpressionKind::Multiply(..)))
        .unwrap()
        .id;
    program.disabled_expressions.push(b);
    let fragment = Fragment::capture(
        &program.expressions,
        &selected,
        BTreeMap::from([(a, Vec2::new(50.0, 20.0)), (b, Vec2::new(300.0, 100.0))]),
        &program.disabled_expressions,
        &program.node_constants,
    )
    .unwrap();
    let insert = fragment
        .instantiate(
            &BTreeSet::new(),
            fragment.offset_to(Vec2::new(500.0, 400.0)),
        )
        .unwrap();
    let new_a = *insert.constants.iter().next().unwrap();
    let new_b = *insert.disabled.iter().next().unwrap();
    assert!(!selected.contains(&new_a));
    assert!(!selected.contains(&new_b));
    assert_eq!(
        insert
            .expressions
            .iter()
            .find(|node| node.id == new_b)
            .unwrap()
            .kind,
        MaterialExpressionKind::Multiply(new_a, new_a)
    );
    assert_eq!(insert.positions[&new_a], Vec2::new(500.0, 400.0));
    assert_eq!(insert.positions[&new_b], Vec2::new(750.0, 480.0));
    assert!(insert.disabled.contains(&new_b));
    assert!(insert.constants.contains(&new_a));
    assert_ne!(
        insert.expressions[0].id,
        fragment
            .instantiate(&BTreeSet::new(), Vec2::ZERO)
            .unwrap()
            .expressions[0]
            .id
    );
}

#[test]
fn missing_external_source_is_rejected_without_silently_copying_extra_nodes() {
    let (program, _) = program();
    let node = program
        .expressions
        .iter()
        .find(|node| matches!(node.kind, MaterialExpressionKind::Multiply(..)))
        .unwrap();
    let fragment = Fragment::capture(
        &program.expressions,
        &BTreeSet::from([node.id]),
        BTreeMap::new(),
        &[],
        &[],
    )
    .unwrap();
    assert!(fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).is_err());
    assert!(
        fragment
            .instantiate(
                &program.expressions.iter().map(|node| node.id).collect(),
                Vec2::ZERO
            )
            .is_ok()
    );
}
