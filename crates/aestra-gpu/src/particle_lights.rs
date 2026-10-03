//! Portable, read-only particle-light selection prototype (F7D1).
//!
//! One output/presentation occurrence per job. Bindings: particles (read), plan
//! (read), keys (read), dispatch (uniform), sorted runs (read), next runs (write),
//! counters (write). Evaluation writes compact sorted blocks, then each merge
//! retains at most K entries. Ping-pong buffers; no atomics determine ordering.
//! Hosts must clear counters before evaluation, dispatch every merge in order,
//! then `finish_lights`, and read back only final records/counters if needed.
//! Global admission merges already quality-capped output runs; no source records
//! cross the CPU boundary. The Bevy adapter owns dispatch and occurrence manifests.
use aestra_core::{CurveInterpolation, valid_light_rgb};
use aestra_runtime::{
    CompiledCurve, CompiledGradient, ParticleLightColorPlan, ParticlePointLightPlan, RuntimeValue,
};
use encase::ShaderType;
use glam::{Mat4, UVec4, Vec3, Vec4};

pub const PARTICLE_LIGHT_WESL: &str = concat!(
    include_str!("shaders/aestra_light_order.wesl"),
    include_str!("shaders/aestra_particle_lights.wesl"),
    include_str!("shaders/aestra_light_merge.wesl")
);
pub const GLOBAL_PARTICLE_LIGHT_WESL: &str = concat!(
    include_str!("shaders/aestra_light_order.wesl"),
    include_str!("shaders/aestra_global_lights.wesl"),
    include_str!("shaders/aestra_light_merge.wesl")
);
pub const GLOBAL_LIGHT_ENTRY_POINTS: &[&str] =
    &["merge_lights", "sum_light_counts", "finish_lights"];
pub const PARTICLE_LIGHT_ENTRY_POINTS: &[&str] =
    &["evaluate_lights", "merge_lights", "finish_lights"];
pub const LIGHT_RECORD_BYTES: u64 = 48;

/// Radius plus priority/source-token/spawn ordinal complete the 48-byte record.
/// Token is a host manifest key, not a truncated UUID or emitter array index.
/// Hosts map it to root instance/epoch/clip occurrence + emitter/region/output.
/// For cross-output merges, token order must match that canonical identity order.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuParticleLight {
    pub position_range: Vec4,
    pub color_intensity: Vec4,
    pub radius: f32,
    pub priority: u32,
    pub source_token: u32,
    pub particle_index: u32,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuLightKey {
    pub value: Vec4,
    pub _padding: Vec3,
    pub time: f32,
}

#[derive(Debug, Clone, Copy, ShaderType)]
pub struct GpuParticleLightPlan {
    /// Applied once to particle positions; radius/range already use world units.
    pub world_from_particle: Mat4,
    pub constant_color: Vec4,
    /// intensity offset/count/interpolation/color mode (0 particle, 1 constant, 2 gradient).
    pub intensity: UVec4,
    /// range offset/count/interpolation/reserved.
    pub range: UVec4,
    /// gradient offset/count/source token/priority.
    pub gradient: UVec4,
    /// emitter index, enabled, use GPU render globals transform, reserved.
    pub source: UVec4,
    pub _padding: Vec3,
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ShaderType)]
pub struct GpuLightDispatch {
    /// particle offset/count; input run width/count.
    pub input: UVec4,
    /// next run width; cap K; reserved; reserved.
    pub output: UVec4,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ShaderType)]
