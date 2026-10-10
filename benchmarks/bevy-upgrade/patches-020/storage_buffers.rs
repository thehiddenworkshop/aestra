//! Real Aestra encoding and candidate adapter, exercised with Bevy 0.20 assets.
#[path = "../../../bevy/aestra-bevy-render/src/gpu/storage_buffers_020.rs"]
mod storage_buffers;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/storage_encoding.rs"]
mod storage_encoding;

use aestra_gpu::{GpuEffectArtifact, GpuGlobals, GpuRenderGlobals, GpuRenderParams};
use bevy::{
    asset::{AssetId, Assets},
    ecs::system::SystemState,
    prelude::*,
    render::{
        render_asset::{AssetExtractionError, RenderAsset},
        render_resource::{
            Buffer, BufferUsages,
            encase::{ShaderType, internal::WriteInto},
        },
        renderer::{RenderDevice, RenderQueue},
        storage::{GpuShaderBuffer, RenderChangedShaderBuffers, ShaderBuffer},
    },
};

fn artifact() -> GpuEffectArtifact {
    let mut asset = aestra_core::EffectAsset::new("Storage compatibility", 3.0);
    asset
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
    let effect = aestra_compiler::EffectCompiler::default()
        .compile(&asset)
        .unwrap();
    GpuEffectArtifact::from_instance(&aestra_runtime::EffectInstance::new(std::sync::Arc::new(
        effect,
    )))
    .unwrap()
}

fn encoded<T: ShaderType + WriteInto>(value: T) {
    let expected = storage_encoding::encode(&value);
    assert_eq!(expected.len() % 4, 0);
    let mut buffer = storage_buffers::new(&value);
    assert_eq!(storage_buffers::bytes(&buffer), Some(expected.as_slice()));
    assert_eq!(buffer.buffer_size(), expected.len() as u64);
    assert_eq!(buffer.asset_usage, Default::default());
    assert!(!buffer.copy_on_resize);
    for _ in 0..2 {
        storage_buffers::update(&mut buffer, &value);
        assert_eq!(storage_buffers::bytes(&buffer), Some(expected.as_slice()));
        assert_eq!(buffer.buffer_size(), expected.len() as u64);
    }
}

#[test]
fn candidate_uploads_every_real_aestra_record_kind_without_rust_memory_casts() {
    let artifact = artifact();
    encoded(artifact.emitters);
    encoded(artifact.renderers);
    encoded(artifact.particles);
    encoded(vec![0_u32; 7]);
    encoded(GpuGlobals::default());
    encoded(GpuRenderGlobals::default());
    encoded(GpuRenderParams::default());
    let indirect = storage_buffers::indirect(vec![4_u32, 7, 0, 0]);
    assert!(indirect.buffer_usage.contains(BufferUsages::INDIRECT));
    assert!(
        indirect
            .buffer_usage
            .contains(BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST)
    );
    assert!(storage_buffers::bytes(&ShaderBuffer::default()).is_none());
}

#[test]
fn extraction_moves_cpu_bytes_and_updates_replace_rather_than_append_frames() {
    let mut buffer = storage_buffers::new(vec![4_u32, 7, 0, 0]);
    let extracted = GpuShaderBuffer::take_gpu_data(&mut buffer, None).unwrap();
    assert_eq!(extracted.buffer_size(), 16);
    assert_eq!(
        buffer.buffer_size(),
        16,
        "draining must retain the GPU allocation size"
    );
    assert!(storage_buffers::bytes(&buffer).is_none());
    for words in [vec![19_u32, 23], vec![31_u32; 8], vec![37_u32]] {
        let expected = storage_encoding::encode(&words);
        storage_buffers::update(&mut buffer, &words);
        storage_buffers::update(&mut buffer, &words); // Multiple host updates before extraction.
        assert_eq!(buffer.buffer_size(), expected.len() as u64);
        assert_eq!(storage_buffers::bytes(&buffer), Some(expected.as_slice()));
        let extracted = GpuShaderBuffer::take_gpu_data(&mut buffer, None).unwrap();
        assert_eq!(
            storage_buffers::bytes(&extracted),
            Some(expected.as_slice())
        );
        assert!(storage_buffers::bytes(&buffer).is_none());
    }
}

fn prepare(
    world: &mut World,
    id: AssetId<ShaderBuffer>,
    source: &mut ShaderBuffer,
    previous: Option<&GpuShaderBuffer>,
) -> GpuShaderBuffer {
    let extracted = GpuShaderBuffer::take_gpu_data(source, previous).unwrap();
    let mut state = SystemState::<<GpuShaderBuffer as RenderAsset>::Param>::new(world);
    GpuShaderBuffer::prepare_asset(extracted, id, &mut state.get_mut(world).unwrap(), previous)
        .unwrap()
}

