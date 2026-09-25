//! Host bindings HB6: CPU/GPU conformance of host binding reads. A moving binding trace is packed
//! into the host-binding ABI each tick, uploaded, and read back on the GPU through the WGSL accessors;
//! the kernel must observe exactly the values the CPU runtime resolves (Live slots follow the trace,
//! the SnapshotOnSpawn slot stays latched, absent optional fields stay absent).
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, BindingFieldId, BindingUpdateMode,
    EffectAsset, EffectBinding, Emitter,
};
use aestra_gpu::{GpuHostBindings, HOST_BINDINGS_WGSL};
use aestra_runtime::{BindingSlot, EffectInstance, SpatialBindingSnapshot};
use std::{borrow::Cow, sync::Arc, sync::mpsc, time::Duration};
use wgpu::util::DeviceExt;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const OUTPUT_FLOATS: usize = 12;

/// One tick of the host trace: target position (`None` = lost), source position, target velocity.
type TraceTick = (Option<[f32; 3]>, [f32; 3], Option<[f32; 3]>);

/// Reads Source (slot 0) position, Target (slot 1) position, and Target's optional velocity.
fn kernel(position_offset: u32, velocity_index: u32, velocity_offset: u32) -> String {
    format!(
        "@group(0) @binding(0) var<storage, read> aestra_host_bindings: array<u32>;\n\
         @group(0) @binding(1) var<storage, read_write> output: array<f32>;\n\
         {HOST_BINDINGS_WGSL}\n\
         @compute @workgroup_size(1) fn read_bindings() {{\n\
             let source = aestra_binding_vec3(0u, {position_offset}u);\n\
             let aimed = aestra_binding_vec3(1u, {position_offset}u);\n\
             output[0] = source.x; output[1] = source.y; output[2] = source.z;\n\
             output[3] = aimed.x; output[4] = aimed.y; output[5] = aimed.z;\n\
             output[6] = select(0.0, 1.0, aestra_binding_bound(0u));\n\
             output[7] = select(0.0, 1.0, aestra_binding_bound(1u));\n\
             let has_velocity = aestra_binding_present(1u, {velocity_index}u);\n\
             output[8] = select(0.0, 1.0, has_velocity);\n\
             let velocity = select(vec3<f32>(0.0), aestra_binding_vec3(1u, {velocity_offset}u), has_velocity);\n\
             output[9] = velocity.x; output[10] = velocity.y; output[11] = velocity.z;\n\
         }}\n"
    )
}

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Harness {
    fn new() -> Option<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        if !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra host-binding conformance"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        Some(Self { device, queue })
    }

    /// Uploads one tick's packed bindings, runs the kernel, and reads the output back.
    fn run(&self, wgsl: &str, packed: &GpuHostBindings) -> Vec<f32> {
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("host binding kernel"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("read_bindings"),
                layout: None,
                module: &module,
                entry_point: Some("read_bindings"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bindings = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra.resource.host_bindings"),
                contents: &packed.to_bytes(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let size = (OUTPUT_FLOATS * 4) as u64;
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("host binding group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: bindings.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(60)),
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the readback map completes")
            .expect("the readback maps");
        let values = slice
            .get_mapped_range()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        readback.unmap();
        values
    }
}

fn instance() -> EffectInstance {
    let mut effect = EffectAsset::new("Bound", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    effect.bindings = vec![
        EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
        target,
    ];
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap();
    EffectInstance::new(Arc::new(compiled))
}

/// The CPU runtime's view of the same values the kernel writes.
fn cpu_expected(instance: &EffectInstance) -> Vec<f32> {
    let position = BindingFieldId::new(AESTRA_FIELD_POSITION);
    let velocity = BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY);
    let vec3 = |slot: BindingSlot, field: &BindingFieldId| {
        instance
            .binding_field(slot, field)
            .map_or([0.0; 3], |values| [values[0], values[1], values[2]])
    };
    let bound = |slot: BindingSlot| f32::from(u8::from(instance.binding(slot).is_some()));
    let target_velocity = instance.binding_field(BindingSlot(1), &velocity);
    let mut expected = Vec::new();
    expected.extend(vec3(BindingSlot(0), &position));
    expected.extend(vec3(BindingSlot(1), &position));
    expected.push(bound(BindingSlot(0)));
    expected.push(bound(BindingSlot(1)));
    expected.push(f32::from(u8::from(target_velocity.is_some())));
    expected.extend(vec3(BindingSlot(1), &velocity));
    expected
}

#[test]
fn gpu_reads_of_a_moving_binding_trace_match_the_cpu_runtime() {
    let Some(harness) = Harness::new() else {
        assert!(
            std::env::var_os(REQUIRED_GPU_ENV).is_none(),
            "{REQUIRED_GPU_ENV} is set but no compute adapter is available"
        );
        eprintln!("skipping host-binding conformance: no compatible compute adapter");
        return;
    };
    let mut instance = instance();
    let layout = &instance.effect().bindings[1].layout;
    let position = layout
        .field(&BindingFieldId::new(AESTRA_FIELD_POSITION))
        .unwrap()
        .1
        .offset;
    let (velocity_index, velocity_packed) = layout
        .field(&BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY))
        .unwrap();
    let wgsl = kernel(position, velocity_index as u32, velocity_packed.offset);

    // The roadmap's moving trace; velocity supplied on some ticks only, target lost on the last.
    let trace: [TraceTick; 5] = [
        (Some([4.0, 0.0, 0.0]), [0.0, 0.0, 0.0], None),
        (
            Some([4.0, 1.0, 0.0]),
            [1.0, 0.0, 0.0],
            Some([0.0, 10.0, 0.0]),
        ),
        (
            Some([3.0, 2.0, 0.0]),
            [2.0, 0.0, 0.0],
            Some([-10.0, 10.0, 0.0]),
        ),
        (Some([2.0, 2.5, 0.5]), [3.0, 0.0, 0.0], None),
        (None, [4.0, 0.0, 0.0], None),
    ];
    for (tick, (target, source, velocity)) in trace.into_iter().enumerate() {
        instance
            .set_spatial_binding("Source", SpatialBindingSnapshot::at(source))
            .unwrap();
        match target {
            Some(position) => {
                let mut snapshot = SpatialBindingSnapshot::at(position);
                snapshot.linear_velocity = velocity;
                instance.set_spatial_binding("Target", snapshot).unwrap();
            }
            None => instance.set_binding(BindingSlot(1), None).unwrap(),
        }
        let gpu = harness.run(&wgsl, &GpuHostBindings::from_instance(&instance));
        assert_eq!(gpu, cpu_expected(&instance), "tick {tick}");
    }
    // The SnapshotOnSpawn source stayed latched at the first tick's position throughout.
    assert_eq!(cpu_expected(&instance)[..3], [0.0, 0.0, 0.0]);
}
