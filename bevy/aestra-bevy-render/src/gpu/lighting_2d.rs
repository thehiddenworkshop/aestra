//! Camera-local XY point lights for semantic particle materials.
use std::num::NonZeroU32;

use bevy::{
    app::SubApp,
    camera::Camera2d,
    ecs::{
        query::{QueryItem, ROQueryItem},
        system::{SystemParamItem, lifetimeless::*},
    },
    extract::{extract_component::ExtractComponent, sync_component::SyncComponent},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, PipelineCache, ShaderStages, ShaderType, StorageBuffer,
            binding_types::storage_buffer_read_only,
        },
        renderer::{RenderDevice, RenderQueue},
    },
};

/// Lights visible to Aestra sprite materials on one native GPU `Camera2d`.
///
/// Materials opt in through Scene Point Irradiance. Positions and radii use
/// world-space XY units. A camera without this component contributes zero.
#[derive(Component, Clone, Debug, Default)]
pub struct AestraLights2d {
    pub lights: Vec<AestraPointLight2d>,
}

/// A point light with a finite radius and an unshadowed radial falloff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AestraPointLight2d {
    pub position: Vec2,
    pub radius: f32,
    /// Linear RGB, multiplied by intensity before evaluating the falloff.
    pub color: Vec3,
    pub intensity: f32,
    pub falloff: PointLightFalloff2d,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointLightFalloff2d {
    #[default]
    Continuous,
    /// Quantize the attenuation of this light before adding its irradiance.
    Stepped(NonZeroU32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
struct GpuPointLight {
    center: Vec2,
    radius: f32,
    bands: u32,
    irradiance: Vec3,
}

#[derive(Default, PartialEq, ShaderType)]
struct GpuLights {
    count: u32,
    #[shader(size(runtime))]
    lights: Vec<GpuPointLight>,
}

impl AestraPointLight2d {
    fn gpu(self) -> Option<GpuPointLight> {
        let irradiance = self.color.max(Vec3::ZERO) * self.intensity;
        (self.position.is_finite()
            && self.radius.is_finite()
            && self.radius > 0.0
            && self.intensity > 0.0
            && self.color.is_finite()
            && irradiance.is_finite()
            && irradiance != Vec3::ZERO)
            .then_some(GpuPointLight {
                center: self.position,
                radius: self.radius,
                bands: match self.falloff {
                    PointLightFalloff2d::Continuous => 0,
                    PointLightFalloff2d::Stepped(bands) => bands.get(),
                },
                irradiance,
            })
    }
}

impl SyncComponent<RenderApp> for AestraLights2d {
    type Target = (Self, PreparedLights2d);
}

impl ExtractComponent<RenderApp> for AestraLights2d {
    type QueryData = (&'static Self, &'static Camera);
    type QueryFilter = With<Camera2d>;
    type Out = Self;

    fn extract_component((lights, camera): QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        camera.is_active.then(|| lights.clone())
    }
}

pub(super) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "aestra 2d point lights",
        &BindGroupLayoutEntries::single(
            ShaderStages::FRAGMENT,
            storage_buffer_read_only::<GpuLights>(false),
        ),
    )
}

#[doc(hidden)]
#[derive(Component)]
pub struct PreparedLights2d {
    buffer: StorageBuffer<GpuLights>,
    bind_group: BindGroup,
}

impl PreparedLights2d {
    fn new(
        data: GpuLights,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Self {
        let mut buffer = StorageBuffer::from(data);
        buffer.write_buffer(device, queue);
        let bind_group = Self::bind(&buffer, layout, device);
        Self { buffer, bind_group }
    }

    fn bind(
        buffer: &StorageBuffer<GpuLights>,
        layout: &BindGroupLayout,
        device: &RenderDevice,
    ) -> BindGroup {
        device.create_bind_group(
            "aestra 2d point lights",
            layout,
            &BindGroupEntries::single(buffer.buffer().unwrap().as_entire_binding()),
        )
    }
}

#[derive(Resource)]
pub(super) struct EmptyLights2d(PreparedLights2d);

pub(super) fn install(app: &mut SubApp) {
    app.add_systems(RenderStartup, init)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups));
}

fn init(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
) {
    commands.insert_resource(EmptyLights2d(PreparedLights2d::new(
        GpuLights {
            count: 0,
            lights: vec![GpuPointLight::default()],
        },
        &cache.get_bind_group_layout(&layout()),
        &device,
        &queue,
    )));
}

fn prepare(
    mut commands: Commands,
    mut views: Query<(Entity, &AestraLights2d, Option<&mut PreparedLights2d>)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut scratch: Local<GpuLights>,
) {
    if views.is_empty() {
        return;
    }
    let limits = device.limits();
    let available = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let stride = GpuPointLight::min_size().get();
    let header = GpuLights::min_size().get() - stride;
    let limit = (available.saturating_sub(header) / stride) as usize;
    let layout = cache.get_bind_group_layout(&layout());
    for (view, lights, prepared) in &mut views {
        scratch.lights.clear();
        let mut valid = lights.lights.iter().filter_map(|light| light.gpu());
        scratch.lights.extend(valid.by_ref().take(limit));
        if valid.next().is_some() {
            bevy::log::warn_once!("Aestra 2D point light buffer reached the device binding limit");
        }
        scratch.count = scratch.lights.len() as u32;
        if scratch.count == 0 {
            if prepared.is_some() {
                commands.entity(view).remove::<PreparedLights2d>();
            }
        } else if let Some(mut prepared) = prepared {
            if prepared.buffer.get() != &*scratch {
                let previous = prepared.buffer.buffer().unwrap().id();
                std::mem::swap(prepared.buffer.get_mut(), &mut *scratch);
                prepared.buffer.write_buffer(&device, &queue);
                if prepared.buffer.buffer().unwrap().id() != previous {
                    prepared.bind_group =
                        PreparedLights2d::bind(&prepared.buffer, &layout, &device);
                }
            }
        } else {
            commands.entity(view).insert(PreparedLights2d::new(
                std::mem::take(&mut *scratch),
                &layout,
                &device,
                &queue,
            ));
        }
    }
}

pub(super) struct SetLights2d<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetLights2d<I> {
    type Param = SRes<EmptyLights2d>;
    type ViewQuery = Option<Read<PreparedLights2d>>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<()>,
        empty: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let lights = view.unwrap_or(&empty.into_inner().0);
        pass.set_bind_group(I, &lights.bind_group, &[]);
        RenderCommandResult::Success
    }
}
