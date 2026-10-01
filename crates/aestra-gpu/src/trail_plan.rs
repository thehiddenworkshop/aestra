//! Checked temporary storage/dispatch planning for large logical history pools.
use crate::Vec3;
use crate::{GpuArtifactError, GpuEmitter};
use encase::ShaderType;

// Keep in sync with the portable history shader. Prefix pages remain 1024;
// sorting and bounds use smaller pages to expose more independent workgroups.
const SORT_PAGE: u32 = 256;
const BOUNDS_PAGE: u32 = 64;

pub const PAGED_TRAIL_ENTRY_POINTS: [&str; 9] = [
    "sort_trail_page",
    "merge_trail_pages",
    "present_trail_heads",
    "reserve_trail_owners",
    "scan_trail_births",
    "scan_trail_birth_pages",
    "update_trail_owners",
    "bound_trail_page",
    "finish_trail_pages",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrailScratchPlan {
    pub aux_words: u32,
    pub max_heads: u32,
    pub max_owners: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TrailPass {
    pub entry: usize,
    pub parameter: u32,
    pub workgroups: u32,
}

impl TrailScratchPlan {
    /// Append disjoint temporary ranges after persistent aux. Spare emitter lanes
    /// carry numeric integers (not denormal bitcasts); enforce exact f32 representation.
    pub fn configure(emitters: &mut [GpuEmitter], records: u32) -> Result<Self, GpuArtifactError> {
        let mut plan = Self {
            aux_words: records.checked_mul(3).ok_or(GpuArtifactError::TrailLimit)?,
            ..Self::default()
        };
        for emitter in emitters {
            emitter._spawn_inverse_padding = Vec3::ZERO;
            if emitter.trail_points < 2
                || (emitter.max_particles <= 1024 && emitter.trail_capacity <= 1024)
            {
                continue;
            }
            let heads = emitter
                .max_particles
                .max(1)
                .checked_next_power_of_two()
                .ok_or(GpuArtifactError::TrailLimit)?;
            let owners = emitter
                .trail_capacity
                .max(1)
                .checked_next_power_of_two()
                .ok_or(GpuArtifactError::TrailLimit)?;
            emitter._spawn_inverse_padding =
                Vec3::new(plan.aux_words as f32, heads as f32, owners as f32);
            plan.aux_words = plan
                .aux_words
                .checked_add(
                    heads
                        .checked_mul(3)
                        .and_then(|n| n.checked_add(owners.checked_mul(2)?))
                        .and_then(|n| n.checked_add(heads.div_ceil(1024)))
                        .and_then(|n| n.checked_add(owners.div_ceil(BOUNDS_PAGE).checked_mul(12)?))
                        .and_then(|n| n.checked_add(heads.max(owners).checked_mul(2)?))
                        .ok_or(GpuArtifactError::TrailLimit)?,
                )
                .ok_or(GpuArtifactError::TrailLimit)?;
            if plan.aux_words > (1 << 24) {
                return Err(GpuArtifactError::TrailLimit);
            }
            plan.max_heads = plan.max_heads.max(heads);
            plan.max_owners = plan.max_owners.max(owners);
        }
        Ok(plan)
    }

    pub fn paged(self) -> bool {
        self.max_heads != 0
    }

    /// One parameter upload is reused for every live fixed tick in this frame.
    pub fn passes(self) -> Vec<TrailPass> {
        if !self.paged() {
            return Vec::new();
        }
        let mut passes = Vec::new();
        let sort = |passes: &mut Vec<TrailPass>, kind: u32, size: u32| {
            passes.push(TrailPass {
                entry: 0,
                parameter: kind,
                workgroups: size.div_ceil(SORT_PAGE),
            });
            let mut parity = 0;
            let mut width = SORT_PAGE;
            while width < size {
                passes.push(TrailPass {
                    entry: 1,
                    parameter: kind | (width.ilog2() << 8) | (parity << 16),
                    workgroups: size.div_ceil(64),
                });
                parity = 1 - parity;
                width *= 2;
            }
            kind | (parity << 16)
        };
        let heads = sort(&mut passes, 0, self.max_heads);
        passes.push(TrailPass {
            entry: 2,
            parameter: heads,
            workgroups: self.max_heads.div_ceil(64),
        });
        // Kind 1 matches/expires owners against the already sorted heads in one
        // pass. There is no owner-ID sorting network or merge chain.
        passes.push(TrailPass {
            entry: 0,
            parameter: 1,
            workgroups: self.max_owners.div_ceil(SORT_PAGE),
        });
        passes.push(TrailPass {
            entry: 3,
            parameter: 1,
            workgroups: self.max_heads.div_ceil(64),
        });
        passes.push(TrailPass {
            entry: 4,
            parameter: 0,
            workgroups: self.max_heads.div_ceil(1024),
        });
        passes.push(TrailPass {
            entry: 5,
            parameter: 0,
            workgroups: 1,
        });
        let candidates = sort(&mut passes, 2, self.max_owners);
        passes.push(TrailPass {
            entry: 6,
            parameter: candidates,
            workgroups: self.max_heads.div_ceil(64),
        });
        passes.push(TrailPass {
            entry: 7,
            parameter: 0,
            workgroups: self.max_owners.div_ceil(BOUNDS_PAGE),
        });
        passes.push(TrailPass {
            entry: 8,
            parameter: 0,
            workgroups: 1,
        });
        passes
    }

    /// Reject unsupported allocations before any capacity-sized buffer is created.
    pub fn fits(
        self,
        records: u32,
        emitters: u32,
        binding_bytes: u64,
        buffer_bytes: u64,
        dispatch: u32,
    ) -> bool {
        let maximum = binding_bytes.min(buffer_bytes);
        u64::from(records) * 48 <= maximum
            && u64::from(self.aux_words) * 4 <= maximum
            && u64::from(emitters) * GpuEmitter::min_size().get() <= maximum
            && emitters <= dispatch
            && self.max_heads.max(self.max_owners).div_ceil(64) <= dispatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_pages_match_shader_dispatch_and_exact_scratch_ranges() {
        assert!(
            crate::shader::SIMULATION_WESL
                .contains(&format!("const TRAIL_BOUNDS_PAGE: u32 = {BOUNDS_PAGE}u;"))
        );
        let mut emitters = [
            GpuEmitter {
                max_particles: 16,
                trail_capacity: 1025,
                trail_points: 64,
                ..Default::default()
            },
            GpuEmitter {
                max_particles: 2053,
                trail_capacity: 2053,
                trail_points: 4,
                ..Default::default()
            },
        ];
        let records = emitters
            .iter()
            .map(|e| e.max_particles + 1 + e.trail_capacity * e.trail_points)
            .sum();
        let plan = TrailScratchPlan::configure(&mut emitters, records).unwrap();
        let mut end = records * 3;
        for e in &emitters {
            let heads = e.max_particles.next_power_of_two();
            let owners = e.trail_capacity.next_power_of_two();
            assert_eq!(
                e._spawn_inverse_padding,
                Vec3::new(end as f32, heads as f32, owners as f32)
            );
            end += 3 * heads
                + 2 * owners
                + heads.div_ceil(1024)
                + 12 * owners.div_ceil(BOUNDS_PAGE)
                + 2 * heads.max(owners);
        }
        assert_eq!(plan.aux_words, end);
        let passes = plan.passes();
        assert_eq!(
            passes
                .iter()
                .find(|pass| pass.entry == 7)
                .unwrap()
                .workgroups,
            64
        );
        assert!(plan.fits(records, 2, 128 << 20, 256 << 20, 64));
        assert!(!plan.fits(records, 2, 128 << 20, 256 << 20, 63));
        // Isolate the aux-byte boundary from the record-byte boundary. Include
        // the enlarged page outputs when checking both binding and buffer limits.
        let mut tiny = [GpuEmitter {
            max_particles: 2048,
            trail_capacity: 2048,
            trail_points: 2,
            ..Default::default()
        }];
        let plan = TrailScratchPlan::configure(&mut tiny, 0).unwrap();
        let bytes = u64::from(plan.aux_words) * 4;
        assert!(plan.fits(0, 1, bytes, bytes, 65535));
        assert!(!plan.fits(0, 1, bytes - 1, bytes, 65535));
        assert!(!plan.fits(0, 1, bytes, bytes - 1, 65535));
        assert!(TrailScratchPlan::configure(&mut tiny, (1 << 24) / 3).is_err());
    }

    #[test]
    fn sorting_pages_match_shader_and_merge_parity_for_mixed_pool_sizes() {
        assert!(
            crate::shader::SIMULATION_WESL
                .contains(&format!("const TRAIL_SORT_PAGE: u32 = {SORT_PAGE}u;"))
        );
        for (heads, owners) in [(16, 2048), (2048, 4096), (8192, 16384)] {
            let passes = TrailScratchPlan {
                max_heads: heads,
                max_owners: owners,
                ..Default::default()
            }
            .passes();
            for kind in [0, 2] {
                let size = if kind == 0 { heads } else { owners };
                let start = passes
                    .iter()
                    .position(|pass| pass.entry == 0 && pass.parameter == kind)
                    .unwrap();
                assert_eq!(passes[start].workgroups, size.div_ceil(SORT_PAGE));
                let mut width = SORT_PAGE;
                let mut parity = 0;
                let mut index = start + 1;
                while width < size {
                    assert_eq!(passes[index].entry, 1);
                    assert_eq!(
                        passes[index].parameter,
                        kind | (width.ilog2() << 8) | (parity << 16)
                    );
                    width *= 2;
                    parity = 1 - parity;
                    index += 1;
                }
                assert_eq!(passes[index].parameter, kind | (parity << 16));
            }
            let reservation = passes
                .iter()
                .position(|pass| pass.entry == 0 && pass.parameter == 1)
                .unwrap();
            assert_eq!(
                passes[reservation - 1].entry,
                2,
                "head presentation clears mappings before matching"
            );
            assert_eq!(passes[reservation].workgroups, owners.div_ceil(SORT_PAGE));
            assert_eq!(passes[reservation + 1].entry, 3);
            assert_eq!(passes[reservation + 1].parameter, 1);
            assert!(
                !passes
                    .iter()
                    .any(|pass| pass.entry == 1 && pass.parameter & 255 == 1)
            );
            if heads == 8192 && owners == 16384 {
                assert_eq!(passes.iter().map(|pass| pass.workgroups).sum::<u32>(), 2986);
            }
        }
    }

    #[test]
    fn plans_disjoint_exact_ranges_and_rejects_overflow_and_device_pressure() {
        let mut emitters = vec![
            GpuEmitter {
                max_particles: 2053,
                trail_capacity: 8192,
                trail_points: 4,
                ..Default::default()
            };
            2
        ];
        let records = 2 * (2053 + 1 + 8192 * 4);
        let plan = TrailScratchPlan::configure(&mut emitters, records).unwrap();
        assert!(emitters[1]._spawn_inverse_padding.x > emitters[0]._spawn_inverse_padding.x);
        assert_eq!(plan.max_heads, 4096);
        assert_eq!(plan.max_owners, 8192);
        assert!(plan.fits(records, 2, 128 << 20, 256 << 20, 65535));
        assert!(!plan.fits(records, 2, 1024, 256 << 20, 65535));
        assert!(!plan.fits(records, 2, 128 << 20, 256 << 20, 1));
        assert!(!plan.fits(records, 65535, 8 << 20, 256 << 20, 65535));
        assert!(TrailScratchPlan::configure(&mut emitters, u32::MAX).is_err());
        emitters[0].max_particles = u32::MAX;
        assert!(TrailScratchPlan::configure(&mut emitters, 1).is_err());
    }
}
