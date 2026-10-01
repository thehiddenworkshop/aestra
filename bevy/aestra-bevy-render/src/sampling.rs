//! Opt-in native presentation policy; authored simulation and compiled assets are unchanged.
use bevy::prelude::Resource;

/// Enlarge undersampled additive sprite quads, attenuating their alpha by inverse area.
/// Native GPU sprite draws only (not flipbooks, meshes, ribbons, trails or CPU reference).
/// This preserves continuous footprint energy, not exact pixel-integrated radiometry.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct SpriteSampling {
    /// Minimum shorter quad axis in physical main-pass pixels; 0 disables treatment.
    /// Normalized to 0..8; non-finite input disables it. Start with 2 pixels.
    pub minimum_pixels: f32,
}

impl SpriteSampling {
    pub fn normalized(self) -> Self {
        Self {
            minimum_pixels: if self.minimum_pixels.is_finite() {
                self.minimum_pixels.clamp(0.0, 8.0)
            } else {
                0.0
            },
        }
    }

    pub(crate) fn apply(self, renderers: &mut [aestra_gpu::GpuRenderer]) {
        let pixels = self.normalized().minimum_pixels;
        for renderer in renderers {
            if renderer.renderer_kind == 0
                && renderer.blend_mode == aestra_gpu::GpuBlend::Additive as u32
            {
                // This lane is strip width on ribbon/trail records, unused on sprite records.
                renderer.attribute_flags.y = pixels.to_bits();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_is_opt_in_finite_and_only_changes_additive_sprite_presentation() {
        assert_eq!(SpriteSampling::default().minimum_pixels, 0.0);
        for (input, expected) in [
            (f32::NAN, 0.0),
            (f32::INFINITY, 0.0),
            (-1.0, 0.0),
            (20.0, 8.0),
            (2.0, 2.0),
        ] {
            assert_eq!(
                SpriteSampling {
                    minimum_pixels: input
                }
                .normalized()
                .minimum_pixels,
                expected
            );
        }
        let mut source = aestra_core::EffectAsset::new("Sampling", 1.0);
        source
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Stars", 1.0));
        let compiled = std::sync::Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&source)
                .unwrap(),
        );
        let mut artifact = aestra_gpu::GpuEffectArtifact::from_instance(
            &aestra_runtime::EffectInstance::new(compiled),
        )
        .unwrap();
        let baseline = artifact.renderers[0];
        artifact.renderers = (0..5)
            .map(|kind| aestra_gpu::GpuRenderer {
                renderer_kind: kind,
                ..baseline
            })
            .collect();
        artifact.renderers.push(aestra_gpu::GpuRenderer {
            blend_mode: aestra_gpu::GpuBlend::Alpha as u32,
            ..baseline
        });
        SpriteSampling {
            minimum_pixels: 2.0,
        }
        .apply(&mut artifact.renderers);
        assert_eq!(f32::from_bits(artifact.renderers[0].attribute_flags.y), 2.0);
        for renderer in &artifact.renderers[1..] {
            assert_eq!(renderer.attribute_flags, baseline.attribute_flags);
        }
        SpriteSampling::default().apply(&mut artifact.renderers);
        assert_eq!(
            artifact.renderers[0].attribute_flags,
            baseline.attribute_flags
        );
    }
}
