//! Actual native rasterization, not a duplicate CPU implementation of the sampling math.
use aestra_compiler::{EffectCompiler, MaterialCompiler};
use aestra_core::{EffectAsset, Emitter, material::MaterialProgram};
use aestra_gpu::{
    GpuEffectArtifact, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer,
    material::{
        MATERIAL_FRAGMENT_ENTRY_POINT, MaterialBackendCapabilities, MaterialShaderCompiler,
    },
    shader::{GpuShaderKind, compile},
};
use bevy::math::{Mat4, Vec3, Vec4};
use std::{borrow::Cow, sync::Arc, time::Duration};
use wgpu::util::DeviceExt;

fn storage<T: encase::ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

fn map(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    buffer: &wgpu::Buffer,
) -> Vec<u8> {
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(60)),
        })
        .unwrap();
    receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

#[test]
fn subpixel_sprites_survive_pixel_phase_without_amplifying_quad_energy() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) if adapter.limits().max_storage_buffers_per_shader_stage >= 6 => adapter,
        _ => {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "native sprite-capable adapter required"
            );
            eprintln!("Skipping sprite sampling conformance: no native adapter");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    assert_projection_math(&device, &queue);
    let legacy = compile(GpuShaderKind::SpriteRender).unwrap();
    // Constant alpha ensures attenuation is not dependent on the authored ParticleOpacity input.
    let semantic = MaterialShaderCompiler
        .compile(
            &MaterialCompiler
                .compile(&MaterialProgram::additive_sprite("Sampling contract"))
                .unwrap(),
            &MaterialBackendCapabilities::portable_minimum(),
        )
        .unwrap();
    let mut effect = EffectAsset::new("Sampling", 1.0);
    effect.emitters.push(Emitter::basic_sprite("Stars", 1.0));
    let artifact = GpuEffectArtifact::from_instance(&aestra_runtime::EffectInstance::new(
        Arc::new(EffectCompiler::default().compile(&effect).unwrap()),
    ))
    .unwrap();
    let mut renderer = artifact.renderers[0];
    renderer.blend_mode = aestra_gpu::GpuBlend::Additive as u32;
    renderer.softness = 0.2;
    renderer.tint = Vec4::ONE;
    for (shader, fragment) in [
        (&legacy.wgsl, "fragment_additive"),
        (&semantic.shader.wgsl, MATERIAL_FRAGMENT_ENTRY_POINT),
    ] {
        let off = raster(&device, &queue, shader, fragment, renderer, 0.25, 0.0);
        assert!(
            cell_energy(&off).contains(&0.0),
            "untreated quarter-pixel quads should demonstrate dropout"
        );
        for floor in [2.0, 4.0] {
            let treated = raster(&device, &queue, shader, fragment, renderer, 0.25, floor);
            for energy in cell_energy(&treated) {
                assert!(
                    energy > 0.0 && energy <= 0.25 * 0.25 + 1e-5,
                    "{fragment}, floor {floor}: invalid cell energy {energy}"
                );
            }
            let zero = raster(&device, &queue, shader, fragment, renderer, 0.0, floor);
            assert!(
                cell_energy(&zero).iter().all(|energy| *energy == 0.0),
                "zero-size sprites must remain invisible"
            );
        }
        let large_off = raster(&device, &queue, shader, fragment, renderer, 4.0, 0.0);
        let large_on = raster(&device, &queue, shader, fragment, renderer, 4.0, 2.0);
        assert_eq!(large_off, large_on, "resolved quads must be unchanged");
        assert_temporal_sampling(&device, &queue, shader, fragment, renderer);
        assert_viewport_clipping(&device, &queue, shader, fragment, renderer);
        assert_trail_sampling(&device, &queue, shader, fragment, renderer);
    }
}

