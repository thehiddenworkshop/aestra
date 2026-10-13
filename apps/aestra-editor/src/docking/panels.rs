use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum ToolPanel {
    #[default]
    Viewport,
    Assets,
    AssetInspector,
    ModuleStack,
    #[serde(alias = "Inspector")]
    Properties,
    Timeline,
    Curves,
    #[serde(alias = "Changes")]
    Diagnostics,
    #[serde(alias = "GeneratedCode")]
    CompilerInspector,
    MaterialGraph,
    Profiler,
    Settings,
}

impl ToolPanel {
    pub(crate) const ALL: [Self; 12] = [
        Self::Viewport,
        Self::Assets,
        Self::AssetInspector,
        Self::ModuleStack,
        Self::Properties,
        Self::Timeline,
        Self::Curves,
        Self::Diagnostics,
        Self::CompilerInspector,
        Self::MaterialGraph,
        Self::Profiler,
        Self::Settings,
    ];

    pub(crate) fn message_id(self) -> &'static str {
        match self {
            Self::Viewport => "panel-viewport",
            Self::Assets => "panel-assets",
            Self::AssetInspector => "panel-asset-inspector",
            Self::ModuleStack => "panel-module-stack",
            Self::Properties => "panel-properties",
            Self::Timeline => "panel-timeline",
            Self::Curves => "panel-curves",
            Self::Diagnostics => "panel-diagnostics",
            Self::CompilerInspector => "panel-compiler-inspector",
            Self::MaterialGraph => "panel-material-graph",
            Self::Profiler => "panel-profiler",
            Self::Settings => "panel-settings",
        }
    }

    pub(crate) fn closable(self) -> bool {
        self != Self::Viewport
    }
}
