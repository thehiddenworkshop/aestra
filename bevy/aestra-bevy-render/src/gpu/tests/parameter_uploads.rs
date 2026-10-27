// Native regressions included in coupled_tests to share its real-device fixtures.
#[test]
fn parameter_pages_preserve_order_across_rollover_and_unwaited_submissions() {
    let Some(scene) = scene(false) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        return;
    };
    use bevy::render::render_resource::BufferDescriptor;
    let buffer = |bytes| scene.device.create_buffer(&BufferDescriptor {
        label: None, size: bytes,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let destination = buffer(1024);
    let captures = [buffer(192 * 1024), buffer(192 * 1024)];
    let mut uploads = super::parameter_uploads::ParameterUploads::default();
    for (frame, capture) in captures.iter().enumerate() {
        uploads.begin();
        let mut encoder = scene.device.create_command_encoder(&Default::default());
        for copy in 0..192 {
            let bytes = vec![(frame * 197 + copy) as u8; 1024];
            uploads.copy(&scene.device, &mut encoder, &destination, &bytes);
            encoder.copy_buffer_to_buffer(&destination, 0, capture, copy as u64 * 1024, 1024);
        }
        uploads.flush(&scene.queue);
        assert_eq!(uploads.retained(), (3, 3 * 65536));
        scene.queue.submit([encoder.finish()]);
        // Deliberately no poll/wait between frames: new writes must follow the
        // previously submitted copies, even though they reuse the same pages.
    }
    for (frame, capture) in captures.iter().enumerate() {
        let bytes = read_back(&scene.device, &scene.queue, capture);
        for (copy, chunk) in bytes.as_chunks::<1024>().0.iter().enumerate() {
            assert!(chunk.iter().all(|b| *b == (frame * 197 + copy) as u8),
                "wrong historical input at frame {frame}, copy {copy}");
        }
    }
    uploads.begin();
    uploads.flush(&scene.queue);
    assert_eq!(uploads.retained(), (0, 0));
}

#[test]
fn parameter_groups_reuse_current_resources_and_physics_changes_invalidate() {
    let Some(mut test) = sized_event_trail_scene(false, 16) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        return;
    };
    test.frame(1, 4);
    super::encoding_profile::begin();
    test.frame(2, 4);
    let reused = super::encoding_profile::finish();
    assert!(!reused.iter().any(|s| matches!(s.name, "parameter_destination" | "stateful_bind_group")));
    let first = cached_physics_scene_buffer(&test.scene.device, &mut test.scene.states, &[0; 4]);
    let second = cached_physics_scene_buffer(&test.scene.device, &mut test.scene.states, &[0; 4]);
    assert_eq!(first.id(), second.id());
    let changed = cached_physics_scene_buffer(&test.scene.device, &mut test.scene.states, &[1, 0, 0, 0]);
    assert_ne!(first.id(), changed.id());
    // Replace the fixture's current world/physics buffer with equal contents but
    // a different resource identity. Every cached group must bind the new resource.
    test.scene.no_world = first;
    super::encoding_profile::begin();
    test.frame(3, 4);
    let changed = super::encoding_profile::finish();
    assert_eq!(changed.iter().find(|s| s.name == "stateful_bind_group").unwrap().bind_groups, 2);
}