fn assert_temporal_sampling(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
) {
    // Cross the pixel lattice continuously in two axes, at two quad orientations.
    // Read linear alpha before exposure/bloom. This is a bounded procedural-mask probe,
    // not a guarantee of shimmer-free arbitrary semantic materials or trail sampling.
    for rotation in [0.0, 0.65] {
        let mut untreated_cv = 0.0;
        let mut two_pixel_cv = 0.0;
        for floor in [0.0, 2.0, 4.0] {
            let mut trajectories = vec![Vec::new(); 16];
            for tick in 0..32 {
                let phase = [tick as f32 / 32.0, tick as f32 * 0.618034 / 32.0];
                let bytes = raster_case(
                    device,
                    queue,
                    source,
                    fragment,
                    renderer,
                    RasterCase {
                        pixels: 0.25,
                        floor,
                        phase,
                        rotation,
                        translation_pixels: [0.0; 2],
                        trail: None,
                    },
                );
                for (trajectory, energy) in trajectories.iter_mut().zip(cell_energy(&bytes)) {
                    trajectory.push(energy);
                }
            }
            let samples = trajectories.iter().flatten().copied().collect::<Vec<_>>();
            let mean = samples.iter().sum::<f32>() / samples.len() as f32;
            let cv = (samples
                .iter()
                .map(|energy| (energy - mean).powi(2))
                .sum::<f32>()
                / samples.len() as f32)
                .sqrt()
                / mean;
            let dropouts = samples.iter().filter(|energy| **energy == 0.0).count();
            eprintln!(
                "{fragment}: rotation={rotation}, floor={floor}, samples={}, dropouts={dropouts}, mean_alpha={mean:.6}, relative_stddev={cv:.4}",
                samples.len()
            );
            let upper_bound = if floor == 0.0 { 1.0 } else { 0.0625 };
            assert!(
                samples.iter().all(|value| value.is_finite()
                    && *value >= 0.0
                    && *value <= upper_bound + 1e-5)
            );
            if floor == 0.0 {
                assert!(
                    dropouts > 0 && mean > 0.0,
                    "probe must demonstrate untreated modulation, not all-invisible geometry"
                );
                untreated_cv = cv;
            } else {
                assert_eq!(
                    dropouts, 0,
                    "{fragment}: moving sampled quads must not drop out"
                );
                assert!(
                    cv < untreated_cv * 0.5,
                    "{fragment}: temporal modulation {cv} did not improve over {untreated_cv}"
                );
                // Integral of the smoothstep-feathered circular mask: R²π(1-f+0.3f²).
                // Phase-averaged sum, not an analytic integral for each rasterized frame.
                let expected = 0.0625 * std::f32::consts::PI * 0.25 * (1.0 - 0.2 + 0.3 * 0.2 * 0.2);
                assert!(
                    (mean - expected).abs() < expected * 0.05,
                    "phase-averaged coverage drifted: {mean} != {expected}"
                );
                if floor == 2.0 {
                    two_pixel_cv = cv;
                } else {
                    assert!(
                        cv < two_pixel_cv * 0.5,
                        "larger floor must improve this bounded procedural probe"
                    );
                }
            }
        }
    }
}

