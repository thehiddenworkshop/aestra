use std::sync::Arc;

use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, Emitter};
use aestra_gpu::{
    GpuEffectArtifact,
    shader::{GpuShaderPackage, SIMULATION_WESL, SPRITE_RENDER_WESL},
};
use aestra_runtime::EffectInstance;
use naga::{
    Module,
    back::{hlsl, spv},
    valid::{Capabilities, ModuleInfo, ValidationFlags, Validator},
};

fn representative_artifact() -> GpuEffectArtifact {
    let mut effect = EffectAsset::new("Shader contract", 2.0);
    effect
        .emitters
        .push(Emitter::basic_sprite("Emitter", effect.duration));
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap()
}

fn normalized(source: &str) -> String {
    source.replace("\r\n", "\n").trim_end().to_owned()
}

fn validated_module(wgsl: &str) -> (Module, ModuleInfo) {
    let module = naga::front::wgsl::parse_str(wgsl).expect("generated WGSL must parse");
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .expect("generated WGSL must validate");
    (module, info)
}

fn assert_translates_to_spirv(wgsl: &str) {
    const SPIRV_MAGIC: u32 = 0x0723_0203;

    let (module, info) = validated_module(wgsl);
    let words = spv::write_vec(&module, &info, &spv::Options::default(), None)
        .expect("validated WGSL must translate to SPIR-V");

    assert_eq!(words.first(), Some(&SPIRV_MAGIC));
    assert!(words.len() > 5, "SPIR-V output must contain a module");
}

fn assert_translates_to_hlsl(wgsl: &str) {
    let (module, info) = validated_module(wgsl);
    let options = hlsl::Options::default();
    let pipeline_options = hlsl::PipelineOptions::default();
    let mut output = String::new();
    let reflection = hlsl::Writer::new(&mut output, &options, &pipeline_options)
        .write(&module, &info, None)
        .expect("validated WGSL must translate to HLSL");

    assert!(!output.trim().is_empty(), "HLSL output must not be empty");
    assert!(
        reflection.entry_point_names.iter().all(Result::is_ok),
        "every portable shader entry point must translate to HLSL"
    );
}

#[test]
fn representative_artifact_produces_naga_validated_shader_package() {
    let artifact = representative_artifact();
    let package = GpuShaderPackage::for_artifact(&artifact).unwrap();

    assert_eq!(package.layout.emitter_count, 1);
    assert_eq!(package.layout.renderer_count, 1);
    assert_eq!(package.layout.total_particle_slots, artifact.total_slots);
    assert!(package.simulation.wgsl.contains("fn simulate"));
    assert!(package.sprite_render.wgsl.contains("fn vertex"));
}

#[test]
fn mesh_wireframe_uses_shared_geometry_and_portable_line_shader() {
    let shader = aestra_gpu::shader::compile_wesl(
        "package::aestra_mesh_wireframe",
        &aestra_gpu::shader::mesh_wireframe_wesl(),
        &["vertex_mesh_wireframe", "fragment_mesh_wireframe"],
    )
    .unwrap();
    assert!(shader.wgsl.contains("aestra_mesh_vertex"));
    assert_translates_to_spirv(&shader.wgsl);
    assert_translates_to_hlsl(&shader.wgsl);
}

#[test]
fn generated_wgsl_matches_reviewable_snapshots() {
    let package = GpuShaderPackage::for_artifact(&representative_artifact()).unwrap();
    if std::env::var_os("AESTRA_UPDATE_SHADER_SNAPSHOTS").is_some() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        std::fs::write(root.join("simulation.wgsl"), &package.simulation.wgsl).unwrap();
        std::fs::write(root.join("sprite_render.wgsl"), &package.sprite_render.wgsl).unwrap();
        return;
    }

    assert_eq!(
        normalized(&package.simulation.wgsl),
        normalized(include_str!("snapshots/simulation.wgsl"))
    );
    assert_eq!(
        normalized(&package.sprite_render.wgsl),
        normalized(include_str!("snapshots/sprite_render.wgsl"))
    );
}

#[test]
fn portable_wesl_has_no_engine_shader_imports() {
    for source in [SIMULATION_WESL, SPRITE_RENDER_WESL] {
        assert!(!source.contains("#import bevy"));
        assert!(!source.contains("bevy::"));
        assert!(!source.contains("wgpu::"));
    }
}

#[test]
fn generated_shaders_translate_to_portable_backend_targets() {
    let package = GpuShaderPackage::for_artifact(&representative_artifact()).unwrap();

    for shader in [&package.simulation, &package.sprite_render] {
        assert_translates_to_spirv(&shader.wgsl);
        assert_translates_to_hlsl(&shader.wgsl);
    }
}
