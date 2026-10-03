//! Example-host bindings. The runtime and adapter know nothing about fireworks.
use aestra_bevy::{
    AestraLightOutput, AestraOutputEvent, CompiledEffectProject, EffectClipId, PointLightPulse,
    QualityTier, RuntimeValue, TransientLightSettings,
};
use bevy::prelude::*;
use std::collections::BTreeMap;

#[derive(Resource)]
pub struct Bindings {
    pulses: BTreeMap<EffectClipId, PointLightPulse>,
    budget: usize,
}

impl Bindings {
    pub fn new(project: &CompiledEffectProject, tier: &QualityTier) -> Result<Self, String> {
        let mut pulses = BTreeMap::new();
        for clip in &project.root.effect_clips {
            let source = project
                .effect(clip.source.id)
                .ok_or("missing light source")?;
            let parameter = source
                .parameters
                .iter()
                .find(|p| p.name == "Main stars color")
                .ok_or("missing bound star color")?;
            // These example-host bindings respect each occurrence's exposed color override.
            // Not an authored light-output schema, nor a lookup of transient child entities.
            let color = clip
                .parameter_overrides
                .iter()
                .find(|p| p.source == parameter.source)
                .map_or(&parameter.default, |p| &p.value);
            let RuntimeValue::Gradient(gradient) = color else {
                return Err("bound star color must be a gradient".into());
            };
            let [r, g, b, _] = gradient.sample(0.5);
            let pulse = PointLightPulse::flash([r, g, b], 500_000.0, 80.0, 0.6);
            if !pulse.is_valid() {
                return Err("invalid bound light pulse".into());
            }
            pulses.insert(clip.source_clip, pulse);
        }
        Ok(Self {
            pulses,
            budget: match tier.name.as_str() {
                "high" => 8,
                "medium" => 4,
                _ => 2,
            },
        })
    }

    pub fn settings(&self) -> TransientLightSettings {
        TransientLightSettings {
            max_lights: self.budget,
            ..default()
        }
    }

    fn intent(&self, output: &AestraOutputEvent) -> Option<AestraLightOutput> {
        if output.event.kind != "main_break" {
            return None;
        }
        let [clip] = output.clip_path.as_slice() else {
            return None;
        };
        AestraLightOutput::from_particle(output, self.pulses.get(clip)?.clone())
    }
}

pub fn bind(
    binding: Res<Bindings>,
    mut outputs: MessageReader<AestraOutputEvent>,
    mut lights: MessageWriter<AestraLightOutput>,
) {
    for output in outputs.read() {
        if let Some(intent) = binding.intent(output) {
            lights.write(intent);
        }
    }
}

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
mod tests {
    use super::*;
    #[test]
    fn host_binding_respects_clip_colors_and_emits_one_pulse_not_one_per_star() {
        let source = crate::fireworks_show::effect();
        let index = aestra_project::ProjectAssetIndex::scan(crate::viewer_asset_root(None));
        let resolved = index.resolve_effect_project(&source).unwrap();
        for (tier, budget) in QualityTier::presets().into_iter().zip([8, 4, 2]) {
            let project = aestra_bevy::EffectCompiler::default()
                .with_tier(tier.clone())
                .compile_resolved_project(&resolved)
                .unwrap();
            let binding = Bindings::new(&project, &tier).unwrap();
            assert_eq!(binding.settings().max_lights, budget);
            assert_eq!(binding.pulses.len(), 13);
            for clip in &project.root.effect_clips {
                let color = clip
                    .parameter_overrides
                    .iter()
                    .find_map(|p| match &p.value {
                        RuntimeValue::Gradient(g) => Some(g.sample(0.5)),
                        _ => None,
                    })
                    .unwrap();
                let pulse = &binding.pulses[&clip.source_clip];
                assert_eq!(pulse.linear_color, [color[0], color[1], color[2]]);
                let mut output = AestraOutputEvent::root(
                    Entity::PLACEHOLDER,
                    aestra_bevy::EffectOutputEvent::new(
                        "main_break",
                        aestra_bevy::EventOrigin::Emitter(0),
                        "",
                        vec![0.0; 3],
                        4096.0,
                        64,
                    ),
                )
                .in_epoch(3);
                output.clip_path = vec![clip.source_clip];
                output.particle = Some(aestra_bevy::ParticleOutputContext {
                    source_effect: clip.source.id,
                    seed: 7,
                    root_time_seconds: clip.start_time + 64.0 / 60.0,
                    world_position: Some([1.0, 2.0, 3.0]),
                });
                let intent = binding.intent(&output).unwrap();
                assert_eq!(intent.light.world_position, [1.0, 2.0, 3.0]);
                assert_eq!(intent.playback_epoch, 3);
                assert_eq!(intent.light.pulse.intensity_lumens.sample(0.0), 500_000.0);
                output.event.kind = "crackle".into();
                assert!(binding.intent(&output).is_none());
                output.event.kind = "main_break".into();
                output.particle = None;
                assert!(binding.intent(&output).is_none());
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
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fireworks-f7");
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
