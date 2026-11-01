//! Runs the actual built-in material entry points against the CPU field sampler.
use aestra_compiler::MaterialFunctionLibrary;
use aestra_core::{
    material::MaterialFunctionRef,
    noise::{NoiseFunction, NoiseSettings},
};
use std::{borrow::Cow, sync::mpsc, time::Duration};
use wgpu::*;

#[test]
#[ignore = "native noise parity and GPU cost oracle; run alone on a GPU"]
fn native_builtin_noise_matches_cpu_with_negative_coordinates_seeds_and_lod() {
    const COUNT: u32 = 4096;
    let instance = Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let timestamp = adapter.features().contains(Features::TIMESTAMP_QUERY);
    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        required_features: if timestamp {
            Features::TIMESTAMP_QUERY
        } else {
            Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let functions = MaterialFunctionLibrary::default();
    let mut results = Vec::new();
    for kind in NoiseFunction::ALL {
        let function = functions
            .get(MaterialFunctionRef::BuiltIn(kind.id()))
            .unwrap();
        let vector = if kind.dimensions == 2 { "p.xy" } else { "p" };
        let offset = if kind.dimensions == 2 {
            "vec2<f32>(-0.25,0.5)"
        } else {
            "vec3<f32>(-0.25,0.5,-0.125)"
        };
        let fractal = if kind.fractal { "8.0,2.0,0.5," } else { "" };
        let source = format!(
            "{}\n@group(0) @binding(0) var<storage,read_write> output: array<f32>;\n@compute @workgroup_size(64) fn verify(@builtin(global_invocation_id) gid: vec3<u32>) {{\nlet i=gid.x;\nlet p=vec3<f32>((f32(i%32u)-16.0)*0.125,(f32((i/32u)%16u)-8.0)*0.1875,(f32(i%7u)-3.0)*0.25);\nlet seed=f32((i/512u)*2097151u);\nlet footprint=select(0.0,0.5,i>=3072u);\noutput[i]=field({vector},1.75,{offset},1.25,seed,{fractal}footprint);\n}}",
            function.custom_wesl.as_ref().unwrap().source
        );
        let compiled =
            aestra_gpu::shader::compile_wesl("noise_oracle", &source, &["verify"]).unwrap();
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some(&kind.name()),
            source: ShaderSource::Wgsl(Cow::Owned(compiled.wgsl)),
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("verify"),
            compilation_options: Default::default(),
            cache: None,
        });
        let output = device.create_buffer(&BufferDescriptor {
            label: None,
            size: u64::from(COUNT) * 4,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&BufferDescriptor {
            label: None,
            size: u64::from(COUNT) * 4,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let query = timestamp.then(|| {
            device.create_query_set(&QuerySetDescriptor {
                label: None,
                ty: QueryType::Timestamp,
                count: 2,
            })
        });
        let resolve = timestamp.then(|| {
            device.create_buffer(&BufferDescriptor {
                label: None,
                size: 16,
                usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        });
        let times = timestamp.then(|| {
            device.create_buffer(&BufferDescriptor {
                label: None,
                size: 16,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[BindGroupEntry {
                binding: 0,
                resource: output.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: None,
                timestamp_writes: query.as_ref().map(|query_set| ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(COUNT / 64, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, u64::from(COUNT) * 4);
        if let (Some(query), Some(resolve), Some(times)) = (&query, &resolve, &times) {
            encoder.resolve_query_set(query, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, times, 0, 16);
        }
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        readback
            .slice(..)
            .map_async(MapMode::Read, move |r| tx.send(r).unwrap());
        let (ttx, trx) = mpsc::channel();
        if let Some(times) = &times {
            times
                .slice(..)
                .map_async(MapMode::Read, move |r| ttx.send(r).unwrap());
        }
        device
            .poll(PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(30)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = readback.slice(..).get_mapped_range().unwrap();
        let mut max_error = 0.0f32;
        for (i, bytes) in mapped.as_chunks::<4>().0.iter().enumerate() {
            let actual = f32::from_le_bytes(*bytes);
            let p = [
                ((i % 32) as f32 - 16.0) * 0.125,
                (((i / 32) % 16) as f32 - 8.0) * 0.1875,
                ((i % 7) as f32 - 3.0) * 0.25,
            ];
            let expected = aestra_runtime::noise::sample(
                p,
                &NoiseSettings {
                    basis: kind.basis,
                    dimensions: kind.dimensions,
                    scale: 1.75,
                    offset: [-0.25, 0.5, if kind.dimensions == 2 { 0.0 } else { -0.125 }],
                    amplitude: 1.25,
                    seed: ((i / 512) * 2097151) as u32,
                    octaves: if kind.fractal { 8 } else { 1 },
                    lacunarity: 2.0,
                    gain: 0.5,
                    footprint: if i >= 3072 { 0.5 } else { 0.0 },
                },
            )
            .unwrap();
            max_error = max_error.max((actual - expected).abs());
            assert!(
                actual.is_finite() && (actual - expected).abs() <= 0.0002,
                "{} sample {i}: GPU {actual}, CPU {expected}",
                kind.name()
            );
        }
        let gpu_ms = times.as_ref().map(|times| {
            trx.recv().unwrap().unwrap();
            let mapped = times.slice(..).get_mapped_range().unwrap();
            let start = u64::from_le_bytes(mapped[..8].try_into().unwrap());
            let end = u64::from_le_bytes(mapped[8..16].try_into().unwrap());
            (end - start) as f64 * f64::from(queue.get_timestamp_period()) / 1e6
        });
        results.push(serde_json::json!({"function":kind.name(),"samples":COUNT,"max_abs_error":max_error,"cold_dispatch_gpu_ms":gpu_ms}));
    }
    let report = serde_json::json!({"adapter":format!("{:?}",adapter.get_info()),"tolerance":0.0002,"results":results,"timing_note":"One cold 4096-sample compute dispatch per function; not a steady-state material/frame benchmark."});
    if let Some(path) = std::env::var_os("AESTRA_NOISE_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
}
