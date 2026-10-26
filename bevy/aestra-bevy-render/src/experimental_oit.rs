//! Bounded, opt-in fragment-list experiment. Not production transparency acceptance.
#[path = "experimental_oit/diagnostics.rs"]
mod diagnostics;
use bevy::{
    core_pipeline::oit::OrderIndependentTransparencySettings,
    extract::extract_component::{ExtractComponent, ExtractComponentPlugin},
    prelude::*,
    render::{
        RenderApp,
        renderer::{RenderAdapter, RenderDevice},
    },
};
pub use diagnostics::{ExperimentalOitDiagnosticSample, ExperimentalOitDiagnostics};

/// Experimental 3D alpha/additive integration with Bevy's fragment-list OIT.
/// Requires explicit `Msaa::Off`. Owns Bevy's OIT camera settings while approved.
/// Eligible rendered Aestra sprites, flipbooks, trails and semantic mesh particles
/// share depth-ordered lists. Semantic state must use Less/LessEqual without depth
/// writes. Additive RGB is coverage-weighted emission with zero extinction alpha.
/// Does not certify overflow, arbitrary materials, all depth paths or smoke art.
/// Known limitation: the resolve overlays ordinary transparent draws regardless of their depth.
/// Ribbons, multiply, depth-writing/overlay/wireframe, legacy meshes and foreign/custom
/// draws are not qualified participants; foreground instances can be incorrectly covered.
/// Healthy diagnostics do not rule this out. Local-sort saturation also approximates
/// already weighted RGB, which can lose additive emission. Keep this opt-in disabled
/// for production mixed scenes pending broader interoperability and cost qualification.
#[derive(Component, Clone, Copy, Debug)]
pub struct ExperimentalOitSettings {
    pub fragments_per_pixel_average: f32,
    pub sorted_fragment_max_count: u32,
    /// Budget for shared node/head storage across all opted-in cameras, not per effect.
    /// The strictest camera budget wins. No automatic quality or MSAA reduction.
    pub max_storage_bytes: u64,
}