#[test]
fn strobe_material_has_true_off_intervals_and_stable_per_particle_phase() {
    use aestra_compiler::MaterialFunctionLibrary;
    use aestra_core::material::MaterialValue;
    use aestra_core::material::{
        MaterialEvaluationDomain, MaterialExpressionKind, MaterialFunction, MaterialInput,
    };
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) if adapter.limits().max_storage_buffers_per_shader_stage >= 6 => adapter,
        _ => {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "native sprite-capable adapter required"
            );
            eprintln!("Skipping strobe conformance: no native adapter");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut effect = EffectAsset::new("Strobe raster", 3.0);
    effect.emitters.push(Emitter::basic_sprite("Stars", 3.0));
    let artifact = GpuEffectArtifact::from_instance(&aestra_runtime::EffectInstance::new(
        Arc::new(EffectCompiler::default().compile(&effect).unwrap()),
    ))
    .unwrap();
    let mut renderer = artifact.renderers[0];
    renderer.blend_mode = aestra_gpu::GpuBlend::Additive as u32;
    renderer.tint = Vec4::ONE;
    let case = RasterCase {
        pixels: 4.0,
        floor: 0.0,
        phase: [0.0; 2],
        rotation: 0.0,
        translation_pixels: [0.0; 2],
        trail: None,
    };
    let compile_program = |p: &MaterialProgram| {
        MaterialShaderCompiler
            .compile(
                &MaterialCompiler.compile(p).unwrap(),
                &MaterialBackendCapabilities::portable_minimum(),
            )
            .unwrap()
    };
    let base_program = MaterialProgram::additive_sprite("Random reference");
    let mut random_program = base_program.clone();
    let random_id = aestra_core::MaterialExpressionId::new();
    let alpha_id = aestra_core::MaterialExpressionId::new();
    random_program.expressions.extend([
        aestra_core::material::MaterialExpression {
            id: random_id,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleRandom),
        },
        aestra_core::material::MaterialExpression {
            id: alpha_id,
            kind: MaterialExpressionKind::Multiply(random_program.outputs.alpha, random_id),
        },
    ]);
    random_program.outputs.alpha = alpha_id;
    let baseline = compile_program(&base_program);
    let random = compile_program(&random_program);
    let render = |source: &str, identity| {
        raster_identity_case(
            &device,
            &queue,
            source,
            MATERIAL_FRAGMENT_ENTRY_POINT,
            renderer,
            case,
            false,
            identity,
        )
    };
    // Independent integer reference for the production presentation hash.
    let random_reference = |ordinal: u32, seed: u32, emitter: u32| {
        let mut value =
            ordinal.wrapping_mul(0x9e37_79b9) ^ (16 + emitter).wrapping_mul(0x85eb_ca6b) ^ seed;
        value ^= value >> 16;
        value = value.wrapping_mul(0x7feb_352d);
        value ^= value >> 15;
        value = value.wrapping_mul(0x846c_a68b);
        value ^= value >> 16;
        value as f32 / u32::MAX as f32
    };
    let seed = 0xf1e0_0123;
    let identity = IdentityCase {
        seed,
        ..Default::default()
    };
    let base_energy = cell_energy(&render(&baseline.shader.wgsl, identity));
    let random_bytes = render(&random.shader.wgsl, identity);
    for (i, (got, base)) in cell_energy(&random_bytes)
        .iter()
        .zip(&base_energy)
        .enumerate()
    {
        assert!(
            (*got / base - random_reference(i as u32, seed, 0)).abs() < 1e-5,
            "particle {i}: got {got}, base {base}, ratio {}, expected {}",
            got / base,
            random_reference(i as u32, seed, 0)
        );
    }
    assert_eq!(
        random_bytes,
        render(
            &random.shader.wgsl,
            IdentityCase {
                reverse_slots: true,
                ..identity
            }
        )
    );
    for changed in [
        IdentityCase {
            seed: seed + 1,
            ..identity
        },
        IdentityCase {
            ordinal_offset: 16,
            ..identity
        },
        IdentityCase {
            emitter: 1,
            ..identity
        },
    ] {
        assert_ne!(random_bytes, render(&random.shader.wgsl, changed));
    }
    let function = MaterialFunction::from_ron(include_str!(
        "../../../assets/test/materials/periodic_gate.aestra.material-function.ron"
    ))
    .unwrap();
    let mut strobe = MaterialProgram::from_ron(include_str!(
        "../../../assets/test/materials/fireworks_strobe_star.aestra.material.ron"
    ))
    .unwrap();
    // Bake defaults only for this bounded raster harness; the actual asset
    // retains live effect-bound controls and is captured through Bevy below.
    for parameter in &mut strobe.parameters {
        parameter.evaluation_domain = MaterialEvaluationDomain::ShaderStatic;
    }
    let compile_strobe = |p: &MaterialProgram| {
        let ir = MaterialCompiler
            .compile_with_functions(p, &MaterialFunctionLibrary::new([function.clone()]))
            .unwrap();
        MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .unwrap()
    };
    let compiled = compile_strobe(&strobe);
    let mut patterns = Vec::new();
    let mut images = Vec::new();
    let mut on_counts = [0; 16];
    for tick in 0..20 {
        let age = tick as f32 / 180.0;
        let bytes = render(&compiled.shader.wgsl, IdentityCase { age, ..identity });
        assert_eq!(
            bytes,
            render(
                &compiled.shader.wgsl,
                IdentityCase {
                    age,
                    reverse_slots: true,
                    ..identity
                }
            )
        );
        let energies = cell_energy(&bytes);
        let mut pattern = Vec::new();
        for (i, energy) in energies.iter().enumerate() {
            let phase = age * 18.0 + random_reference(i as u32, seed, 0);
            let expected = phase - phase.floor() < 0.18;
            assert_eq!(*energy > 0.0, expected, "tick {tick} particle {i}");
            on_counts[i] += u32::from(expected);
            pattern.push(expected);
            if !expected {
                for y in i / 4 * 8..i / 4 * 8 + 8 {
                    for x in i % 4 * 8..i % 4 * 8 + 8 {
                        let at = (y * 32 + x) * 16;
                        assert!(
                            bytes[at..at + 16].iter().all(|b| *b == 0),
                            "off cell must have exact zero RGB and alpha"
                        );
                    }
                }
            }
        }
        patterns.push(pattern);
        images.push(bytes);
    }
    // Repeated and out-of-order ages represent seek/restart presentation:
    // no mutable clock, cached gate or accumulated phase may influence it.
    for tick in [12, 0, 19, 3, 0] {
        assert_eq!(
            images[tick],
            render(
                &compiled.shader.wgsl,
                IdentityCase {
                    age: tick as f32 / 180.0,
                    ..identity
                }
            )
        );
    }
    assert!(
        on_counts.iter().all(|n| (2..=4).contains(n)),
        "every star flashes and spends most time off"
    );
    assert!(
        patterns
            .iter()
            .any(|p| p.iter().any(|b| *b) && p.iter().any(|b| !b)),
        "phases must not flash in lockstep"
    );
    for duty in [0.0, 1.0] {
        let mut endpoint = strobe.clone();
        endpoint
            .parameters
            .iter_mut()
            .find(|p| p.name == "Duty")
            .unwrap()
            .default = Some(MaterialValue::Float(duty));
        let source = compile_strobe(&endpoint);
        let energy = cell_energy(&render(&source.shader.wgsl, identity));
        assert!(energy.iter().all(|e| (*e > 0.0) == (duty == 1.0)));
    }
}

