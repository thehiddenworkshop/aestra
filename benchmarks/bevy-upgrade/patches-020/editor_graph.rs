//! Production graph navigation, node state, marquee observers and bounded layout solvers.
//! Full drag transactions use real identity, geometry registry, snapping and insertion types.
//! Geometry collection and semantic preparation/history storage use actual production modules.
//! Activation/event dispatch and focused target routing use production code too.
//! Actual catalog/session publication and scoped semantic history execute on both engines.
//! Complete UI handlers and native panel composition remain host-qualified separately.
#[path = "../../../apps/aestra-editor/src/session.rs"]
#[allow(dead_code, reason = "Native application consumers remain host-owned")]
mod session;
pub(crate) use project_catalog::EditorProjectContent as ProjectEffectCatalog;
use project_catalog::{EditorProjectContent, PreparedProject};
use refresh::ProjectEffectWatchState;
pub(crate) use session::EditorSession;
#[path = "../../../apps/aestra-editor/src/project_content/io.rs"]
#[allow(
    dead_code,
    reason = "Remaining filesystem action consumers are host-owned"
)]
mod io;
#[path = "../../../apps/aestra-editor/src/localization.rs"]
#[allow(
    dead_code,
    reason = "Settings and About panel consumers are host-owned"
)]
mod localization;
#[path = "../../../apps/aestra-editor/src/project_content/refresh.rs"]
#[allow(
    dead_code,
    reason = "Legacy synchronous refresh consumers are host-owned"
)]
mod refresh;
#[path = "../../../apps/aestra-editor/src/project_content/services.rs"]
mod services;
// Only the surrounding About panel marker is outside this lifecycle gate.
mod menus {
    #[derive(bevy::prelude::Component)]
    pub(crate) struct AboutDescription;
}
#[path = "../../../apps/aestra-editor/src/preview_mocks.rs"]
#[allow(
    dead_code,
    reason = "Native preview sampling consumers remain host-owned"
)]
mod preview_mocks;
#[path = "../../../apps/aestra-editor/src/test_support.rs"]
#[allow(
    dead_code,
    reason = "Shared test fixture includes other host consumers"
)]
mod test_support;
mod material_document {
    pub(crate) use crate::material_target::MaterialEditingTarget;
}
#[path = "../../../apps/aestra-editor/src/material_document/context.rs"]
#[allow(dead_code, reason = "Other graph panel consumers remain host-owned")]
mod material_document_context;
mod material_function_editor {
    pub(crate) use crate::function_history::FunctionEditor;
    pub(crate) mod history {
        pub(crate) use crate::function_history::Key;
    }
}
#[path = "../../../apps/aestra-editor/src/history/document_execution.rs"]
#[allow(
    dead_code,
    reason = "Menu availability and application handlers remain host-owned"
)]
mod document_execution;
#[path = "../../../apps/aestra-editor/src/material_function_editor/execution.rs"]
#[allow(dead_code, reason = "Other function controls remain host-owned")]
mod function_execution;
#[path = "../../../apps/aestra-editor/src/history/material_program_execution.rs"]
#[allow(
    dead_code,
    reason = "Other semantic action consumers remain host-owned"
)]
mod material_program_execution;
#[path = "../../../apps/aestra-editor/src/history/asset_order/session_context.rs"]
#[allow(
    dead_code,
    reason = "Filesystem chronology consumers remain host-owned"
)]
mod session_context;
const MATERIAL_GRAPH_LAB_EFFECT_SOURCE: &str =
    include_str!("../../../assets/test/effects/material_graph_lab.aestra.ron");
const MATERIAL_GRAPH_LAB_PROGRAM_SOURCE: &str =
    include_str!("../../../assets/test/materials/material_graph_lab.aestra.material.ron");
