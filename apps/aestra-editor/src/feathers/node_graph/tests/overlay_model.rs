use super::*;

fn inputs() -> BTreeMap<String, Input> {
    (0..3)
        .map(|i| {
            (
                i.to_string(),
                Input {
                    base: Vec2::new(0.0, i as f32 * 122.0),
                    size: Vec2::splat(100.0),
                    compact: Vec2::splat(100.0),
                    collapsed: false,
                    revision: 1,
                    frozen: false,
                },
            )
        })
        .collect()
}

fn preview(input: &mut BTreeMap<String, Input>, key: &str, visible: bool) {
    input.get_mut(key).unwrap().size.y = if visible { 250.0 } else { 100.0 };
}

fn update(state: &mut Overlay, input: &BTreeMap<String, Input>) {
    state.update(input.clone(), 0, &BTreeSet::new()).unwrap();
}

#[test]
fn two_previews_compose_and_close_in_either_order_without_drift() {
    for order in [["0", "1"], ["1", "0"]] {
        let mut input = inputs();
        let mut state = Overlay::default();
        update(&mut state, &input);
        preview(&mut input, "0", true);
        update(&mut state, &input);
        assert_eq!(state.offsets["1"], Vec2::new(0.0, 150.0));
        preview(&mut input, "1", true);
        update(&mut state, &input);
        assert_eq!(state.offsets["2"], Vec2::new(0.0, 300.0));
        let both = state.offsets.clone();
        for _ in 0..10 {
            update(&mut state, &input);
            assert_eq!(state.offsets, both);
        }
        preview(&mut input, order[0], false);
        update(&mut state, &input);
        assert_eq!(state.offsets["2"], Vec2::new(0.0, 150.0));
        preview(&mut input, order[1], false);
        update(&mut state, &input);
        assert!(state.offsets.is_empty());
    }
}

#[test]
fn restart_reconstructs_active_previews_from_bases_not_effective_positions() {
    let mut input = inputs();
    preview(&mut input, "0", true);
    preview(&mut input, "1", true);
    let mut original = Overlay::default();
    update(&mut original, &input);
    let mut reopened = Overlay::default();
    update(&mut reopened, &input);
    assert_eq!(original.offsets, reopened.offsets);
    assert_eq!(input["1"].base.y, 122.0);
}

#[test]
fn manual_move_is_protected_until_old_causes_end_and_is_not_permanently_pinned() {
    let mut input = inputs();
    let mut state = Overlay::default();
    preview(&mut input, "0", true);
    update(&mut state, &input);
    let moved = input.get_mut("1").unwrap();
    moved.base = Vec2::new(350.0, 272.0);
    moved.revision += 1;
    update(&mut state, &input);
    assert!(!state.offsets.contains_key("1"));
    assert!(state.protected.contains("1"));
    preview(&mut input, "0", false);
    update(&mut state, &input);
    assert!(state.offsets.is_empty());
    assert!(state.protected.is_empty());
    assert_eq!(input["1"].base, Vec2::new(350.0, 272.0));
}

#[test]
fn restore_revalidates_inserted_obstacle_and_preserves_safe_offsets_on_conflict() {
    let mut input = inputs();
    let mut state = Overlay::default();
    preview(&mut input, "0", true);
    update(&mut state, &input);
    let safe = state.offsets.clone();
    input.insert(
        "inserted".into(),
        Input {
            base: Vec2::new(0.0, 122.0),
            ..input["1"].clone()
        },
    );
    preview(&mut input, "0", false);
    assert!(matches!(
        state.update(input.clone(), 0, &BTreeSet::new()),
        Err(Conflict::Restore(..))
    ));
    assert_eq!(state.offsets, safe);
    input.remove("inserted");
    update(&mut state, &input);
    assert!(state.offsets.is_empty());
}

#[test]
fn frozen_conflict_never_applies_a_partial_overlay() {
    let mut input = inputs();
    input.get_mut("1").unwrap().frozen = true;
    let mut state = Overlay::default();
    update(&mut state, &input);
    preview(&mut input, "0", true);
    assert!(state.update(input, 0, &BTreeSet::new()).is_err());
    assert!(state.offsets.is_empty());
}

#[test]
fn collapse_expansion_and_history_replay_are_reversible() {
    let mut input = inputs();
    let mut state = Overlay::default();
    input.get_mut("0").unwrap().size.y = 30.0;
    input.get_mut("0").unwrap().compact.y = 30.0;
    input.get_mut("0").unwrap().collapsed = true;
    update(&mut state, &input);
    input.get_mut("0").unwrap().size.y = 250.0;
    input.get_mut("0").unwrap().compact.y = 250.0;
    input.get_mut("0").unwrap().collapsed = false;
    update(&mut state, &input);
    let expanded = state.offsets.clone();
    assert_eq!(expanded["1"].y, 150.0);
    input.get_mut("0").unwrap().size.y = 30.0;
    input.get_mut("0").unwrap().compact.y = 30.0;
    input.get_mut("0").unwrap().collapsed = true;
    state.update(input.clone(), 1, &BTreeSet::new()).unwrap();
    assert!(state.offsets.is_empty());
    input.get_mut("0").unwrap().size.y = 250.0;
    input.get_mut("0").unwrap().compact.y = 250.0;
    input.get_mut("0").unwrap().collapsed = false;
    state.update(input.clone(), 2, &BTreeSet::new()).unwrap();
    assert_eq!(state.offsets, expanded);
}

#[test]
fn removed_nodes_cannot_be_resurrected_by_closing_a_preview() {
    let mut input = inputs();
    let mut state = Overlay::default();
    preview(&mut input, "0", true);
    update(&mut state, &input);
    input.remove("1");
    preview(&mut input, "0", false);
    update(&mut state, &input);
    assert!(state.offsets.is_empty());
    assert!(!state.previous.contains_key("1"));
}

#[test]
fn combined_causes_share_global_work_budgets() {
    let mut input = BTreeMap::new();
    for i in 0..70 {
        input.insert(
            format!("{i:03}"),
            Input {
                base: Vec2::new(0.0, i as f32 * 122.0),
                size: Vec2::new(100.0, 250.0),
                ..inputs()["0"].clone()
            },
        );
    }
    let mut state = Overlay::default();
    assert!(matches!(
        state.update(input, 0, &BTreeSet::new()),
        Err(Conflict::Solve(resize::Conflict::Budget(_)))
    ));
    assert!(state.offsets.is_empty());
}
