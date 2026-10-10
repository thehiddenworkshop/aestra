//! Compile production shader bodies through Bevy 0.20's actual ShaderCache and
//! published WESL libraries. No stub View/cluster/UI definitions or regex preprocessing.
#[path = "../../../bevy/aestra-bevy-render/src/gpu/shader_composition.rs"]
mod composition;

mod shader_support;
use bevy::shader::ShaderDefVal;
use composition::Dialect;
use shader_support::{Compiled, compile};
use std::path::Path;

fn entry_points(compiled: &Compiled, expected: &[(&str, naga::ShaderStage)]) {
    let actual: Vec<_> = compiled
        .module
        .entry_points
        .iter()
        .map(|entry| (entry.name.as_str(), entry.stage))
        .collect();
    assert_eq!(actual, expected);
    assert!(!compiled.wgsl.contains("#import"));
}

#[test]
fn real_smoke_fire_and_liquid_volume_shaders_compile_for_storage_uniform_and_depth_variants() {
    // The real fire/smoke and liquid marchers, not a stand-in fragment.
    for (program, entry) in [
        (
            include_str!("../../../extensions/aestra-fluid/src/volume.wgsl"),
            "fluid_volume",
        ),
        (
            include_str!("../../../extensions/aestra-fluid/src/liquid_look.wgsl"),
            "liquid_look",
        ),
    ] {
        for storage in [false, true] {
            for (depth, multisampled) in [(false, false), (true, false), (true, true)] {
                let mut defs = vec![ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3)];
                if storage {
                    defs.push("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into());
                } else {
                    defs.push(ShaderDefVal::UInt(
                        "PER_OBJECT_BUFFER_BATCH_SIZE".into(),
                        64,
                    ));
                }
                if depth {
                    defs.push("DEPTH_PREPASS".into());
                }
                if multisampled {
                    defs.push("MULTISAMPLED".into());
                }
                let compiled = compile(
                    composition::compose_volume(program, entry, Dialect::Bevy020),
                    &defs,
                );
                entry_points(
                    &compiled,
                    &[
                        ("vertex", naga::ShaderStage::Vertex),
                        ("fragment", naga::ShaderStage::Fragment),
                    ],
                );
                let bindings: Vec<_> = compiled
                    .module
                    .global_variables
                    .iter()
                    .filter_map(|(_, var)| var.binding.as_ref())
                    .collect();
                for binding in 0..=6 {
                    assert!(
                        bindings
                            .iter()
                            .any(|resource| resource.group == 3 && resource.binding == binding),
                        "missing volume binding {binding}"
                    );
                }
                assert_eq!(
                    bindings
                        .iter()
                        .filter(|binding| binding.group == 0 && binding.binding == 20)
                        .count(),
                    usize::from(depth)
                );
            }
        }
    }
}

fn lit_smoke() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../assets/test/materials/fireworks_lit_smoke.aestra.material.ron");
    let program = aestra_core::material::MaterialProgram::load_ron(path).unwrap();
    let ir = aestra_compiler::MaterialCompiler.compile(&program).unwrap();
    aestra_gpu::material::MaterialShaderCompiler
        .compile(
            &ir,
            &aestra_gpu::material::MaterialBackendCapabilities::portable_minimum(),
        )
        .unwrap()
        .shader
        .wgsl
}

#[test]
fn generated_lit_smoke_keeps_one_view_binding_and_neutral_2d_branch() {
    let portable = lit_smoke();
    assert_eq!(
        composition::compose_material(&portable, false, Dialect::Bevy020),
        portable
    );
    let source = composition::compose_material(&portable, true, Dialect::Bevy020);
    for lighting in [false, true] {
        for storage in [false, true] {
            let mut defs = Vec::new();
            if lighting {
                defs.push("AESTRA_SCENE_POINT_LIGHTING".into());
            }
            if storage {
                defs.push("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into());
            }
            let compiled = compile(source.clone(), &defs);
            assert_eq!(
                compiled
                    .module
                    .global_variables
                    .iter()
                    .filter(|(_, var)| var
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.group == 0 && binding.binding == 0))
                    .count(),
                1
            );
            assert_eq!(
                compiled
                    .module
                    .global_variables
                    .iter()
                    .filter(|(_, var)| var
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.group == 0 && binding.binding == 8))
                    .count(),
                usize::from(lighting)
            );
        }
    }
}