impl Default for ExperimentalOitSettings {
    fn default() -> Self {
        Self {
            fragments_per_pixel_average: 4.0,
            sorted_fragment_max_count: 8,
            max_storage_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Approval/allocation estimate, not a fragment-overflow or exactness guarantee.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct ExperimentalOitStatus {
    pub estimated_storage_bytes: u64,
    pub rejection: Option<&'static str>,
}

/// CPU-side buffer ownership metadata only; never maps particle/fragment buffers.
#[derive(Resource, Clone, Default)]
pub struct ExperimentalOitStatistics(std::sync::Arc<std::sync::Mutex<ExperimentalOitAllocation>>);

#[derive(Clone, Copy, Debug, Default)]
pub struct ExperimentalOitAllocation {
    pub node_capacity: usize,
    pub head_capacity: usize,
    pub storage_bytes: u64,
}

impl ExperimentalOitStatistics {
    pub fn snapshot(&self) -> ExperimentalOitAllocation {
        *self.0.lock().unwrap()
    }
}

#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
pub(crate) struct OitApproved;

pub(crate) fn install(app: &mut App) {
    diagnostics::install(app);
    let statistics = ExperimentalOitStatistics::default();
    app.insert_resource(statistics.clone());
    app.add_plugins(ExtractComponentPlugin::<OitApproved, RenderApp>::default())
        .add_systems(PreUpdate, approve)
        .add_systems(PostUpdate, approve.after(bevy::camera::CameraUpdateSystems));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.insert_resource(statistics).add_systems(
            bevy::render::Render,
            publish.after(bevy::render::RenderSystems::PrepareResources),
        );
    }
}

fn publish(
    buffers: Option<Res<bevy::core_pipeline::oit::OitBuffers>>,
    statistics: Res<ExperimentalOitStatistics>,
) {
    let allocation = buffers.map_or_else(ExperimentalOitAllocation::default, |buffers| {
        ExperimentalOitAllocation {
            node_capacity: buffers.nodes.capacity(),
            head_capacity: buffers.heads.capacity(),
            storage_bytes: buffers.nodes.buffer().map_or(0, |b| b.size())
                + buffers.heads.buffer().map_or(0, |b| b.size()),
        }
    });
    *statistics.0.lock().unwrap() = allocation;
}

fn estimate(size: UVec2, average: f32) -> Option<u64> {
    let pixels = u64::from(size.x).checked_mul(u64::from(size.y))?;
    if pixels == 0 || pixels > u64::from(u32::MAX) {
        return None;
    }
    // Match Bevy's f32 capacity calculation; node = three u32s, head = one u32.
    let nodes = (pixels as f32 * average) as u64;
    if nodes == 0 || nodes > u64::from(u32::MAX) {
        return None;
    }
    nodes.checked_mul(12)?.checked_add(pixels.checked_mul(4)?)
}

fn valid(settings: &ExperimentalOitSettings) -> bool {
    settings.fragments_per_pixel_average.is_finite()
        && settings.fragments_per_pixel_average > 0.0
        && settings.fragments_per_pixel_average <= 16.0
        && (1..=32).contains(&settings.sorted_fragment_max_count)
        && settings.max_storage_bytes > 0
}

#[allow(clippy::type_complexity)]
fn approve(
    mut commands: Commands,
    cameras: Query<
        (
            Entity,
            &Camera,
            &Msaa,
            Option<&ExperimentalOitSettings>,
            Has<OitApproved>,
            Has<OrderIndependentTransparencySettings>,
            Has<ExperimentalOitStatus>,
        ),
        With<Camera3d>,
    >,
    device: Option<Res<RenderDevice>>,
    adapter: Option<Res<RenderAdapter>>,
    raw_cameras: Query<
        (
            Has<ExperimentalOitSettings>,
            Has<OitApproved>,
            Has<Camera3d>,
        ),
        With<OrderIndependentTransparencySettings>,
    >,
) {
    let supported = device
        .as_ref()
        .is_some_and(|d| d.limits().max_storage_buffers_per_shader_stage >= 12)
        && adapter.as_ref().is_some_and(|a| {
            a.get_downlevel_capabilities()
                .flags
                .contains(bevy::render::render_resource::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE)
        });
    let mut extent = UVec2::ZERO;
    let mut average = 0.0_f32;
    let mut budget = u64::MAX;
    let foreign_oit = raw_cameras
        .iter()
        .any(|(settings, owned, three_d)| !owned && (!settings || !three_d))
        || cameras
            .iter()
            .any(|(_, _, _, settings, owned, raw, _)| raw && !owned && settings.is_none());
    for (_, camera, msaa, settings, _, _, _) in &cameras {
        if let Some(settings) = settings
            && valid(settings)
            && *msaa == Msaa::Off
            && let Some(size) = camera.physical_target_size()
        {
            extent = extent.max(size);
            average = average.max(settings.fragments_per_pixel_average);
            budget = budget.min(settings.max_storage_bytes);
        }
    }
    let bytes = estimate(extent, average);
    // Each individual storage binding must also fit the physical device limit.
    let device_budget = device
        .as_ref()
        .map_or(0, |d| d.limits().max_storage_buffer_binding_size);
    let capacity_rejection = match bytes {
        None => Some("invalid shared OIT target capacity"),
        Some(bytes) if bytes > budget.min(device_budget) => {
            Some("shared OIT storage exceeds camera/device budget")
        }
        _ => None,
    };
    for (entity, camera, msaa, settings, owned, _, status) in &cameras {
        let Some(settings) = settings else {
            if owned {
                commands.entity(entity).remove::<(
                    OrderIndependentTransparencySettings,
                    OitApproved,
                    ExperimentalOitStatus,
                )>();
            } else if status {
                commands.entity(entity).remove::<ExperimentalOitStatus>();
            }
            continue;
        };
        let rejection = if !valid(settings) {
            Some("invalid experimental OIT settings")
        } else if *msaa != Msaa::Off {
            Some("experimental OIT requires explicit Msaa::Off")
        } else if camera.physical_target_size().is_none() {
            Some("camera target is not ready")
        } else if !supported {
            Some("experimental OIT requires fragment storage writes and 12 storage bindings")
        } else if foreign_oit {
            Some("shared OIT buffers have an unmanaged camera")
        } else {
            capacity_rejection
        };
        commands.entity(entity).insert(ExperimentalOitStatus {
            estimated_storage_bytes: bytes.unwrap_or(0),
            rejection,
        });
        if rejection.is_none() {
            commands.entity(entity).insert((
                OitApproved,
                OrderIndependentTransparencySettings {
                    fragments_per_pixel_average: settings.fragments_per_pixel_average,
                    sorted_fragment_max_count: settings.sorted_fragment_max_count,
                    alpha_threshold: 0.0,
                },
            ));
        } else {
            commands
                .entity(entity)
                .remove::<(OrderIndependentTransparencySettings, OitApproved)>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_estimate_and_configuration_are_bounded() {
        assert_eq!(estimate(UVec2::new(480, 360), 4.0), Some(8_985_600));
        assert_eq!(estimate(UVec2::ZERO, 4.0), None);
        assert_eq!(estimate(UVec2::splat(u32::MAX), 4.0), None);
        assert!(valid(&ExperimentalOitSettings::default()));
        for average in [f32::NAN, f32::INFINITY, -1.0, 0.0, 17.0] {
            assert!(!valid(&ExperimentalOitSettings {
                fragments_per_pixel_average: average,
                ..default()
            }));
        }
        for count in [0, 33, u32::MAX] {
            assert!(!valid(&ExperimentalOitSettings {
                sorted_fragment_max_count: count,
                ..default()
            }));
        }
    }
}
