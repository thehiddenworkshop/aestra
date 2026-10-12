//! Import the real registry and post-layout collector, not fixture stand-ins.
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/geometry/model.rs"]
mod model;
pub(crate) use model::*;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/geometry/collect.rs"]
mod collect;