pub struct GpuLightCounters {
    /// Alive (state 1) particles of the requested emitter in the supplied pool.
    pub requested: u32,
    /// Valid positive-lumen candidates, before admission.
    pub candidates: u32,
    pub selected: u32,
    /// candidates - selected; invalid/dead inputs are not budget drops.
    pub dropped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParticleLightError {
    #[error("particle light table/address exceeds u32 or configured device storage limits")]
    Capacity,
    #[error("invalid particle-light plan, color override, transform or curve")]
    InvalidPlan,
}

/// Lower once per plan/parameter change, not per particle. Variable-sized key
/// storage preserves all 32 authored curve keys and does not truncate gradients
/// to the simulation ABI's unrelated eight-key limit. Invalid live overrides
/// return an error: adapters must disable this output, never reuse stale color.
pub fn lower_plan(
    plan: &ParticlePointLightPlan,
    parameters: &[RuntimeValue],
    emitter_index: u32,
    source_token: u32,
    world_from_particle: Mat4,
    max_storage_bytes: u64,
) -> Result<(GpuParticleLightPlan, Vec<GpuLightKey>), ParticleLightError> {
    if max_storage_bytes < GpuParticleLightPlan::min_size().get() {
        return Err(ParticleLightError::Capacity);
    }
    if emitter_index > u16::MAX as u32
        || !world_from_particle.is_finite()
        || world_from_particle.row(3) != Vec4::W
        || !plan.radius.is_finite()
        || plan.radius < 0.0
    {
        return Err(ParticleLightError::InvalidPlan);
    }
    let mut keys = Vec::new();
    let intensity = lower_curve(&plan.intensity, &mut keys, false)?;
    let range = lower_curve(&plan.range, &mut keys, true)?;
    let mut constant_color = Vec4::ONE;
    let (mode, gradient) = match &plan.color {
        ParticleLightColorPlan::ParticleColor => (0, UVec4::ZERO),
        ParticleLightColorPlan::Constant(rgb) => {
            if !valid_light_rgb(*rgb) {
                return Err(ParticleLightError::InvalidPlan);
            }
            constant_color = Vec3::from_array(*rgb).extend(1.0);
            (1, UVec4::ZERO)
        }
        ParticleLightColorPlan::Gradient(gradient) => {
            (2, lower_gradient(gradient, &mut keys, max_storage_bytes)?)
        }
        ParticleLightColorPlan::GradientParameter(slot) => {
            let Some(RuntimeValue::Gradient(gradient)) = parameters.get(slot.0) else {
                return Err(ParticleLightError::InvalidPlan);
            };
            (2, lower_gradient(gradient, &mut keys, max_storage_bytes)?)
        }
    };
    if keys.len() as u64 * GpuLightKey::min_size().get() > max_storage_bytes {
        return Err(ParticleLightError::Capacity);
    }
    Ok((
        GpuParticleLightPlan {
            world_from_particle,
            constant_color,
            intensity: UVec4::new(intensity.x, intensity.y, intensity.z, mode),
            range,
            gradient: UVec4::new(gradient.x, gradient.y, source_token, plan.priority),
            source: UVec4::new(emitter_index, u32::from(plan.max_lights != 0), 0, 0),
            radius: plan.radius,
            _padding: Vec3::ZERO,
        },
        keys,
    ))
}

fn lower_curve(
    curve: &CompiledCurve,
    keys: &mut Vec<GpuLightKey>,
    positive: bool,
) -> Result<UVec4, ParticleLightError> {
    let first = curve.first().ok_or(ParticleLightError::InvalidPlan)?;
    let values =
        std::iter::once(first).chain(curve.segments().iter().map(|s| (s.end_time, s.end_value)));
    let offset = u32::try_from(keys.len()).map_err(|_| ParticleLightError::Capacity)?;
    let mut previous = -1.0;
    for (time, value) in values {
        if !time.is_finite()
            || !(0.0..=1.0).contains(&time)
            || time <= previous
            || !value.is_finite()
            || value < 0.0
            || (positive && value == 0.0)
        {
            return Err(ParticleLightError::InvalidPlan);
        }
        keys.push(GpuLightKey {
            value: Vec4::splat(value),
            time,
            _padding: Vec3::ZERO,
        });
        previous = time;
    }
    let count = u32::try_from(keys.len()).map_err(|_| ParticleLightError::Capacity)? - offset;
    if count > 32 {
        return Err(ParticleLightError::InvalidPlan);
    }
    let interpolation = match curve.interpolation() {
        CurveInterpolation::Smooth => 0,
        CurveInterpolation::Linear => 1,
        CurveInterpolation::Step => 2,
    };
    Ok(UVec4::new(offset, count, interpolation, 0))
}

fn lower_gradient(
    gradient: &CompiledGradient,
    keys: &mut Vec<GpuLightKey>,
    max_storage_bytes: u64,
) -> Result<UVec4, ParticleLightError> {
    let first = gradient.first().ok_or(ParticleLightError::InvalidPlan)?;
    let required = gradient
        .segments()
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_add(keys.len()))
        .ok_or(ParticleLightError::Capacity)?;
    if required > u32::MAX as usize
        || required as u64 * GpuLightKey::min_size().get() > max_storage_bytes
    {
        return Err(ParticleLightError::Capacity);
    }
    let offset = u32::try_from(keys.len()).map_err(|_| ParticleLightError::Capacity)?;
    let values = std::iter::once(first).chain(
        gradient
            .segments()
            .iter()
            .map(|s| (s.end_time, s.end_color)),
    );
    let mut previous = -1.0;
    for (time, color) in values {
        if !time.is_finite()
            || !(0.0..=1.0).contains(&time)
            || time <= previous
            || !valid_light_rgb(color[..3].try_into().unwrap())
        {
            return Err(ParticleLightError::InvalidPlan);
        }
        keys.push(GpuLightKey {
            value: Vec4::from_array(color),
            time,
            _padding: Vec3::ZERO,
        });
        previous = time;
    }
    let count = u32::try_from(keys.len()).map_err(|_| ParticleLightError::Capacity)? - offset;
    Ok(UVec4::new(offset, count, 0, 0))
}

