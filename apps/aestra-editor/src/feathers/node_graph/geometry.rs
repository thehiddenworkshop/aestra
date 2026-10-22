//! Host geometry collector scheduling; the shared registry is engine-qualified independently.
use bevy::prelude::*;
#[path = "geometry/model.rs"]
mod model;
pub(crate) use model::*;
#[path = "geometry/collect.rs"]
mod collect;
#[cfg(test)]
#[path = "geometry/tests.rs"]
pub(crate) mod tests;

pub(super) fn register(app: &mut App) {
    app.init_resource::<GraphGeometryRegistry>().add_systems(
        PostUpdate,
        (
            project_root.pipe(collect::collect_graph_geometry),
            super::framing::frame_graph_viewports,
        )
            .chain()
            .after(bevy::ui::UiSystems::PostLayout),
    );
}

/// Read the current catalog immediately before collection; never retain a mirrored root resource.
fn project_root(catalog: Option<Res<crate::ProjectEffectCatalog>>) -> Option<std::path::PathBuf> {
    catalog.map(|catalog| catalog.root().to_owned())
}
