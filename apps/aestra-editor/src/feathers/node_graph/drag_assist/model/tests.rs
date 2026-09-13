use super::*;

fn shape(x: f32, y: f32, w: f32, h: f32, rows: &[f32]) -> Shape {
    Shape {
        rect: Rect::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h)),
        rows: rows.to_vec(),
    }
}

#[test]
fn soft_grid_allows_arbitrary_positions_and_negative_coordinates() {
    let moving = shape(0.0, 0.0, 100.0, 50.0, &[]);
    assert_eq!(
        snap(Vec2::new(-30.0, 61.0), &moving, &[], 1.0, true, false).0,
        Vec2::new(-32.0, 64.0)
    );
    assert_eq!(
        snap(Vec2::new(15.0, 17.0), &moving, &[], 1.0, true, false).0,
        Vec2::new(15.0, 17.0)
    );
    assert_eq!(
        snap(Vec2::new(31.0, 65.0), &moving, &[], 1.0, false, false).0,
        Vec2::new(31.0, 65.0)
    );
}

#[test]
fn edges_centers_and_ports_use_measured_sizes() {
    let moving = shape(0.0, 0.0, 80.0, 50.0, &[20.0]);
    let target = shape(200.0, 100.0, 140.0, 90.0, &[35.0]);
    let targets = [target];
    for (raw, expected) in [
        (Vec2::new(198.0, 12.0), Vec2::new(200.0, 12.0)), // left
        (Vec2::new(229.0, 12.0), Vec2::new(230.0, 12.0)), // center
        (Vec2::new(258.0, 12.0), Vec2::new(260.0, 12.0)), // right
        (Vec2::new(12.0, 114.0), Vec2::new(12.0, 115.0)), // row
    ] {
        let (actual, guides) = snap(raw, &moving, &targets, 1.0, false, true);
        assert_eq!(actual, expected);
        assert_eq!(guides.len(), 1);
    }
}

#[test]
fn snap_radius_is_screen_space_not_graph_space() {
    let moving = shape(0.0, 0.0, 400.0, 50.0, &[]);
    for zoom in [0.25, 0.5, 1.0, 2.0] {
        let near = Vec2::new(64.0 + (5.0_f32 / zoom).min(15.0), 0.0);
        assert_eq!(snap(near, &moving, &[], zoom, true, false).0.x, 64.0);
        let targets = [shape(200.0, 150.0, 400.0, 50.0, &[])];
        assert_eq!(
            snap(
                Vec2::new(200.0 + 5.0 / zoom, 0.0),
                &moving,
                &targets,
                zoom,
                false,
                true
            )
            .0
            .x,
            200.0
        );
        assert_eq!(
            snap(
                Vec2::new(200.0 + 7.0 / zoom, 0.0),
                &moving,
                &targets,
                zoom,
                false,
                true
            )
            .0
            .x,
            200.0 + 7.0 / zoom
        );
    }
}

#[test]
fn geometric_alignment_has_priority_over_grid_and_is_order_independent() {
    let moving = shape(0.0, 0.0, 80.0, 50.0, &[]);
    let a = shape(61.0, 100.0, 80.0, 50.0, &[]);
    let b = shape(63.0, 100.0, 80.0, 50.0, &[]);
    let first = snap(
        Vec2::new(62.0, 12.0),
        &moving,
        &[a.clone(), b.clone()],
        1.0,
        true,
        true,
    );
    let second = snap(Vec2::new(62.0, 12.0), &moving, &[b, a], 1.0, true, true);
    assert_eq!(first, second);
    assert_eq!(first.0.x, 61.0);
}

#[test]
fn distant_or_over_budget_geometry_does_not_attract_drag() {
    let moving = shape(0.0, 0.0, 80.0, 50.0, &[]);
    let raw = Vec2::new(62.0, 12.0);
    assert_eq!(
        snap(
            raw,
            &moving,
            &[shape(60.0, 1000.0, 80.0, 50.0, &[])],
            1.0,
            false,
            true
        )
        .0,
        raw
    );
    let targets = vec![shape(60.0, 100.0, 80.0, 50.0, &[]); MAX_NODES + 1];
    assert_eq!(snap(raw, &moving, &targets, 1.0, false, true).0, raw);
}
