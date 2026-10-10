//! Shared main-world presentation payload and material bindings.
use crate::material::{
    MaterialBindingContext, MaterialBindingError, MaterialRuntimeBinding, compile_material_program,
};
use aestra_core::{AssetId, EmitterId, MaterialId, MaterialProgramId};
use aestra_gpu::material::CompiledMaterialProgram;
use aestra_runtime::{
    CompiledEffect, EffectInstance, ParticleSample, PlaybackHistoryPolicy, SeekQuality,
};
use bevy::prelude::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// Selects how an effect's presentation geometry is shaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectRenderMode {
    #[default]
    Rendered,
    Wireframe,
}

/// Renderer input for one effect instance. Playback clocks and application events live elsewhere.
#[derive(Component, Debug, Clone)]
#[require(Transform, Visibility)]
pub struct PresentedEffect {
    pub instance: EffectInstance,
    /// Stable across extraction clones, distinct for a replacement presentation
    /// even when it reuses the same compiled artifact and starts at epoch zero.
    pub(crate) output_identity: Arc<()>,
    /// The fidelity requested of stateful seeks this frame (hybrid roadmap M12). `Exact` by default;
    /// a host sets `Preview` for the duration of a scrub gesture (synced from the player) so the GPU
    /// backend bounds its per-frame reconstruction and keeps scrubbing responsive.
    pub(crate) seek_quality: SeekQuality,
    pub(crate) render_mode: EffectRenderMode,
    pub(crate) texture_overrides: BTreeMap<AssetId, Handle<Image>>,
    pub(crate) mesh_overrides: BTreeMap<AssetId, Handle<bevy::prelude::Mesh>>,
    pub(crate) material_bindings: BTreeMap<MaterialId, MaterialRuntimeBinding>,
    pub(crate) compiled_material_programs:
        BTreeMap<MaterialProgramId, Arc<CompiledMaterialProgram>>,
    pub(crate) automatic_material_bindings:
        BTreeMap<(EmitterId, MaterialId), MaterialRuntimeBinding>,
    pub(crate) cpu_samples: Vec<ParticleSample>,
    pub(crate) gpu_samples: Vec<ParticleSample>,
    pub(crate) cpu_evaluation_time: Option<Duration>,
}

impl PresentedEffect {
    pub fn with_history_policy(mut self, policy: PlaybackHistoryPolicy) -> Self {
        self.set_history_policy(policy);
        self
    }

    pub fn history_policy(&self) -> PlaybackHistoryPolicy {
        self.instance.history_policy()
    }

    /// Drop cached snapshots on the next render preparation, keeping live state.
    /// Explicit seeks without snapshots reconstruct from zero when necessary.
    pub fn set_history_policy(&mut self, policy: PlaybackHistoryPolicy) {
        self.instance.set_history_policy(policy);
    }
    pub fn new(effect: Arc<CompiledEffect>) -> Self {
        let mut presented = Self {
            instance: EffectInstance::new(effect),
            output_identity: Arc::new(()),
            seek_quality: SeekQuality::Exact,
            render_mode: EffectRenderMode::Rendered,
            texture_overrides: BTreeMap::new(),
            mesh_overrides: BTreeMap::new(),
            material_bindings: BTreeMap::new(),
            compiled_material_programs: BTreeMap::new(),
            automatic_material_bindings: BTreeMap::new(),
            cpu_samples: Vec::new(),
            gpu_samples: Vec::new(),
            cpu_evaluation_time: None,
        };
        presented.rebuild_automatic_material_bindings();
        presented
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.instance.effect()
    }

    /// Supplies an instance-owned image instead of loading its registry path. Configure before
    /// spawning/preparing the player. Useful for isolated previews without replacing shared assets.
    pub fn with_texture_overrides(mut self, textures: BTreeMap<AssetId, Handle<Image>>) -> Self {
        self.texture_overrides = textures;
        self
    }

    pub(crate) fn texture_override(&self, asset: AssetId) -> Option<&Handle<Image>> {
        self.texture_overrides.get(&asset)
    }

    /// Instance-owned geometry for isolated previews; shared path-loaded meshes are unchanged.
    pub fn with_mesh_overrides(
        mut self,
        meshes: BTreeMap<AssetId, Handle<bevy::prelude::Mesh>>,
    ) -> Self {
        self.mesh_overrides = meshes;
        self
    }

    pub(crate) fn mesh_override(&self, asset: AssetId) -> Option<&Handle<bevy::prelude::Mesh>> {
        self.mesh_overrides.get(&asset)
    }

    pub fn simulation_time(&self) -> f32 {
        self.instance.time()
    }

    /// The fidelity requested of stateful seeks this frame (hybrid roadmap M12).
    pub fn seek_quality(&self) -> SeekQuality {
        self.seek_quality
    }

    /// Sets the requested stateful-seek fidelity for this frame (hybrid roadmap M12). The plugin syncs
    /// this from the player; `Preview` bounds the GPU reconstruction during scrubbing.
    pub fn set_seek_quality(&mut self, quality: SeekQuality) {
        self.seek_quality = quality;
    }