#[test]
fn actual_graph_grid_and_wire_bodies_compile_with_bevy_020_ui_vertex_output() {
    for source in [
        include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_grid.wgsl"),
        include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_wire.wgsl"),
    ] {
        // Only the import changes; preserve production geometry, zoom/DPI and derivatives.
        let body = source
            .strip_prefix("#import bevy_ui::ui_vertex_output::UiVertexOutput")
            .unwrap();
        assert!(!body.contains('#'));
        let compiled = compile(
            format!("import bevy_ui_render::ui_vertex_output::UiVertexOutput;\n{body}"),
            &[],
        );
        entry_points(&compiled, &[("fragment", naga::ShaderStage::Fragment)]);
        assert_eq!(
            compiled
                .module
                .global_variables
                .iter()
                .filter(|(_, var)| var
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.group == 1 && binding.binding == 0))
                .count(),
            1
        );
    }
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_shader_modules_and_render_pipelines_validate() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("native Vulkan adapter required; do not count a skip as qualification");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    println!("Native shader qualification: {info:?}");
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut count = 0;
    for (program, entry) in [
        (
            include_str!("../../../extensions/aestra-fluid/src/volume.wgsl"),
            "fluid_volume",
        ),
        (
            include_str!("../../../extensions/aestra-fluid/src/liquid_look.wgsl"),
            "liquid_look",
        ),
    ] {
        for storage in [false, true] {
            for (depth, multisampled) in [(false, false), (true, false), (true, true)] {
                let mut defs = vec![ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3)];
                if storage {
                    defs.push("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into());
                } else {
                    defs.push(ShaderDefVal::UInt(
                        "PER_OBJECT_BUFFER_BATCH_SIZE".into(),
                        64,
                    ));
                }
                if depth {
                    defs.push("DEPTH_PREPASS".into());
                }
                if multisampled {
                    defs.push("MULTISAMPLED".into());
                }
                let compiled = compile(
                    composition::compose_volume(program, entry, Dialect::Bevy020),
                    &defs,
                );
                let label = format!("{entry}/storage={storage}/depth={depth}/msaa={multisampled}");
                pipeline(&device, &compiled.wgsl, &label, "vertex", "fragment", true);
                count += 1;
            }
        }
    }
    let source = composition::compose_material(&lit_smoke(), true, Dialect::Bevy020);
    for lighting in [false, true] {
        for storage in [false, true] {
            let mut defs = Vec::new();
            if lighting {
                defs.push("AESTRA_SCENE_POINT_LIGHTING".into());
            }
            if storage {
                defs.push("AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into());
            }
            let compiled = compile(source.clone(), &defs);
            pipeline(
                &device,
                &compiled.wgsl,
                &format!("lit-smoke/lighting={lighting}/storage={storage}"),
                "vertex",
                "fragment_material",
                false,
            );
            count += 1;
        }
    }
    for (name, source) in [
        (
            "grid",
            include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_grid.wgsl"),
        ),
        (
            "wire",
            include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_wire.wgsl"),
        ),
    ] {
        let body = source
            .strip_prefix("#import bevy_ui::ui_vertex_output::UiVertexOutput")
            .unwrap();
        let source = format!(
            r#"
import bevy_ui_render::ui_vertex_output::UiVertexOutput;
{body}
// Fixture vertex only. The fragment and vertex-output type are real production sources.
@vertex fn qualification_vertex() -> UiVertexOutput {{
    var output: UiVertexOutput;
    output.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return output;
}}
"#
        );
        let compiled = compile(source, &[]);
        pipeline(
            &device,
            &compiled.wgsl,
            name,
            "qualification_vertex",
            "fragment",
            false,
        );
        count += 1;
    }
    assert_eq!(count, 18);
    println!(
        "Validated {count} native wgpu 30 shader modules and render pipelines; no image/visual parity claim."
    );
}

fn pipeline(
    device: &wgpu::Device,
    source: &str,
    label: &str,
    vertex: &str,
    fragment: &str,
    mesh: bool,
) {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let attributes = wgpu::vertex_attr_array![0 => Float32x3];
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attributes,
    })];
    let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        // Inferred layout checks shader-stage linkage and resource compatibility, not the
        // not-yet-ported Bevy Material/UiMaterial CPU-side pipeline specializations.
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: if mesh { &buffers } else { &[] },
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let error = pollster::block_on(scope.pop());
    assert!(
        error.is_none(),
        "native pipeline validation failed: {label}: {error:?}"
    );
}