fn assert_viewport_clipping(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
) {
    // One column/row's centers are a quarter pixel beyond each clip edge. The original
    // quads are entirely outside, but expanded footprints correctly reach the viewport.
    // This is why world-space AABB culling cannot discard treated draws unchanged.
    for translation_pixels in [[28.25, 0.0], [-29.0, 0.0], [0.0, 28.25], [0.0, -29.0]] {
        for floor in [0.0, 2.0, 4.0] {
            let bytes = raster_case(
                device,
                queue,
                source,
                fragment,
                renderer,
                RasterCase {
                    pixels: 0.25,
                    floor,
                    phase: [0.0; 2],
                    rotation: 0.0,
                    translation_pixels,
                    trail: None,
                },
            );
            let energy = cell_energy(&bytes).iter().sum::<f32>();
            assert!(energy.is_finite());
            if floor == 0.0 {
                assert_eq!(
                    energy, 0.0,
                    "untreated {fragment} edge {translation_pixels:?}"
                );
            } else {
                assert!(
                    energy > 0.0 && energy <= 4.0 * 0.0625,
                    "expanded {fragment} edge {translation_pixels:?}, floor {floor}: {energy}"
                );
            }
        }
    }
    // The maximum policy must not pull a distant offscreen draw back into view.
    for floor in [0.0, 2.0, 4.0, 8.0] {
        let bytes = raster_case(
            device,
            queue,
            source,
            fragment,
            renderer,
            RasterCase {
                pixels: 0.25,
                floor,
                phase: [0.0; 2],
                rotation: 0.65,
                translation_pixels: [64.0, 0.0],
                trail: None,
            },
        );
        assert!(
            bytes.iter().all(|byte| *byte == 0),
            "fully clipped {fragment}, floor {floor}"
        );
    }
}

