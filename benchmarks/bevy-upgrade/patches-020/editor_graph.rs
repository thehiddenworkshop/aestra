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
#[path = "../../../apps/aestra-editor/src/shell/action.rs"]
mod shell_action;
pub(crate) use shell_action::EditorAction;
#[path = "../../../apps/aestra-editor/src/shell/scroll_key.rs"]
#[allow(
    dead_code,
    reason = "Other panels consume the remaining actual scroll-memory keys"
)]
mod scroll_key;
pub(crate) use scroll_key::ScrollMemoryKey;
#[path = "../../../apps/aestra-editor/src/curves/action.rs"]
#[allow(
    dead_code,
    reason = "Remaining Curves controls are qualified with that panel"
)]
mod curves_action;
mod curves {
    pub(crate) use crate::curves_action::CurvesAction;
}
#[path = "../../../apps/aestra-editor/src/feathers/button/action.rs"]
mod action_button;
#[path = "../../../apps/aestra-editor/src/feathers/field_row.rs"]
pub(crate) mod field_row;
pub(crate) use action_button::spawn_action_button as spawn_feathers_action_button;
#[path = "editor_color_support.rs"]
mod color_support;
#[path = "../../../apps/aestra-editor/src/feathers/combo_box/selection.rs"]
mod combo_selection;
#[path = "../../../apps/aestra-editor/src/feathers/combo_box/selection_input_020.rs"]
mod selection_input;
pub(crate) use action_activation::EditorNativeControl;
pub(crate) use localization::Localizer;
#[path = "../../../apps/aestra-editor/src/feathers/code_editor/buffer.rs"]
#[allow(
    dead_code,
    reason = "Rendering and editing consumers remain in the widget gate"
)]
mod code_buffer;
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
#[path = "../../../apps/aestra-editor/src/persistence/state.rs"]
#[allow(
    dead_code,
    reason = "Native document action consumers remain host-owned"
)]
mod protection_state;
#[path = "../../../apps/aestra-editor/src/project_content/refresh.rs"]
#[allow(
    dead_code,
    reason = "Legacy synchronous refresh consumers are host-owned"
)]
mod refresh;
#[path = "../../../apps/aestra-editor/src/project_content/services.rs"]
mod services;
mod persistence {
    pub(crate) use crate::protection_state::{DocumentAction, DocumentProtectionState};
}
mod docking {
    pub(crate) use crate::editor_identity::EditorViewId;
    pub(crate) use crate::tool_panels::ToolPanel;
}
#[path = "../../../apps/aestra-editor/src/asset_actions/action.rs"]
#[allow(
    dead_code,
    reason = "Other asset action consumers remain host-qualified"
)]
mod asset_action;
#[path = "../../../apps/aestra-editor/src/asset_actions/state.rs"]
#[allow(dead_code, reason = "Extraction dialog consumers remain host-owned")]
mod asset_operation_state;
#[path = "../../../apps/aestra-editor/src/feathers/context_menu/composition.rs"]
#[allow(dead_code, reason = "Other popup consumers remain host-qualified")]
mod context_menu_composition;
#[path = "../../../apps/aestra-editor/src/feathers/icon.rs"]
#[allow(
    dead_code,
    reason = "Breadcrumb consumers have a separate composition gate"
)]
mod icon;
#[path = "../../../apps/aestra-editor/src/input.rs"]
mod input;
#[path = "../../../apps/aestra-editor/src/input/api_020.rs"]
mod input_api;
#[path = "../../../apps/aestra-editor/src/feathers/pointer_020.rs"]
#[allow(
    dead_code,
    unused_imports,
    reason = "Other pointer consumers have separate gates"
)]
mod pointer;
#[path = "../../../apps/aestra-editor/src/feathers/tooltip.rs"]
#[allow(
    dead_code,
    clippy::too_many_arguments,
    reason = "Tooltip system lint policy matches shipping; other consumers remain separate"
)]
mod tooltip;
#[path = "../../../apps/aestra-editor/src/transport.rs"]
mod transport;
#[path = "../../../apps/aestra-editor/src/transport/control_input_020.rs"]
mod transport_input;
#[path = "../../../apps/aestra-editor/src/viewport/keyboard.rs"]
mod viewport_keyboard;
#[path = "../../../apps/aestra-editor/src/viewport/shortcuts.rs"]
#[allow(
    dead_code,
    reason = "Rendered viewport action execution remains host-qualified"
)]
mod viewport_shortcuts;
mod viewport {
    pub(crate) use crate::viewport_shortcuts::{PreviewCanvas, ViewportAction, ViewportSet};
}
mod asset_actions {
    pub(crate) use crate::asset_action::AssetAction;
    pub(crate) use crate::asset_operation_state::AssetOperationState;
}
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/menu_state.rs"]
#[allow(
    dead_code,
    reason = "Function menu rendering and action consumers remain host-owned"
)]
mod function_menu_state;
#[path = "../../../apps/aestra-editor/src/material_graph/palette_state.rs"]
#[allow(
    dead_code,
    reason = "Palette rendering and action consumers remain host-owned"
)]
pub(crate) mod material_palette_state;
use material_palette_state as palette_state;
#[path = "../../../apps/aestra-editor/src/properties/palette_state.rs"]
#[allow(
    dead_code,
    reason = "Module stack rendering and action consumers remain host-owned"
)]
mod module_palette_state;
pub(crate) use module_palette_state::ModulePaletteState;
#[path = "../../../apps/aestra-editor/src/properties/module_data.rs"]
mod module_data;
pub(crate) use module_data::{EditorModuleRegistry, localized_properties_input, module_parameter};
#[path = "../../../apps/aestra-editor/src/dock_ui/metrics.rs"]
#[allow(dead_code, reason = "Floating window consumers remain host-owned")]
mod dock_metrics;
mod dock_ui {
    pub(crate) use crate::dock_metrics::EDITOR_MIN_SIZE;
}
#[path = "../../../apps/aestra-editor/src/feathers/scenes/root.rs"]
mod editor_root;
#[path = "../../../apps/aestra-editor/src/history/controls.rs"]
mod history_controls;
#[path = "../../../apps/aestra-editor/src/feathers/list_row/state.rs"]
mod list_row_state;
#[path = "../../../apps/aestra-editor/src/menus/state.rs"]
mod menu_state;
#[path = "../../../apps/aestra-editor/src/docking/panels.rs"]
#[allow(dead_code, reason = "Other docking consumers remain host-owned")]
mod tool_panels;
use history_controls as controls;
#[path = "../../../apps/aestra-editor/src/history/control_input_020.rs"]
mod control_input;
#[path = "../../../apps/aestra-editor/src/history/focus_pointer_020.rs"]
mod focus_pointer;
#[path = "../../../apps/aestra-editor/src/history/focus.rs"]
#[allow(dead_code, reason = "Full panel focus composition remains host-owned")]
mod history_focus;
#[path = "../../../apps/aestra-editor/src/history/keyboard.rs"]
pub(crate) mod history_keyboard;
#[path = "../../../apps/aestra-editor/src/history/ownership.rs"]
mod history_ownership;
use history_ownership as ownership;
#[path = "../../../apps/aestra-editor/src/asset_browser/bookmarks/model.rs"]
#[allow(dead_code, reason = "Browser toolbar labels remain host-owned")]
pub(crate) mod bookmarks;
#[path = "../../../apps/aestra-editor/src/asset_browser/state.rs"]
#[allow(dead_code, reason = "Other browser actions remain host-owned")]
pub(crate) mod browser_state;
#[path = "../../../apps/aestra-editor/src/asset_browser/deletion/panel.rs"]
pub(crate) mod deletion_panel;
#[path = "../../../apps/aestra-editor/src/asset_browser/deletion/documents.rs"]
pub(crate) mod documents;
#[path = "../../../apps/aestra-editor/src/asset_browser/deletion/history.rs"]
pub(crate) mod filesystem_history;
#[path = "../../../apps/aestra-editor/src/history/routing.rs"]
pub(crate) mod history_routing;
#[path = "../../../apps/aestra-editor/src/asset_browser/deletion/reference_checks.rs"]
#[allow(dead_code, reason = "Inspector action consumers remain host-owned")]
pub(crate) mod reference_checks;
#[path = "../../../apps/aestra-editor/src/feathers/scenes/common.rs"]
#[allow(dead_code, reason = "Other widgets have separate qualification gates")]
pub(crate) mod scenes;
#[path = "../../../apps/aestra-editor/src/feathers/scroll.rs"]
pub(crate) mod scrollbars;
mod asset_browser {
    pub(crate) use crate::browser_state as state;
    pub(crate) use crate::browser_state::{AssetBrowserState, BrowserSurface};
    pub(crate) mod deletion {
        pub(crate) use crate::{documents, filesystem_history as history, reference_checks};
    }
}
use history_action::HistoryAction;
mod menus {
    pub(crate) use crate::menu_state::*;
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
    pub(crate) mod graph {
        pub(crate) use crate::function_clipboard_context as clipboard_context;
        pub(crate) use crate::function_menu_state::FunctionGraphMenuState;
    }
    pub(crate) mod history {
        pub(crate) use crate::function_history::Key;
    }
}
#[path = "../../../apps/aestra-editor/src/asset_drop/transport.rs"]
mod asset_drop_transport;
#[path = "../../../apps/aestra-editor/src/history/asset_order.rs"]
#[allow(
    dead_code,
    reason = "Full filesystem action composition remains host-owned"
)]
pub(crate) mod asset_order;
#[path = "../../../apps/aestra-editor/src/asset_drop/payload.rs"]
#[allow(
    dead_code,
    reason = "Other authoring asset-assignment consumers remain host-qualified"
)]
mod asset_payload;
#[path = "../../../apps/aestra-editor/src/material_graph/clipboard/model.rs"]
#[allow(
    dead_code,
    reason = "Material clipboard adapters remain host-qualified"
)]
pub(crate) mod clipboard_model;
#[path = "../../../apps/aestra-editor/src/history/compound.rs"]
mod compound;
#[path = "../../../apps/aestra-editor/src/feathers/context_menu/dismissal.rs"]
pub(crate) mod context_menu_dismissal;
#[path = "../../../apps/aestra-editor/src/feathers/context_menu/intent.rs"]
mod context_menu_intent;
#[path = "../../../apps/aestra-editor/src/curves/state.rs"]
#[allow(
    dead_code,
    reason = "Production curve state is shared; full curves panel consumers remain separate"
)]
mod curves_state;
#[path = "../../../apps/aestra-editor/src/history/document_execution.rs"]
#[allow(
    dead_code,
    reason = "Menu availability and application handlers remain host-owned"
)]
mod document_execution;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/clipboard_context.rs"]
pub(crate) mod function_clipboard_context;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/clipboard/execution.rs"]
mod function_clipboard_execution;
#[path = "../../../apps/aestra-editor/src/material_function_editor/execution.rs"]
#[allow(dead_code, reason = "Other function controls remain host-owned")]
mod function_execution;
#[path = "../../../apps/aestra-editor/src/material_graph/selection.rs"]
#[allow(
    dead_code,
    reason = "Remaining material panel selection consumers are host-qualified"
)]
mod graph_selection;
#[path = "../../../apps/aestra-editor/src/material_graph/clipboard/execution.rs"]
mod material_clipboard_execution;
#[path = "../../../apps/aestra-editor/src/material_graph/edit_context.rs"]
mod material_edit_context;
#[path = "../../../apps/aestra-editor/src/material_graph/execution.rs"]
pub(crate) mod material_edit_execution;
#[path = "../../../apps/aestra-editor/src/properties/material_state.rs"]
mod material_inspector_state;
#[path = "../../../apps/aestra-editor/src/material_graph/keyboard.rs"]
mod material_keyboard;
#[path = "editor_timeline.rs"]
#[allow(
    dead_code,
    reason = "Full timeline action/UI composition remains separately qualified"
)]
mod timeline;
pub(crate) use curves_state::CurvesState;
mod asset_drop {
    pub(crate) use crate::asset_drop_transport::{active, cancelled, nearest};
    pub(crate) use crate::asset_payload::{AssetPayload, AuthoringDropGuard};
}
#[path = "../../../apps/aestra-editor/src/feathers/automation_curve.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Production raster query matches shipping editor lint policy"
)]
#[allow(
    dead_code,
    reason = "Timeline keyboard uses production value projection; rendered composition stays separate"
)]
pub(crate) mod automation_curve;
mod properties {
    pub(crate) use crate::material_inspector_state::MaterialStackInspectorState;
}
#[path = "../../../apps/aestra-editor/src/history/material_program_execution.rs"]
#[allow(
    dead_code,
    reason = "Other semantic action consumers remain host-owned"
)]
mod material_program_execution;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_state.rs"]
#[allow(
    dead_code,
    reason = "Raster worker and panel consumers remain host-owned"
)]
mod preview_state;
#[path = "../../../apps/aestra-editor/src/history/synchronization.rs"]
mod synchronization;
use preview_target::MaterialGraphPreviewTarget;
#[path = "../../../apps/aestra-editor/src/material_graph/graph_inventory.rs"]
mod graph_inventory;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/context.rs"]
#[allow(
    dead_code,
    reason = "Arrangement and panel capture consumers remain host-owned"
)]
mod presentation_context;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/state.rs"]
mod presentation_state;
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
#[path = "../../../apps/aestra-editor/src/feathers/button/tool.rs"]
mod tool_button;
mod history {
    pub(crate) use crate::asset_order;
    pub(crate) use crate::document_execution;
    pub(crate) use crate::document_execution::EditorHistoryLedger;
    pub(crate) use crate::history_keyboard as keyboard;
    pub(crate) use crate::history_keyboard::history_keyboard_input;
    pub(crate) use crate::history_routing as routing;
    pub(crate) use crate::material_program::MaterialProgramEditHistory;
    pub(crate) use crate::publication;
    pub(crate) mod material_program {
        pub(crate) use crate::material_program::{
            MATERIAL_HISTORY_LIMIT, MaterialProgramEditHistory, MaterialProgramHistoryEntry,
        };
    }
    pub(crate) use crate::history_action::{HistoryAction, queue_history_action_activation};
}
#[path = "../../../apps/aestra-editor/src/material_function_editor/history.rs"]
mod function_history;
#[path = "../../../apps/aestra-editor/src/material_drafts.rs"]
#[allow(dead_code, reason = "Save/recovery UI consumers remain host-owned")]
mod material_drafts;
#[path = "../../../apps/aestra-editor/src/project_content/model.rs"]
#[allow(dead_code, reason = "Project watch and UI consumers remain host-owned")]
mod project_catalog;
mod project_content {
    pub(crate) use crate::io;
    pub(crate) use crate::project_catalog::EditorProjectContent;
}
#[path = "../../../apps/aestra-editor/src/material_graph/keys.rs"]
pub(crate) mod graph_keys;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/transaction.rs"]
mod presentation_transaction;
use presentation_transaction as transaction;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_target.rs"]
mod preview_target;
#[path = "../../../apps/aestra-editor/src/history/publication.rs"]
pub(crate) mod publication;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/semantic.rs"]
mod semantic_snapshot;
use semantic_snapshot as semantic;
mod material_graph {
    pub(crate) use crate::command_preparation;
    pub(crate) use crate::material_edit_context::{MaterialGraphAction, MaterialGraphViewport};
    pub(crate) use crate::material_edit_execution as execution;
    pub(crate) use crate::material_palette_state as palette_state;
    pub(crate) mod clipboard {
        pub(crate) use crate::clipboard_model::*;
        pub(crate) use crate::material_clipboard_execution::{capture_material, insert_material};
    }
    pub(crate) use crate::graph_inventory;
    pub(crate) use crate::graph_keys as keys;
    pub(crate) use crate::graph_keys::function_graph_memory_key;
    pub(crate) use crate::graph_selection::MaterialGraphSelectionState;
    pub(crate) use crate::material_palette_state::{
        MaterialGraphPaletteState, MaterialSelectionScope,
    };
    pub(crate) use crate::preview_state::MaterialGraphPreviewState;
    pub(crate) use crate::preview_target::MaterialGraphPreviewTarget;
    pub(crate) mod presentation {
        pub(crate) use crate::presentation_state::State;
        pub(crate) use crate::presentation_transaction::Snapshot;
        pub(crate) use crate::presentation_transaction::Transaction;
    }
}
#[path = "../../../apps/aestra-editor/src/editor_target.rs"]
mod editor_target;
#[path = "../../../apps/aestra-editor/src/material_target.rs"]
mod material_target;
mod feathers {
    pub(crate) mod combo_box {
        pub(crate) use crate::combo_selection::{ComboOption, spawn_combo_control};
    }
    pub(crate) use crate::color_support::{color_picker, number_input, text_input};
    pub(crate) mod button {
        pub(crate) use crate::tool_button::spawn_tool_button;
    }
    pub(crate) use crate::field_row;
    pub(crate) mod list_row {
        pub(crate) use crate::list_row_state::*;
    }
    pub(crate) use crate::automation_curve;
    pub(crate) mod context_menu {
        pub(crate) use crate::context_menu_composition::*;
        pub(crate) use crate::context_menu_dismissal::*;
        pub(crate) use crate::context_menu_intent::*;
    }
    pub(crate) use crate::pointer;
    pub(crate) use crate::{icon, tooltip};
    pub(crate) mod scenes {
        pub(crate) use crate::editor_root::editor_root;
        pub(crate) use crate::scenes::*;
    }
    pub(crate) mod scroll {
        pub(crate) use crate::scrollbars::*;
    }
    pub(crate) mod code_editor {
        pub(crate) use crate::code_buffer::{ActiveCodeEditor, CodeEditor};
    }
    pub(crate) use crate::action_activation;
    pub(crate) mod node_graph {
        pub(crate) use crate::geometry;
        pub(crate) use crate::marquee::GraphSelectionMode;
        pub(crate) use crate::memory::GraphViewportMemory;
        pub(crate) use crate::navigation::FeathersGraphNavigationBlocker;
        pub(crate) use crate::navigation::FeathersGraphViewport;
        pub(crate) use crate::node::FeathersGraphNode;
    }
}
#[path = "../../../apps/aestra-editor/src/material_graph/command_preparation.rs"]
pub(crate) mod command_preparation;
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
pub(crate) mod geometry;
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
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
#[allow(
    dead_code,
    reason = "Other panels consume the remaining palette entries"
)]
mod palette;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/resize.rs"]
mod resize;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/drag_assist/model.rs"]
pub(crate) mod snapping;
#[path = "../../../apps/aestra-editor/src/theme/bevy_020.rs"]
mod theme_boundary;
mod theme {
    pub(crate) use crate::palette::*;
    pub(crate) use crate::theme_boundary::feathers_theme;
}
mod drag_assist {
    pub(super) use crate::drag_interaction::{Context, State};
    pub(super) use crate::snapping as model;
}
