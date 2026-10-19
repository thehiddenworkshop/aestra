//! Actual production Profiler state, presentation, action consumers and plugin.
#[path = "../../../apps/aestra-editor/src/profiler/state.rs"]
mod state;
pub(crate) use state::ProfilerState;
#[path = "../../../apps/aestra-editor/src/profiler/control_input_020.rs"]
mod control_input;
#[path = "../../../apps/aestra-editor/src/profiler/controls.rs"]
mod controls;
#[path = "../../../apps/aestra-editor/src/profiler/plugin.rs"]
mod plugin;
#[path = "../../../apps/aestra-editor/src/profiler/workspace.rs"]
mod workspace;