fn assert_trail_sampling(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
) {
    let render = |pixels, floor, phase, fade, caps| {
        raster_case(
            device,
            queue,
            source,
            fragment,
            renderer,
            RasterCase {
                pixels,
                floor,
                phase: [phase, 0.0],
                rotation: 0.0,
                translation_pixels: [0.0; 2],
                trail: Some(TrailCase { fade, caps }),
            },
        )
    };
    for fade in [1.0_f32, 0.5] {
        for caps in [false, true] {
            let mut baseline_cv = 0.0;
            for floor in [0.0, 2.0, 4.0] {
                let mut energies = Vec::new();
                for tick in 0..32 {
                    energies.extend(cell_energy(&render(
                        0.25,
                        floor,
                        tick as f32 / 32.0,
                        fade,
                        caps,
                    )));
                }
                let mean = energies.iter().sum::<f32>() / energies.len() as f32;
                let cv = (energies
                    .iter()
                    .map(|energy| (energy - mean).powi(2))
                    .sum::<f32>()
                    / energies.len() as f32)
                    .sqrt()
                    / mean;
                let dropouts = energies.iter().filter(|energy| **energy == 0.0).count();
                eprintln!(
                    "trail {fragment}: fade={fade}, caps={caps}, floor={floor}, samples={}, dropouts={dropouts}, mean_alpha={mean:.6}, relative_stddev={cv:.4}",
                    energies.len()
                );
                assert!(
                    energies
                        .iter()
                        .all(|energy| energy.is_finite() && *energy >= 0.0)
                );
                if floor == 0.0 {
                    assert!(dropouts > 0 && mean > 0.0);
                    baseline_cv = cv;
                } else {
                    assert_eq!(dropouts, 0, "sampled trail body/joins must stay visible");
                    assert!(cv < baseline_cv * 0.5);
                    // Four-pixel length × authored width × age fade; strip mask integral
                    // is (1 - feather/2). Caps add only their original quarter-pixel area,
                    // not a large new halo from the widened circle.
                    let body_integral = 4.0 * 0.25 * fade.powi(2) * 0.9;
                    assert!(
                        (mean - body_integral).abs() < body_integral * 0.15,
                        "trail energy changed: {mean} vs {body_integral}"
                    );
                }
            }
        }
    }
    for floor in [2.0, 4.0] {
        assert!(
            render(0.0, floor, 0.0, 1.0, true)
                .iter()
                .all(|value| *value == 0)
        );
        assert!(
            render(0.25, floor, 0.0, 0.0, true)
                .iter()
                .all(|value| *value == 0)
        );
    }
    assert_eq!(
        render(4.0, 0.0, 0.0, 1.0, false),
        render(4.0, 2.0, 0.0, 1.0, false),
        "resolved trail widths must be unchanged"
    );
    for rotation in [0.65, 1.2] {
        for tick in 0..16 {
            let bytes = raster_case(
                device,
                queue,
                source,
                fragment,
                renderer,
                RasterCase {
                    pixels: 0.25,
                    floor: 2.0,
                    phase: [tick as f32 / 16.0, 0.0],
                    rotation,
                    translation_pixels: [0.0; 2],
                    trail: Some(TrailCase {
                        fade: 1.0,
                        caps: true,
                    }),
                },
            );
            assert!(
                cell_energy(&bytes)
                    .iter()
                    .all(|energy| energy.is_finite() && *energy > 0.0 && *energy < 1.2),
                "angled trail must stay visible without gaining energy"
            );
        }
    }
    for (translation_pixels, floor) in [
        ([28.25, 0.0], 0.0),
        ([28.25, 0.0], 2.0),
        ([-29.0, 0.0], 2.0),
        ([64.0, 0.0], 8.0),
    ] {
        let bytes = raster_case(
            device,
            queue,
            source,
            fragment,
            renderer,
            RasterCase {
                pixels: 0.25,
                floor,
                phase: [0.0; 2],
                rotation: 0.0,
                translation_pixels,
                trail: Some(TrailCase {
                    fade: 1.0,
                    caps: false,
                }),
            },
        );
        let energy: f32 = cell_energy(&bytes).into_iter().sum();
        assert_eq!(
            energy > 0.0,
            floor == 2.0,
            "expanded trail edge must survive; wholly offscreen trails must clip"
        );
    }
    // Compare actual production trail rasterization with/without the native
    // indirect cull, not a duplicate CPU formula. Includes expanded side edges,
    // rotated/age-faded strips and wholly offscreen histories.
    for caps in [false, true] {
        for fade in [1.0, 0.5] {
            for translation_pixels in [
                [28.25, 0.0],
                [-29.0, 0.0],
                [0.0, 28.25],
                [0.0, -29.0],
                [64.0, 0.0],
            ] {
                let case = RasterCase {
                    pixels: 0.25,
                    floor: 4.0,
                    phase: [0.0; 2],
                    rotation: 0.65,
                    translation_pixels,
                    trail: Some(TrailCase { fade, caps }),
                };
                assert_eq!(
                    raster_case(device, queue, source, fragment, renderer, case),
                    raster_case_with_culling(device, queue, source, fragment, renderer, case, true),
                    "culled raster differs at {translation_pixels:?}, caps={caps}, fade={fade}"
                );
            }
        }
    }
    for caps in [false, true] {
        for (translation_pixels, rotation) in [
            ([30.25, 0.0], std::f32::consts::FRAC_PI_2),
            ([0.0, 30.25], 0.0),
        ] {
            let case = RasterCase {
                pixels: 0.25,
                floor: 4.0,
                phase: [0.0; 2],
                rotation,
                translation_pixels,
                trail: Some(TrailCase { fade: 1.0, caps }),
            };
            let direct = raster_case(device, queue, source, fragment, renderer, case);
            assert_eq!(
                cell_energy(&direct).into_iter().sum::<f32>() > 0.0,
                caps,
                "only the widened round cap enters this viewport"
            );
            assert_eq!(
                direct,
                raster_case_with_culling(device, queue, source, fragment, renderer, case, true),
                "cap-only overlap must survive culling"
            );
        }
    }
    let mut alpha = renderer;
    alpha.blend_mode = aestra_gpu::GpuBlend::Alpha as u32;
    let alpha_case = |floor| {
        raster_case(
            device,
            queue,
            source,
            fragment,
            alpha,
            RasterCase {
                pixels: 0.25,
                floor,
                phase: [0.0; 2],
                rotation: 0.0,
                translation_pixels: [0.0; 2],
                trail: Some(TrailCase {
                    fade: 1.0,
                    caps: true,
                }),
            },
        )
    };
    assert_eq!(
        alpha_case(0.0),
        alpha_case(2.0),
        "non-additive trails must stay untouched"
    );
}

