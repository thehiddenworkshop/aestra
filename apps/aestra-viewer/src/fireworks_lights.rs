//! Viewer validation geometry and authored light-output acceptance.
use bevy::prelude::*;
/// Neutral diffuse cards near shell height make illumination distinct from bloom
/// or emissive VFX. They are host validation geometry, not part of the saved show.
pub fn spawn_receivers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mesh = meshes.add(Cuboid::new(10.0, 12.0, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.6),
        perceptual_roughness: 1.0,
        ..default()
    });
    for x in [-38.0, 38.0] {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x, 40.0, -6.0),
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// Fixture authoring only: saved assets carry this data; playback hosts do not.
    pub(crate) fn author_lights(shell: &mut aestra_bevy::EffectAsset) {
        let base = shell.id.as_uuid().as_u128();
        let mut pulse = aestra_bevy::PointLightPulse::flash([1.0; 3], 500_000.0, 80.0, 0.6);
        pulse.intensity_lumens.id = aestra_bevy::CurveId::from_u128(base + 960);
        pulse.range.id = aestra_bevy::CurveId::from_u128(base + 961);
        let mut binding = aestra_bevy::PointLightBinding::new(shell.particle_outputs[1].id, pulse);
        binding.id = aestra_bevy::EventRouteId::from_u128(base + 930);
        binding.color_parameter = Some(aestra_bevy::LightColorParameter {
            parameter: shell
                .parameters
                .iter()
                .find(|p| p.name == "Main stars color")
                .unwrap()
                .id,
            normalized_age: 0.5,
        });
        shell.point_lights = vec![binding];
    }

    #[test]
    fn saved_bindings_respect_each_clip_color_and_do_not_multiply_packet_magnitude() {
        use aestra_bevy::{EffectOutputEvent, EventOrigin, QualityTier, RuntimeValue};
        let source = crate::fireworks_show::effect();
        let index = aestra_project::ProjectAssetIndex::scan(crate::viewer_asset_root(None));
        let resolved = index.resolve_effect_project(&source).unwrap();
        for tier in QualityTier::presets() {
            let project = aestra_bevy::EffectCompiler::default()
                .with_tier(tier)
                .compile_resolved_project(&resolved)
                .unwrap();
            for clip in &project.root.effect_clips {
                let source = project.effect(clip.source.id).unwrap();
                assert_eq!(source.point_lights.len(), 1);
                let color = clip
                    .parameter_overrides
                    .iter()
                    .find_map(|p| match &p.value {
                        RuntimeValue::Gradient(g) => Some(g.sample(0.5)),
                        _ => None,
                    })
                    .unwrap();
                let event = EffectOutputEvent::new(
                    "main_break",
                    EventOrigin::Emitter(0),
                    "",
                    vec![0.0; 3],
                    4096.0,
                    80,
                );
                let (_, pulse) = project
                    .point_light_for_output(&[clip.source_clip], clip.source.id, &event, &[])
                    .unwrap()
                    .unwrap();
                assert_eq!(pulse.linear_color, [color[0], color[1], color[2]]);
                assert_eq!(pulse.intensity_lumens.sample(0.0), 500_000.0);
                let mut unbound = event.clone();
                unbound.kind = "crackle".into();
                assert!(
                    project
                        .point_light_for_output(&[clip.source_clip], clip.source.id, &unbound, &[])
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
    #[test]
    fn light_flag_is_opt_in_and_rejects_unbound_modes() {
        assert!(
            !crate::ViewerConfig::from_iter(Vec::<String>::new())
                .unwrap()
                .transient_lights
        );
        assert!(crate::ViewerConfig::from_iter(["--transient-lights"].map(String::from)).is_err());
        let args = [
            "--fireworks-f0",
            "--fireworks-f0-probe",
            "f6-show",
            "--backend",
            "gpu",
            "--transient-lights",
        ]
        .map(String::from);
        assert!(
            crate::ViewerConfig::from_iter(args.clone())
                .unwrap()
                .transient_lights
        );
        let mut cpu = args;
        cpu[4] = "cpu".into();
        assert!(crate::ViewerConfig::from_iter(cpu).is_err());
    }

    #[test]
    #[ignore = "requires the documented native receivers-on/off-final captures"]
    fn native_receivers_respond_without_bloom_and_return_to_baseline() {
        let directory = std::env::var_os("AESTRA_LIGHT_CAPTURE_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fireworks-f7")
            });
        let mut red_changes = Vec::new();
        for enabled in [false, true] {
            let name = if enabled {
                "receivers-on-final"
            } else {
                "receivers-off-final"
            };
            let report: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.join(name).join("preview-report.json")).unwrap(),
            )
            .unwrap();
            let response = &report["capture"]["response"];
            assert_eq!(response["hdr"], true);
            assert_eq!(response["bloom_intensity"], 0.0);
            assert_eq!(response["transient_lights"], enabled);
            let frames: Vec<_> = report["capture"]["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|frame| frame["simulation_frame"].as_u64().unwrap())
                .collect();
            assert_eq!(frames, [75, 85, 90, 100, 120]);
        }
        for index in 0..5 {
            let file = format!("frame-{index:03}.png");
            let off = image::open(directory.join("receivers-off-final").join(&file))
                .unwrap()
                .to_rgb8();
            let on = image::open(directory.join("receivers-on-final").join(&file))
                .unwrap()
                .to_rgb8();
            assert_eq!(off.dimensions(), (960, 540));
            assert_eq!(on.dimensions(), off.dimensions());
            if index == 0 || index == 4 {
                assert_eq!(on, off, "before birth and after expiry must match exactly");
            } else {
                // Left diffuse panel ROI, outside the early star footprint at frame 85.
                let red_delta: f32 = (215..240)
                    .flat_map(|y| (330..365).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        f32::from(on.get_pixel(x, y)[0]) - f32::from(off.get_pixel(x, y)[0])
                    })
                    .sum::<f32>()
                    / (25.0 * 35.0);
                red_changes.push(red_delta);
            }
        }
        assert!(
            red_changes[0] > 2.0,
            "receiver has no measurable burst response: {red_changes:?}"
        );
        assert!(
            red_changes
                .windows(2)
                .all(|pair| pair[0] > pair[1] && pair[1] > 0.0),
            "receiver should fade with occurrence-time envelope: {red_changes:?}"
        );
    }
}
