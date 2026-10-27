//! Compile production shader bodies through Bevy 0.20's actual ShaderCache and
//! published WESL libraries. No stub View/cluster/UI definitions or regex preprocessing.
#[path = "../../../bevy/aestra-bevy-render/src/gpu/shader_composition.rs"]
mod composition;

mod shader_support;
use bevy::shader::ShaderDefVal;
use shader_support::{Compiled, compile};
use std::path::Path;
mod overflow_sort;

#[test]
fn native_overflow_link_sort_compiles_with_real_view_and_settings() {
    let source = include_str!("../../../bevy/aestra-bevy-render/src/experimental_oit/sort.wesl");
    let compiled = compile(source.to_owned(), &[]);
    entry_points(&compiled, &[("main", naga::ShaderStage::Compute)]);
}

#[test]
fn native_aestra_resolve_compiles_with_real_oit_packing_and_depth_variants() {
    let source = include_str!("../../../bevy/aestra-bevy-render/src/experimental_oit/resolve.wesl");
    for sorted_max in [1, 8, 32] {
        for prepass in [false, true] {
            let mut defs = vec![
                ShaderDefVal::UInt("SORTED_FRAGMENT_MAX_COUNT".into(), sorted_max),
                "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(),
            ];
            if prepass {
                defs.push("DEPTH_PREPASS".into());
            }
            let compiled = compile(source.to_owned(), &defs);
            entry_points(&compiled, &[("fragment", naga::ShaderStage::Fragment)]);
            assert_eq!(
                compiled
                    .module
                    .global_variables
                    .iter()
                    .filter(|(_, v)| v
                        .binding
                        .as_ref()
                        .is_some_and(|b| b.group == 0 && b.binding == 0))
                    .count(),
                1
            );
        }
    }
}

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
fn production_editor_preview_grid_shader_compiles_with_the_real_material_binding_contract() {
    let compiled = compile(
        include_str!("../../../assets/shaders/preview_grid.wesl").to_owned(),
        &[],
    );
    entry_points(&compiled, &[("fragment", naga::ShaderStage::Fragment)]);
    let bindings: Vec<_> = compiled
        .module
        .global_variables
        .iter()
        .filter_map(|(_, variable)| variable.binding.as_ref())
        .collect();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].group,
        bevy::pbr::MATERIAL_BIND_GROUP_INDEX as u32
    );
    assert_eq!(bindings[0].binding, 0);
    let grid = compiled
        .module
        .types
        .iter()
        .find_map(|(_, ty)| {
            if ty.name.as_deref() == Some("PreviewGridUniform") {
                Some(&ty.inner)
            } else {
                None
            }
        })
        .unwrap();
    let naga::TypeInner::Struct { members, span } = grid else {
        panic!("grid uniform must be a struct")
    };
    assert_eq!(*span, 96);
    assert_eq!(
        members
            .iter()
            .map(|member| member.offset)
            .collect::<Vec<_>>(),
        [0, 16, 32, 48, 64, 80]
    );
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
                let compiled = compile(composition::compose_volume(program, entry), &defs);
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
fn fragment_list_alpha_additive_variants_compile_with_real_bevy_imports() {
    for (semantic, lit) in [(false, false), (true, false), (true, true)] {
        let portable = if !semantic {
            aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned()
        } else if lit {
            lit_smoke()
        } else {
            let program = aestra_core::material::MaterialProgram::additive_sprite("Mixed OIT");
            let ir = aestra_compiler::MaterialCompiler.compile(&program).unwrap();
            aestra_gpu::material::MaterialShaderCompiler
                .compile(
                    &ir,
                    &aestra_gpu::material::MaterialBackendCapabilities::portable_minimum(),
                )
                .unwrap()
                .shader
                .wgsl
        };
        let source = composition::compose_fragment_list(
            &composition::compose_material(&portable, lit),
            semantic,
        );
        // Prove the exact source compiled below is also produced from Windows
        // and mixed-line-ending inputs, including the lit-material view rewrite.
        let lf = portable.replace("\r\n", "\n");
        for input in [
            lf.replace('\n', "\r\n"),
            lf.replace("@fragment\n", "@fragment\r\n"),
        ] {
            let variant = composition::compose_fragment_list(
                &composition::compose_material(&input, lit),
                semantic,
            );
            assert_eq!(variant, source);
        }
        for oit in [false, true] {
            for additive in [false, true] {
                let mut defs = vec![ShaderDefVal::Bool(
                    "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(),
                    true,
                )];
                if lit {
                    defs.push("AESTRA_SCENE_POINT_LIGHTING".into());
                }
                if oit {
                    defs.extend([
                        "AESTRA_FRAGMENT_LIST_OIT".into(),
                        "OIT_ENABLED".into(),
                        "DEPTH_PREPASS".into(),
                    ]);
                    if additive {
                        defs.push("AESTRA_FRAGMENT_LIST_ADDITIVE".into());
                    }
                }
                let compiled = compile(source.clone(), &defs);
                assert!(compiled.module.entry_points.iter().any(|e| e.name
                    == if semantic {
                        "fragment_material"
                    } else {
                        "fragment_additive"
                    }));
                assert_eq!(
                    compiled
                        .module
                        .global_variables
                        .iter()
                        .filter(|(_, var)| var
                            .binding
                            .as_ref()
                            .is_some_and(|b| b.group == 0 && b.binding == 0))
                        .count(),
                    1
                );
            }
        }
    }
}

#[test]
fn generated_lit_smoke_keeps_one_view_binding_and_neutral_2d_branch() {
    let portable = lit_smoke();
    assert_eq!(composition::compose_material(&portable, false), portable);
    let source = composition::compose_material(&portable, true);
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
        include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_grid.wesl"),
        include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_wire.wesl"),
    ] {
        assert!(!source.contains('#'));
        let compiled = compile(source.to_owned(), &[]);
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
                let compiled = compile(composition::compose_volume(program, entry), &defs);
                let label = format!("{entry}/storage={storage}/depth={depth}/msaa={multisampled}");
                pipeline(&device, &compiled.wgsl, &label, "vertex", "fragment", true);
                count += 1;
            }
        }
    }
    let source = composition::compose_material(&lit_smoke(), true);
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
            include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_grid.wesl"),
        ),
        (
            "wire",
            include_str!("../../../apps/aestra-editor/src/feathers/shaders/node_graph_wire.wesl"),
        ),
    ] {
        let source = format!(
            r#"
{source}
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
