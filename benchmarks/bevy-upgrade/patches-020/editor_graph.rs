//! Production graph navigation, node state, marquee observers and bounded layout solvers.
//! Full drag transactions use real identity, geometry registry, snapping and insertion types.
//! Geometry collection and semantic preparation/history storage use actual production modules.
//! Activation/event dispatch and focused target routing use production code too.
//! Actual catalog/session publication and scoped semantic history execute on both engines.
//! Complete UI handlers and native panel composition remain host-qualified separately.
extern crate self as aestra_bevy_render;
#[path = "../../../bevy/aestra-bevy-render/src/render_set.rs"]
mod render_set;
pub use render_set::AestraRenderSet;
#[path = "../../../bevy/aestra-bevy-render/src/material.rs"]
pub mod material;
#[path = "../../../bevy/aestra-bevy-render/src/material_layout.rs"]
#[allow(dead_code, reason = "Other renderer layout consumers remain separate")]
mod material_layout;
#[path = "../../../bevy/aestra-bevy-render/src/presented_effect.rs"]
#[allow(
    dead_code,
    reason = "Other renderer presentation consumers remain separate"
)]
mod presented_effect;
pub use presented_effect::{EffectRenderMode, PresentedEffect};
#[path = "../../../bevy/aestra-bevy-render/src/gpu/event_statistics.rs"]
#[allow(
    dead_code,
    reason = "Native counter producer remains in the renderer qualification gate"
)]
mod event_statistics;
#[path = "editor_telemetry.rs"]
#[allow(
    dead_code,
    reason = "GPU device producers remain in the separate renderer gate"
)]
mod gpu;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/output_context.rs"]
#[allow(
    dead_code,
    reason = "Native output production remains in the renderer qualification gate"
)]
mod output_context;
pub use aestra_runtime::{
    BackendCapabilities, CompatibilityIssueCode, CompatibilityReport, CompatibilityTarget,
    EffectRequirements, RendererCapability,
};
#[path = "../../../bevy/aestra-bevy-render/src/capabilities.rs"]
#[allow(
    dead_code,
    reason = "Other device capability consumers remain renderer-owned"
)]
mod capabilities;
pub use capabilities::{ActiveBackend, EffectRuntimeStatus};
#[path = "../../../bevy/aestra-bevy-render/src/render_settings.rs"]
#[allow(
    dead_code,
    reason = "Other presentation settings consumers remain renderer-owned"
)]
mod render_settings;
pub use render_settings::PresentationMode;
#[path = "../../../apps/aestra-editor/src/docking/maximized.rs"]
mod maximized;
#[path = "../../../apps/aestra-editor/src/persistence/status.rs"]
#[allow(
    dead_code,
    reason = "Other document persistence status consumers remain separate"
)]
mod persistence_status;
#[path = "../../../bevy/aestra-bevy-render/src/preview.rs"]
#[allow(
    dead_code,
    reason = "Other photographic presentation consumers remain separate"
)]
pub(crate) mod preview;
#[path = "editor_profiler.rs"]
mod profiler;
#[path = "../../../apps/aestra-editor/src/session.rs"]
#[allow(dead_code, reason = "Native application consumers remain host-owned")]
mod session;
#[path = "../../../apps/aestra-editor/src/settings.rs"]
#[allow(
    dead_code,
    reason = "Other native settings panel and startup consumers remain separate"
)]
mod settings;
pub(crate) use project_catalog::EditorProjectContent as ProjectEffectCatalog;
use project_catalog::{EditorProjectContent, PreparedProject};
use refresh::ProjectEffectWatchState;
pub(crate) use session::EditorSession;
#[path = "../../../apps/aestra-editor/src/shell/sets.rs"]
mod editor_sets;
#[path = "../../../apps/aestra-editor/src/shell/action.rs"]
mod shell_action;
use editor_sets::EditorSet;
#[path = "../../../apps/aestra-editor/src/persistence/sets.rs"]
mod persistence_sets;
use persistence_sets::PersistenceSet;
#[path = "../../../apps/aestra-editor/src/material_graph/function_layout.rs"]
pub(crate) mod material_function_layout;
#[path = "../../../apps/aestra-editor/src/material_graph/host_integration.rs"]
pub(crate) mod material_host_integration;
#[path = "../../../apps/aestra-editor/src/material_graph/layout_lifecycle.rs"]
pub(crate) mod material_layout_lifecycle;
#[path = "../../../apps/aestra-editor/src/material_graph/layout_persistence.rs"]
pub(crate) mod material_layout_persistence;
pub(crate) use shell_action::EditorAction;
#[path = "../../../apps/aestra-editor/src/shell/scroll_key.rs"]
#[allow(
    dead_code,
    reason = "Other panels consume the remaining actual scroll-memory keys"
)]
mod scroll_key;
pub(crate) use scroll_key::ScrollMemoryKey;
#[path = "../../../apps/aestra-editor/src/feathers/button/action.rs"]
mod action_button;
#[path = "editor_curves.rs"]
mod curves;
#[path = "../../../apps/aestra-editor/src/curves/action.rs"]
#[allow(
    dead_code,
    reason = "Remaining Curves controls are qualified with that panel"
)]
mod curves_action;
#[path = "../../../apps/aestra-editor/src/feathers/field_row.rs"]
pub(crate) mod field_row;
pub(crate) use action_button::spawn_action_button as spawn_feathers_action_button;
#[path = "../../../apps/aestra-editor/src/properties/stack_button.rs"]
mod property_stack_button;
pub(crate) use property_stack_button::stack_button;
pub(crate) use tool_button::spawn_tool_button as mini_button;
#[path = "editor_color_support.rs"]
mod color_support;
#[path = "../../../apps/aestra-editor/src/feathers/combo_box/actions.rs"]
mod combo_actions;
#[path = "../../../apps/aestra-editor/src/feathers/combo_box/selection.rs"]
mod combo_selection;
#[path = "../../../apps/aestra-editor/src/material_graph/insertion.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production insertion dependencies retain shipping lint policy"
)]
pub(crate) mod material_insertion;
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
    pub(crate) use crate::persistence_status::persist_editor_settings;
    pub(crate) use crate::protection_state::{DocumentAction, DocumentProtectionState};
}
mod docking {
    pub(crate) use crate::dock_model::*;
    pub(crate) use crate::editor_identity::EditorViewId;
    pub(crate) use crate::maximized::MaximizedPanel;
    pub(crate) use crate::tool_panels::ToolPanel;
}
mod document {
    pub(crate) use crate::document_registry::DocumentManager;
    pub(crate) use crate::editor_identity::{DocumentId, DocumentKey};
}
mod editor_view {
    pub(crate) use crate::editor_view_registry::{
        ActiveEditorContext, EditorViewKind, EditorViewManager,
    };
}
#[path = "../../../apps/aestra-editor/src/docking/model.rs"]
#[allow(
    dead_code,
    reason = "Other docking and persisted-layout consumers remain separate"
)]
mod dock_model;
pub(crate) use dock_model::WorkspaceLayout;
pub(crate) use tool_panels::ToolPanel;
#[path = "../../../apps/aestra-editor/src/shell/reveal.rs"]
mod panel_reveal;
pub(crate) use panel_reveal::reveal_dock_panel;
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
#[path = "editor_viewport.rs"]
mod viewport;
#[path = "../../../apps/aestra-editor/src/viewport/keyboard.rs"]
mod viewport_keyboard;
#[path = "../../../apps/aestra-editor/src/viewport/shortcuts.rs"]
mod viewport_shortcuts;
mod asset_actions {
    pub(crate) use crate::asset_action::AssetAction;
    pub(crate) use crate::asset_operation_state::AssetOperationState;
}
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/body_input.rs"]
pub(crate) mod function_body_input;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/connection.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production function socket observers retain shipping dependency/lint policy"
)]
pub(crate) mod function_connection;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/context_input.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Production context observer dependencies retain shipping lint policy"
)]
pub(crate) mod function_context_input;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/composition.rs"]
pub(crate) mod function_graph_composition;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/model.rs"]
#[allow(
    dead_code,
    reason = "Function input handlers remain in the native host; composition uses their actual components"
)]
pub(crate) mod function_graph_model;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/registration.rs"]
pub(crate) mod function_graph_registration;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/menu_state.rs"]
#[allow(
    dead_code,
    reason = "Function menu rendering and action consumers remain host-owned"
)]
mod function_menu_state;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/modified_input.rs"]
pub(crate) mod function_modified_input;
#[path = "../../../apps/aestra-editor/src/material_function_editor/registration.rs"]
mod function_registration;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/selection_input.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Production function selection queries retain shipping lint policy"
)]
pub(crate) mod function_selection_input;
#[path = "../../../apps/aestra-editor/src/material_function_editor/signature.rs"]
pub(crate) mod function_signature;
#[path = "../../../apps/aestra-editor/src/material_function_editor/signature_input.rs"]
pub(crate) mod function_signature_input;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/socket_palette.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production function socket palette observer retains shipping dependency/lint policy"
)]
pub(crate) mod function_socket_palette;
#[path = "../../../apps/aestra-editor/src/material_function_editor/graph/wires.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Production wire system dependencies retain shipping lint policy"
)]
pub(crate) mod function_wires;
#[path = "../../../apps/aestra-editor/src/feathers/graph_layout/elk.rs"]
pub(crate) mod layout_elk;
#[path = "../../../apps/aestra-editor/src/feathers/graph_layout/model.rs"]
pub(crate) mod layout_model;
#[path = "../../../apps/aestra-editor/src/feathers/graph_layout/native.rs"]
pub(crate) mod layout_native;
#[path = "../../../apps/aestra-editor/src/feathers/graph_layout/partial.rs"]
pub(crate) mod layout_partial;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/batch_input.rs"]
pub(crate) mod presentation_batch_input;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/edits.rs"]
pub(crate) mod presentation_edits;
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/tests.rs"]
pub(crate) mod presentation_history_tests;
#[path = "../../../apps/aestra-editor/src/material_graph/semantic_placement.rs"]
pub(crate) mod semantic_placement;
use layout_model::*;
#[path = "../../../apps/aestra-editor/src/material_function_editor/signature_model.rs"]
pub(crate) mod function_signature_model;
#[path = "../../../apps/aestra-editor/src/material_graph/palette_state.rs"]
#[allow(
    dead_code,
    reason = "Palette rendering and action consumers remain host-owned"
)]
pub(crate) mod material_palette_state;
#[path = "../../../apps/aestra-editor/src/material_graph/workspace_host.rs"]
pub(crate) mod material_workspace_host;
use material_palette_state as palette_state;
#[path = "../../../apps/aestra-editor/src/material_graph/appearance_020.rs"]
pub(crate) mod appearance_input;
#[path = "../../../apps/aestra-editor/src/material_graph/appearance.rs"]
pub(crate) mod graph_appearance;
#[path = "../../../apps/aestra-editor/src/material_graph/click_020.rs"]
pub(crate) mod graph_click_input;
#[path = "../../../apps/aestra-editor/src/material_graph/connection_drag.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original material connection system dependencies match shipping lint policy"
)]
pub(crate) mod graph_connection_drag;
#[path = "../../../apps/aestra-editor/src/material_graph/connection_state.rs"]
pub(crate) mod graph_connection_state;
#[path = "../../../apps/aestra-editor/src/material_graph/connections.rs"]
pub(crate) mod graph_connections;
#[path = "../../../apps/aestra-editor/src/material_graph/control_input_020.rs"]
pub(crate) mod graph_control_input;
#[path = "../../../apps/aestra-editor/src/material_graph/controls.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original material control system dependencies match shipping lint policy"
)]
pub(crate) mod graph_controls;
#[path = "../../../apps/aestra-editor/src/material_graph/drag_020.rs"]
pub(crate) mod graph_drag_input;
#[path = "../../../apps/aestra-editor/src/material_graph/header.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Original header dependencies match shipping lint policy"
)]
pub(crate) mod graph_header;
#[path = "../../../apps/aestra-editor/src/asset_browser/actions/locate.rs"]
mod graph_locate;
#[path = "../../../apps/aestra-editor/src/material_graph/menu_actions.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original material action system queries match shipping lint policy"
)]
pub(crate) mod graph_menu_actions;
#[path = "../../../apps/aestra-editor/src/material_graph/menu_opening.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original material graph pointer queries match shipping lint policy"
)]
pub(crate) mod graph_menu_opening;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/metrics.rs"]
mod graph_metrics;
#[path = "../../../apps/aestra-editor/src/material_graph/node_actions.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original node consumer system dependencies match shipping lint policy"
)]
pub(crate) mod graph_node_actions;
#[path = "../../../apps/aestra-editor/src/material_graph/node_controls.rs"]
pub(crate) mod graph_node_controls;
#[path = "../../../apps/aestra-editor/src/material_graph/node_metrics.rs"]
pub(crate) mod graph_node_metrics;
#[path = "../../../apps/aestra-editor/src/asset_browser/actions/open_function.rs"]
mod graph_open_function;
#[path = "../../../apps/aestra-editor/src/material_graph/palette.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original palette ECS queries match shipping lint policy"
)]
pub(crate) mod graph_palette;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/placement.rs"]
pub(crate) mod graph_placement;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/preview_toggle.rs"]
mod graph_preview_toggle;
#[path = "../../../apps/aestra-editor/src/material_graph/arrange.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production arrangement dependencies retain shipping lint policy"
)]
pub(crate) mod material_arrange;
#[path = "../../../apps/aestra-editor/src/material_graph/layout_adapter.rs"]
pub(crate) mod material_layout_adapter;
use drag_assist::controls as graph_snap_controls;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/socket.rs"]
mod graph_socket_geometry;
#[path = "../../../apps/aestra-editor/src/material_graph/sockets.rs"]
pub(crate) mod graph_sockets;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/toolbar.rs"]
pub(crate) mod graph_toolbar;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/wire_layer.rs"]
mod graph_wire_layer;
#[path = "../../../apps/aestra-editor/src/material_graph/workspace_layout.rs"]
pub(crate) mod graph_workspace_layout;
#[path = "../../../apps/aestra-editor/src/feathers/search_field.rs"]
mod search_field;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/toolbar_input_020.rs"]
mod toolbar_input;
use color_support::text_input;
use feathers::button;
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
#[path = "../../../apps/aestra-editor/src/feathers/scenes/viewport.rs"]
mod viewport_pane;
mod asset_browser {
    pub(crate) use crate::browser_state as state;
    pub(crate) use crate::browser_state::{AssetBrowserState, BrowserSurface};
    pub(crate) use crate::graph_locate::LocateInAssets;
    pub(crate) use crate::graph_open_function::OpenFunction;
    pub(crate) mod deletion {
        pub(crate) use crate::deletion_panel::history_step;
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
    pub(crate) use crate::function_graph_composition::spawn as spawn_graph;
    pub(crate) use crate::function_history::FunctionEditor;
    pub(crate) use crate::function_registration::register;
    pub(crate) use crate::function_signature::spawn;
    pub(crate) use crate::function_signature_input as signature_input;
    pub(crate) use crate::function_signature_model as signature_model;
    pub(crate) mod graph {
        pub(crate) use crate::function_body_input as body_input;
        pub(crate) use crate::function_clipboard_context as clipboard_context;
        pub(crate) use crate::function_clipboard_execution as clipboard;
        pub(crate) use crate::function_connection as connection;
        pub(crate) use crate::function_context_input as context_input;
        pub(crate) use crate::function_graph_composition as composition;
        pub(crate) use crate::function_graph_composition::{bootstrap_layout, estimated_size};
        pub(crate) use crate::function_graph_model as model;
        pub(crate) use crate::function_graph_registration::register;
        pub(crate) use crate::function_menu_state as menu_state;
        pub(crate) use crate::function_menu_state::FunctionGraphMenuState;
        pub(crate) use crate::function_modified_input as modified_input;
        pub(crate) use crate::function_selection_input as selection_input;
        pub(crate) use crate::function_socket_palette as socket_palette;
        pub(crate) use crate::function_socket_palette::tests::setup_with_registration as setup_with_registration_for_test;
        pub(crate) use crate::function_wires as wires;
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
pub(crate) mod graph_selection;
#[path = "../../../apps/aestra-editor/src/material_graph/clipboard/execution.rs"]
mod material_clipboard_execution;
#[path = "../../../apps/aestra-editor/src/material_graph/edit_context.rs"]
mod material_edit_context;
#[path = "../../../apps/aestra-editor/src/material_graph/execution.rs"]
pub(crate) mod material_edit_execution;
#[path = "../../../apps/aestra-editor/src/properties/material_state.rs"]
mod material_inspector_state;
#[path = "../../../apps/aestra-editor/src/material_graph/keyboard.rs"]
pub(crate) mod material_keyboard;
#[path = "editor_timeline.rs"]
#[allow(
    dead_code,
    reason = "Full timeline action/UI composition remains separately qualified"
)]
mod timeline;
pub(crate) use curves_state::CurvesState;
mod asset_drop {
    pub(crate) use crate::asset_drop_feedback::{
        Feedback, clear_feedback, register_feedback, resolve, show_feedback,
    };
    pub(crate) use crate::asset_drop_input as input;
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
    pub(crate) use crate::module_data::EditorModuleRegistry;
    pub(crate) use crate::module_data::module_parameter;
}
#[path = "../../../apps/aestra-editor/src/history/material_program_execution.rs"]
#[allow(
    dead_code,
    reason = "Other semantic action consumers remain host-owned"
)]
mod material_program_execution;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_state.rs"]
pub(crate) mod preview_state;
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
#[path = "../../../apps/aestra-editor/src/material_graph/presentation/preview.rs"]
mod presentation_preview;
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
#[path = "../../../apps/aestra-editor/src/history/dispatch.rs"]
mod history_dispatch;
#[path = "../../../apps/aestra-editor/src/feathers/button/tool.rs"]
mod tool_button;
mod history {
    pub(crate) use crate::asset_order;
    pub(crate) use crate::document_execution;
    pub(crate) use crate::document_execution::EditorHistoryLedger;
    pub(crate) use crate::history_dispatch::execute_history_action;
    pub(crate) use crate::history_keyboard as keyboard;
    pub(crate) use crate::history_keyboard::history_keyboard_input;
    pub(crate) use crate::history_routing as routing;
    pub(crate) use crate::material_program::MaterialProgramEditHistory;
    pub(crate) use crate::publication;
    pub(crate) use crate::synchronization::{clear_document_order, clear_presentation_redo};
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
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/composition.rs"]
mod composition;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/composition_input_020.rs"]
mod composition_input;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/node_control_input_020.rs"]
mod node_control_input;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/node_controls.rs"]
mod node_controls;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/viewport_presentation.rs"]
mod viewport_presentation;
use graph_metrics as metrics;
use graph_preview_toggle as preview_toggle;
use graph_socket_geometry as socket;
use graph_wire_layer as wire_layer;
#[path = "../../../apps/aestra-editor/src/asset_drop/feedback.rs"]
pub(crate) mod asset_drop_feedback;
#[path = "../../../apps/aestra-editor/src/asset_drop/input_020.rs"]
pub(crate) mod asset_drop_input;
#[path = "../../../apps/aestra-editor/src/material_graph/ports.rs"]
pub(crate) mod graph_ports;
#[path = "../../../apps/aestra-editor/src/material_graph/asset_drop.rs"]
pub(crate) mod material_asset_drop;
#[path = "../../../apps/aestra-editor/src/material_graph/asset_preview.rs"]
pub(crate) mod material_asset_preview;
#[path = "../../../apps/aestra-editor/src/material_graph/composition.rs"]
pub(crate) mod material_composition;
#[path = "../../../apps/aestra-editor/src/material_graph/projection.rs"]
pub(crate) mod material_projection;
#[path = "../../../apps/aestra-editor/src/material_graph/registration.rs"]
pub(crate) mod material_registration;
#[path = "../../../apps/aestra-editor/src/material_graph/workspace.rs"]
pub(crate) mod material_workspace;
#[path = "../../../apps/aestra-editor/src/material_graph/node_previews.rs"]
pub(crate) mod node_previews;
#[path = "../../../apps/aestra-editor/src/material_graph/preset_previews.rs"]
pub(crate) mod preset_previews;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_pixels.rs"]
pub(crate) mod preview_pixels;
#[path = "../../../apps/aestra-editor/src/material_graph/preview_surface.rs"]
pub(crate) mod preview_surface;

mod material_graph {
    pub(crate) use crate::appearance_input;
    pub(crate) use crate::command_preparation;
    pub(crate) use crate::graph_appearance as appearance;
    pub(crate) use crate::graph_click_input as click_input;
    pub(crate) use crate::graph_connection_drag as connection_drag;
    pub(crate) use crate::graph_connection_drag::update_wire_material;
    pub(crate) use crate::graph_connection_state as connection_state;
    pub(crate) use crate::graph_connections as connections;
    pub(crate) use crate::graph_connections::{edge_target, input_target};
    pub(crate) use crate::graph_control_input as control_input;
    pub(crate) use crate::graph_controls as controls;
    pub(crate) use crate::graph_drag_input as drag_input;
    pub(crate) use crate::graph_header as header;
    pub(crate) use crate::graph_menu_actions as menu_actions;
    pub(crate) use crate::graph_menu_opening as menu_opening;
    pub(crate) use crate::graph_node_actions as node_actions;
    pub(crate) use crate::graph_node_controls as node_controls;
    pub(crate) use crate::graph_node_metrics as node_metrics;
    pub(crate) use crate::graph_palette as palette;
    pub(crate) use crate::graph_ports as ports;
    pub(crate) use crate::graph_ports::input_port_presentation;
    pub(crate) use crate::graph_selection as selection;
    pub(crate) use crate::graph_sockets as sockets;
    pub(crate) use crate::graph_sockets::distance_to_graph_wire;
    pub(crate) use crate::graph_sockets::viewport_local_position;
    pub(crate) use crate::graph_workspace_layout as workspace_layout;
    pub(crate) use crate::graph_workspace_layout::expression_depth;
    pub(crate) use crate::material_arrange as arrange;
    pub(crate) use crate::material_asset_drop as asset_drop;
    pub(crate) use crate::material_composition as composition;
    pub(crate) use crate::material_edit_context::{MaterialGraphAction, MaterialGraphViewport};
    pub(crate) use crate::material_edit_execution as execution;
    pub(crate) use crate::material_function_layout as function_layout;
    pub(crate) use crate::material_host_integration as host_integration;
    pub(crate) use crate::material_host_integration::clear_document_transients;
    pub(crate) use crate::material_insertion as insertion;
    pub(crate) use crate::material_keyboard as keyboard;
    pub(crate) use crate::material_layout_adapter as layout_adapter;
    pub(crate) use crate::material_layout_lifecycle as layout_lifecycle;
    pub(crate) use crate::material_layout_persistence as layout_persistence;
    pub(crate) use crate::material_palette_state as palette_state;
    pub(crate) use crate::material_projection as projection;
    pub(crate) use crate::material_registration::EditorMaterialGraphPlugin;
    pub(crate) use crate::material_workspace as workspace;
    pub(crate) use crate::node_previews;
    pub(crate) use crate::preset_previews;
    pub(crate) use crate::preview_pixels;
    pub(crate) use crate::preview_state;
    pub(crate) use crate::preview_surface;
    pub(crate) use crate::semantic_placement;
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
        pub(crate) use crate::presentation_batch_input as batch_input;
        pub(crate) use crate::presentation_batch_input::batch_edit;
        pub(crate) use crate::presentation_context as context;
        pub(crate) use crate::presentation_context::visible;
        pub(crate) use crate::presentation_edits::{arrange, register};
        pub(crate) use crate::presentation_history_tests as tests;
        pub(crate) use crate::presentation_preview as preview;
        pub(crate) use crate::presentation_preview::preview_edit;
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
    pub(crate) mod graph_layout {
        pub(crate) use crate::layout_elk::ElkLayeredLayout;
        pub(crate) use crate::layout_model::*;
        pub(crate) use crate::layout_native as native;
        pub(crate) use crate::layout_partial as partial;
    }
    pub(crate) use crate::panel_chrome as panel;
    pub(crate) mod combo_box {
        pub(crate) use crate::combo_actions::*;
        pub(crate) use crate::combo_selection as selection;
        pub(crate) use crate::combo_selection::{ComboOption, spawn_combo_control};
    }
    pub(crate) use crate::color_support::{color_picker, number_input, text_input};
    pub(crate) mod button {
        pub(crate) use crate::action_activation::FeathersActionButton;
        pub(crate) use crate::action_button::spawn_action_button;
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
    pub(crate) use crate::search_field;
    pub(crate) use crate::{icon, tooltip};
    pub(crate) mod scenes {
        pub(crate) use crate::color_support::number_input::feathers_scalar_input;
        pub(crate) use crate::editor_root::editor_root;
        pub(crate) use crate::scenes::*;
        pub(crate) use crate::viewport_pane::viewport_pane;
    }
    pub(crate) mod scroll {
        pub(crate) use crate::scrollbars::*;
    }
    pub(crate) mod code_editor {
        pub(crate) use crate::code_buffer::{ActiveCodeEditor, CodeEditor};
    }
    pub(crate) use crate::action_activation;
    pub(crate) mod node_graph {
        pub(crate) use crate::composition::*;
        pub(crate) use crate::drag_assist;
        pub(crate) use crate::geometry;
        pub(crate) use crate::graph_metrics::*;
        pub(crate) use crate::graph_placement as placement;
        pub(crate) use crate::graph_preview_toggle::FeathersGraphNodePreviewToggle;
        pub(crate) use crate::graph_snap_controls::spawn_controls as spawn_graph_drag_controls;
        pub(crate) use crate::graph_socket_geometry::{
            GraphSocketSide, compact_graph_socket_offset,
        };
        pub(crate) use crate::graph_toolbar as toolbar;
        pub(crate) use crate::graph_toolbar::{
            GraphFrameAction, graph_toolbar_bundle, spawn_graph_frame_button,
            spawn_graph_tool_button, spawn_graph_toolbar_summary,
        };
        pub(crate) use crate::graph_wire_layer::FeathersGraphWireLayer;
        pub(crate) use crate::insertion;
        pub(crate) use crate::integration::GraphWidgetSync;
        pub(crate) use crate::marquee::{GraphMarqueeSelection, GraphSelectionMode};
        pub(crate) use crate::materials::GraphWireMaterial;
        pub(crate) use crate::memory::GraphViewportMemory;
        pub(crate) use crate::navigation::FeathersGraphNavigationBlocker;
        pub(crate) use crate::navigation::{FeathersGraphViewport, GraphFrameTarget};
        pub(crate) use crate::node::{
            FeathersGraphNode, FeathersGraphNodePreview, GraphModifiedDragTarget,
            GraphModifiedNodeDrag, GraphNodeDragModifier, GraphPinEdit, GraphPresentationBatchEdit,
            GraphPresentationEdit,
        };
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
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/geometry.rs"]
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
#[path = "../../../apps/aestra-editor/src/feathers/panel.rs"]
#[allow(
    dead_code,
    reason = "Other shared panel chrome consumers remain separate"
)]
pub(crate) mod panel_chrome;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/resize.rs"]
mod resize;
#[path = "../../../apps/aestra-editor/src/theme/bevy_020.rs"]
mod theme_boundary;
mod theme {
    pub(crate) use crate::palette::*;
    pub(crate) use crate::theme_boundary::feathers_theme;
}
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/drag_assist.rs"]
pub(crate) mod drag_assist;
use drag_interaction as drag_context;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/framing.rs"]
mod framing;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/integration.rs"]
mod integration;
#[path = "../../../apps/aestra-editor/src/feathers/node_graph/overlay.rs"]
mod overlay;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy::{ui::RelativeCursorPosition, ui_widgets::Activate};
use composition::*;
use drag::*;
use graph_cursor::{EntityCursor, OverrideCursor};
use graph_metrics::{NODE_PREVIEW_SIZE, NODE_WIDTH};
use graph_socket_geometry::GraphSocketSide;
use graph_toolbar as toolbar;
use graph_toolbar::GraphFrameAction;
use memory::GraphViewportMemory;
use navigation::*;
use node::*;
use node_controls::*;
use std::collections::{BTreeMap, BTreeSet};

use integration::GraphNodeControlFilter;