fn cell_energy(bytes: &[u8]) -> Vec<f32> {
    (0..16)
        .map(|cell| {
            let mut energy = 0.0;
            for y in (cell / 4 * 8)..(cell / 4 * 8 + 8) {
                for x in (cell % 4 * 8)..(cell % 4 * 8 + 8) {
                    let offset = (y * 32 + x) * 16 + 12;
                    energy += f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                }
            }
            energy
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn raster(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
    pixels: f32,
    floor: f32,
) -> Vec<u8> {
    raster_case(
        device,
        queue,
        source,
        fragment,
        renderer,
        RasterCase {
            pixels,
            floor,
            phase: [0.0; 2],
            rotation: 0.0,
            translation_pixels: [0.0; 2],
            trail: None,
        },
    )
}

#[derive(Clone, Copy)]
struct RasterCase {
    pixels: f32,
    floor: f32,
    phase: [f32; 2],
    rotation: f32,
    translation_pixels: [f32; 2],
    trail: Option<TrailCase>,
}

#[derive(Clone, Copy)]
struct TrailCase {
    fade: f32,
    caps: bool,
}

fn raster_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
    case: RasterCase,
) -> Vec<u8> {
    raster_case_with_culling(device, queue, source, fragment, renderer, case, false)
}

#[allow(clippy::too_many_arguments)]
fn raster_case_with_culling(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
    case: RasterCase,
    culling: bool,
) -> Vec<u8> {
    raster_identity_case(
        device,
        queue,
        source,
        fragment,
        renderer,
        case,
        culling,
        IdentityCase::default(),
    )
}

#[derive(Clone, Copy, Default)]
struct IdentityCase {
    age: f32,
    seed: u32,
    ordinal_offset: u32,
    emitter: u32,
    reverse_slots: bool,
}

#[allow(clippy::too_many_arguments)]
fn raster_identity_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    mut renderer: GpuRenderer,
    case: RasterCase,
    culling: bool,
    identity: IdentityCase,
) -> Vec<u8> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sprite sampling raster"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let entries = (0..8)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: if binding == 5 || binding == 6 {
                wgpu::ShaderStages::FRAGMENT
            } else {
                wgpu::ShaderStages::VERTEX_FRAGMENT
            },
            ty: match binding {
                5 => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                6 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                _ => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&view_layout), Some(&draw_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let init = |bytes: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    };
    // Bevy ViewUniform prefix through main_pass_viewport, with seven identity matrices.
    let mut view = vec![0_u8; 512];
    for matrix in 0..7 {
        for diagonal in 0..4 {
            let offset = matrix * 64 + diagonal * 20;
            view[offset..offset + 4].copy_from_slice(&1_f32.to_le_bytes());
        }
    }
    for offset in [472, 476, 488, 492] {
        view[offset..offset + 4].copy_from_slice(&32_f32.to_le_bytes());
    }
    let view = init(&view, wgpu::BufferUsages::UNIFORM);
    let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &view_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        }],
    });
    renderer.attribute_flags.y = case.floor.to_bits();
    renderer.emitter_index = identity.emitter;
    let mut particles = (0..16)
        .map(|i| {
            let x = 4.0
                + (i % 4) as f32 * 8.0
                + ((i % 4) as f32 * 0.25 + case.phase[0]).fract()
                + case.translation_pixels[0];
            let y = 4.0
                + (i / 4) as f32 * 8.0
                + ((i / 4) as f32 * 0.25 + case.phase[1]).fract()
                + case.translation_pixels[1];
            GpuParticle {
                color: Vec4::ONE,
                position: Vec3::new(x / 16.0 - 1.0, 1.0 - y / 16.0, 0.5),
                size: case.pixels / 16.0,
                rotation: case.rotation,
                packed_emitter_alive: identity.emitter << 16 | 1,
                particle_index: i + identity.ordinal_offset,
                normalized_age: identity.age,
            }
        })
        .collect::<Vec<_>>();
    let mut aux = vec![0_u32; 128];
    let mut instance_count = 16;
    if let Some(trail) = case.trail {
        renderer.renderer_kind = 4;
        renderer.frame_count = 3;
        renderer.frame_rate = 1.0;
        renderer.flipbook_flags = if trail.caps { 2 } else { 0 };
        renderer.attribute_flags.y = (case.pixels / 16.0).to_bits();
        renderer.attribute_flags.z = 0;
        renderer.frames[63].w = case.floor;
        let centers = particles;
        particles = vec![GpuParticle::default(); 49];
        aux = vec![0_u32; 49 * 3];
        for (owner, center) in centers.into_iter().enumerate() {
            let base = 1 + owner * 3;
            // Two body segments share their midpoint; both caps use the same endpoint frame.
            for (slot, offset) in [(base, 2.0_f32), (base + 1, -2.0), (base + 2, 0.0)] {
                particles[slot] = GpuParticle {
                    position: center.position
                        + Vec3::new(-case.rotation.sin(), case.rotation.cos(), 0.0)
                            * (offset / 16.0),
                    size: 1.0,
                    rotation: trail.fade - 1.0,
                    ..center
                };
            }
            aux[base * 3 + 1] = 2;
        }
        if culling {
            let minimum = particles[1..]
                .iter()
                .fold(Vec3::splat(f32::INFINITY), |bounds, particle| {
                    bounds.min(particle.position)
                });
            let maximum = particles[1..]
                .iter()
                .fold(Vec3::splat(f32::NEG_INFINITY), |bounds, particle| {
                    bounds.max(particle.position)
                });
            particles[0] = GpuParticle {
                position: minimum,
                color: maximum.extend(1.0),
                size: 1.0,
                packed_emitter_alive: 1,
                ..Default::default()
            };
            aux[0] = 9;
        }
        instance_count = 16 * if trail.caps { 18 } else { 2 };
    }
    let mut indices = (0..16_u32).collect::<Vec<_>>();
    if identity.reverse_slots {
        assert!(case.trail.is_none());
        particles.reverse();
        indices.reverse();
    }
    let data = [
        storage(&vec![renderer]),
        storage(&particles),
        storage(&indices),
        storage(&GpuRenderGlobals {
            world_from_effect: Mat4::IDENTITY,
            seed: identity.seed,
            ..Default::default()
        }),
        storage(&GpuRenderParams::default()),
        storage(&aux),
    ];
    let buffers = data
        .iter()
        .map(|bytes| init(bytes, wgpu::BufferUsages::STORAGE))
        .collect::<Vec<_>>();
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let white_view = white.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let mut draw_entries = (0..5)
        .map(|binding| wgpu::BindGroupEntry {
            binding,
            resource: buffers[binding as usize].as_entire_binding(),
        })
        .collect::<Vec<_>>();
    draw_entries.extend([
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(&white_view),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 7,
            resource: buffers[5].as_entire_binding(),
        },
    ]);
    let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &draw_layout,
        entries: &draw_entries,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32 * 32 * 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let indirect = culling.then(|| trail_indirect(device, &mut encoder, &buffers, instance_count));
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &view_group, &[]);
        pass.set_bind_group(1, &draw_group, &[]);
        if let Some(indirect) = &indirect {
            pass.draw_indirect(indirect, 0);
        } else {
            pass.draw(0..4, 0..instance_count);
        }
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(32),
            },
        },
        wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
    );
    map(device, queue, encoder, &readback)
}

