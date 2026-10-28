//! Image-review views driven by the public root clock, never by replay or seeks.
use super::*;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewCamera {
    HostDefault,
    Audience,
    Close,
    Wide,
    Side,
    Near,
    Orbit,
}

impl ReviewCamera {
    pub(super) fn parse(name: &str, images: bool) -> Result<Self, String> {
        let camera = match name {
            "host-default" => Self::HostDefault,
            "audience" => Self::Audience,
            "close" => Self::Close,
            "wide" => Self::Wide,
            "side" => Self::Side,
            "near" => Self::Near,
            "orbit" if images => Self::Orbit,
            "orbit" => {
                return Err(
                    "Orbit review requires the image lane, separate from cost/profiling".into(),
                );
            }
            _ => return Err(
                "Report camera must be audience, close, wide, side, near, orbit or host-default"
                    .into(),
            ),
        };
        Ok(camera)
    }

    pub(super) fn pose(self, seconds: f32) -> Option<Transform> {
        let (eye, target) = match self {
            Self::HostDefault => return None,
            Self::Audience => (Vec3::new(0.0, 48.0, 180.0), Vec3::new(0.0, 28.0, 0.0)),
            Self::Close => (Vec3::new(0.0, 42.0, 145.0), Vec3::new(0.0, 24.0, 0.0)),
            Self::Wide => (Vec3::new(0.0, 60.0, 235.0), Vec3::new(0.0, 26.0, 0.0)),
            Self::Side => (Vec3::new(180.0, 48.0, 0.0), Vec3::new(0.0, 28.0, 0.0)),
            Self::Near => (Vec3::new(0.0, 44.0, 95.0), Vec3::new(0.0, 32.0, 0.0)),
            Self::Orbit => {
                // Smooth 16-second sweep from -45 to +105 degrees and back.
                // Constant radius/height isolate viewing direction from zoom.
                let yaw =
                    (30.0 - 75.0 * (seconds * std::f32::consts::TAU / 16.0).cos()).to_radians();
                (
                    Vec3::new(yaw.sin() * 180.0, 48.0, yaw.cos() * 180.0),
                    Vec3::new(0.0, 28.0, 0.0),
                )
            }
        };
        Some(Transform::from_translation(eye).looking_at(target, Vec3::Y))
    }
}

pub(super) fn motion(
    root: Query<&EffectPlayer, With<ShowRoot>>,
    readiness: Res<Readiness>,
    mut cameras: Query<(&ReviewCamera, &mut Transform, &mut GlobalTransform), With<AudienceCamera>>,
) {
    if !readiness.started {
        return;
    }
    let Ok(root) = root.single() else {
        return;
    };
    for (camera, mut transform, mut global) in &mut cameras {
        if *camera == ReviewCamera::Orbit {
            *transform = camera.pose(root.elapsed()).expect("orbit pose");
            // The demo camera is unparented. Update view-dependent main-world
            // preparation now, rather than waiting for PostUpdate propagation.
            *global = GlobalTransform::from(*transform);
        }
    }
}

#[cfg(test)]
#[path = "report_camera_tests.rs"]
mod tests;