#[path = "../../../apps/aestra-editor/src/feathers/action_activation.rs"]
#[allow(dead_code, reason = "Native control audit consumers remain host-owned")]
pub(crate) mod action_activation;
#[path = "../../../apps/aestra-editor/src/feathers/button/activation_020.rs"]
#[allow(unused_imports, reason = "Control audit consumers remain host-owned")]
mod button_activation;
#[path = "../../../apps/aestra-editor/src/history/action.rs"]
mod history_action;
mod history {
    pub(crate) use crate::material_program::MaterialProgramEditHistory;
    pub(crate) use crate::publication;
    pub(crate) mod material_program {
        pub(crate) use crate::material_program::{
            MATERIAL_HISTORY_LIMIT, MaterialProgramEditHistory, MaterialProgramHistoryEntry,
        };
    }
    pub(crate) use crate::history_action::{HistoryAction, queue_history_action_activation};
    pub(crate) mod asset_order {
        pub(crate) use crate::edit_order::{Context, EditOrder};
    }
}
#[path = "../../../apps/aestra-editor/src/history/asset_order/model.rs"]
#[allow(
    dead_code,
    reason = "Filesystem and session chronology consumers stay host-side"
)]
mod edit_order;
#[path = "../../../apps/aestra-editor/src/material_function_editor/history.rs"]
mod function_history;
#[path = "../../../apps/aestra-editor/src/material_drafts.rs"]
#[allow(dead_code, reason = "Save/recovery UI consumers remain host-owned")]
mod material_drafts;
#[path = "../../../apps/aestra-editor/src/project_content/model.rs"]
#[allow(dead_code, reason = "Project watch and UI consumers remain host-owned")]
mod project_catalog;
mod project_content {
    pub(crate) use crate::project_catalog::EditorProjectContent;
}
#[path = "../../../apps/aestra-editor/src/material_graph/keys.rs"]
pub(crate) mod graph_keys;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/transaction.rs"]
mod presentation_transaction;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_target.rs"]
mod preview_target;
#[path = "../../../apps/aestra-editor/src/history/publication.rs"]
pub(crate) mod publication;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/semantic.rs"]
mod semantic_snapshot;
mod material_graph {
    pub(crate) use crate::graph_keys as keys;
    pub(crate) use crate::preview_target::MaterialGraphPreviewTarget;
    pub(crate) mod presentation {
        pub(crate) use crate::presentation_transaction::Transaction;
    }
}
#[path = "../../../apps/aestra-editor/src/editor_target.rs"]
mod editor_target;
#[path = "../../../apps/aestra-editor/src/material_target.rs"]
mod material_target;
mod feathers {
    pub(crate) use crate::action_activation;
    pub(crate) mod node_graph {
        pub(crate) use crate::memory::GraphViewportMemory;
    }
}
#[path = "../../../apps/aestra-editor/src/material_graph/command_preparation.rs"]
mod command_preparation;
#[path = "../../../apps/aestra-editor/src/document_registry.rs"]
mod document_registry;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/drag.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "System lint policy matches shipping editor"
)]
mod drag;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/drag_context.rs"]
#[allow(
    dead_code,
    clippy::too_many_arguments,
    reason = "Toolbar consumers are outside this gate; system lint policy matches shipping editor"
)]
mod drag_interaction;
#[path = "../../../apps/aestra-editor/src/editor_identity.rs"]
#[allow(
    dead_code,
    reason = "Host document managers consume the remaining identity API"
)]
mod editor_identity;
#[path = "../../../apps/aestra-editor/src/editor_view_registry.rs"]
mod editor_view_registry;
#[path = "editor_graph_geometry.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Explicit collector system dependencies match shipping editor lint policy"
)]
mod geometry;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/navigation/cursor_020.rs"]
mod graph_cursor;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/interaction/pointer_020.rs"]
mod graph_pointer;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/insertion.rs"]
#[allow(
    dead_code,
    reason = "Host semantic action consumers are qualified separately"
)]
mod insertion;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/marquee.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "System lint policy matches shipping editor"
)]
mod marquee;
#[path = "../../../apps/aestra-editor/src/history/material_program.rs"]
#[allow(
    dead_code,
    reason = "Host catalog/session adapters consume the remaining history API"
)]
mod material_program;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/materials.rs"]
mod materials;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/memory.rs"]
#[allow(
    dead_code,
    reason = "Panel memory lifecycle call sites are outside this gate"
)]
mod memory;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/navigation.rs"]
#[allow(
    dead_code,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Panel call sites are out of scope; system lint policy matches shipping editor"
)]
mod navigation;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/node.rs"]
#[allow(
    dead_code,
    reason = "Pin and selection host consumers are outside this transaction gate"
)]
mod node;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/resize.rs"]
mod resize;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/drag_assist/model.rs"]
pub(crate) mod snapping;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
#[allow(dead_code, reason = "Only graph colors are consumed by this fixture")]
mod theme;
mod drag_assist {
    pub(super) use crate::drag_interaction::{Context, State};
    pub(super) use crate::snapping as model;
}
