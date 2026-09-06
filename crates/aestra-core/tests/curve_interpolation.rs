use aestra_core::{
    Curve, CurveInterpolation, CurveKey, EmitterTransform, HostTransformKey, HostTransformTrack,
};

#[test]
fn normalized_step_boundaries_share_a_rounding_tolerance_but_seconds_are_exact() {
    let mut curve = Curve::new(vec![CurveKey::new(0.0, 0.0), CurveKey::new(0.4, 1.0)]);
    curve.interpolation = CurveInterpolation::Step;
    assert_eq!(curve.sample(0.4_f32.next_down()), 1.0);
    assert_eq!(curve.sample(0.4 - 4.0 * f32::EPSILON), 0.0);
    assert_eq!(curve.sample_at(0.4_f32.next_down()), 0.0);
    assert_eq!(curve.sample_at(0.4), 1.0);
}

#[test]
fn interpolation_samples_keys_and_holds_endpoints_in_authored_seconds() {
    for (mode, expected) in [
        (CurveInterpolation::Step, 2.0),
        (CurveInterpolation::Linear, 4.0),
        (CurveInterpolation::Smooth, 3.25),
    ] {
        let mut curve = Curve::new(vec![CurveKey::new(0.0, 2.0), CurveKey::new(4.0, 10.0)]);
        curve.interpolation = mode;
        assert_eq!(curve.sample_at(1.0), expected);
        assert_eq!(curve.sample_at(-1.0), 2.0);
        assert_eq!(curve.sample_at(4.0), 10.0);
        assert_eq!(curve.sample_at(8.0), 10.0);
        curve.keys[1].time = f32::EPSILON / 2.0;
        assert_eq!(curve.sample_at(curve.keys[1].time), 10.0);
    }
}

#[test]
fn old_curves_remain_smooth_and_legacy_pose_tracks_remain_linear() {
    let curve = Curve::new(vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)]);
    let source = ron::to_string(&curve).unwrap();
    assert!(!source.contains("interpolation"));
    assert_eq!(
        ron::from_str::<Curve>(&source).unwrap().interpolation,
        CurveInterpolation::Smooth
    );
    let source = "(keys:[(time:0.0,transform:(translation:(0.0,0.0,0.0),rotation:(0.0,0.0,0.0,1.0),scale:(1.0,1.0,1.0))),(time:4.0,transform:(translation:(8.0,0.0,0.0),rotation:(0.0,0.0,0.0,1.0),scale:(1.0,1.0,1.0)))],repeat:false)";
    let track: HostTransformTrack = ron::from_str(source).unwrap();
    track.validate().unwrap();
    assert_eq!(
        track.curves.translation[0].interpolation,
        CurveInterpolation::Linear
    );
    assert_eq!(track.curves.sample_at(1.0).translation[0], 2.0);
    let canonical = ron::to_string(&track).unwrap();
    assert!(canonical.starts_with("(curves:"));
    assert_eq!(
        ron::from_str::<HostTransformTrack>(&canonical).unwrap(),
        track
    );
    assert_eq!(ron::from_str::<HostTransformTrack>(source).unwrap(), track);
}

#[test]
fn pose_edits_and_retiming_preserve_independent_channel_keys() {
    let mut track = HostTransformTrack::from_pose_keys(
        vec![
            HostTransformKey {
                time: 0.0,
                transform: EmitterTransform::default(),
            },
            HostTransformKey {
                time: 4.0,
                transform: EmitterTransform::default(),
            },
        ],
        false,
    );
    let before = track.curves.clone();
    let pose = EmitterTransform {
        translation: [5.0, 0.0, 0.0],
        ..Default::default()
    };
    track.set_pose(2.0, pose);
    assert_eq!(track.curves.translation[0].keys.len(), 3);
    assert_eq!(track.curves.translation[1..], before.translation[1..]);
    assert_eq!(track.curves.rotation, before.rotation);
    assert_eq!(track.curves.scale, before.scale);
    track.curves.retime(2.0, 3.0);
    assert_eq!(track.curves.key_times(), vec![0.0, 3.0, 4.0]);
    track.curves.remove_time(3.0);
    assert_eq!(track.curves, before);
    track.validate().unwrap();
}

#[test]
fn rotation_modes_keep_unit_quaternions_and_shortest_arc() {
    let mut track = HostTransformTrack::from_pose_keys(
        vec![
            HostTransformKey {
                time: 0.0,
                transform: EmitterTransform::default(),
            },
            HostTransformKey {
                time: 4.0,
                transform: EmitterTransform {
                    rotation: [
                        0.0,
                        0.0,
                        -std::f32::consts::FRAC_1_SQRT_2,
                        -std::f32::consts::FRAC_1_SQRT_2,
                    ],
                    ..Default::default()
                },
            },
        ],
        false,
    );
    for mode in [
        CurveInterpolation::Step,
        CurveInterpolation::Linear,
        CurveInterpolation::Smooth,
    ] {
        track.curves.rotation.interpolation = mode;
        let q = track.curves.rotation.sample_at(1.0);
        let angle = mode.weight(0.25) * std::f32::consts::FRAC_PI_4;
        assert!((q[2] - angle.sin()).abs() < 1e-6);
        assert!((q[3] - angle.cos()).abs() < 1e-6);
        assert!((q.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-6);
    }
}
