//! Active-instance aggregation without engine or renderer dependencies.
use crate::{EffectProfile, ProfileValue};
use aestra_core::{EffectClipId, EffectId};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectInstanceProfile {
    pub path: Vec<EffectClipId>,
    pub effect: EffectId,
    pub name: String,
    pub profile: EffectProfile,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectProfile {
    pub total: EffectProfile,
    /// Active instances only, ordered by stable clip path (root first).
    pub instances: Vec<ProjectInstanceProfile>,
}

impl Default for ProjectProfile {
    fn default() -> Self {
        use ProfileValue::{Measured as M, Unavailable as U};
        Self {
            instances: Vec::new(),
            total: EffectProfile {
                trail_capacity: M(0),
                occupied_trails: M(0),
                retired_trails: M(0),
                trail_evictions: M(0),
                truncated_trails: M(0),
                cpu_time_ns: M(0),
                gpu_time_ns: U,
                gpu_simulation_time_ns: U,
                alive_particles: M(0),
                submitted_instances: M(0),
                submitted_vertices: M(0),
                submitted_primitives: M(0),
                peak_particles: M(0),
                particle_capacity: M(0),
                emitter_count: M(0),
                draw_calls: M(0),
                dispatch_count: M(0),
                estimated_overdraw: U,
                texture_sample_count: M(0),
                buffer_memory_bytes: M(0),
                texture_memory_bytes: M(0),
                collision_time_ns: U,
                emitters: Vec::new(),
                platform_warnings: Vec::new(),
            },
        }
    }
}

impl ProjectProfile {
    /// Replace the active set. Repeated sources remain distinct by path; removed
    /// instances are forgotten. Returns whether the displayed structure changed.
    pub fn update(&mut self, mut instances: Vec<ProjectInstanceProfile>) -> bool {
        instances.sort_by(|a, b| a.path.cmp(&b.path));
        if self
            .instances
            .iter()
            .find(|i| i.path.is_empty())
            .zip(instances.iter().find(|i| i.path.is_empty()))
            .is_some_and(|(old, new)| old.effect != new.effect)
        {
            *self = Self::default();
        }
        let changed = self.instances.len() != instances.len()
            || self.instances.iter().zip(&instances).any(|(a, b)| {
                a.path != b.path
                    || a.effect != b.effect
                    || a.name != b.name
                    || a.profile
                        .emitters
                        .iter()
                        .map(|e| (&e.name, e.source, e.particle_capacity))
                        .ne(b
                            .profile
                            .emitters
                            .iter()
                            .map(|e| (&e.name, e.source, e.particle_capacity)))
            });
        for instance in &mut instances {
            if let Some(old) = self
                .instances
                .binary_search_by(|old| old.path.cmp(&instance.path))
                .ok()
                .map(|index| &self.instances[index])
                .filter(|old| {
                    old.effect == instance.effect
                        && old.profile.particle_capacity == instance.profile.particle_capacity
                })
            {
                instance.profile.peak_particles =
                    peak(old.profile.peak_particles, instance.profile.alive_particles);
                for emitter in &mut instance.profile.emitters {
                    if let Some(previous) = old
                        .profile
                        .emitters
                        .iter()
                        .find(|e| e.source == emitter.source)
                    {
                        emitter.peak_particles =
                            previous.peak_particles.max(emitter.alive_particles);
                    }
                }
            }
            // Effects with no trail renderer contribute a known zero, not missing telemetry.
            if instance.profile.trail_capacity.value() == Some(0) {
                instance
                    .profile
                    .record_trail_usage(Some(Default::default()));
            }
            if instance.profile.particle_capacity.value() == Some(0) {
                instance.profile.alive_particles = ProfileValue::Measured(0);
                instance.profile.peak_particles = ProfileValue::Measured(0);
                instance.profile.submitted_instances = ProfileValue::Measured(0);
                instance.profile.submitted_vertices = ProfileValue::Measured(0);
                instance.profile.submitted_primitives = ProfileValue::Measured(0);
                instance.profile.draw_calls = ProfileValue::Measured(0);
                // No simulation dispatch exists for an empty choreography carrier.
                instance.profile.gpu_simulation_time_ns = ProfileValue::Measured(0);
            }
        }
        let mut total = Self::default().total;
        macro_rules! sum {
            ($($field:ident),* $(,)?) => {$(
                total.$field = instances.iter().fold(ProfileValue::Measured(0), |sum, i| {
                    add(sum, i.profile.$field, |a,b| a.saturating_add(b))
                });
            )*};
        }
        sum!(
            trail_capacity,
            occupied_trails,
            retired_trails,
            trail_evictions,
            truncated_trails,
            cpu_time_ns,
            gpu_time_ns,
            gpu_simulation_time_ns,
            alive_particles,
            submitted_instances,
            submitted_vertices,
            submitted_primitives,
            particle_capacity,
            emitter_count,
            draw_calls,
            dispatch_count,
            texture_sample_count,
            buffer_memory_bytes,
            collision_time_ns
        );
        total.peak_particles = peak(self.total.peak_particles, total.alive_particles);
        // Texture assets may be shared between instances; summing would double count.
        // Overdraw is also not additive. Keep these unavailable for compositions.
        if instances.len() == 1 {
            total.texture_memory_bytes = instances[0].profile.texture_memory_bytes;
            total.estimated_overdraw = instances[0].profile.estimated_overdraw;
        } else if !instances.is_empty() {
            total.texture_memory_bytes = ProfileValue::Unavailable;
        }
        for instance in &instances {
            let label = instance.label();
            total
                .emitters
                .extend(instance.profile.emitters.iter().cloned().map(|mut e| {
                    e.name = format!("{label} / {}", e.name);
                    e
                }));
            total.platform_warnings.extend(
                instance
                    .profile
                    .platform_warnings
                    .iter()
                    .map(|w| format!("{label}: {w}")),
            );
        }
        self.instances = instances;
        self.total = total;
        changed
    }

    pub fn reset_peaks(&mut self) {
        self.total.reset_peaks();
        for instance in &mut self.instances {
            instance.profile.reset_peaks();
        }
    }
}

impl ProjectInstanceProfile {
    pub fn label(&self) -> String {
        if self.path.is_empty() {
            format!("{} [root]", self.name)
        } else {
            format!(
                "{} [{}]",
                self.name,
                self.path
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("/")
            )
        }
    }
}

fn add<T: Copy>(
    a: ProfileValue<T>,
    b: ProfileValue<T>,
    combine: impl FnOnce(T, T) -> T,
) -> ProfileValue<T> {
    match (a, b) {
        (ProfileValue::Measured(a), ProfileValue::Measured(b)) => {
            ProfileValue::Measured(combine(a, b))
        }
        (ProfileValue::Unavailable, _) | (_, ProfileValue::Unavailable) => {
            ProfileValue::Unavailable
        }
        (a, b) => ProfileValue::Estimated(combine(a.value().unwrap(), b.value().unwrap())),
    }
}

fn peak(previous: ProfileValue<u32>, current: ProfileValue<u32>) -> ProfileValue<u32> {
    match (previous, current) {
        (ProfileValue::Unavailable, ProfileValue::Unavailable) => ProfileValue::Unavailable,
        (known, ProfileValue::Unavailable) | (ProfileValue::Unavailable, known) => {
            ProfileValue::Estimated(known.value().unwrap())
        }
        (previous, current) => add(previous, current, u32::max),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProfileValue::{Estimated as E, Measured as M, Unavailable as U};

    fn entry(path: Vec<EffectClipId>, source: EffectId, alive: u32) -> ProjectInstanceProfile {
        let mut profile = ProjectProfile::default().total;
        profile.alive_particles = M(alive);
        profile.peak_particles = M(alive);
        profile.particle_capacity = M(100);
        ProjectInstanceProfile {
            path,
            effect: source,
            name: "Effect".into(),
            profile,
        }
    }

    #[test]
    fn geometry_totals_preserve_missing_values_and_empty_carrier_zeroes() {
        let source = EffectId::new();
        let mut carrier = entry(vec![], source, 0);
        carrier.profile.particle_capacity = M(0);
        carrier.profile.draw_calls = E(0);
        carrier.profile.submitted_vertices = U;
        carrier.profile.submitted_primitives = U;
        let mut child = entry(vec![EffectClipId::new()], source, 1);
        child.profile.submitted_instances = M(2);
        child.profile.submitted_vertices = M(8);
        child.profile.submitted_primitives = M(4);
        child.profile.draw_calls = M(1);
        let mut project = ProjectProfile::default();
        project.update(vec![carrier.clone(), child.clone()]);
        assert_eq!(project.total.submitted_vertices, M(8));
        assert_eq!(project.total.submitted_primitives, M(4));
        assert_eq!(project.total.draw_calls, M(1));
        child.profile.submitted_vertices = U;
        project.update(vec![carrier, child.clone()]);
        assert_eq!(project.total.submitted_vertices, U);
        child.profile.submitted_vertices = M(u64::MAX);
        child.profile.submitted_primitives = M(u64::MAX);
        let mut other = child.clone();
        other.path.push(EffectClipId::new());
        project.update(vec![child, other]);
        assert_eq!(project.total.submitted_vertices, M(u64::MAX));
        assert_eq!(project.total.submitted_primitives, M(u64::MAX));
    }

    #[test]
    fn simulation_timing_totals_ignore_empty_carriers_but_not_missing_measurements() {
        let source = EffectId::new();
        let mut carrier = entry(vec![], source, 0);
        carrier.profile.particle_capacity = M(0);
        let mut child = entry(vec![EffectClipId::new()], source, 1);
        let mut project = ProjectProfile::default();
        project.update(vec![carrier.clone(), child.clone()]);
        assert_eq!(project.total.gpu_simulation_time_ns, U);
        child.profile.gpu_simulation_time_ns = M(1234);
        project.update(vec![carrier, child.clone()]);
        assert_eq!(project.total.gpu_simulation_time_ns, M(1234));
        assert_eq!(project.total.gpu_time_ns, U);
        child.profile.gpu_simulation_time_ns = M(u64::MAX);
        let mut other = child.clone();
        other.path.push(EffectClipId::new());
        project.update(vec![child, other]);
        assert_eq!(project.total.gpu_simulation_time_ns, M(u64::MAX));
    }

    #[test]
    fn active_paths_are_distinct_and_project_peak_is_simultaneous_not_sum_of_peaks() {
        let source = EffectId::new();
        let path = vec![EffectClipId::new()];
        let mut project = ProjectProfile::default();
        project.update(vec![
            entry(vec![], source, 10),
            entry(path.clone(), source, 1),
        ]);
        assert_eq!(project.total.alive_particles, M(11));
        project.update(vec![
            entry(vec![], source, 1),
            entry(path.clone(), source, 10),
        ]);
        assert_eq!(project.total.peak_particles, M(11));
        assert_eq!(
            project
                .instances
                .iter()
                .map(|i| i.profile.peak_particles.value().unwrap())
                .sum::<u32>(),
            20
        );
        project.update(vec![entry(vec![], source, 2)]);
        assert_eq!(project.total.particle_capacity, M(100));
        assert_eq!(project.total.alive_particles, M(2));
        project.reset_peaks();
        assert_eq!(project.total.peak_particles, M(2));
        project.update(vec![entry(vec![], source, 2), entry(path, source, 3)]);
        assert_eq!(project.instances[1].profile.peak_particles, M(3));
        project.update(Vec::new());
        assert_eq!(project.total.alive_particles, M(0));
        assert_eq!(project.total.particle_capacity, M(0));
        assert!(project.instances.is_empty());
    }

    #[test]
    fn unknown_and_estimated_metrics_keep_their_provenance_and_do_not_double_count_textures() {
        let source = EffectId::new();
        let root = entry(vec![], source, 1);
        let mut child = entry(vec![EffectClipId::new()], source, 2);
        child.profile.alive_particles = U;
        child.profile.buffer_memory_bytes = E(20);
        child.profile.trail_capacity = M(4);
        child.profile.occupied_trails = U;
        let mut project = ProjectProfile::default();
        project.update(vec![root.clone(), child.clone()]);
        assert_eq!(project.total.alive_particles, U);
        assert_eq!(project.total.buffer_memory_bytes, E(20));
        assert_eq!(project.total.texture_memory_bytes, U);
        assert_eq!(project.total.occupied_trails, U);
        child.profile.alive_particles = E(2);
        child.profile.record_trail_usage(Some(crate::TrailUsage {
            occupied: 3,
            retired: 1,
            evictions: 2,
            truncated: 1,
        }));
        project.update(vec![root, child]);
        assert_eq!(project.total.alive_particles, E(3));
        assert_eq!(project.total.trail_capacity, M(4));
        assert_eq!(project.total.occupied_trails, M(3));
        assert_eq!(project.total.retired_trails, M(1));
        assert_eq!(project.total.trail_evictions, M(2));
        assert_eq!(project.total.truncated_trails, M(1));
    }

    #[test]
    fn saturation_and_root_replacement_reset_cost_history() {
        let source = EffectId::new();
        let mut project = ProjectProfile::default();
        project.update(vec![
            entry(vec![], source, u32::MAX),
            entry(vec![EffectClipId::new()], source, 10),
        ]);
        assert_eq!(project.total.alive_particles, M(u32::MAX));
        project.update(vec![entry(vec![], EffectId::new(), 1)]);
        assert_eq!(project.total.peak_particles, M(1));
    }

    #[test]
    fn missing_frames_keep_only_an_estimated_observed_peak_until_reset() {
        let source = EffectId::new();
        let mut project = ProjectProfile::default();
        project.update(vec![entry(Vec::new(), source, 12)]);
        let mut unknown = entry(Vec::new(), source, 0);
        unknown.profile.alive_particles = U;
        project.update(vec![unknown]);
        assert_eq!(project.total.peak_particles, E(12));
        project.update(vec![entry(Vec::new(), source, 3)]);
        assert_eq!(project.total.peak_particles, E(12));
        project.reset_peaks();
        assert_eq!(project.total.peak_particles, M(3));
    }
}
