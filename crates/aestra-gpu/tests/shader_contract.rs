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
fn wesl_modules_resolve_package_qualified_imports() {
    // A helper module imported by an entry-point module. The importer references it by its
    // package-qualified path; the resolver serves it from the supplied imports.
    let helper = "fn add(a: f32, b: f32) -> f32 { return a + b; }";
    let main = "import package::helpers::add;\n\
                @fragment fn probe() -> @location(0) vec4<f32> { return vec4<f32>(add(1.0, 2.0)); }";

    let compiled = aestra_gpu::shader::compile_wesl_with_imports(
        "package::main",
        main,
        &["probe"],
        &[("package::helpers", helper)],
    )
    .expect("a package-qualified import must resolve against the supplied modules");
    assert!(compiled.wgsl.contains("fn probe"));

    // Without the helper module supplied, the same import cannot resolve.
    assert!(
        aestra_gpu::shader::compile_wesl_with_imports("package::main", main, &["probe"], &[])
            .is_err(),
        "an import with no matching module must fail to compose"
    );
}

#[test]
fn authored_imports_remain_public_and_support_aliases_and_transitive_modules() {
    let root = "import package::helpers::evaluate as sample;\n\
                @fragment fn probe() -> @location(0) vec4<f32> { return vec4<f32>(sample()); }";
    let helper = "import package::constants::level;\nfn evaluate() -> f32 { return level; }";
    let compiled = aestra_gpu::shader::compile_wesl_with_imports(
        "package::main",
        root,
        &["probe"],
        &[
            ("package::helpers", helper),
            ("package::constants", "const level: f32 = 0.5;"),
            // A project library may also include the root asset. It must not shadow it.
            ("package::main", "this is not a shader"),
        ],
    )
    .unwrap();
    assert_translates_to_spirv(&compiled.wgsl);
    assert_translates_to_hlsl(&compiled.wgsl);
    assert!(compiled.wgsl.contains("fn probe"));
}

#[test]
fn invalid_authored_sources_keep_actionable_diagnostics() {
    use aestra_gpu::shader::{GpuShaderError, compile_wesl};

    for source in [
        "@fragment fn probe( {",
        "import package::missing::sample; @fragment fn probe() -> @location(0) vec4<f32> { return sample(); }",
    ] {
        let error = compile_wesl("package::broken", source, &["probe"]).unwrap_err();
        let GpuShaderError::Wesl {
            module,
            message,
            wesl,
        } = error
        else {
            panic!("expected an authored WESL diagnostic, got {error:?}");
        };
        assert_eq!(module, "package::broken");
        assert_eq!(wesl, source);
        assert!(!message.is_empty());
    }
}

#[test]
fn missing_entry_points_are_rejected_after_composition() {
    use aestra_gpu::shader::{GpuShaderError, compile_wesl};

    let error = compile_wesl(
        "package::missing_entry",
        "@compute @workgroup_size(1) fn present() {}",
        &["absent"],
    )
    .unwrap_err();
    assert_eq!(
        error,
        GpuShaderError::MissingEntryPoint {
            module: "package::missing_entry".to_owned(),
            entry_point: "absent".to_owned(),
        }
    );
}

#[test]
fn naga_rejects_invalid_shader_types_and_preserves_both_sources() {
    use aestra_gpu::shader::{GpuShaderError, compile_wesl};

    let source = "@fragment fn probe() -> @location(0) vec4<f32> { return vec3<f32>(1.0); }";
    let error = compile_wesl("package::invalid_type", source, &["probe"]).unwrap_err();
    let (module, message, wesl, wgsl) = match error {
        GpuShaderError::Wgsl {
            module,
            message,
            wesl,
            wgsl,
        }
        | GpuShaderError::Validation {
            module,
            message,
            wesl,
            wgsl,
        } => (module, message, wesl, wgsl),
        other => panic!("expected a Naga diagnostic, got {other:?}"),
    };
    assert_eq!(module, "package::invalid_type");
    assert_eq!(wesl, source);
    assert!(!message.is_empty());
    assert!(wgsl.contains("fn probe"));
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
fn paged_history_uses_portable_passes_with_eight_storage_bindings() {
    let shader = aestra_gpu::shader::compile_wesl(
        "package::paged_trails",
        aestra_gpu::shader::SIMULATION_WESL,
        &aestra_gpu::PAGED_TRAIL_ENTRY_POINTS,
    )
    .unwrap();
    assert_translates_to_spirv(&shader.wgsl);
    assert_translates_to_hlsl(&shader.wgsl);
    assert_eq!(shader.wgsl.matches("@binding(").count(), 8);
}

#[test]
fn trail_compaction_is_portable_and_keeps_existing_vertex_binding_budget() {
    let source = aestra_gpu::shader::trail_compact_wesl();
    assert!(!source.contains("bevy::"));
    let shader = aestra_gpu::shader::compile_wesl(
        "package::aestra_trail_compact",
        &source,
        &[
            "classify_trail",
            "prefix_trail",
            "prefix_trail_pages",
            "scatter_trail",
        ],
    )
    .unwrap();
    assert_translates_to_spirv(&shader.wgsl);
    assert_translates_to_hlsl(&shader.wgsl);
    assert!(SPRITE_RENDER_WESL.contains("alive_indices[4u + draw_index]"));
    assert!(!SPRITE_RENDER_WESL.contains("@binding(8)"));
}

#[test]
fn trail_history_culling_uses_portable_compute_and_shared_particle_abi() {
    let source = aestra_gpu::shader::trail_cull_wesl();
    assert!(!source.contains("bevy::"));
    let shader =
        aestra_gpu::shader::compile_wesl("package::aestra_trail_cull", &source, &["cull_trail"])
            .unwrap();
    assert_translates_to_spirv(&shader.wgsl);
    assert_translates_to_hlsl(&shader.wgsl);
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

#[test]
fn external_birth_capture_translates_to_spirv_and_hlsl() {
    for source in [
        aestra_gpu::DOMAIN_SPAWN_PLAN_WGSL.to_owned(),
        aestra_gpu::domain_spawn_wgsl(),
        aestra_gpu::PARTICLE_OUTPUT_WGSL.to_owned(),
    ] {
        assert_translates_to_spirv(&source);
        assert_translates_to_hlsl(&source);
    }
}
