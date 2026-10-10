use aestra_gpu::GpuWorldSdf;
use bevy::prelude::Resource;

/// The host's world geometry for fluid and stateful-particle collisions. Packed
/// data is shared by Arc; extraction must not copy an entire distance field.
#[derive(Resource, Debug, Clone, Default)]
pub struct AestraWorldSdf {
    world: Option<GpuWorldSdf>,
    revision: u64,
}

impl AestraWorldSdf {
    /// A world made of `volume`.
    pub fn new(volume: &aestra_runtime::SdfVolume) -> Self {
        let mut world = Self::default();
        world.set(volume);
        world
    }

    /// Replaces the world with `volume`.
    pub fn set(&mut self, volume: &aestra_runtime::SdfVolume) {
        self.revision += 1;
        self.world = Some(GpuWorldSdf::new(volume, self.revision));
    }

    /// Removes the world: nothing collides with it any more.
    pub fn clear(&mut self) {
        self.revision += 1;
        let mut absent = GpuWorldSdf::absent();
        absent.revision = self.revision;
        self.world = Some(absent);
    }

    pub(super) fn packed(&self) -> Option<&GpuWorldSdf> {
        self.world.as_ref()
    }

    /// Changes whenever the world is set or cleared: colliding stateful
    /// particles restart their history then (host bindings HB10).
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}
