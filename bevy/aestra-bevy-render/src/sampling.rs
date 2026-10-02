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

/// Opt-in minimum projected width for native additive Trail bodies and round caps.
/// Not history sampling/LOD: authored widths, simulation and retained points are unchanged.
/// Expanded bodies attenuate final alpha by inverse width; caps by inverse area.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct TrailRasterSampling {
    /// Physical main-pass pixels; 0 disables treatment. Normalized to finite 0..8.
    pub minimum_pixels: f32,
}

impl TrailRasterSampling {
    pub fn normalized(self) -> Self {
        Self {
            minimum_pixels: SpriteSampling {
                minimum_pixels: self.minimum_pixels,
            }
            .normalized()
            .minimum_pixels,
        }
    }

    pub(crate) fn apply(self, renderers: &mut [aestra_gpu::GpuRenderer]) {
        let pixels = self.normalized().minimum_pixels;
        for renderer in renderers {
            if renderer.renderer_kind == 4
                && renderer.blend_mode == aestra_gpu::GpuBlend::Additive as u32
            {
                // Trail records only use frames[0].x for tile length. Reserve the last W
                // lane for presentation width without changing the shared renderer ABI.
                renderer.frames[63].w = pixels;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trail_raster_policy_preserves_width_history_and_other_renderer_kinds() {
        let mut source = aestra_core::EffectAsset::new("Trail sampling", 1.0);
        source
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Test", 1.0));
        let compiled = std::sync::Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&source)
                .unwrap(),
        );
        let baseline = aestra_gpu::GpuEffectArtifact::from_instance(
            &aestra_runtime::EffectInstance::new(compiled),
        )
        .unwrap()
        .renderers[0];
        for kind in 0..5 {
            for blend in [aestra_gpu::GpuBlend::Alpha, aestra_gpu::GpuBlend::Additive] {
                let mut original = baseline;
                original.renderer_kind = kind;
                if kind == 4 {
                    original.frames[63].w = 0.0;
                }
                original.blend_mode = blend as u32;
                original.attribute_flags.y = 0.17_f32.to_bits();
                original.frames[0].x = 12.0;
                let mut renderers = [original];
                TrailRasterSampling {
                    minimum_pixels: 2.0,
                }
                .apply(&mut renderers);
                let mut expected = original;
                if kind == 4 && blend == aestra_gpu::GpuBlend::Additive {
                    expected.frames[63].w = 2.0;
                }
                assert_eq!(storage(&renderers[0]), storage(&expected));
                TrailRasterSampling::default().apply(&mut renderers);
                assert_eq!(storage(&renderers[0]), storage(&original));
            }
        }
        for input in [f32::NAN, f32::INFINITY, -1.0] {
            assert_eq!(
                TrailRasterSampling {
                    minimum_pixels: input
                }
                .normalized()
                .minimum_pixels,
                0.0
            );
        }
        assert_eq!(
            TrailRasterSampling {
                minimum_pixels: 100.0
            }
            .normalized()
            .minimum_pixels,
            8.0
        );
    }

    fn storage(value: &aestra_gpu::GpuRenderer) -> Vec<u8> {
        let mut buffer = bevy::render::render_resource::encase::StorageBuffer::new(Vec::new());
        buffer.write(value).unwrap();
        buffer.into_inner()
    }

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
