//! Actual production shader on sparse/shuffled linked lists, not a CPU algorithm copy.
use crate::shader_support::compile;
use wgpu::util::DeviceExt;

#[test]
#[ignore = "native Vulkan in-place OIT link-sort contract; run alone"]
fn native_overflow_links_are_sorted_without_loss_cross_pixel_writes_or_new_storage() {
    let source = include_str!("../../../bevy/aestra-bevy-render/src/experimental_oit/sort.wesl");
    let compiled = compile(source.to_owned(), &[]);
    let mut instance = wgpu::InstanceDescriptor::new_without_display_handle();
    instance.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(instance);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("actual OIT overflow link sort"),
        source: wgpu::ShaderSource::Wgsl(compiled.wgsl.as_str().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Reflect the real View ABI rather than guessing the viewport offset.
    let view_type = compiled
        .module
        .global_variables
        .iter()
        .find(|(_, v)| {
            v.binding
                .as_ref()
                .is_some_and(|b| b.group == 0 && b.binding == 0)
        })
        .unwrap()
        .1
        .ty;
    let naga::TypeInner::Struct { members, span } = &compiled.module.types[view_type].inner else {
        panic!("View ABI changed")
    };
    let viewport = members
        .iter()
        .find(|m| m.name.as_deref() == Some("viewport"))
        .unwrap()
        .offset as usize;
    let mut view_bytes = vec![0; *span as usize];
    view_bytes[viewport + 8..viewport + 12].copy_from_slice(&16_f32.to_le_bytes());
    view_bytes[viewport + 12..viewport + 16].copy_from_slice(&4_f32.to_le_bytes());
    let view = buffer(&device, &view_bytes, wgpu::BufferUsages::UNIFORM);
    let sizes = [
        0, 1, 2, 7, 8, 9, 31, 32, 33, 40, 41, 64, 127, 256, 1025, 4097,
    ];
    let mut original = Vec::<[u32; 3]>::new();
    let mut lists = Vec::new();
    let mut heads = Vec::new();
    let mut state = 0x93af_1234_u32;
    for order in 0..3 {
        for size in sizes {
            let mut ids: Vec<_> = (0..size)
                .map(|_| {
                    let id = original.len() as u32;
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    original.push([
                        id,
                        if id.is_multiple_of(17) {
                            0x12345680
                        } else {
                            state
                        },
                        u32::MAX,
                    ]);
                    // Unreachable canaries ensure rewrites don't cross sparse gaps.
                    original.push([0xdeadbeef, 0xfeedface, u32::MAX]);
                    id
                })
                .collect();
            ids.sort_by_key(|i| (original[*i as usize][1], original[*i as usize][0]));
            if order == 1 {
                ids.reverse();
            }
            if order == 2 {
                for i in (1..ids.len()).rev() {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    ids.swap(i, state as usize % (i + 1));
                }
            }
            for pair in ids.windows(2) {
                original[pair[0] as usize][2] = pair[1];
            }
            heads.push(ids.first().map_or(0, |id| id + 1));
            lists.push(ids);
        }
    }
    let invalid_pixel = heads.len();
    heads.push(u32::MAX); // out-of-range head, not END
    let cycle_pixel = heads.len();
    let cycle = original.len() as u32;
    original.push([123, 456, cycle]);
    heads.push(cycle + 1);
    heads.resize(64, 0);
    for threshold in [1_u32, 8, 32] {
        let settings = buffer(
            &device,
            &words(&[threshold, 0, 0, 0]),
            wgpu::BufferUsages::UNIFORM,
        );
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        let input: Vec<_> = original.iter().flatten().copied().collect();
        let nodes = buffer(&device, &words(&input), usage);
        let heads_buffer = buffer(&device, &words(&heads), usage);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[&view, &settings, &nodes, &heads_buffer]
                .into_iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(2, 1, 1);
        }
        queue.submit([encoder.finish()]);
        let output = read(&device, &queue, &nodes);
        let out_heads = read(&device, &queue, &heads_buffer);
        for (pixel, ids) in lists.iter().enumerate() {
            let mut expected = ids.clone();
            if ids.len() > threshold as usize {
                expected.sort_by_key(|i| {
                    std::cmp::Reverse((original[*i as usize][1], original[*i as usize][0]))
                });
            }
            let mut next = out_heads[pixel].wrapping_sub(1);
            for id in expected {
                assert_eq!(next, id, "pixel {pixel}, threshold {threshold}");
                next = output[next as usize * 3 + 2];
            }
            assert_eq!(next, u32::MAX, "lost/cyclic suffix");
        }
        assert_eq!(out_heads[invalid_pixel], 0);
        assert_eq!(out_heads[cycle_pixel], 0);
        for (i, node) in original.iter().enumerate() {
            assert_eq!(
                &output[i * 3..i * 3 + 2],
                &node[..2],
                "color/depth payload changed"
            );
            if node[0] == 0xdeadbeef {
                assert_eq!(output[i * 3 + 2], node[2]);
            }
        }
    }
    println!(
        "48 sparse lists through 4097 nodes, three thresholds, empty/padded pixels, ties and two corrupt-chain controls passed"
    );
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn buffer(device: &wgpu::Device, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytes,
        usage,
    })
}
fn read(device: &wgpu::Device, queue: &wgpu::Queue, source: &wgpu::Buffer) -> Vec<u32> {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: source.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, source.size());
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = staging.slice(..).get_mapped_range().unwrap();
    let result = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    drop(bytes);
    staging.unmap();
    result
}