/// Device-bounded scratch/dispatch schedule. Empty/disabled jobs allocate and
/// dispatch nothing. Host passes the minimum of output quality cap and host cap.
/// Input is a contiguous live-particle pool, not trail history records. Counts
/// include dead slots; the shader filters them. Buffers are provisioned once at
/// this capacity and reused across frames. Counters need 16 additional bytes.
#[derive(Debug, Clone)]
pub struct ParticleLightWorkPlan {
    pub evaluate: GpuLightDispatch,
    pub merges: Vec<GpuLightDispatch>,
    pub finish: GpuLightDispatch,
    pub scratch_records: u32,
    pub scratch_bytes_per_buffer: u64,
    pub selected_capacity: u32,
}

/// Merge independently capped sorted output runs, padded to `input_width` by
/// the adapter. Device limits apply to each scratch buffer and all dispatches.
/// Counts must be summed separately (sum_light_counts), then finish_lights.
#[derive(Debug, Clone)]
pub struct GlobalLightWorkPlan {
    pub input_width: u32,
    pub input_runs: u32,
    pub scratch_records: u32,
    pub scratch_bytes_per_buffer: u64,
    pub merges: Vec<GpuLightDispatch>,
    pub finish: GpuLightDispatch,
    pub selected_capacity: u32,
}

impl GlobalLightWorkPlan {
    pub fn new(
        width: u32,
        runs: u32,
        cap: u32,
        max_storage_bytes: u64,
        max_workgroups: u32,
    ) -> Result<Option<Self>, ParticleLightError> {
        if width == 0 || runs == 0 || cap == 0 {
            return Ok(None);
        }
        if width > cap {
            return Err(ParticleLightError::InvalidPlan);
        }
        let mut records = width
            .checked_mul(runs)
            .ok_or(ParticleLightError::Capacity)?;
        if runs.div_ceil(64) > max_workgroups {
            return Err(ParticleLightError::Capacity);
        }
        let mut current_width = width;
        let mut current_runs = runs;
        let mut merges = Vec::new();
        while current_runs > 1 {
            if current_runs
                .checked_mul(current_width)
                .ok_or(ParticleLightError::Capacity)?
                .div_ceil(64)
                > max_workgroups
            {
                return Err(ParticleLightError::Capacity);
            }
            let next_width = current_width
                .checked_mul(2)
                .ok_or(ParticleLightError::Capacity)?
                .min(cap);
            merges.push(GpuLightDispatch {
                input: UVec4::new(0, 0, current_width, current_runs),
                output: UVec4::new(next_width, cap, 0, 0),
            });
            current_runs = current_runs.div_ceil(2);
            current_width = next_width;
            records = records.max(
                current_width
                    .checked_mul(current_runs)
                    .ok_or(ParticleLightError::Capacity)?,
            );
        }
        let bytes = u64::from(records) * LIGHT_RECORD_BYTES;
        if bytes > max_storage_bytes || u64::from(runs) * 16 > max_storage_bytes {
            return Err(ParticleLightError::Capacity);
        }
        Ok(Some(Self {
            input_width: width,
            input_runs: runs,
            scratch_records: records,
            scratch_bytes_per_buffer: bytes,
            merges,
            finish: GpuLightDispatch {
                input: UVec4::new(0, runs, current_width, 1),
                output: UVec4::ZERO,
            },
            selected_capacity: current_width,
        }))
    }
}

impl ParticleLightWorkPlan {
    /// Preferred output-specific entry point: authored quality cap and host cap
    /// are independently honored. Disabled compiled emitters must be skipped by
    /// the adapter before lowering/scheduling their outputs.
    pub fn for_output(
        plan: &ParticlePointLightPlan,
        particle_offset: u32,
        particle_count: u32,
        host_cap: u32,
        max_storage_bytes: u64,
        max_workgroups: u32,
    ) -> Result<Option<Self>, ParticleLightError> {
        Self::new(
            particle_offset,
            particle_count,
            host_cap.min(plan.max_lights),
            max_storage_bytes,
            max_workgroups,
        )
    }

