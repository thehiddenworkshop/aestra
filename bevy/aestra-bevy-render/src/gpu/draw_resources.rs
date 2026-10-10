//! GPU draw work records shared by producers and render commands; no particle copies.
use bevy::{
    prelude::*,
    render::render_resource::{BindGroup, Buffer},
};
use std::{collections::BTreeMap, sync::Mutex};
pub(super) const MAX_DRAWS: usize = 2048;

/// Producer ordering shared with effect-bind-group preparation.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PrepareAlphaSort;

/// Simulation completes before view-local compute producers and graph drawing.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct SimulateEffects;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct SortAlpha;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CullTrails;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TrailCompactionSystems {
    Prepare,
    Compact,
}

pub(super) struct AlphaSortEntry {
    pub(super) count: u32,
    pub(super) runs: [Buffer; 2],
    pub indices: Buffer,
    pub render_params: Buffer,
    pub(super) uniforms: Vec<Buffer>,
    pub(super) bindings: Vec<BindGroup>,
}
#[derive(Resource, Default)]
pub(super) struct AlphaSort {
    pub(super) entries: BTreeMap<(Entity, Entity), AlphaSortEntry>,
    pub(super) dispatched: bool,
}
impl AlphaSort {
    pub fn buffers(&self, view: Entity, draw: Entity) -> Option<(&Buffer, &Buffer)> {
        self.entries
            .get(&(view, draw))
            .map(|e| (&e.indices, &e.render_params))
    }
    pub fn prepared(&self, view: Entity, draw: Entity) -> bool {
        self.entries.contains_key(&(view, draw))
    }
}
pub(super) struct TrailCompactEntry {
    pub(super) owner: Entity,
    pub(super) output: Buffer,
    pub(super) fallback: Buffer,
    pub(super) render_params: Buffer,
    pub(super) scratch: Buffer,
    pub(super) params: Buffer,
    pub(super) bindings: BindGroup,
    pub(super) count: u32,
    pub(super) owners: u32,
    pub(super) renderer: u32,
}

#[derive(Resource, Default)]
pub(super) struct TrailCompaction {
    pub(super) entries: BTreeMap<Entity, TrailCompactEntry>,
    pub(super) dispatched: bool,
}

impl TrailCompaction {
    pub fn output(&self, draw: Entity) -> Option<&Buffer> {
        self.dispatched
            .then(|| self.entries.get(&draw))
            .flatten()
            .map(|e| &e.output)
    }
}

pub(super) struct TrailCullEntry {
    pub(super) owner: Entity,
    pub(super) params: Buffer,
    pub(super) indirect: Buffer,
    pub(super) bindings: BindGroup,
    pub(super) compact_bindings: BindGroup,
}

#[derive(Resource, Default)]
pub(super) struct TrailCulling {
    pub(super) entries: BTreeMap<(Entity, Entity), TrailCullEntry>,
    pub(super) dispatched: bool,
}

impl TrailCulling {
    pub(super) fn indirect(&self, view: Entity, draw: Entity) -> Option<&Buffer> {
        self.dispatched
            .then(|| self.entries.get(&(view, draw)))
            .flatten()
            .map(|entry| &entry.indirect)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Topology {
    Strip,
    Triangles,
    Lines,
}

pub(super) struct Draw {
    pub(super) owner: Entity,
    pub(super) command: Option<(Buffer, u64)>,
    pub(super) direct: [u32; 2],
    pub(super) topology: Topology,
}

#[derive(Default)]
pub(super) struct Frame {
    pub(super) draws: Vec<Draw>,
    pub(super) overflow: bool,
}

#[derive(Resource, Default)]
pub(super) struct Submissions(pub(super) Mutex<Frame>);

impl Submissions {
    pub(super) fn record(
        &self,
        owner: Entity,
        command: Option<(&Buffer, u64)>,
        direct: [u32; 2],
        topology: Topology,
    ) {
        let mut frame = self.0.lock().unwrap();
        if frame.draws.len() == MAX_DRAWS {
            frame.overflow = true;
            return;
        }
        frame.draws.push(Draw {
            owner,
            command: command.map(|(b, o)| (b.clone(), o)),
            direct,
            topology,
        });
    }
}
