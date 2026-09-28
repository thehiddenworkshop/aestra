//! Compile-time quality tiers (fluid F12): one authored effect, one deterministic artifact per tier.
//!
//! An effect is authored at its best quality. A tier says how far below that it is compiled for a
//! class of GPU — relative scales that each stage lowerer applies to what it lowers (a fluid's grid
//! resolution, its solver's iterations, its look's march steps, its particle budgets). The tier is
//! chosen when compiling, never at run time: the runtime never switches algorithms behind an asset's
//! back, so each tier's artifact simulates the same way on every run, and its checkpoints stay its
//! own.

/// How far below its authored quality an effect is compiled. Every scale is in `(0, 1]`; the `high`
/// tier is the authored effect itself.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityTier {
    pub name: String,
    /// Cells per axis of simulation grids (the simulated region keeps its size: cells grow).
    pub resolution: f32,
    /// Caps of iterative solvers (pressure iterations).
    pub iterations: f32,
    /// Presentation sampling (volume and surface march steps).
    pub presentation: f32,
    /// Particle budgets and the particles asked for (liquid particles, secondary emission).
    pub particles: f32,
}

impl Default for QualityTier {
    fn default() -> Self {
        Self::high()
    }
}

impl QualityTier {
    /// The authored effect, unchanged: for high-end GPUs.
    pub fn high() -> Self {
        Self::new("high", 1.0, 1.0, 1.0, 1.0)
    }

    /// Three quarters of the resolution (about 40% of the cells), half the solver iterations and half
    /// the particles: for mid-range GPUs. (Iterative solves are capped harder than the grid is
    /// coarsened: a GPU pays for every capped iteration, converged or not, and the pressure solves
    /// converge in well under their authored caps.)
    pub fn medium() -> Self {
        Self::new("medium", 0.75, 0.5, 0.75, 0.5)
    }

    /// Half the resolution (an eighth of the cells), about a third of the solver iterations and a
    /// quarter of the particles: for entry-level GPUs.
    pub fn low() -> Self {
        Self::new("low", 0.5, 0.35, 0.5, 0.25)
    }

    /// The built-in tiers, best first.
    pub fn presets() -> [Self; 3] {
        [Self::high(), Self::medium(), Self::low()]
    }

    /// A built-in tier by name.
    pub fn preset(name: &str) -> Option<Self> {
        Self::presets().into_iter().find(|tier| tier.name == name)
    }

    fn new(
        name: &str,
        resolution: f32,
        iterations: f32,
        presentation: f32,
        particles: f32,
    ) -> Self {
        Self {
            name: name.into(),
            resolution,
            iterations,
            presentation,
            particles,
        }
    }

    /// Whether this is the authored quality — every scale 1.
    pub fn is_authored(&self) -> bool {
        [
            self.resolution,
            self.iterations,
            self.presentation,
            self.particles,
        ]
        .iter()
        .all(|&scale| scale == 1.0)
    }

    /// Whether the tier is well formed: a name, and every scale in `(0, 1]`.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a quality tier needs a name".into());
        }
        let scales = [
            ("resolution", self.resolution),
            ("iterations", self.iterations),
            ("presentation", self.presentation),
            ("particles", self.particles),
        ];
        for (what, scale) in scales {
            if !(scale > 0.0 && scale <= 1.0) {
                return Err(format!(
                    "tier '{}': its {what} scale must be in (0, 1], got {scale}",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// `value` scaled by `scale` for a count: rounded to the nearest multiple of `multiple` (at least
    /// 1), and never below `min` — nor above `value`, when `value` already honours them.
    pub fn scale_count(value: u32, scale: f32, multiple: u32, min: u32) -> u32 {
        if scale >= 1.0 {
            return value;
        }
        let multiple = multiple.max(1);
        let scaled = (value as f32 * scale / multiple as f32).round() as u32 * multiple;
        scaled.max(min).max(multiple).min(value.max(min))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_scale_counts_to_valid_multiples_and_high_is_the_authored_effect() {
        assert!(QualityTier::high().is_authored());
        assert!(!QualityTier::low().is_authored());
        for tier in QualityTier::presets() {
            tier.validate().unwrap();
            assert_eq!(QualityTier::preset(&tier.name), Some(tier.clone()));
        }
        assert!(QualityTier::preset("ultra").is_none());
        let mut bad = QualityTier::low();
        bad.resolution = 1.5;
        assert!(bad.validate().is_err());
        // 48 cells at half resolution: 24; at three quarters: 36 — multiples of 4 either way.
        assert_eq!(QualityTier::scale_count(48, 0.5, 4, 8), 24);
        assert_eq!(QualityTier::scale_count(48, 0.75, 4, 8), 36);
        // Never below the minimum, and a multiple of the brick edge for a sparse grid.
        assert_eq!(QualityTier::scale_count(16, 0.25, 4, 8), 8);
        assert_eq!(QualityTier::scale_count(512, 0.75, 8, 8), 384);
        // The authored value is kept at scale 1, whatever it is.
        assert_eq!(QualityTier::scale_count(30, 1.0, 4, 8), 30);
        // Iterations: at least one.
        assert_eq!(QualityTier::scale_count(1, 0.5, 1, 1), 1);
    }
}
