//! Reusable GPU capture harness for background thumbnails: an isolated
//! image-target camera, framed to fit its subject, driven through a
//! settle → screenshot → reframe loop.
//!
//! The producer (effect instances today; standalone materials next, M-MG) owns
//! *what* is rendered and *when* it counts as settled; this harness owns the
//! render target, the camera, the screenshot capture, and the fit refinement.
//! Extracted from the effect `GpuJob` in M-MG1 with no behavior change.
use super::*;

/// The conservative default camera framing for a subject centered at `center`
/// with bounding `radius`, before the fit refinement tightens it. Shared by the
/// capture harness and the live hover preview so both start from the same view.
pub(super) fn default_framing(center: Vec3, radius: f32) -> (Transform, OrthographicProjection) {
    let projection = OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: radius * 2.0,
        },
        near: 0.01,
        far: radius * 8.0 + 1.0,
        ..OrthographicProjection::default_3d()
    };
    let transform =
        Transform::from_translation(center + Vec3::new(0.7, 0.4, 1.0).normalize() * radius * 3.0)
            .looking_at(center, Vec3::Y);
    (transform, projection)
}

/// The outcome of one [`CaptureHarness::advance`] tick.
pub(super) enum Poll {
    /// Not finished. When `evaluate_ready` is true the producer should compute
    /// its readiness and call [`CaptureHarness::settle`]; otherwise it waits.
    Pending { evaluate_ready: bool },
    /// The capture budget elapsed; the producer builds the diagnostic error.
    TimedOut,
    /// A capture completed and was refined; the pixels, or a capture error.
    Done(Result<Vec<u8>, String>),
}

/// Owns the image target, the isolated capture camera, and the settle/screenshot/
/// reframe state. One per in-flight capture.
pub(super) struct CaptureHarness {
    layer: usize,
    target: Handle<Image>,
    camera: Entity,
    camera_transform: Transform,
    projection: OrthographicProjection,
    started: Instant,
    settled: u8,
    capture: Option<Entity>,
    result: Option<Result<Vec<u8>, String>>,
    refinements: u8,
}

impl CaptureHarness {
    /// Spawns the `EDGE`×`EDGE` render target and an orthographic camera framing
    /// `center`/`radius` on `layer`, at render `order` (kept behind the viewport).
    pub(super) fn new(
        center: Vec3,
        radius: f32,
        layer: usize,
        order: isize,
        commands: &mut Commands,
        images: &mut Assets<Image>,
    ) -> Self {
        let mut image = Image::new_target_texture(EDGE, EDGE, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = images.add(image);
        let (camera_transform, projection) = default_framing(center, radius);
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order,
                    clear_color: ClearColorConfig::Custom(Color::srgb(0.025, 0.028, 0.035)),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                Projection::Orthographic(projection.clone()),
                camera_transform,
                RenderLayers::layer(layer),
                Msaa::Off,
            ))
            .id();
        Self {
            layer,
            target,
            camera,
            camera_transform,
            projection,
            started: Instant::now(),
            settled: 0,
            capture: None,
            result: None,
            refinements: 0,
        }
    }

    pub(super) fn layer(&self) -> usize {
        self.layer
    }

    #[cfg(test)]
    pub(super) fn camera(&self) -> Entity {
        self.camera
    }

    #[cfg(test)]
    pub(super) fn target(&self) -> &Handle<Image> {
        &self.target
    }

    /// The final refined camera framing, so a live hover preview can match the
    /// static thumbnail's tight fit instead of the conservative history bounds.
    pub(super) fn framing(&self) -> (Transform, OrthographicProjection) {
        (self.camera_transform, self.projection.clone())
    }

    /// Whether `entity` is this harness's in-flight screenshot capture.
    pub(super) fn matches_capture(&self, entity: Entity) -> bool {
        self.capture == Some(entity)
    }

    /// Records the pixels (or error) delivered by the screenshot observer.
    pub(super) fn store_capture(&mut self, result: Result<Vec<u8>, String>) {
        self.result = Some(result);
    }

    /// Finalizes or refines a completed capture, times out, or waits for an
    /// in-flight capture. `Pending { evaluate_ready: true }` asks the producer to
    /// compute its readiness and call [`Self::settle`].
    pub(super) fn advance(&mut self, commands: &mut Commands) -> Poll {
        if let Some(result) = self.result.take() {
            if self.refinements < 2
                && let Ok(bytes) = &result
                && let Some((offset, scale)) = framing::fit(bytes)
                && let ScalingMode::FixedVertical { viewport_height } =
                    &mut self.projection.scaling_mode
            {
                self.camera_transform.translation +=
                    self.camera_transform.rotation * (offset * *viewport_height).extend(0.0);
                *viewport_height *= scale;
                commands.entity(self.camera).insert((
                    self.camera_transform,
                    Projection::Orthographic(self.projection.clone()),
                ));
                if let Some(capture) = self.capture.take() {
                    commands.entity(capture).try_despawn();
                }
                self.refinements += 1;
                self.settled = 0;
                return Poll::Pending {
                    evaluate_ready: false,
                };
            }
            return Poll::Done(result);
        }
        if self.started.elapsed() >= TIMEOUT {
            return Poll::TimedOut;
        }
        if self.capture.is_some() {
            return Poll::Pending {
                evaluate_ready: false,
            };
        }
        Poll::Pending {
            evaluate_ready: true,
        }
    }

    /// Advances the settle counter with the producer's readiness and, once stable
    /// for a few frames, spawns the screenshot capture routed back through
    /// [`receive`].
    pub(super) fn settle(&mut self, ready: bool, commands: &mut Commands) {
        self.settled = if ready {
            self.settled.saturating_add(1)
        } else {
            0
        };
        if self.settled >= 4 {
            self.capture = Some(
                commands
                    .spawn(Screenshot::image(self.target.clone()))
                    .observe(receive)
                    .id(),
            );
        }
    }

    /// Despawns the camera and any in-flight capture and drops the target image.
    /// The producer cleans up its own spawned instances and bound assets.
    pub(super) fn cleanup(self, commands: &mut Commands, images: &mut Assets<Image>) {
        commands.entity(self.camera).try_despawn();
        if let Some(capture) = self.capture {
            commands.entity(capture).try_despawn();
        }
        images.remove(self.target.id());
    }
}