// Blocking is confined to this explicit native test; production uploads never wait.
fn read(world: &World, buffer: &Buffer) -> Vec<u8> {
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Aestra 0.20 storage qualification readback"),
        size: buffer.size(),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    let (tx, rx) = std::sync::mpsc::channel();
    encoder.map_buffer_on_submit(&staging, wgpu::MapMode::Read, .., move |result| {
        tx.send(result).unwrap()
    });
    let submission = queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let result = staging.get_mapped_range(..).unwrap().to_vec();
    staging.unmap();
    result
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_storage_uploads_reuse_resize_and_invalidate_bindings() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("native Vulkan GPU required; no qualification skip");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    println!("Native storage qualification: {info:?}");
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut world = World::new();
    world.insert_resource(RenderDevice::new(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<RenderChangedShaderBuffers>();
    let mut assets = Assets::<ShaderBuffer>::default();
    let handle = assets.add(storage_buffers::indirect(vec![4_u32, 7, 0, 0]));
    let mut asset = assets.get_mut(&handle).unwrap();
    let source = &mut *asset;
    let mut gpu = prepare(&mut world, handle.id(), source, None);
    assert!(
        world
            .resource::<RenderChangedShaderBuffers>()
            .contains(&handle.id())
    );
    assert!(matches!(
        GpuShaderBuffer::take_gpu_data(source, Some(&gpu)),
        Err(AssetExtractionError::AlreadyExtracted)
    ));
    assert_eq!(
        read(&world, &gpu.buffer),
        storage_encoding::encode(&vec![4_u32, 7, 0, 0])
    );
    assert!(gpu.buffer.usage().contains(BufferUsages::INDIRECT));
    for words in [vec![8_u32, 9, 0, 0], vec![11_u32; 8], vec![13_u32; 2]] {
        world.resource_mut::<RenderChangedShaderBuffers>().clear();
        storage_buffers::update(source, &words);
        let same_size = source.buffer_size() == gpu.buffer.size();
        let next = prepare(&mut world, handle.id(), source, Some(&gpu));
        assert_eq!(next.buffer.id() == gpu.buffer.id(), same_size);
        assert_eq!(
            world
                .resource::<RenderChangedShaderBuffers>()
                .contains(&handle.id()),
            !same_size
        );
        assert_eq!(read(&world, &next.buffer), storage_encoding::encode(&words));
        gpu = next;
    }
    // Label/usage changes also invalidate identity even at the same allocation size.
    for label_change in [true, false] {
        world.resource_mut::<RenderChangedShaderBuffers>().clear();
        storage_buffers::update(source, vec![17_u32; 2]);
        if label_change {
            source.label = "relabeled Aestra buffer".into();
        } else {
            source.buffer_usage |= BufferUsages::VERTEX;
        }
        let next = prepare(&mut world, handle.id(), source, Some(&gpu));
        assert_ne!(next.buffer.id(), gpu.buffer.id());
        assert!(
            world
                .resource::<RenderChangedShaderBuffers>()
                .contains(&handle.id())
        );
        assert_eq!(
            read(&world, &next.buffer),
            storage_encoding::encode(&vec![17_u32; 2])
        );
        gpu = next;
    }
    // Real emitter/renderer/particle encodings reach the GPU unchanged, not just words.
    let artifact = artifact();
    for expected in [
        storage_encoding::encode(&artifact.emitters),
        storage_encoding::encode(&artifact.renderers),
        storage_encoding::encode(&artifact.particles),
        storage_encoding::encode(&GpuGlobals::default()),
        storage_encoding::encode(&GpuRenderGlobals::default()),
        storage_encoding::encode(&GpuRenderParams::default()),
    ] {
        let mut source = ShaderBuffer::new(expected.clone(), Default::default());
        let gpu = prepare(&mut world, handle.id(), &mut source, None);
        assert_eq!(read(&world, &gpu.buffer), expected);
    }
    // GPU-owned storage has no CPU particle mirror. Its explicit resize preserves
    // the prefix on the GPU and zero-initializes the newly allocated tail.
    let mut source = ShaderBuffer::with_size(16, Default::default());
    let old = prepare(&mut world, handle.id(), &mut source, None);
    world.resource::<RenderQueue>().write_buffer(
        &old.buffer,
        0,
        &storage_encoding::encode(&vec![23_u32; 4]),
    );
    source.resize_buffer(32);
    source.copy_on_resize = true;
    world.resource_mut::<RenderChangedShaderBuffers>().clear();
    let next = prepare(&mut world, handle.id(), &mut source, Some(&old));
    assert_ne!(next.buffer.id(), old.buffer.id());
    assert!(
        world
            .resource::<RenderChangedShaderBuffers>()
            .contains(&handle.id())
    );
    assert!(storage_buffers::bytes(&source).is_none());
    let mut expected = storage_encoding::encode(&vec![23_u32; 4]);
    expected.resize(32, 0);
    assert_eq!(read(&world, &next.buffer), expected);
}
