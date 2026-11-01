use super::*;

#[test]
fn partition_extents_are_disjoint_complete_and_bounded() {
    for (size, grid) in [
        (UVec2::new(257, 193), UVec2::splat(4)),
        (UVec2::new(7, 3), UVec2::new(7, 3)),
    ] {
        let mut pixels = vec![0; (size.x * size.y) as usize];
        for r in regions(size, grid).unwrap() {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    pixels[(y * size.x + x) as usize] += 1;
                }
            }
        }
        assert!(pixels.into_iter().all(|n| n == 1));
    }
    for (size, grid) in [
        (UVec2::ZERO, UVec2::ONE),
        (UVec2::splat(7), UVec2::ZERO),
        (UVec2::splat(7), UVec2::splat(8)),
        (UVec2::splat(100), UVec2::splat(9)),
        (UVec2::splat(u32::MAX), UVec2::ONE),
    ] {
        assert!(regions(size, grid).is_err());
    }
}

#[test]
fn pending_rejected_resized_and_removed_views_invalidate_late_samples() {
    let status = ExperimentalOitPartitionStatus::default();
    let key = Key {
        camera: Entity::PLACEHOLDER,
        size: UVec2::new(257, 193),
        grid: UVec2::splat(4),
        cameras: 1,
        capacity: 100,
        settings: (8, 4.0f32.to_bits()),
    };
    let generation = status.configure(Some(key), true, None);
    let sample = ExperimentalOitPartitionSample {
        sequence: 1,
        empty_phase: false,
        attempted_fragments: 110,
        dropped_fragments: 10,
        invalid_links: 0,
        regions: vec![[110, 2, 0, 9]],
        node_capacity: 100,
    };
    status.publish(generation, sample.clone());
    let mut clean = sample.clone();
    clean.sequence = 3;
    clean.dropped_fragments = 0;
    clean.empty_phase = true;
    clean.attempted_fragments = 0;
    clean.regions = vec![[0; 4]];
    status.publish(generation, clean.clone());
    status.publish(generation, sample.clone()); // Out-of-order failure stays observed.
    assert!(status.snapshot().unwrap().observed_loss);
    assert_eq!(status.snapshot().unwrap().sample.unwrap().sequence, 3);
    assert_eq!(
        status.snapshot().unwrap().sample.unwrap().dropped_fragments,
        0
    );
    status.configure(Some(key), false, None);
    status.publish(generation, sample.clone());
    assert!(status.snapshot().unwrap().sample.is_none());
    assert!(status.snapshot().unwrap().observed_loss);
    let next = status.configure(Some(key), true, None);
    status.publish(next, sample.clone());
    status.configure(Some(key), false, Some("foreign draw"));
    status.publish(next, sample.clone());
    assert!(status.snapshot().unwrap().sample.is_none());
    let next = status.configure(Some(key), true, None);
    let mut resized = key;
    resized.size.x += 1;
    status.configure(Some(resized), true, None);
    status.publish(next, sample.clone());
    assert!(status.snapshot().unwrap().sample.is_none());
    status.configure(None, false, None);
    status.publish(next, sample);
    assert!(status.snapshot().is_none());
}