fn trail_indirect(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    buffers: &[wgpu::Buffer],
    instance_count: u32,
) -> wgpu::Buffer {
    let code = aestra_gpu::shader::compile_wesl(
        "package::trail_cull",
        &aestra_gpu::shader::trail_cull_wesl(),
        &["cull_trail"],
    )
    .unwrap();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production trail culling before raster"),
        source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("cull_trail"),
        compilation_options: Default::default(),
        cache: None,
    });
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &storage(&aestra_gpu::GpuTrailCullParams {
            clip_from_world: Mat4::IDENTITY,
            renderer_index: 0,
            instance_count,
            epoch: 9,
            pixel_radius_per_clip_w: 1.0001 / 32.0,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let indirect = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
        mapped_at_creation: false,
    });
    let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &storage(&vec![4_u32, instance_count, 0, 0]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let inputs = [
        &buffers[1],
        &buffers[0],
        &params,
        &indirect,
        &buffers[3],
        &buffers[5],
        &source,
    ];
    let entries = inputs
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.dispatch_workgroups(1, 1, 1);
    indirect
}

fn assert_projection_math(device: &wgpu::Device, queue: &wgpu::Queue) {
    let source = format!(
        "{}\n{}",
        include_str!("../../../crates/aestra-gpu/src/shaders/aestra_sprite_sampling.wesl"),
        r#"
        @group(0) @binding(0) var<storage, read_write> results: array<vec4<f32>>;
        @compute @workgroup_size(1) fn check() {
            let widths = array<f32, 8>(0.25, 0.5, 1.0, 2.0, 4.0, 0.0, 0.25, 0.00000001);
            for (var i = 0u; i < 8u; i++) {
                let floor = select(2.0, 0.0, i == 6u);
                results[i] = vec4<f32>(aestra_sprite_sampling(widths[i], floor), 0.0, 0.0);
            }
            let identity = mat4x4<f32>(vec4<f32>(1,0,0,0),vec4<f32>(0,1,0,0),vec4<f32>(0,0,1,0),vec4<f32>(0,0,0,1));
            let x = vec3<f32>(0.02,0,0); let y = vec3<f32>(0,0.01,0);
            results[8].x = aestra_sprite_projected_pixels(identity, vec4<f32>(0,0,0,1), x, y, vec2<f32>(100,200));
            results[9].x = aestra_sprite_projected_pixels(identity, vec4<f32>(0,0,0,1), x, y, vec2<f32>(50,100));
            let perspective = mat4x4<f32>(vec4<f32>(2,0,0,0),vec4<f32>(0,2,0,0),vec4<f32>(0,0,0,-1),vec4<f32>(0,0,0.1,0));
            let px = vec3<f32>(0.1,0,0); let py = vec3<f32>(0,0.1,0);
            results[10].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,-10,1), px, py, vec2<f32>(100));
            results[11].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,-20,1), px, py, vec2<f32>(100));
            results[12].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,10,1), px, py, vec2<f32>(100));
        }
    "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("check"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 13 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 13 * 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 13 * 16);
    let bytes = map(device, queue, encoder, &readback);
    let results = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|value| {
            let words: &[[u8; 4]; 4] = value.as_chunks::<4>().0.try_into().unwrap();
            words.map(f32::from_le_bytes)
        })
        .collect::<Vec<_>>();
    for (actual, expected) in results.iter().zip([
        [8.0, 0.015625],
        [4.0, 0.0625],
        [2.0, 0.25],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
    ]) {
        assert_eq!(actual[..2], expected);
        assert_eq!(
            actual[0] * actual[0] * actual[1],
            1.0,
            "continuous area times coverage must be invariant"
        );
    }
    for (actual, expected) in results[8..].iter().zip([1.0, 0.5, 1.0, 0.5, 0.0]) {
        assert!((actual[0] - expected).abs() < 1e-6);
    }
}
