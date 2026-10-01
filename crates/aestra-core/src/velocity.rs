use serde::{Deserialize, Serialize};

/// Initial velocity distribution, independent of the emitter's position shape.
/// `LegacyCone` preserves the pre-F2 analytic/stateful samplers. New modes use
/// an axis in emitter-local space and independent speed and direction streams.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[repr(u32)]
pub enum VelocityDistribution {
    #[default]
    LegacyCone = 0,
    Constant = 1,
    Cone = 2,
    Sphere = 3,
    Hemisphere = 4,
    /// Uniform area in the plane perpendicular to the axis. The disk radius
    /// scales the independently sampled speed; `Ring` keeps that speed intact.
    Disk = 5,
    Ring = 6,
}

impl VelocityDistribution {
    pub const ALL: [Self; 7] = [
        Self::LegacyCone,
        Self::Constant,
        Self::Cone,
        Self::Sphere,
        Self::Hemisphere,
        Self::Disk,
        Self::Ring,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::LegacyCone => "Legacy Cone",
            Self::Constant => "Constant",
            Self::Cone => "Cone",
            Self::Sphere => "Sphere",
            Self::Hemisphere => "Hemisphere",
            Self::Disk => "Disk",
            Self::Ring => "Ring",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    pub fn is_legacy(&self) -> bool {
        *self == Self::LegacyCone
    }
}