    pub fn render_mode(&self) -> EffectRenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, mode: EffectRenderMode) {
        self.render_mode = mode;
    }

    /// Selects the semantic material program and instance values used by one renderer material.
    ///
    /// Renderers without a binding continue through the legacy compatibility shader.
    pub fn bind_material(&mut self, material: MaterialId, binding: MaterialRuntimeBinding) {
        self.material_bindings.insert(material, binding);
    }

    pub fn unbind_material(&mut self, material: MaterialId) -> Option<MaterialRuntimeBinding> {
        self.material_bindings.remove(&material)
    }

    pub fn material_binding(&self, material: MaterialId) -> Option<&MaterialRuntimeBinding> {
        self.material_bindings.get(&material).or_else(|| {
            self.automatic_material_bindings
                .iter()
                .find_map(|((_, candidate), binding)| (*candidate == material).then_some(binding))
        })
    }

    pub fn material_binding_for_emitter(
        &self,
        material: MaterialId,
        emitter: EmitterId,
    ) -> Option<&MaterialRuntimeBinding> {
        self.material_bindings
            .get(&material)
            .or_else(|| self.automatic_material_bindings.get(&(emitter, material)))
    }

    /// Re-resolves automatic bindings against the current effect/emitter parameter state.
    pub fn refresh_automatic_material_bindings(&mut self) {
        let instance = &self.instance;
        for ((emitter, _), binding) in &mut self.automatic_material_bindings {
            let context = MaterialBindingContext::for_emitter(instance, *emitter);
            if let Err(error) = binding.refresh_dynamic_values(&context) {
                bevy::log::warn!("semantic material binding could not be refreshed: {error}");
            }
        }
    }

    /// Refreshes one bound material after effect/emitter automation or parameter edits.
    ///
    /// This updates only dynamic uniform/resource values. It retains the compiled shader and
    /// pipeline-compatible program already held by the binding.
    pub fn refresh_material_binding(
        &mut self,
        material: MaterialId,
        context: &MaterialBindingContext,
    ) -> Result<(), MaterialBindingError> {
        let binding = self
            .material_bindings
            .get_mut(&material)
            .ok_or(MaterialBindingError::UnknownMaterial(material))?;
        binding.refresh_dynamic_values(context)
    }

    fn rebuild_automatic_material_bindings(&mut self) {
        self.compiled_material_programs.clear();
        self.automatic_material_bindings.clear();
        let effect = Arc::clone(self.effect());
        for program in &effect.material_programs {
            match compile_material_program(program) {
                Ok(compiled) => {
                    self.compiled_material_programs.insert(program.id, compiled);
                }
                Err(error) => bevy::log::warn!(
                    "semantic material program {} could not be compiled: {error}",
                    program.id
                ),
            }
        }
        for emitter in &effect.emitters {
            let context = MaterialBindingContext::for_emitter(&self.instance, emitter.source);
            for renderer in &emitter.renderers {
                let Some(instance) = effect.material_instance(renderer.material) else {
                    continue;
                };
                let Some(program) = self.compiled_material_programs.get(&instance.program.id())
                else {
                    continue;
                };
                match MaterialRuntimeBinding::from_instance_with_context(
                    Arc::clone(program),
                    instance,
                    &context,
                ) {
                    Ok(binding) => {
                        self.automatic_material_bindings
                            .insert((emitter.source, renderer.material), binding);
                    }
                    Err(error) => bevy::log::warn!(
                        "semantic material {} could not be resolved for emitter {}: {error}",
                        renderer.material,
                        emitter.source
                    ),
                }
            }
        }
    }

    pub fn gpu_samples(&self) -> &[ParticleSample] {
        &self.gpu_samples
    }

    /// Samples from the most recent CPU evaluation, independent of buffered GPU readback.
    pub fn cpu_samples(&self) -> &[ParticleSample] {
        &self.cpu_samples
    }

    pub fn samples(&self) -> &[ParticleSample] {
        if self.gpu_samples.is_empty() {
            &self.cpu_samples
        } else {
            &self.gpu_samples
        }
    }

    /// Duration of the most recent CPU-reference evaluation, when the active backend used one.
    pub fn cpu_evaluation_time(&self) -> Option<Duration> {
        self.cpu_evaluation_time
    }

    pub fn take_gpu_samples(&mut self) -> Vec<ParticleSample> {
        std::mem::take(&mut self.gpu_samples)
    }

    pub fn restore_gpu_samples(&mut self, samples: Vec<ParticleSample>) {
        self.gpu_samples = samples;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_overrides_are_owned_by_one_presentation_not_the_shared_effect() {
        let effect = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&aestra_core::EffectAsset::new("Preview", 3.0))
                .unwrap(),
        );
        let asset = aestra_core::AssetId::from_u128(123);
        let mut images = Assets::<Image>::default();
        let handle = images.add(Image::default());
        let mut meshes = Assets::<Mesh>::default();
        let mesh = meshes.add(Cuboid::default());
        let active = PresentedEffect::new(effect.clone());
        let preview = PresentedEffect::new(effect.clone())
            .with_texture_overrides(BTreeMap::from([(asset, handle.clone())]))
            .with_mesh_overrides(BTreeMap::from([(asset, mesh.clone())]));
        assert!(active.texture_override(asset).is_none());
        assert_eq!(preview.texture_override(asset), Some(&handle));
        assert!(active.mesh_override(asset).is_none());
        assert_eq!(preview.mesh_override(asset), Some(&mesh));
        assert!(Arc::ptr_eq(active.effect(), preview.effect()));
        assert!(effect.assets.is_empty());
    }
}
