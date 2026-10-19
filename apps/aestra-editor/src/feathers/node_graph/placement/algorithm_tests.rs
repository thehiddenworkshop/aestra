use super::*;

#[test]
fn estimated_seed_is_not_measured_and_named_placement_is_registered() {
    let mut area = Area::default();
    let key = GraphNodeKey::MaterialOutputs;
    let size = Vec2::new(224.0, 90.0);
    area.seed(key, Vec2::ZERO, size);
    area.seed(key, Vec2::ONE, size);
    assert_eq!(area.rect(key), Some(Rect::from_corners(Vec2::ZERO, size)));
    let placed = area.place_node(
        GraphNodeKey::FunctionOutputs,
        Vec2::new(400.0, 200.0),
        size,
        Neighborhood::Cursor,
    );
    assert!(!placed.assisted);
    assert_eq!(
        area.rect(GraphNodeKey::FunctionOutputs),
        Some(Rect::from_corners(placed.position, placed.position + size))
    );
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h))
}

#[test]
fn free_cursor_position_is_preserved_without_quantization() {
    let preferred = Vec2::new(-37.25, 41.5);
    assert_eq!(
        find_space(preferred, Vec2::new(224.0, 90.0), &[], |_| true),
        Some(preferred)
    );
}

#[test]
fn occupied_cursor_uses_nearest_gap_and_never_changes_obstacles() {
    let obstacles = vec![rect(0.0, 0.0, 224.0, 200.0), rect(248.0, 0.0, 224.0, 200.0)];
    let before = obstacles.clone();
    let placed = find_space(
        Vec2::new(10.0, 20.0),
        Vec2::new(224.0, 80.0),
        &obstacles,
        |_| true,
    )
    .unwrap();
    assert_eq!(placed, Vec2::new(10.0, -104.0));
    assert_eq!(obstacles, before);
    let reversed = obstacles.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(
        find_space(
            Vec2::new(10.0, 20.0),
            Vec2::new(224.0, 80.0),
            &reversed,
            |_| true
        ),
        Some(placed)
    );
}

#[test]
fn socket_creation_stays_on_its_requested_side() {
    for after in [true, false] {
        let key = GraphNodeKey::MaterialOutputs;
        let mut area = Area {
            nodes: BTreeMap::from([(key, rect(0.0, 0.0, 224.0, 300.0))]),
            measured: true,
            ..default()
        };
        let result = area.place(
            Vec2::new(0.0, 100.0),
            Vec2::new(224.0, 80.0),
            if after {
                Neighborhood::After(key)
            } else {
                Neighborhood::Before(key)
            },
        );
        assert!(result.assisted);
        assert_eq!(result.position.x, if after { 248.0 } else { -248.0 });
    }
}

#[test]
fn helper_nodes_reserve_distinct_space_in_the_same_creation_batch() {
    let mut area = Area {
        measured: true,
        ..default()
    };
    let size = Vec2::new(224.0, 90.0);
    let a = area.place(Vec2::ZERO, size, Neighborhood::Cursor);
    let b = area.place(Vec2::ZERO, size, Neighborhood::Cursor);
    assert!(a.assisted && b.assisted);
    assert_ne!(a.position, b.position);
    assert!(area.nodes.is_empty());
}

#[test]
fn invalid_oversized_and_blocked_inputs_fail_boundedly() {
    let size = Vec2::new(224.0, 90.0);
    assert_eq!(find_space(Vec2::splat(f32::NAN), size, &[], |_| true), None);
    assert_eq!(
        find_space(
            Vec2::ZERO,
            size,
            &vec![rect(0.0, 0.0, 1.0, 1.0); MAX_OBSTACLES + 1],
            |_| true
        ),
        None
    );
    assert_eq!(
        find_space(
            Vec2::ZERO,
            size,
            &[rect(-5000.0, -5000.0, 10000.0, 10000.0)],
            |_| true
        ),
        None
    );
    let mut missing = Area::default();
    let placed = missing.place(Vec2::new(10.0, 20.0), size, Neighborhood::Cursor);
    assert!(!placed.assisted);
    assert_eq!(placed.position, Vec2::new(10.0, 20.0));
}