    pub fn new(
        particle_offset: u32,
        particle_count: u32,
        cap: u32,
        max_storage_bytes: u64,
        max_workgroups: u32,
    ) -> Result<Option<Self>, ParticleLightError> {
        if cap == 0 || particle_count == 0 {
            return Ok(None);
        }
        let end = particle_offset
            .checked_add(particle_count)
            .ok_or(ParticleLightError::Capacity)?;
        if u64::from(end) * 48 > max_storage_bytes {
            return Err(ParticleLightError::Capacity);
        }
        let cap = cap.min(particle_count);
        let mut runs = particle_count.div_ceil(64);
        let mut width = cap.min(64);
        let records = runs
            .checked_mul(width)
            .ok_or(ParticleLightError::Capacity)?;
        let bytes = u64::from(records) * LIGHT_RECORD_BYTES;
        if bytes > max_storage_bytes || runs > max_workgroups {
            return Err(ParticleLightError::Capacity);
        }
        let evaluate = GpuLightDispatch {
            input: UVec4::new(particle_offset, particle_count, 0, runs),
            output: UVec4::new(width, cap, 0, 0),
        };
        let mut merges = Vec::new();
        let mut scratch_records = records;
        while runs > 1 {
            let next_width = width
                .checked_mul(2)
                .ok_or(ParticleLightError::Capacity)?
                .min(cap);
            let next_runs = runs.div_ceil(2);
            let input_records = runs
                .checked_mul(width)
                .ok_or(ParticleLightError::Capacity)?;
            if input_records.div_ceil(64) > max_workgroups {
                return Err(ParticleLightError::Capacity);
            }
            let next_records = next_runs
                .checked_mul(next_width)
                .ok_or(ParticleLightError::Capacity)?;
            scratch_records = scratch_records.max(next_records);
            if u64::from(scratch_records) * LIGHT_RECORD_BYTES > max_storage_bytes {
                return Err(ParticleLightError::Capacity);
            }
            merges.push(GpuLightDispatch {
                input: UVec4::new(0, 0, width, runs),
                output: UVec4::new(next_width, cap, 0, 0),
            });
            width = next_width;
            runs = next_runs;
        }
        Ok(Some(Self {
            evaluate,
            merges,
            finish: GpuLightDispatch {
                input: UVec4::new(0, 0, width, 1),
                output: UVec4::ZERO,
            },
            scratch_records,
            scratch_bytes_per_buffer: u64::from(scratch_records) * LIGHT_RECORD_BYTES,
            selected_capacity: width,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_and_portable_shader_validate() {
        assert_eq!(GpuParticleLight::min_size().get(), LIGHT_RECORD_BYTES);
        assert_eq!(GpuLightCounters::min_size().get(), 16);
        assert_eq!(GpuLightDispatch::min_size().get(), 32);
        assert_eq!(GpuLightKey::min_size().get(), 32);
        assert_eq!(GpuParticleLightPlan::min_size().get(), 160);
        crate::shader::compile_wesl(
            "package::particle_lights",
            PARTICLE_LIGHT_WESL,
            PARTICLE_LIGHT_ENTRY_POINTS,
        )
        .unwrap();
        crate::shader::compile_wesl(
            "package::global_lights",
            GLOBAL_PARTICLE_LIGHT_WESL,
            GLOBAL_LIGHT_ENTRY_POINTS,
        )
        .unwrap();
    }

    #[test]
    fn schedules_are_bounded_and_zero_budget_is_no_work() {
        assert!(
            ParticleLightWorkPlan::new(0, u32::MAX, 0, 0, 0)
                .unwrap()
                .is_none()
        );
        assert!(ParticleLightWorkPlan::new(u32::MAX, 2, 16, u64::MAX, u32::MAX).is_err());
        assert!(ParticleLightWorkPlan::new(0, 4096, 16, 100, 65535).is_err());
        assert!(ParticleLightWorkPlan::new(0, 4096, 16, u64::MAX, 1).is_err());
        for count in [1, 63, 64, 65, 1025, 4096, 262144] {
            for cap in [1, 16, 63, 64, 65, 129, 4096] {
                let job = ParticleLightWorkPlan::new(0, count, cap, u64::MAX, 65535)
                    .unwrap()
                    .unwrap();
                assert_eq!(job.selected_capacity, count.min(cap));
                for merge in &job.merges {
                    assert!(merge.input.z * merge.input.w <= job.scratch_records);
                    assert!(merge.output.x * merge.input.w.div_ceil(2) <= job.scratch_records);
                }
            }
        }
    }

    #[test]
    fn global_runs_honor_caps_padding_and_device_bounds() {
        assert!(
            GlobalLightWorkPlan::new(64, 100, 0, 0, 0)
                .unwrap()
                .is_none()
        );
        assert!(GlobalLightWorkPlan::new(65, 2, 64, u64::MAX, 65535).is_err());
        assert!(GlobalLightWorkPlan::new(64, u32::MAX, 64, u64::MAX, u32::MAX).is_err());
        assert!(GlobalLightWorkPlan::new(16, 100, 32, 100, 65535).is_err());
        assert!(GlobalLightWorkPlan::new(16, 100, 32, u64::MAX, 1).is_err());
        for runs in [1, 2, 3, 17, 65, 1025] {
            for width in [1, 3, 64, 65] {
                let work = GlobalLightWorkPlan::new(width, runs, 129, u64::MAX, 65535)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    work.selected_capacity,
                    129.min(width * runs.next_power_of_two())
                );
                for d in &work.merges {
                    assert!(d.input.z * d.input.w <= work.scratch_records);
                    assert!(d.output.x * d.input.w.div_ceil(2) <= work.scratch_records);
                }
            }
        }
    }

    #[test]
    fn lower_plan_preserves_tables_and_rejects_invalid_live_values() {
        use aestra_core::{ColorKey, Curve, CurveKey, Gradient, ParticleLightSelectionPolicy};
        use aestra_runtime::ParameterSlot;
        let curve = CompiledCurve::compile(&Curve::new(
            (0..32)
                .map(|i| CurveKey::new(i as f32 / 31.0, 1.0))
                .collect(),
        ));
        let gradient = CompiledGradient::compile(&Gradient::new(
            (0..20)
                .map(|i| ColorKey::new(i as f32 / 19.0, [0.5; 4]))
                .collect(),
        ));
        let mut plan = ParticlePointLightPlan {
            color: ParticleLightColorPlan::GradientParameter(ParameterSlot(0)),
            intensity: curve.clone(),
            range: curve,
            radius: 0.1,
            selection_policy: ParticleLightSelectionPolicy::Brightest,
            max_lights: 16,
            priority: 5,
        };
        let values = [RuntimeValue::Gradient(gradient)];
        assert_eq!(
            ParticleLightWorkPlan::for_output(&plan, 0, 1024, 64, 1 << 20, 65535)
                .unwrap()
                .unwrap()
                .selected_capacity,
            16
        );
        assert_eq!(
            ParticleLightWorkPlan::for_output(&plan, 0, 1024, 4, 1 << 20, 65535)
                .unwrap()
                .unwrap()
                .selected_capacity,
            4
        );
        let (gpu, keys) = lower_plan(&plan, &values, 12, 99, Mat4::IDENTITY, 4096).unwrap();
        assert_eq!(keys.len(), 84);
        assert_eq!(gpu.intensity.y, 32);
        assert_eq!(gpu.range.y, 32);
        assert_eq!(gpu.gradient, UVec4::new(64, 20, 99, 5));
        assert_eq!(gpu.source.x, 12);
        assert_eq!(
            lower_plan(&plan, &values, 12, 99, Mat4::IDENTITY, 64).unwrap_err(),
            ParticleLightError::Capacity
        );
        assert!(lower_plan(&plan, &[], 12, 99, Mat4::IDENTITY, 4096).is_err());
        assert!(
            lower_plan(
                &plan,
                &[RuntimeValue::Bool(false)],
                12,
                99,
                Mat4::IDENTITY,
                4096
            )
            .is_err()
        );
        let empty = RuntimeValue::Gradient(CompiledGradient::compile(&Gradient::new(vec![])));
        assert!(lower_plan(&plan, &[empty], 12, 99, Mat4::IDENTITY, 4096).is_err());
        plan.color = ParticleLightColorPlan::Constant([f32::NAN, 0.0, 0.0]);
        assert!(lower_plan(&plan, &[], 12, 99, Mat4::IDENTITY, 4096).is_err());
        plan.color = ParticleLightColorPlan::ParticleColor;
        assert!(lower_plan(&plan, &[], 65536, 99, Mat4::IDENTITY, 4096).is_err());
        assert!(
            lower_plan(
                &plan,
                &[],
                12,
                99,
                Mat4::from_cols_array(&[f32::NAN; 16]),
                4096
            )
            .is_err()
        );
        plan.max_lights = 0;
        assert!(
            ParticleLightWorkPlan::for_output(&plan, 0, 1024, 64, 1 << 20, 65535)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            lower_plan(&plan, &[], 12, 99, Mat4::IDENTITY, 4096)
                .unwrap()
                .0
                .source
                .y,
            0
        );
    }
}
