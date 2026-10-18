//! Actual workspace scroll-memory identity shared by panel compositions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ScrollMemoryKey {
    AssetBrowserSources,
    AssetBrowserBuiltInSources,
    AssetBrowserItems,
    AssetBrowserBuiltIns,
    AssetBrowserDocument,
    Properties,
    PropertiesInspector,
    CompilerInspector,
    Profiler,
    Settings,
    Diagnostics,
    TransactionConfirmation,
    Curves,
    MaterialGraphPalette,
    MaterialFunctionInspector,
    /// Per-view so two views of one WESL module scroll independently.
    WeslSource(crate::docking::EditorViewId),
}
