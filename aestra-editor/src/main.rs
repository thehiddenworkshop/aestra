mod docking;
mod localization;
mod session;
mod settings;
mod theme;
mod ui_shell;

use aestra_authoring::{ChangeKind, EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraSet, BlendMode, ColorKey, CurveKey, Diagnostic,
    DiagnosticCode, DiagnosticSeverity, EffectAsset, EffectPlayer, EffectRuntimeStatus,
    EmitterShape, FlipbookPlaybackMode, FlipbookTimeSource, MaterialInput, MaterialProperties,
    ModuleId, ModuleInstance, ModuleParameters, RendererId, RendererProperties, StageKind,
    ValidationReport, Value,
};
use aestra_compiler::{InputControl, InputMetadata, ModuleMetadata, ModuleRegistry};
use aestra_runtime::{CompiledEffect, CompiledEmitter, Instruction, RuntimeStage};
use aestra_runtime::{EffectProfile, ProfileValue, ProfileValueSource};
use bevy::{
    asset::AssetPlugin,
    camera::{RenderTarget, Viewport, visibility::RenderLayers},
    ecs::system::SystemParam,
    feathers::{
        FeathersPlugins,
        constants::{fonts, icons},
        containers::{group, group_body, group_header, pane_header},
        controls::{NumberInputValue, UpdateNumberInput},
        display::{icon, label, label_dim},
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemedText},
        tokens,
    },
    gizmos::transform_gizmo::{
        TransformGizmoCamera, TransformGizmoFocus, TransformGizmoMode, TransformGizmoPlugin,
        TransformGizmoSettings, TransformGizmoSpace, TransformGizmoSystems,
    },
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    },
    picking::events::{Click, Drag, DragDrop, DragEnd, DragStart, Out, Over, Pointer, Scroll},
    picking::pointer::PointerButton,
    prelude::*,
    text::FontSource,
    ui::{Checked, InteractionDisabled, Pressed, RelativeCursorPosition, UiSystems},
    ui_widgets::{
        Activate, ScrollArea, ScrollIntoView, Scrollbar, ValueChange,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
    window::{
        CursorIcon, PrimaryWindow, SystemCursorIcon, WindowCloseRequested, WindowMoved,
        WindowPosition, WindowRef, WindowResizeConstraints, WindowResized, WindowResolution,
    },
};
use docking::{DockAxis, DockDrop, DockNode, DockNodeId, DockPanel, DockStack, WorkspaceLayout};
use fluent_bundle::FluentArgs;
use localization::{Localizer, SUPPORTED_LOCALES};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use session::EditorSession;
use settings::{EditorSettings, SettingsPersistence};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const EDITOR_ASSET_ROOT: &str = "../assets";
const MAX_PREVIEW_PARTICLE_LIMIT: usize = 384;
const INSPECTOR_HIGHLIGHT_DURATION: f32 = 1.6;
const INSPECTOR_TOOLTIP_DELAY: Duration = Duration::from_millis(650);
const PROFILER_HISTORY_SAMPLES: usize = 96;

fn main() {
    let (mut settings, persistence) = SettingsPersistence::load();
    let localizer =
        Localizer::new(&settings.language.locale).expect("embedded Fluent catalogs must be valid");
    settings.language.locale = localizer.locale().into();
    let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
    session.playing = settings.preview.play_on_open;
    if let Some(diagnostic) = persistence.diagnostic() {
        session.status = diagnostic.into();
    }
    let menu = MenuState {
        show_grid: settings.preview.show_grid,
        ..default()
    };
    let ui_scale = settings.appearance.ui_scale;
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(session)
        .insert_resource(settings)
        .insert_resource(persistence)
        .insert_resource(localizer)
        .insert_resource(UiScale(ui_scale))
        .insert_resource(EffectCatalog::scan())
        .insert_resource(menu)
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<DiagnosticsPanelState>()
        .init_resource::<ProfilerState>()
        .init_resource::<SettingsPanelState>()
        .init_resource::<InspectorFocus>()
        .init_resource::<InspectorTooltipState>()
        .init_resource::<ScrollMemoryState>()
        .init_resource::<WorkspaceState>()
        .init_resource::<DockDragState>()
        .init_resource::<ResizeState>()
        .init_resource::<PreviewCameraController>()
        .insert_resource(WorkspaceLayout::load())
        .init_resource::<RenderedUiRevision>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: EDITOR_ASSET_ROOT.into(),
                    ..default()
                })
                .set(WindowPlugin {
                    close_when_requested: false,
                    primary_window: Some(Window {
                        title: "Aestra — VFX Choreography Editor".into(),
                        resolution: WindowResolution::new(1440, 900),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(FeathersPlugins)
        .add_plugins(TransformGizmoPlugin)
        .add_plugins(AestraPlugin)
        .insert_resource(theme::feathers_theme())
        .add_observer(handle_settings_toggle_change)
        .add_observer(handle_settings_integer_change)
        .add_observer(handle_settings_scalar_change)
        .add_observer(handle_inspector_toggle_change)
        .add_observer(handle_module_enabled_change)
        .add_observer(handle_renderer_enabled_change)
        .add_observer(handle_renderer_scalar_change)
        .add_observer(handle_renderer_toggle_change)
        .add_observer(handle_emitter_scalar_change)
        .add_observer(handle_inspector_integer_change)
        .add_observer(handle_inspector_scalar_change)
        .add_observer(handle_inspector_numeric_scroll)
        .add_observer(begin_inspector_tooltip)
        .add_observer(select_inspector_header)
        .add_observer(queue_feathers_action_activation)
        .add_systems(
            Startup,
            (setup_window_cursor, setup_editor_fonts, setup_editor),
        )
        .add_systems(
            Update,
            (
                (
                    apply_editor_fonts,
                    module_palette_keyboard,
                    keyboard_shortcuts,
                    audit_editor_action_controls,
                    handle_buttons,
                    handle_window_close_requests,
                    persist_native_window_geometry,
                    dismiss_open_menus,
                    scrub_timeline,
                    advance_playback,
                    sync_rendered_preview,
                    update_preview,
                    update_profiler_labels,
                    update_localized_text,
                    update_editor_labels,
                    update_compile_status,
                    update_history_actions,
                    navigate_preview_camera,
                    update_transform_gizmo_controls,
                    draw_preview_scene_gizmos,
                )
                    .chain(),
                (
                    update_playhead,
                    update_layer_selection,
                    update_menu_visibility,
                    update_panel_visibility_labels,
                    update_floating_window_titles,
                    clear_finished_dock_drag,
                    sync_dock_drop_hints,
                    sync_tab_reorder_hints,
                    sync_tab_append_hint,
                    update_dock_zone_style,
                    remember_scroll_positions,
                    rebuild_editor_ui,
                    restore_scroll_positions,
                    (
                        sync_settings_number_inputs,
                        sync_emitter_number_inputs,
                        sync_inspector_number_inputs,
                        sync_renderer_number_inputs,
                    )
                        .chain(),
                    sync_native_floating_windows,
                    scroll_inspector_to_focus,
                    update_scrollbar_visibility,
                    update_inspector_highlight,
                    update_inspector_tooltip,
                )
                    .chain(),
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            sync_preview_camera_viewport
                .after(UiSystems::Layout)
                .before(TransformGizmoSystems),
        )
        .configure_sets(Update, AestraSet::Playback.after(sync_rendered_preview))
        .run();
}

#[derive(Component, Clone, Copy)]
enum EditorAction {
    NewEffect,
    OpenEffect,
    OpenCatalog(usize),
    TogglePlayback,
    Restart,
    StepFrame(i8),
    AdjustPreviewSeed(i8),
    Save,
    SaveAs,
    Exit,
    Undo,
    Redo,
    AddLayer,
    DuplicateLayer,
    DeleteLayer,
    SelectLayer(usize),
    EffectDuration(f32),
    OpenModulePalette(StackStage),
    CloseModulePalette,
    AddModule(usize),
    AddSpriteMaterial,
    AddGridFlipbook,
    AddSpriteRenderer,
    AddFlipbookRenderer,
    EditComplexInput(ModuleId, u8),
    AddComplexKey,
    DeleteComplexKey,
    AdjustComplexTime(i8),
    AdjustCurveValue(i8),
    AdjustGradientChannel(u8, i8),
    MoveModule(ModuleId, i8),
    DuplicateModule(ModuleId),
    DeleteModule(ModuleId),
    DuplicateRenderer(RendererId),
    DeleteRenderer(RendererId),
    ApplyPendingChange,
    DiscardPendingChange,
    SetDiagnosticsFilter(DiagnosticsFilter),
    SelectDiagnostic {
        source: DiagnosticSource,
        index: usize,
    },
    SelectCompiledTarget(SemanticTarget),
    ResetProfilerPeaks,
    SelectDockPanel(DockPanel),
    CloseDockPanel(DockPanel),
    ShowDockPanel(DockPanel),
    ToggleDockPanel(DockPanel),
    FloatDockPanel(DockPanel, [f32; 2]),
    ToggleMenu(MenuKind),
    TogglePanelsSubmenu,
    ToggleGrid,
    ResetWorkspaceLayout,
    SelectSettingsCategory(SettingsCategory),
    ToggleInspectorSection(InspectorSection),
    SetLocale(usize),
    SetModuleChoice {
        module: ModuleId,
        input: u8,
        choice: u8,
    },
    SetRendererMaterial(RendererId, usize),
    SetRendererBlend(RendererId, BlendMode),
    SetRendererTexture(RendererId, Option<usize>),
    SetRendererFlipbook(RendererId, usize),
    SetFlipbookTimeSource(RendererId, FlipbookTimeSource),
    SetFlipbookPlayback(RendererId, FlipbookPlaybackMode),
    ResetEditorSettings,
    ShowAbout,
    CloseAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComplexSelection {
    module: ModuleId,
    input: u8,
    key: usize,
}

#[derive(Resource, Default)]
struct WorkspaceState {
    complex: Option<ComplexSelection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum InspectorSection {
    Module(ModuleId),
    Renderer(RendererId),
}

#[derive(Component)]
struct InspectorHelp(String);

#[derive(Component)]
struct InspectorTooltipPopup;

#[derive(Resource, Default)]
struct InspectorTooltipState {
    target: Option<Entity>,
    hovered_at: Option<Instant>,
    popup: Option<Entity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScrollMemoryKey {
    Inspector,
    GeneratedCode,
    Profiler,
    Settings,
    Diagnostics,
    ChangesList,
    ChangesReview,
    Curves,
}

#[derive(Resource, Default)]
struct ScrollMemoryState(HashMap<ScrollMemoryKey, Vec2>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackStage {
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Render,
}

impl StackStage {
    const ALL: [Self; 4] = [
        Self::EmitterUpdate,
        Self::ParticleSpawn,
        Self::ParticleUpdate,
        Self::Render,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::EmitterUpdate => "EMITTER UPDATE",
            Self::ParticleSpawn => "PARTICLE SPAWN",
            Self::ParticleUpdate => "PARTICLE UPDATE",
            Self::Render => "RENDER",
        }
    }

    fn semantic(self) -> Option<StageKind> {
        match self {
            Self::EmitterUpdate => Some(StageKind::EmitterUpdate),
            Self::ParticleSpawn => Some(StageKind::ParticleSpawn),
            Self::ParticleUpdate => Some(StageKind::ParticleUpdate),
            Self::Render => None,
        }
    }
}

#[derive(Resource)]
struct EditorModuleRegistry(ModuleRegistry);

impl Default for EditorModuleRegistry {
    fn default() -> Self {
        Self(ModuleRegistry::builtin())
    }
}

#[derive(Resource)]
struct ModulePaletteState {
    open: bool,
    stage: StackStage,
    query: String,
}

impl Default for ModulePaletteState {
    fn default() -> Self {
        Self {
            open: false,
            stage: StackStage::EmitterUpdate,
            query: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum DiagnosticsFilter {
    #[default]
    All,
    Errors,
    Warnings,
    Info,
}

impl DiagnosticsFilter {
    const ALL: [Self; 4] = [Self::All, Self::Errors, Self::Warnings, Self::Info];

    fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Errors => "ERRORS",
            Self::Warnings => "WARNINGS",
            Self::Info => "INFO",
        }
    }

    fn matches(self, severity: DiagnosticSeverity) -> bool {
        match self {
            Self::All => true,
            Self::Errors => severity == DiagnosticSeverity::Error,
            Self::Warnings => severity == DiagnosticSeverity::Warning,
            Self::Info => severity == DiagnosticSeverity::Info,
        }
    }
}

#[derive(Resource, Default)]
struct DiagnosticsPanelState {
    filter: DiagnosticsFilter,
}

#[derive(Resource, Default)]
struct ProfilerState {
    profile: Option<EffectProfile>,
    cpu_history_ns: VecDeque<u64>,
}

impl ProfilerState {
    fn record_cpu_frame(
        &mut self,
        effect: &CompiledEffect,
        samples: &[aestra_bevy::ParticleSample],
        elapsed: Duration,
    ) -> bool {
        let rebuilt = self
            .profile
            .as_ref()
            .is_none_or(|profile| !profile.matches_compiled(effect));
        if rebuilt {
            self.profile = Some(EffectProfile::from_compiled(effect));
            self.cpu_history_ns.clear();
        }
        let profile = self.profile.as_mut().expect("profile was initialized");
        profile.record_cpu_frame(elapsed, samples);
        self.cpu_history_ns
            .push_back(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64);
        while self.cpu_history_ns.len() > PROFILER_HISTORY_SAMPLES {
            self.cpu_history_ns.pop_front();
        }
        rebuilt
    }

    fn reset_peaks(&mut self) {
        if let Some(profile) = &mut self.profile {
            profile.reset_peaks();
        }
        self.cpu_history_ns.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticSource {
    Current,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
    View,
    Help,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SettingsCategory {
    #[default]
    General,
    Preview,
    Performance,
    Capture,
    Appearance,
    Language,
    Keybindings,
}

impl SettingsCategory {
    const ALL: [Self; 7] = [
        Self::General,
        Self::Preview,
        Self::Performance,
        Self::Capture,
        Self::Appearance,
        Self::Language,
        Self::Keybindings,
    ];

    fn message_id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Preview => "settings-preview",
            Self::Performance => "settings-performance",
            Self::Capture => "settings-capture",
            Self::Appearance => "settings-appearance",
            Self::Language => "settings-language",
            Self::Keybindings => "settings-keybindings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsToggle {
    ConfirmUnsavedChanges,
    ShowGrid,
    PlayOnOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsNumber {
    PreviewParticleLimit,
    CaptureFrameRate,
    ContactSheetColumns,
    UiScale,
}

#[derive(Resource, Default)]
struct SettingsPanelState {
    category: SettingsCategory,
}

#[derive(Resource)]
struct MenuState {
    open: Option<MenuKind>,
    panels_open: bool,
    tab_context: Option<TabContextMenu>,
    show_grid: bool,
    show_about: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: None,
            panels_open: false,
            tab_context: None,
            show_grid: true,
            show_about: false,
        }
    }
}

#[derive(Clone, Copy)]
struct TabContextMenu {
    panel: DockPanel,
    position: [f32; 2],
}

#[derive(Resource, Default)]
struct RenderedUiRevision(u64);

#[derive(Component)]
struct EditorRoot;

#[derive(Component)]
struct EditorContent;

#[derive(Resource)]
struct EditorFonts {
    mono: Handle<Font>,
}

#[derive(Component)]
struct DocumentMenuLabel;

#[derive(Component)]
struct DocumentToolbarLabel;

#[derive(Component)]
struct UndoMenuItem;

#[derive(Component)]
struct RedoMenuItem;

#[derive(Component)]
struct DockPane(DockNodeId);

#[derive(Component)]
struct DockTab(DockPanel);

#[derive(Component)]
struct DockTabAppendZone(DockNodeId);

#[derive(Component)]
struct DockTabAppendIndicator(DockNodeId);

#[derive(Component)]
struct DiagnosticsFilterButton(DiagnosticsFilter);

#[derive(Component)]
struct SettingsCategoryButton(SettingsCategory);

#[derive(Component)]
struct FeathersActionButton;

/// Marks an intentional editor-native interaction that has no equivalent Feathers control.
/// Standard buttons carrying an [`EditorAction`] should use [`FeathersActionButton`] instead.
#[derive(Component)]
struct EditorNativeControl;

type UnclassifiedEditorActionControl = (
    Added<EditorAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

#[derive(Component)]
struct PendingFeathersActivation;

#[derive(Component)]
struct SettingsToggleControl(SettingsToggle);

#[derive(Component)]
struct SettingsNumberControl(SettingsNumber);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorNumberKind {
    U32,
    Scalar,
    Vector,
    Range,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorNumberControl {
    module: ModuleId,
    parameter: &'static str,
    component: u8,
    kind: InspectorNumberKind,
    step: f32,
    min: Option<f32>,
    max: Option<f32>,
}

#[derive(Component, Debug, Clone, Copy)]
enum EmitterNumberControl {
    Start,
    Duration,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorToggleControl {
    module: ModuleId,
    parameter: &'static str,
}

#[derive(Component, Debug, Clone, Copy)]
struct ModuleEnabledControl(ModuleId);

#[derive(Component, Debug, Clone, Copy)]
struct RendererEnabledControl(RendererId);

#[derive(Component, Debug, Clone, Copy)]
enum RendererNumberControl {
    Softness(RendererId),
    Uv(RendererId, u8),
    FlipbookFrameRate(RendererId),
}

#[derive(Component, Debug, Clone, Copy)]
enum RendererToggleControl {
    FlipbookLooping(RendererId),
    FlipbookRandomStart(RendererId),
}

#[derive(Component, Debug, Clone, Copy)]
struct PersistedScroll(ScrollMemoryKey);

#[derive(Component)]
struct LocalizedText(&'static str);

#[derive(Component)]
struct AboutDescription;

#[derive(Component)]
struct DiagnosticRow;

#[derive(Component)]
struct CompiledPlanRow;

#[derive(Debug, Clone, Copy)]
enum ProfilerMetric {
    CpuTime,
    GpuTime,
    AliveParticles,
    PeakParticles,
    ParticleCapacity,
    Emitters,
    DrawCalls,
    Dispatches,
    BufferMemory,
}

#[derive(Debug, Clone, Copy)]
enum ProfilerMetricPart {
    Value,
    Source,
}

#[derive(Component)]
struct ProfilerMetricText {
    metric: ProfilerMetric,
    part: ProfilerMetricPart,
}

#[derive(Component)]
struct ProfilerEmitterValue(usize);

#[derive(Component)]
struct ProfilerHistoryBar(usize);

#[derive(Component)]
struct ProfilerHistorySummary;

#[derive(Component)]
struct InspectorSemanticTarget {
    target: SemanticTarget,
    base_border: Color,
}

#[derive(Component, Debug, Clone, Copy)]
struct InspectorSelectionTarget(SemanticTarget);

#[derive(Resource, Default)]
struct InspectorFocus {
    target: Option<SemanticTarget>,
    wait_frames: u8,
    highlight: Option<SemanticTarget>,
    highlight_remaining: f32,
}

#[derive(Component)]
struct PanelsSubmenu;

#[derive(Component)]
struct PanelVisibilityLabel(DockPanel);

#[derive(Component)]
struct NativeFloatingWindow(DockPanel);

#[derive(Component)]
struct NativeFloatingCamera(DockPanel);

#[derive(Component)]
struct NativeFloatingUi {
    panel: DockPanel,
    window: Entity,
    camera: Entity,
}

#[derive(Component)]
struct SplitterGrip;

#[derive(Component)]
struct DockCloseButton;

#[derive(Component)]
struct DockDropHint(DockNodeId);

#[derive(Component)]
struct DockDropZone {
    node: DockNodeId,
    drop: DockDrop,
}

#[derive(Component)]
struct DockDropZoneLabel;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct DockSplitter {
    node: DockNodeId,
    axis: DockAxis,
}

#[derive(Component)]
struct DockFirstPane(DockNodeId);

#[derive(Resource, Default)]
struct DockDragState(Option<DockPanel>);

#[derive(Resource, Default)]
struct ResizeState(Option<DockSplitter>);

#[derive(Component)]
struct MenuDropdown(MenuKind);

#[derive(Component)]
struct MenuButton;

#[derive(Component)]
struct MenuSurface;

#[derive(Component)]
struct AboutOverlay;

#[derive(Component)]
struct TimelineCanvas;

#[derive(Component)]
struct CurveGraph;

struct CatalogEntry {
    name: String,
    path: PathBuf,
}

#[derive(Resource, Default)]
struct EffectCatalog {
    entries: Vec<CatalogEntry>,
}

impl EffectCatalog {
    fn scan() -> Self {
        let mut entries = fs::read_dir("assets/effects")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ron"))
            .filter_map(|path| {
                let effect = EffectAsset::load_ron(&path).ok()?;
                Some(CatalogEntry {
                    name: effect.name,
                    path,
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Self { entries }
    }
}

#[derive(Component)]
struct PreviewCanvas;

#[derive(Component)]
struct PreviewRenderCamera;

#[derive(Resource)]
struct PreviewCameraController {
    focus: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

impl Default for PreviewCameraController {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 140.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

#[derive(Component)]
struct PreviewEffectPlayer;

#[derive(Component)]
struct GizmoModeLabel;

#[derive(Component)]
struct PlaybackLabel;

#[derive(Component)]
struct TimeLabel;

#[derive(Component)]
struct InspectorTitle;

#[derive(Component)]
struct ParticleCountLabel;

#[derive(Component)]
struct CompileStatusLabel;

#[derive(Component)]
struct CompileStatusButton;

#[derive(Component)]
struct CompileStatusDot;

#[derive(Component)]
struct Playhead;

#[derive(Component)]
struct LayerRow(usize);

#[derive(Clone, Copy)]
struct PanelSources<'a> {
    session: &'a EditorSession,
    catalog: &'a EffectCatalog,
    registry: &'a EditorModuleRegistry,
    palette: &'a ModulePaletteState,
    diagnostics_panel: &'a DiagnosticsPanelState,
    profiler: &'a ProfilerState,
    settings: &'a EditorSettings,
    settings_panel: &'a SettingsPanelState,
    settings_persistence: &'a SettingsPersistence,
    localizer: &'a Localizer,
    preview_camera: Entity,
}

#[derive(SystemParam)]
struct UiBuildResources<'w, 's> {
    catalog: Res<'w, EffectCatalog>,
    layout: Res<'w, WorkspaceLayout>,
    menu: Res<'w, MenuState>,
    registry: Res<'w, EditorModuleRegistry>,
    palette: Res<'w, ModulePaletteState>,
    diagnostics_panel: Res<'w, DiagnosticsPanelState>,
    profiler: Res<'w, ProfilerState>,
    settings: Res<'w, EditorSettings>,
    settings_panel: Res<'w, SettingsPanelState>,
    settings_persistence: Res<'w, SettingsPersistence>,
    localizer: Res<'w, Localizer>,
    workspace: Res<'w, WorkspaceState>,
    preview_camera: Single<'w, 's, Entity, With<PreviewRenderCamera>>,
}

#[derive(SystemParam)]
struct SetupUiResources<'w> {
    registry: Res<'w, EditorModuleRegistry>,
    palette: Res<'w, ModulePaletteState>,
    workspace: Res<'w, WorkspaceState>,
    diagnostics_panel: Res<'w, DiagnosticsPanelState>,
    profiler: Res<'w, ProfilerState>,
    settings: Res<'w, EditorSettings>,
    settings_panel: Res<'w, SettingsPanelState>,
    settings_persistence: Res<'w, SettingsPersistence>,
    localizer: Res<'w, Localizer>,
}

#[derive(SystemParam)]
struct DockDropQueries<'w, 's> {
    zones: Query<'w, 's, &'static DockDropZone>,
    tabs: Query<'w, 's, &'static DockTab>,
    parents: Query<'w, 's, &'static ChildOf>,
}

#[derive(SystemParam)]
struct DockResizeQueries<'w, 's> {
    splitters: Query<'w, 's, &'static DockSplitter>,
    parents: Query<'w, 's, &'static ChildOf>,
    computed: Query<'w, 's, &'static ComputedNode>,
    first_panes: Query<'w, 's, (&'static DockFirstPane, &'static mut Node)>,
    colors: Query<'w, 's, &'static mut BackgroundColor, With<DockSplitter>>,
}

fn setup_editor(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    catalog: Res<EffectCatalog>,
    layout: Res<WorkspaceLayout>,
    editor_resources: SetupUiResources,
    mut rendered: ResMut<RenderedUiRevision>,
) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
        RenderLayers::layer(31),
    ));
    let preview_camera = spawn_preview_camera(&mut commands);
    spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
    let sources = PanelSources {
        session: &session,
        catalog: &catalog,
        registry: &editor_resources.registry,
        palette: &editor_resources.palette,
        diagnostics_panel: &editor_resources.diagnostics_panel,
        profiler: &editor_resources.profiler,
        settings: &editor_resources.settings,
        settings_panel: &editor_resources.settings_panel,
        settings_persistence: &editor_resources.settings_persistence,
        localizer: &editor_resources.localizer,
        preview_camera,
    };
    spawn_editor_ui(
        &mut commands,
        &menu,
        &editor_resources.workspace,
        &layout,
        sources,
    );
    rendered.0 = session.ui_revision;
}

fn spawn_preview_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            PreviewRenderCamera,
            TransformGizmoCamera,
            Camera3d::default(),
            Camera {
                order: -2,
                clear_color: ClearColorConfig::Custom(theme::VIEWPORT),
                viewport: Some(Viewport {
                    physical_size: UVec2::splat(128),
                    ..default()
                }),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 140.0).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(0),
        ))
        .id()
}

fn sync_preview_camera_viewport(
    canvas: Single<(&ComputedNode, &UiGlobalTransform), With<PreviewCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Camera, With<PreviewRenderCamera>>,
) {
    let (computed, transform) = *canvas;
    let size = computed.size();
    if !size.is_finite() || size.x < 16.0 || size.y < 16.0 {
        return;
    }
    let top_left = transform.translation.trunc() - size * 0.5;
    let target_size = UVec2::new(
        window.physical_width().max(1),
        window.physical_height().max(1),
    );
    let position = top_left.max(Vec2::ZERO).as_uvec2().min(target_size - 1);
    let available = target_size.saturating_sub(position).max(UVec2::ONE);
    let physical_size = size.as_uvec2().max(UVec2::ONE).min(available);
    camera.viewport = Some(Viewport {
        physical_position: position,
        physical_size,
        ..default()
    });
}

fn navigate_preview_camera(
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    mut controller: ResMut<PreviewCameraController>,
    mut camera: Single<&mut Transform, With<PreviewRenderCamera>>,
) {
    let cursor_over = canvas.cursor_over();
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let scroll_delta = wheel.read().fold(0.0, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.02,
        };
        sum + event.y * scale
    });
    if !cursor_over {
        return;
    }

    let mut changed = false;
    if buttons.pressed(MouseButton::Middle) && pointer_delta != Vec2::ZERO {
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            let right = camera.rotation * Vec3::X;
            let up = camera.rotation * Vec3::Y;
            let units_per_pixel = controller.distance * 0.0018;
            controller.focus += (-right * pointer_delta.x + up * pointer_delta.y) * units_per_pixel;
        } else {
            controller.yaw -= pointer_delta.x * 0.005;
            controller.pitch = (controller.pitch - pointer_delta.y * 0.005).clamp(-1.54, 1.54);
        }
        changed = true;
    }
    if scroll_delta != 0.0 {
        controller.distance =
            (controller.distance * (-scroll_delta * 0.12).exp()).clamp(1.0, 4_000.0);
        changed = true;
    }
    if !changed {
        return;
    }

    let orbit = Quat::from_rotation_y(controller.yaw) * Quat::from_rotation_x(controller.pitch);
    camera.translation = controller.focus + orbit * Vec3::Z * controller.distance;
    camera.look_at(controller.focus, Vec3::Y);
}

fn update_transform_gizmo_controls(
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut labels: Query<&mut Text, With<GizmoModeLabel>>,
) {
    if canvas.cursor_over() {
        if keys.just_pressed(KeyCode::Digit1) {
            settings.mode = TransformGizmoMode::Translate;
        }
        if keys.just_pressed(KeyCode::Digit2) {
            settings.mode = TransformGizmoMode::Rotate;
        }
        if keys.just_pressed(KeyCode::Digit3) {
            settings.mode = TransformGizmoMode::Scale;
        }
        if keys.just_pressed(KeyCode::KeyX) {
            settings.space = match settings.space {
                TransformGizmoSpace::World => TransformGizmoSpace::Local,
                TransformGizmoSpace::Local => TransformGizmoSpace::World,
            };
        }
    }
    if !settings.is_changed() {
        return;
    }
    let mode = match settings.mode {
        TransformGizmoMode::Translate => "MOVE",
        TransformGizmoMode::Rotate => "ROTATE",
        TransformGizmoMode::Scale => "SCALE",
    };
    let space = match settings.space {
        TransformGizmoSpace::World => "WORLD",
        TransformGizmoSpace::Local => "LOCAL",
    };
    for mut label in &mut labels {
        **label = format!("1 MOVE  2 ROTATE  3 SCALE  |  {mode} · {space}");
    }
}

fn draw_preview_scene_gizmos(
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    player: Single<&GlobalTransform, With<PreviewEffectPlayer>>,
    mut gizmos: Gizmos,
) {
    if menu.show_grid {
        gizmos
            .grid(
                Isometry3d::from_translation(Vec3::new(0.0, 0.0, -0.05)),
                UVec2::new(16, 10),
                Vec2::splat(20.0),
                theme::GRID.with_alpha(0.7),
            )
            .outer_edges();
    }

    let Some((_, shape)) = selected_shape_module(&session) else {
        return;
    };
    let translation = player.translation();
    let rotation = player.rotation();
    let scale = player.to_scale_rotation_translation().0;
    let axis_scale = scale.x.abs().max(scale.y.abs()).max(0.001);
    let isometry = Isometry3d::new(translation, rotation);
    let accent = theme::ACCENT.with_alpha(0.9);
    match shape {
        EmitterShape::Point => {
            gizmos.cross(isometry, 2.0 * axis_scale, accent);
        }
        EmitterShape::Circle { radius } => {
            gizmos
                .circle(isometry, radius * axis_scale, accent)
                .resolution(64);
            gizmos.line(
                player.transform_point(Vec3::ZERO),
                player.transform_point(Vec3::X * radius),
                accent,
            );
        }
        EmitterShape::Ring { radius } => {
            gizmos
                .circle(isometry, radius * axis_scale, accent)
                .resolution(64);
            gizmos
                .circle(
                    isometry,
                    radius * axis_scale * 0.92,
                    accent.with_alpha(0.45),
                )
                .resolution(64);
        }
        EmitterShape::Cone { radius, depth } => {
            let origin = player.transform_point(Vec3::ZERO);
            let left = player.transform_point(Vec3::new(-radius, depth, 0.0));
            let right = player.transform_point(Vec3::new(radius, depth, 0.0));
            gizmos.line(origin, left, accent);
            gizmos.line(origin, right, accent);
            gizmos.line(left, right, accent);
            gizmos
                .circle(
                    Isometry3d::new(player.transform_point(Vec3::Y * depth), player.rotation()),
                    radius * axis_scale,
                    accent.with_alpha(0.62),
                )
                .resolution(64);
        }
    }
}

fn configured_preview_player(session: &EditorSession) -> Option<EffectPlayer> {
    let preview = session.preview.as_ref()?;
    let mut player = EffectPlayer::from_compiled(preview.effect().clone());
    player.playing = false;
    player.speed = session.speed;
    player.set_seed(session.preview_seed);
    player.seek_frame(session.frame());
    Some(player)
}

fn spawn_preview_effect_player(
    commands: &mut Commands,
    session: &EditorSession,
    transform: Transform,
) {
    if let Some(player) = configured_preview_player(session) {
        commands.spawn((
            PreviewEffectPlayer,
            TransformGizmoFocus,
            player,
            transform,
            RenderLayers::layer(0),
        ));
    }
}

fn setup_editor_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(EditorFonts {
        mono: asset_server.load(fonts::MONO),
    });
}

/// Replaces Bevy's ASCII-only default font with Feathers' complete Fira Mono face.
/// Explicit Feathers font choices are preserved, so standard widgets can continue to use their
/// regular and bold faces while editor-native labels gain the glyphs required by localization.
fn apply_editor_fonts(fonts: Res<EditorFonts>, mut text_fonts: Query<&mut TextFont, Added<Text>>) {
    for mut text_font in &mut text_fonts {
        if matches!(
            &text_font.font,
            FontSource::Handle(handle) if handle == &Handle::<Font>::default()
        ) {
            text_font.font = fonts.mono.clone().into();
        }
    }
}

fn setup_window_cursor(mut commands: Commands, window: Single<Entity, With<PrimaryWindow>>) {
    commands.entity(*window).insert(CursorIcon::default());
}

fn spawn_editor_ui(
    commands: &mut Commands,
    menu: &MenuState,
    workspace: &WorkspaceState,
    layout: &WorkspaceLayout,
    sources: PanelSources<'_>,
) {
    commands
        .spawn(EditorRoot)
        .apply_scene(ui_shell::editor_root())
        .with_children(|root| {
            spawn_menu_bar(root, sources.session, layout, sources.localizer);
            spawn_toolbar(root, sources.session, sources.localizer);
            spawn_editor_content(root, menu, workspace, layout, sources);
            spawn_status_bar(root, sources.session, sources.localizer);
            spawn_about_overlay(root, menu.show_about, sources.localizer);
        });
}

fn spawn_tab_context_menu(
    parent: &mut ChildSpawnerCommands,
    context: Option<TabContextMenu>,
    localizer: &Localizer,
) {
    let Some(context) = context else {
        return;
    };
    parent
        .spawn((
            GlobalZIndex(180),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(context.position[0]),
                top: Val::Px(context.position[1]),
                width: Val::Px(188.0),
                padding: UiRect::all(Val::Px(5.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|menu| {
            menu.spawn_empty()
                .apply_scene(ui_shell::feathers_plain_button())
                .insert((
                    EditorAction::FloatDockPanel(context.panel, context.position),
                    FeathersActionButton,
                    AccessibleLabel(localizer.text("dock-float-panel")),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ))
                .with_children(|item| {
                    item.spawn((
                        LocalizedText("dock-float-panel"),
                        Text::new(localizer.text("dock-float-panel")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        ThemedText,
                        Pickable::IGNORE,
                    ));
                });
        });
}

fn spawn_editor_content(
    parent: &mut ChildSpawnerCommands,
    menu: &MenuState,
    workspace: &WorkspaceState,
    layout: &WorkspaceLayout,
    sources: PanelSources<'_>,
) {
    parent
        .spawn((EditorContent, RelativeCursorPosition::default()))
        .apply_scene(ui_shell::editor_content())
        .with_children(|content| {
            spawn_dock_node(content, &layout.root, workspace, sources);
            spawn_tab_context_menu(content, menu.tab_context, sources.localizer);
        });
}

fn spawn_menu_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    layout: &WorkspaceLayout,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                grid_row: GridPlacement::start(1),
                width: Val::Percent(100.0),
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
            ThemeBorderColor(tokens::PANE_HEADER_BORDER),
        ))
        .with_children(|bar| {
            spawn_standard_menu(
                bar,
                "menu-file",
                MenuKind::File,
                &[
                    ("file-new-effect", "Ctrl+N", EditorAction::NewEffect),
                    ("file-open", "Ctrl+O", EditorAction::OpenEffect),
                    ("file-save", "Ctrl+S", EditorAction::Save),
                    ("file-save-as", "Ctrl+Shift+S", EditorAction::SaveAs),
                    (
                        "file-settings",
                        "",
                        EditorAction::ShowDockPanel(DockPanel::Settings),
                    ),
                    ("file-exit", "Alt+F4", EditorAction::Exit),
                ],
                localizer,
            );
            spawn_standard_menu(
                bar,
                "menu-edit",
                MenuKind::Edit,
                &[
                    ("edit-undo", "Ctrl+Z", EditorAction::Undo),
                    ("edit-redo", "Ctrl+Y", EditorAction::Redo),
                    ("edit-add-emitter", "Ctrl+Enter", EditorAction::AddLayer),
                    (
                        "edit-duplicate-emitter",
                        "Ctrl+D",
                        EditorAction::DuplicateLayer,
                    ),
                    ("edit-delete-emitter", "Delete", EditorAction::DeleteLayer),
                ],
                localizer,
            );
            spawn_view_menu(bar, layout, localizer);
            spawn_standard_menu(
                bar,
                "menu-help",
                MenuKind::Help,
                &[("help-about", "", EditorAction::ShowAbout)],
                localizer,
            );
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let file = session
                .source_path
                .as_ref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled");
            bar.spawn((
                DocumentMenuLabel,
                Text::new(format!(
                    "{}{}  |  {}",
                    if session.dirty { "* " } else { "" },
                    session.effect.name,
                    file
                )),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_standard_menu(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    menu: MenuKind,
    items: &[(&'static str, &str, EditorAction)],
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu_root| {
            menu_button(menu_root, message_id, menu, localizer);
            spawn_dropdown(menu_root, menu, items, localizer);
        });
}

fn menu_button(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    menu: MenuKind,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_button())
        .insert((
            MenuButton,
            FeathersActionButton,
            EditorAction::ToggleMenu(menu),
            AccessibleLabel(localizer.text(message_id)),
        ))
        .with_children(|button| {
            button.spawn((
                LocalizedText(message_id),
                Text::new(localizer.text(message_id)),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_dropdown(
    parent: &mut ChildSpawnerCommands,
    menu: MenuKind,
    items: &[(&'static str, &str, EditorAction)],
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_popup())
        .insert((
            MenuDropdown(menu),
            MenuSurface,
            RelativeCursorPosition::default(),
        ))
        .with_children(|dropdown| {
            for (message_id, shortcut, action) in items {
                if matches!(
                    action,
                    EditorAction::ShowDockPanel(DockPanel::Settings) | EditorAction::Exit
                ) {
                    dropdown
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_divider());
                }
                let mut item =
                    spawn_feathers_menu_item(dropdown, message_id, shortcut, *action, localizer);
                match action {
                    EditorAction::Undo => {
                        item.insert(UndoMenuItem);
                    }
                    EditorAction::Redo => {
                        item.insert(RedoMenuItem);
                    }
                    _ => {}
                }
            }
        });
}

fn spawn_view_menu(
    parent: &mut ChildSpawnerCommands,
    layout: &WorkspaceLayout,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu_root| {
            menu_button(menu_root, "menu-view", MenuKind::View, localizer);
            menu_root
                .spawn_empty()
                .apply_scene(ui_shell::feathers_menu_popup())
                .insert((
                    MenuDropdown(MenuKind::View),
                    MenuSurface,
                    RelativeCursorPosition::default(),
                ))
                .with_children(|dropdown| {
                    for (message_id, shortcut, action) in [
                        ("view-toggle-grid", "G", EditorAction::ToggleGrid),
                        ("view-restart-preview", "R", EditorAction::Restart),
                        ("view-panels", ">", EditorAction::TogglePanelsSubmenu),
                    ] {
                        spawn_feathers_menu_item(dropdown, message_id, shortcut, action, localizer);
                    }
                    dropdown
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_divider());
                    spawn_feathers_menu_item(
                        dropdown,
                        "view-reset-workspace",
                        "",
                        EditorAction::ResetWorkspaceLayout,
                        localizer,
                    );

                    dropdown
                        .spawn((
                            PanelsSubmenu,
                            MenuSurface,
                            RelativeCursorPosition::default(),
                            GlobalZIndex(101),
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: Val::Percent(100.0),
                                top: Val::Px(63.0),
                                min_width: Val::Px(206.0),
                                padding: UiRect::axes(Val::Px(0.0), Val::Px(4.0)),
                                flex_direction: FlexDirection::Column,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            ThemeBackgroundColor(tokens::MENU_BG),
                            ThemeBorderColor(tokens::MENU_BORDER),
                        ))
                        .with_children(|submenu| {
                            for panel in DockPanel::ALL {
                                let visible = layout.is_visible(panel);
                                let mut item = submenu.spawn_empty();
                                item.apply_scene(ui_shell::feathers_menu_item()).insert((
                                    Interaction::None,
                                    EditorAction::ToggleDockPanel(panel),
                                    FeathersActionButton,
                                    AccessibleLabel(panel_visibility_label(
                                        localizer, panel, visible,
                                    )),
                                ));
                                if !panel.closable() {
                                    item.insert(InteractionDisabled);
                                }
                                item.with_children(|row| {
                                    row.spawn((
                                        PanelVisibilityLabel(panel),
                                        Text::new(panel_visibility_label(
                                            localizer, panel, visible,
                                        )),
                                        ThemedText,
                                        Pickable::IGNORE,
                                    ));
                                });
                            }
                        });
                });
        });
}

fn spawn_feathers_menu_item<'a>(
    parent: &'a mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    action: EditorAction,
    localizer: &Localizer,
) -> EntityCommands<'a> {
    let label = localizer.text(message_id);
    let mut item = parent.spawn_empty();
    item.apply_scene(ui_shell::feathers_menu_item())
        .insert((
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
        ))
        .with_children(|item| {
            item.spawn((
                LocalizedText(message_id),
                Text::new(label),
                ThemedText,
                Pickable::IGNORE,
            ));
            item.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            item.spawn((
                Text::new(shortcut),
                ThemeTextColor(tokens::TEXT_DIM),
                Pickable::IGNORE,
            ));
        });
    item
}

fn panel_visibility_label(localizer: &Localizer, panel: DockPanel, visible: bool) -> String {
    format!(
        "[{}]  {}",
        if visible { "x" } else { " " },
        localizer.text(panel.message_id())
    )
}

fn spawn_about_overlay(parent: &mut ChildSpawnerCommands, visible: bool, localizer: &Localizer) {
    parent
        .spawn((
            AboutOverlay,
            GlobalZIndex(200),
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(430.0),
                        padding: UiRect::all(Val::Px(24.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::ACCENT_DIM),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new("AESTRA"),
                        TextFont {
                            font_size: FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    let mut args = FluentArgs::new();
                    args.set("version", env!("CARGO_PKG_VERSION"));
                    dialog.spawn((
                        AboutDescription,
                        Text::new(localizer.text_with("about-description", &args)),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::justify(Justify::Center),
                    ));
                    localized_action_button(
                        dialog,
                        "common-close",
                        EditorAction::CloseAbout,
                        localizer,
                    );
                });
        });
}

fn spawn_toolbar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                grid_row: GridPlacement::start(2),
                width: Val::Percent(100.0),
                height: Val::Px(54.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(14.0)),
                column_gap: Val::Px(9.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_BODY_BG),
            ThemeBorderColor(tokens::PANE_HEADER_BORDER),
        ))
        .with_children(|bar| {
            bar.spawn((
                Text::new("AESTRA"),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Node {
                    width: Val::Px(112.0),
                    ..default()
                },
            ));
            toolbar_button(
                bar,
                "toolbar-play",
                EditorAction::TogglePlayback,
                PlaybackLabel,
                localizer,
            );
            toolbar_button(
                bar,
                "toolbar-restart",
                EditorAction::Restart,
                PlainMarker,
                localizer,
            );
            toolbar_button(
                bar,
                "toolbar-save",
                EditorAction::Save,
                PlainMarker,
                localizer,
            );
            bar.spawn((
                Node {
                    width: Val::Px(1.0),
                    height: Val::Px(26.0),
                    margin: UiRect::horizontal(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(theme::BORDER),
            ));
            bar.spawn((
                DocumentToolbarLabel,
                Text::new(format!(
                    "{}  /  {}",
                    session.effect.name.to_uppercase(),
                    localizer.text("toolbar-choreography")
                )),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            bar.spawn((
                LocalizedText("toolbar-runtime"),
                Text::new(localizer.text("toolbar-runtime")),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

#[derive(Component)]
struct PlainMarker;

fn toolbar_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    action: EditorAction,
    marker: M,
    localizer: &Localizer,
) {
    let mut button = parent.spawn_empty();
    if matches!(action, EditorAction::TogglePlayback) {
        button.apply_scene(ui_shell::feathers_primary_button());
    } else {
        button.apply_scene(ui_shell::feathers_button());
    }
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(message_id)),
        ))
        .with_children(|button| {
            button.spawn((
                LocalizedText(message_id),
                Text::new(localizer.text(message_id)),
                ThemedText,
                marker,
                Pickable::IGNORE,
            ));
        });
}

fn plain_toolbar_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: EditorAction,
    marker: M,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                height: Val::Px(32.0),
                min_width: Val::Px(78.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                ThemedText,
                marker,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_dock_node(
    parent: &mut ChildSpawnerCommands,
    node: &DockNode,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    match node {
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let direction = match axis {
                DockAxis::Horizontal => FlexDirection::Row,
                DockAxis::Vertical => FlexDirection::Column,
            };
            parent
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: direction,
                    ..default()
                })
                .with_children(|split| {
                    split
                        .spawn((
                            DockFirstPane(*id),
                            Node {
                                width: if *axis == DockAxis::Horizontal {
                                    Val::Percent(*ratio * 100.0)
                                } else {
                                    Val::Percent(100.0)
                                },
                                height: if *axis == DockAxis::Vertical {
                                    Val::Percent(*ratio * 100.0)
                                } else {
                                    Val::Percent(100.0)
                                },
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ))
                        .with_children(|pane| {
                            spawn_dock_node(pane, first, workspace, sources);
                        });
                    spawn_tree_splitter(split, *id, *axis);
                    split
                        .spawn(Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            ..default()
                        })
                        .with_children(|pane| {
                            spawn_dock_node(pane, second, workspace, sources);
                        });
                });
        }
        DockNode::Tabs { id, stack } => {
            spawn_dock_stack(parent, *id, stack, workspace, sources);
        }
    }
}

fn spawn_dock_stack(
    parent: &mut ChildSpawnerCommands,
    node: DockNodeId,
    stack: &DockStack,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    parent
        .spawn((
            DockPane(node),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
        ))
        .apply_scene(ui_shell::dock_pane())
        .with_children(|pane| {
            spawn_dock_tab_bar(pane, node, stack, sources.localizer);
            if let Some(panel) = stack.active {
                spawn_panel_content(pane, panel, workspace, sources);
            }
            spawn_dock_drop_overlay(pane, node);
        });
}

fn spawn_panel_content(
    parent: &mut ChildSpawnerCommands,
    panel: DockPanel,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    match panel {
        DockPanel::Viewport => spawn_preview(parent, sources.preview_camera, sources.session),
        DockPanel::Assets => spawn_asset_browser(parent, sources.session, sources.catalog),
        DockPanel::Inspector => {
            spawn_inspector(
                parent,
                sources.session,
                sources.registry,
                sources.palette,
                sources.localizer,
                sources.settings,
            );
        }
        DockPanel::Timeline => spawn_timeline(parent, sources.session),
        DockPanel::Curves => {
            spawn_curves_workspace(parent, sources.session, sources.registry, workspace);
        }
        DockPanel::Diagnostics => {
            spawn_diagnostics_workspace(parent, sources.session, sources.diagnostics_panel);
        }
        DockPanel::GeneratedCode => spawn_generated_code_workspace(parent, sources.session),
        DockPanel::Profiler => spawn_profiler_workspace(parent, sources.session, sources.profiler),
        DockPanel::Changes => spawn_changes_workspace(parent, sources.session),
        DockPanel::Settings => spawn_settings_workspace(
            parent,
            sources.settings,
            sources.settings_panel,
            sources.settings_persistence,
            sources.localizer,
        ),
    }
}

fn spawn_native_floating_ui(
    commands: &mut Commands,
    panel: DockPanel,
    window: Entity,
    camera: Entity,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    commands
        .spawn((
            NativeFloatingUi {
                panel,
                window,
                camera,
            },
            UiTargetCamera(camera),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|root| {
            spawn_panel_content(root, panel, workspace, sources);
        });
}

fn spawn_dock_drop_overlay(parent: &mut ChildSpawnerCommands, node: DockNodeId) {
    parent
        .spawn((
            DockDropHint(node),
            Visibility::Hidden,
            Pickable::IGNORE,
            GlobalZIndex(80),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(32.0),
                bottom: Val::Px(0.0),
                ..default()
            },
        ))
        .with_children(|overlay| {
            spawn_dock_drop_zone(overlay, node, DockDrop::Left);
            spawn_dock_drop_zone(overlay, node, DockDrop::Right);
            spawn_dock_drop_zone(overlay, node, DockDrop::Top);
            spawn_dock_drop_zone(overlay, node, DockDrop::Bottom);
        });
}

fn spawn_dock_drop_zone(parent: &mut ChildSpawnerCommands, node: DockNodeId, drop: DockDrop) {
    let (node_style, label) = match drop {
        DockDrop::Left => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Percent(25.0),
                width: Val::Percent(50.0),
                height: Val::Percent(50.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT LEFT",
        ),
        DockDrop::Right => (
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Percent(25.0),
                width: Val::Percent(50.0),
                height: Val::Percent(50.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT RIGHT",
        ),
        DockDrop::Top => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                height: Val::Percent(25.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT ABOVE",
        ),
        DockDrop::Bottom => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Percent(25.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT BELOW",
        ),
        DockDrop::Center => return,
    };
    parent
        .spawn((
            DockDropZone { node, drop },
            Interaction::None,
            RelativeCursorPosition::default(),
            Pickable::default(),
            node_style,
            BackgroundColor(theme::DOCK_TARGET_IDLE),
        ))
        .observe(dock_panel_drop)
        .with_children(|zone| {
            zone.spawn((
                DockDropZoneLabel,
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::DOCK_TARGET_TEXT_IDLE),
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::DOCK_TARGET_LABEL_IDLE),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_tree_splitter(parent: &mut ChildSpawnerCommands, node: DockNodeId, axis: DockAxis) {
    let splitter = DockSplitter { node, axis };
    let horizontal_bar = axis == DockAxis::Vertical;
    parent
        .spawn(splitter)
        .apply_scene(ui_shell::splitter(horizontal_bar))
        .observe(begin_workspace_resize)
        .observe(resize_workspace_pane)
        .observe(finish_workspace_resize)
        .observe(show_resize_cursor)
        .observe(reset_cursor)
        .with_children(|gutter| {
            gutter.spawn((
                SplitterGrip,
                Node {
                    width: if horizontal_bar {
                        Val::Px(52.0)
                    } else {
                        Val::Px(2.0)
                    },
                    height: if horizontal_bar {
                        Val::Px(2.0)
                    } else {
                        Val::Px(52.0)
                    },
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(theme::SPLITTER),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_dock_tab_bar(
    parent: &mut ChildSpawnerCommands,
    node: DockNodeId,
    stack: &DockStack,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(32.0),
                align_items: AlignItems::End,
                padding: UiRect::horizontal(Val::Px(4.0)),
                column_gap: Val::Px(2.0),
                border: UiRect::bottom(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::MENU),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|bar| {
            for panel in &stack.tabs {
                spawn_dock_tab(bar, *panel, stack.active == Some(*panel), localizer);
            }
            bar.spawn((
                DockTabAppendZone(node),
                RelativeCursorPosition::default(),
                Pickable::default(),
                Node {
                    height: Val::Percent(100.0),
                    min_width: Val::Px(28.0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::left(Val::Px(2.0)),
                    ..default()
                },
            ))
            .observe(append_dock_tab)
            .with_children(|zone| {
                zone.spawn((
                    DockTabAppendIndicator(node),
                    Visibility::Hidden,
                    Node {
                        width: Val::Px(4.0),
                        height: Val::Px(24.0),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme::DOCK_TARGET),
                    Pickable::IGNORE,
                ));
            });
        });
}

fn spawn_dock_tab(
    parent: &mut ChildSpawnerCommands,
    panel: DockPanel,
    selected: bool,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            EditorAction::SelectDockPanel(panel),
            DockTab(panel),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Node {
                height: Val::Px(28.0),
                min_width: Val::Px(92.0),
                padding: UiRect::left(Val::Px(9.0)),
                align_items: AlignItems::Center,
                border: UiRect::new(
                    Val::Px(1.0),
                    Val::Px(1.0),
                    Val::Px(1.0),
                    Val::Px(if selected { 0.0 } else { 1.0 }),
                ),
                border_radius: BorderRadius::top(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::PANEL
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(if selected {
                theme::ACCENT_DIM
            } else {
                theme::BORDER
            }),
        ))
        .observe(begin_dock_tab_drag)
        .observe(move_dock_tab)
        .observe(reset_dock_tab)
        .observe(reorder_dock_tab)
        .observe(open_dock_tab_context_menu)
        .with_children(|tab| {
            tab.spawn((
                LocalizedText(panel.message_id()),
                Text::new(localizer.text(panel.message_id())),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            tab.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if panel.closable() {
                tab.spawn((
                    Button,
                    EditorNativeControl,
                    EditorAction::CloseDockPanel(panel),
                    DockCloseButton,
                    Node {
                        width: Val::Px(24.0),
                        height: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|close| {
                    close.spawn((
                        Text::new("x"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
}

fn resize_workspace_pane(
    drag: On<Pointer<Drag>>,
    mut queries: DockResizeQueries,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let Ok(splitter) = queries.splitters.get(drag.event_target()) else {
        return;
    };
    let Ok(parent) = queries.parents.get(drag.event_target()) else {
        return;
    };
    let Ok(parent_node) = queries.computed.get(parent.parent()) else {
        return;
    };
    let scale = window.scale_factor().max(0.01);
    let (delta, span) = match splitter.axis {
        DockAxis::Horizontal => (drag.delta.x / scale, parent_node.size().x / scale),
        DockAxis::Vertical => (drag.delta.y / scale, parent_node.size().y / scale),
    };
    if !layout.resize_split(splitter.node, delta, span) {
        return;
    }
    let Some(DockNode::Split { ratio, .. }) = find_dock_node(&layout.root, splitter.node) else {
        return;
    };
    for (pane, mut node) in &mut queries.first_panes {
        if pane.0 != splitter.node {
            continue;
        }
        match splitter.axis {
            DockAxis::Horizontal => node.width = Val::Percent(*ratio * 100.0),
            DockAxis::Vertical => node.height = Val::Percent(*ratio * 100.0),
        }
    }
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = queries.colors.get_mut(drag.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn find_dock_node(node: &DockNode, target: DockNodeId) -> Option<&DockNode> {
    if node.id() == target {
        return Some(node);
    }
    match node {
        DockNode::Split { first, second, .. } => {
            find_dock_node(first, target).or_else(|| find_dock_node(second, target))
        }
        DockNode::Tabs { .. } => None,
    }
}

fn begin_workspace_resize(
    drag: On<Pointer<DragStart>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
    mut state: ResMut<ResizeState>,
) {
    let Ok(splitter) = splitters.get(drag.event_target()) else {
        return;
    };
    state.0 = Some(*splitter);
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = colors.get_mut(drag.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn finish_workspace_resize(
    drag: On<Pointer<DragEnd>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
    mut state: ResMut<ResizeState>,
    layout: Res<WorkspaceLayout>,
) {
    if !splitters.contains(drag.event_target()) {
        return;
    }
    state.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Default);
    for mut color in &mut colors {
        color.0 = theme::SPLITTER_GUTTER;
    }
    if let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}

fn resize_cursor(splitter: DockSplitter) -> CursorIcon {
    CursorIcon::System(match splitter.axis {
        DockAxis::Horizontal => SystemCursorIcon::EwResize,
        DockAxis::Vertical => SystemCursorIcon::NsResize,
    })
}

fn show_resize_cursor(
    over: On<Pointer<Over>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
) {
    let Ok(splitter) = splitters.get(over.event_target()) else {
        return;
    };
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = colors.get_mut(over.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn reset_cursor(
    out: On<Pointer<Out>>,
    splitters: Query<&DockSplitter>,
    state: Res<ResizeState>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
) {
    if splitters.contains(out.event_target()) && state.0.is_none() {
        **cursor = CursorIcon::System(SystemCursorIcon::Default);
        if let Ok(mut color) = colors.get_mut(out.event_target()) {
            color.0 = theme::SPLITTER_GUTTER;
        }
    }
}

fn select_inspector_header(
    click: On<Pointer<Click>>,
    selectable: Query<&InspectorSelectionTarget>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.event_target();
    let target = loop {
        if let Ok(target) = selectable.get(entity) {
            break Some(target.0);
        }
        let Ok(parent) = parents.get(entity) else {
            break None;
        };
        entity = parent.parent();
    };
    let Some(target) = target else {
        return;
    };
    if session.selection.primary != target {
        session.selection.primary = target;
        session.status = format!("Selected {target}");
        session.ui_revision += 1;
    }
}

fn move_dock_tab(drag: On<Pointer<Drag>>, mut tabs: Query<&mut UiTransform, With<DockTab>>) {
    if let Ok(mut transform) = tabs.get_mut(drag.event_target()) {
        transform.translation = Val2::px(drag.distance.x, drag.distance.y);
    }
}

fn begin_dock_tab_drag(
    drag: On<Pointer<DragStart>>,
    tabs: Query<&DockTab>,
    mut commands: Commands,
    mut state: ResMut<DockDragState>,
) {
    if let Ok(tab) = tabs.get(drag.event_target()) {
        state.0 = Some(tab.0);
        commands
            .entity(drag.event_target())
            .insert(GlobalZIndex(160));
    }
}

fn reset_dock_tab(
    drag: On<Pointer<DragEnd>>,
    mut tabs: Query<&mut UiTransform, With<DockTab>>,
    mut commands: Commands,
    mut state: ResMut<DockDragState>,
) {
    if let Ok(mut transform) = tabs.get_mut(drag.event_target()) {
        transform.translation = Val2::ZERO;
        commands
            .entity(drag.event_target())
            .remove::<GlobalZIndex>();
    }
    state.0 = None;
}

fn clear_finished_dock_drag(
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<DockDragState>,
    mut tabs: Query<(Entity, &mut UiTransform), With<DockTab>>,
    mut commands: Commands,
) {
    if state.0.is_none() || buttons.pressed(MouseButton::Left) {
        return;
    }
    state.0 = None;
    for (entity, mut transform) in &mut tabs {
        transform.translation = Val2::ZERO;
        commands.entity(entity).remove::<GlobalZIndex>();
    }
}

fn open_dock_tab_context_menu(
    mut click: On<Pointer<Click>>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    layout: Res<WorkspaceLayout>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let tab = loop {
        if let Ok(tab) = tabs.get(entity) {
            break tab;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if tab.0 == DockPanel::Viewport
        || layout
            .floating
            .iter()
            .any(|floating| floating.panel == tab.0)
    {
        return;
    }
    menu.open = None;
    let maximum_x = (window.width() - 188.0).max(0.0);
    let maximum_y = (window.height() - 148.0).max(0.0);
    menu.tab_context = Some(TabContextMenu {
        panel: tab.0,
        position: [
            click.pointer_location.position.x.clamp(0.0, maximum_x),
            (click.pointer_location.position.y - 84.0).clamp(0.0, maximum_y),
        ],
    });
    session.ui_revision += 1;
    click.propagate(false);
}

fn dock_panel_drop(
    mut drop: On<Pointer<DragDrop>>,
    queries: DockDropQueries,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let mut target_entity = drop.event_target();
    let zone = loop {
        if let Ok(zone) = queries.zones.get(target_entity) {
            break zone;
        }
        let Ok(parent) = queries.parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let mut tab_entity = drop.dropped;
    let tab = loop {
        if let Ok(tab) = queries.tabs.get(tab_entity) {
            break tab;
        }
        let Ok(parent) = queries.parents.get(tab_entity) else {
            return;
        };
        tab_entity = parent.parent();
    };
    drag_state.0 = None;
    if layout.dock(tab.0, zone.node, zone.drop) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!("Docked {} panel", tab.0.title().to_ascii_lowercase());
    }
    drop.propagate(false);
}

fn reorder_dock_tab(
    mut drop: On<Pointer<DragDrop>>,
    tabs: Query<(&DockTab, &RelativeCursorPosition)>,
    parents: Query<&ChildOf>,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let mut target_entity = drop.event_target();
    let (target, cursor) = loop {
        if let Ok(tab) = tabs.get(target_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let mut source_entity = drop.dropped;
    let source = loop {
        if let Ok((tab, _)) = tabs.get(source_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(source_entity) else {
            return;
        };
        source_entity = parent.parent();
    };
    if source.0 == target.0 {
        return;
    }
    let before = cursor.normalized.is_none_or(|position| position.x < 0.5);
    drag_state.0 = None;
    if layout.reorder_tab(source.0, target.0, before) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!(
            "Moved {} {} {}",
            source.0.title().to_ascii_lowercase(),
            if before { "before" } else { "after" },
            target.0.title().to_ascii_lowercase()
        );
    }
    drop.propagate(false);
}

fn append_dock_tab(
    mut drop: On<Pointer<DragDrop>>,
    append_zones: Query<&DockTabAppendZone>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(zone) = append_zones.get(drop.event_target()) else {
        return;
    };
    let mut source_entity = drop.dropped;
    let source = loop {
        if let Ok(tab) = tabs.get(source_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(source_entity) else {
            return;
        };
        source_entity = parent.parent();
    };
    drag_state.0 = None;
    if layout.dock(source.0, zone.0, DockDrop::Center) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!(
            "Moved {} to the end of the tab strip",
            source.0.title().to_ascii_lowercase()
        );
    }
    drop.propagate(false);
}

fn sync_dock_drop_hints(
    state: Res<DockDragState>,
    panes: Query<(&DockPane, &RelativeCursorPosition, &ComputedNode)>,
    mut hints: Query<(&DockDropHint, &mut Visibility)>,
) {
    let hovered = state.0.and_then(|_| {
        panes
            .iter()
            .filter(|(_, cursor, _)| cursor.cursor_over())
            .min_by(|(_, _, left), (_, _, right)| {
                left.size()
                    .element_product()
                    .total_cmp(&right.size().element_product())
            })
            .map(|(pane, _, _)| pane.0)
    });
    for (hint, mut visibility) in &mut hints {
        *visibility = if hovered == Some(hint.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::type_complexity)]
fn sync_tab_reorder_hints(
    state: Res<DockDragState>,
    layout: Res<WorkspaceLayout>,
    mut tabs: Query<(
        &DockTab,
        &RelativeCursorPosition,
        &mut Node,
        &mut BorderColor,
    )>,
) {
    for (tab, cursor, mut node, mut border) in &mut tabs {
        let base = if layout.is_active(tab.0) {
            theme::ACCENT_DIM
        } else {
            theme::BORDER
        };
        node.border.left = Val::Px(1.0);
        node.border.right = Val::Px(1.0);
        border.left = base;
        border.right = base;

        if state.0.is_none_or(|dragged| dragged == tab.0) || !cursor.cursor_over() {
            continue;
        }
        let before = cursor.normalized.is_none_or(|position| position.x < 0.5);
        if before {
            node.border.left = Val::Px(4.0);
            border.left = theme::DOCK_TARGET;
        } else {
            node.border.right = Val::Px(4.0);
            border.right = theme::DOCK_TARGET;
        }
    }
}

fn sync_tab_append_hint(
    state: Res<DockDragState>,
    zones: Query<(&DockTabAppendZone, &RelativeCursorPosition)>,
    mut indicators: Query<(&DockTabAppendIndicator, &mut Visibility)>,
) {
    for (indicator, mut visibility) in &mut indicators {
        let hovered = state.0.is_some()
            && zones
                .iter()
                .any(|(zone, cursor)| zone.0 == indicator.0 && cursor.cursor_over());
        *visibility = if hovered {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::type_complexity)]
fn update_dock_zone_style(
    state: Res<DockDragState>,
    mut zones: Query<
        (&RelativeCursorPosition, &Children, &mut BackgroundColor),
        With<DockDropZone>,
    >,
    mut labels: Query<
        (&mut TextColor, &mut BackgroundColor),
        (With<DockDropZoneLabel>, Without<DockDropZone>),
    >,
) {
    for (cursor, children, mut background) in &mut zones {
        let hovered = state.0.is_some() && cursor.cursor_over();
        background.0 = if hovered {
            theme::DOCK_TARGET_HOVER
        } else {
            theme::DOCK_TARGET_IDLE
        };
        for child in children.iter() {
            if let Ok((mut text, mut label_background)) = labels.get_mut(child) {
                text.0 = if hovered {
                    theme::TEXT
                } else {
                    theme::DOCK_TARGET_TEXT_IDLE
                };
                label_background.0 = if hovered {
                    theme::DOCK_TARGET_LABEL
                } else {
                    theme::DOCK_TARGET_LABEL_IDLE
                };
            }
        }
    }
}

fn spawn_asset_browser(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &EffectCatalog,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(
                panel,
                "CURRENT EFFECT",
                if session.dirty { "MODIFIED" } else { "SAVED" },
            );
            panel
                .spawn((
                    Node {
                        margin: UiRect::all(Val::Px(10.0)),
                        padding: UiRect::all(Val::Px(10.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        ..default()
                    },
                    BackgroundColor(theme::SELECTION),
                    BorderColor::all(theme::ACCENT_DIM),
                ))
                .with_children(|asset| {
                    asset.spawn((
                        Text::new(&session.effect.name),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                    asset.spawn((
                        Text::new(session.effect.id.to_string()),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                });

            panel_heading(
                panel,
                "PROJECT EFFECTS",
                &format!("{} FOUND", catalog.entries.len()),
            );
            for (index, entry) in catalog.entries.iter().enumerate() {
                panel
                    .spawn((
                        Button,
                        EditorNativeControl,
                        EditorAction::OpenCatalog(index),
                        Node {
                            height: Val::Px(31.0),
                            margin: UiRect::horizontal(Val::Px(8.0)),
                            padding: UiRect::horizontal(Val::Px(9.0)),
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(&entry.name),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                        ));
                    });
            }

            panel_heading(
                panel,
                "RENDER ASSETS",
                &format!("{} REGISTERED", session.effect.assets.len()),
            );
            if session.effect.assets.is_empty() {
                panel.spawn((
                    Text::new("No render assets in this effect."),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        margin: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                        ..default()
                    },
                ));
            }
            for asset in &session.effect.assets {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(format!("{:?}  {}", asset.kind, asset.name)),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        row.spawn((
                            Text::new(&asset.path),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            panel_heading(
                panel,
                "MATERIALS",
                &format!("{} REGISTERED", session.effect.materials.len()),
            );
            plain_toolbar_button(
                panel,
                "+ Add Sprite Material",
                EditorAction::AddSpriteMaterial,
                PlainMarker,
            );
            for material in &session.effect.materials {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(&material.name),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        row.spawn((
                            Text::new(format!("Sprite  ·  {:?}", material.blend)),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }
            panel_heading(
                panel,
                "FLIPBOOKS",
                &format!("{} REGISTERED", session.effect.flipbooks.len()),
            );
            plain_toolbar_button(
                panel,
                "+ Add 4×1 Flipbook",
                EditorAction::AddGridFlipbook,
                PlainMarker,
            );
            for flipbook in &session.effect.flipbooks {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(&flipbook.name),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        row.spawn((
                            Text::new(format!(
                                "Flipbook · {} frames · {:.0} FPS",
                                flipbook.frames.len(),
                                flipbook.frame_rate
                            )),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            panel_heading(
                panel,
                "LAYERS",
                &format!("{} ACTIVE", session.effect.emitters.len()),
            );
            plain_toolbar_button(panel, "+ Add Emitter", EditorAction::AddLayer, PlainMarker);
            for (index, layer) in session.effect.emitters.iter().enumerate() {
                let selected = index == session.selected_layer_index();
                panel
                    .spawn((
                        Button,
                        EditorNativeControl,
                        EditorAction::SelectLayer(index),
                        LayerRow(index),
                        Node {
                            height: Val::Px(42.0),
                            margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                            padding: UiRect::horizontal(Val::Px(9.0)),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(9.0),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme::SELECTION
                        } else {
                            theme::PANEL_DARK
                        }),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(7.0),
                                height: Val::Px(24.0),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(layer_color(index)),
                        ));
                        row.spawn((
                            Text::new(&layer.name),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(if layer.enabled {
                                theme::TEXT
                            } else {
                                theme::TEXT_FAINT
                            }),
                        ));
                    });
            }
        });
}

fn spawn_preview(
    parent: &mut ChildSpawnerCommands,
    _preview_camera: Entity,
    _session: &EditorSession,
) {
    parent
        .spawn(())
        .apply_scene(ui_shell::viewport_pane())
        .with_children(|column| {
            column
                .spawn((
                    PreviewCanvas,
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(180.0),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    RelativeCursorPosition::default(),
                ))
                .with_children(|canvas| {
                    canvas.spawn((
                        Text::new("1 MOVE  2 ROTATE  3 SCALE  |  MOVE · WORLD"),
                        GizmoModeLabel,
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    canvas.spawn((
                        Text::new("MMB ORBIT  |  SHIFT+MMB PAN  |  WHEEL DOLLY"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Node {
                            position_type: PositionType::Absolute,
                            right: Val::Px(12.0),
                            top: Val::Px(10.0),
                            ..default()
                        },
                    ));
                });
            column.spawn((
                Text::new("0 LIVE PARTICLES  |  60 FPS"),
                ParticleCountLabel,
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn selected_shape_module(session: &EditorSession) -> Option<(ModuleId, EmitterShape)> {
    if session.pending_change.is_some() {
        return None;
    }
    let SemanticTarget::Module(module_id) = session.selection.primary else {
        return None;
    };
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == module_id)?;
    match module_parameter(module, "shape") {
        Some(Value::Shape(shape)) if module.enabled => Some((module_id, shape)),
        _ => None,
    }
}

fn spawn_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
    localizer: &Localizer,
    settings: &EditorSettings,
) {
    let layer = session.selected_layer();
    let emitter_index = session.selected_layer_index();
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(panel, "MODULE STACK", "LIVE COMPILE");
            panel.spawn((
                Text::new(&layer.name),
                InspectorTitle,
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    margin: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                    ..default()
                },
            ));
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Inspector,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::bottom(Val::Px(12.0)),
                            ..default()
                        },
                        |stack| {
                    spawn_inspector_parameters(stack, session);
                    stack.spawn((
                        InspectorSemanticTarget {
                            target: SemanticTarget::Emitter(layer.id),
                            base_border: theme::BORDER_BRIGHT,
                        },
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(1.0),
                            ..default()
                        },
                    ));
                    spawn_emitter_timing_controls(stack);
                    for stage in StackStage::ALL {
                        spawn_stage_header(stack, stage);
                        if stage == StackStage::Render {
                            for (renderer_index, renderer) in layer.renderers.iter().enumerate() {
                                spawn_renderer_card(
                                    stack,
                                    renderer,
                                    &format!(
                                        "effect.emitters[{emitter_index}].renderers[{renderer_index}]"
                                    ),
                                    session,
                                    inspector_renderer_collapsed(settings, renderer),
                                );
                            }
                            spawn_stage_diagnostics(
                                stack,
                                stage,
                                &format!("effect.emitters[{emitter_index}].renderers"),
                                session,
                                registry,
                            );
                            continue;
                        }
                        let semantic = stage.semantic().expect("module stage has semantics");
                        for (module_index, module) in layer.modules.iter().enumerate() {
                            if module.stage != semantic {
                                continue;
                            }
                            spawn_module_card(
                                stack,
                                module,
                                registry.0.get(&module.module_type),
                                &format!(
                                    "effect.emitters[{emitter_index}].modules[{module_index}]"
                                ),
                                session,
                                localizer,
                                inspector_module_collapsed(settings, module),
                            );
                        }
                        spawn_stage_diagnostics(
                            stack,
                            stage,
                            &format!("effect.emitters[{emitter_index}].modules"),
                            session,
                            registry,
                        );
                    }
                        },
                    );
                });
            if palette.open {
                spawn_module_palette(panel, registry, palette);
            }
        });
}

fn inspector_module_collapsed(settings: &EditorSettings, module: &ModuleInstance) -> bool {
    let key = inspector_module_key(module);
    !settings
        .inspector
        .section_expansion
        .get(&key)
        .copied()
        .unwrap_or(!matches!(module.stage, StageKind::ParticleUpdate))
}

fn inspector_renderer_collapsed(
    settings: &EditorSettings,
    renderer: &aestra_bevy::RendererInstance,
) -> bool {
    !settings
        .inspector
        .section_expansion
        .get(&inspector_renderer_key(renderer))
        .copied()
        .unwrap_or(false)
}

fn inspector_module_key(module: &ModuleInstance) -> String {
    format!("module/{}", module.module_type.0)
}

fn inspector_renderer_key(renderer: &aestra_bevy::RendererInstance) -> String {
    match renderer.properties {
        RendererProperties::Sprite => "renderer/sprite",
        RendererProperties::Flipbook { .. } => "renderer/flipbook",
        _ => "renderer/unknown",
    }
    .into()
}

fn toggle_persisted_inspector_section(
    session: &EditorSession,
    settings: &mut EditorSettings,
    section: InspectorSection,
) -> bool {
    let (key, default_expanded) = match section {
        InspectorSection::Module(id) => {
            let Some(module) = session
                .selected_layer()
                .modules
                .iter()
                .find(|module| module.id == id)
            else {
                return false;
            };
            (
                inspector_module_key(module),
                !matches!(module.stage, StageKind::ParticleUpdate),
            )
        }
        InspectorSection::Renderer(id) => {
            let Some(renderer) = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == id)
            else {
                return false;
            };
            (inspector_renderer_key(renderer), false)
        }
    };
    let expanded = settings
        .inspector
        .section_expansion
        .get(&key)
        .copied()
        .unwrap_or(default_expanded);
    settings.inspector.section_expansion.insert(key, !expanded);
    true
}

fn spawn_emitter_timing_controls(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            InspectorHelp("Start offset and active duration for this emitter.".into()),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(29.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for (title, control) in [
                ("Start", EmitterNumberControl::Start),
                ("Duration", EmitterNumberControl::Duration),
            ] {
                row.spawn_empty().apply_scene(label(title));
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(62.0),
                    ..default()
                })
                .with_children(|input| {
                    input
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((control, AccessibleLabel(format!("{title} in seconds"))));
                });
                row.spawn_empty().apply_scene(label_dim("s"));
            }
        });
}

fn spawn_stage_header(parent: &mut ChildSpawnerCommands, stage: StackStage) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                margin: UiRect::top(Val::Px(3.0)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(stage.title()),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            mini_button(row, "+", EditorAction::OpenModulePalette(stage));
        });
}

fn spawn_inspector_parameters(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    if session.effect.parameters.is_empty() {
        return;
    }
    parent.spawn((
        Text::new("EFFECT PARAMETERS"),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
    ));
    for parameter in &session.effect.parameters {
        parent
            .spawn((
                InspectorSemanticTarget {
                    target: SemanticTarget::Parameter(parameter.id),
                    base_border: theme::BORDER_BRIGHT,
                },
                Node {
                    width: Val::Auto,
                    min_height: Val::Px(34.0),
                    margin: UiRect::axes(Val::Px(9.0), Val::Px(3.0)),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_LIGHT),
                BorderColor::all(theme::BORDER_BRIGHT),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(&parameter.name),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(format_value(parameter.default.clone())),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
            });
    }
}

fn spawn_module_card(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    collapsed: bool,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let help = metadata.map_or(
        "This module is not available in the current registry.",
        |item| item.description,
    );
    let base_border = if session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(diagnostic_path))
    {
        Color::srgb(0.82, 0.28, 0.24)
    } else if session.selection.primary == SemanticTarget::Module(module.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    parent
        .spawn((
            InspectorSemanticTarget {
                target: SemanticTarget::Module(module.id),
                base_border,
            },
            Node {
                width: Val::Auto,
                margin: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(if collapsed { 3.0 } else { 5.0 })),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(if collapsed { 0.0 } else { 2.0 }),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if module.enabled {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(base_border),
        ))
        .with_children(|card| {
            card.spawn((
                InspectorSelectionTarget(SemanticTarget::Module(module.id)),
                Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                },
            ))
            .with_children(|header| {
                spawn_inspector_disclosure(
                    header,
                    InspectorSection::Module(module.id),
                    collapsed,
                    display_name,
                    module.enabled,
                    Some(help),
                );
                let mut enabled = header.spawn_empty();
                enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                    ModuleEnabledControl(module.id),
                    AccessibleLabel(format!("Enable {display_name}")),
                ));
                if module.enabled {
                    enabled.insert(Checked);
                }
                spawn_action_menu(
                    header,
                    &format!("{display_name} actions"),
                    &[
                        ComboOption {
                            label: "Move up".into(),
                            selected: false,
                            action: EditorAction::MoveModule(module.id, -1),
                        },
                        ComboOption {
                            label: "Move down".into(),
                            selected: false,
                            action: EditorAction::MoveModule(module.id, 1),
                        },
                        ComboOption {
                            label: "Duplicate".into(),
                            selected: false,
                            action: EditorAction::DuplicateModule(module.id),
                        },
                        ComboOption {
                            label: "Delete…".into(),
                            selected: false,
                            action: EditorAction::DeleteModule(module.id),
                        },
                    ],
                );
            });
            if collapsed {
                return;
            }
            if let Some(metadata) = metadata {
                for (input_index, input) in metadata.inputs.iter().enumerate() {
                    spawn_input_control(card, module, input, input_index as u8, localizer);
                }
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        });
}

fn spawn_inspector_disclosure(
    parent: &mut ChildSpawnerCommands,
    section: InspectorSection,
    collapsed: bool,
    title: &str,
    enabled: bool,
    help: Option<&str>,
) {
    let mut disclosure = parent.spawn_empty();
    disclosure
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            EditorAction::ToggleInspectorSection(section),
            FeathersActionButton,
            AccessibleLabel(format!(
                "{} {title}",
                if collapsed { "Expand" } else { "Collapse" }
            )),
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                height: Val::Px(26.0),
                padding: UiRect::horizontal(Val::Px(2.0)),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ));
    if let Some(help) = help {
        disclosure.insert((
            InspectorHelp(help.to_owned()),
            RelativeCursorPosition::default(),
        ));
    }
    disclosure.with_children(|button| {
        button
            .spawn_empty()
            .apply_scene(icon(if collapsed {
                icons::CHEVRON_RIGHT
            } else {
                icons::CHEVRON_DOWN
            }))
            .insert(Pickable::IGNORE);
        button.spawn((
            Text::new(title),
            ThemedText,
            TextColor(if enabled {
                theme::TEXT
            } else {
                theme::TEXT_FAINT
            }),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    });
}

fn spawn_input_control(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    localizer: &Localizer,
) {
    let display_name = localized_inspector_input(localizer, input.name, input.display_name, false);
    let description = localized_inspector_input(localizer, input.name, input.description, true);
    let Some(value) = module_parameter(module, input.name) else {
        spawn_inspector_read_only_control(parent, &display_name, "Missing authored value");
        return;
    };
    match (&input.control, value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => inspector_action_button(
            parent,
            &format!("{}  ·  {} keys  →", display_name, curve.keys.len()),
            EditorAction::EditComplexInput(module.id, input_index),
            Some(&description),
        ),
        (InputControl::Gradient, Value::Gradient(gradient)) => inspector_action_button(
            parent,
            &format!("{}  ·  {} color keys  →", display_name, gradient.keys.len()),
            EditorAction::EditComplexInput(module.id, input_index),
            Some(&description),
        ),
        (InputControl::Toggle, Value::Bool(value)) => {
            spawn_inspector_toggle_control(
                parent,
                module.id,
                input,
                &display_name,
                &description,
                value,
            );
        }
        (InputControl::Number { .. }, Value::U32(_)) => {
            spawn_inspector_integer_control(parent, module.id, input, &display_name, &description);
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Scalar,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("", value, 0)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1), ("Z", value[2], 2)],
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[
                    ("X", value[0], 0),
                    ("Y", value[1], 1),
                    ("Z", value[2], 2),
                    ("W", value[3], 3),
                ],
            );
        }
        (InputControl::Range { step, min, max }, Value::Range(value)) => {
            spawn_inspector_number_controls(
                parent,
                input,
                &display_name,
                &description,
                InspectorNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: InspectorNumberKind::Range,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("MIN", value.min, 0), ("MAX", value.max, 1)],
            );
        }
        (InputControl::Choice, value) => spawn_inspector_choice_control(
            parent,
            module.id,
            input_index,
            &display_name,
            &description,
            &value,
        ),
        (_, value) => spawn_inspector_read_only_control(
            parent,
            &display_name,
            &format!("{}{}", format_value(value), unit_suffix(input)),
        ),
    }
}

fn localized_inspector_input(
    localizer: &Localizer,
    input: &str,
    fallback: &str,
    description: bool,
) -> String {
    let message = match (input, description) {
        ("spawn_rate", false) => "inspector-input-spawn-rate",
        ("spawn_rate", true) => "inspector-input-spawn-rate-description",
        ("burst_count", false) => "inspector-input-burst-count",
        ("burst_count", true) => "inspector-input-burst-count-description",
        ("shape", false) => "inspector-input-shape",
        ("shape", true) => "inspector-input-shape-description",
        ("lifetime", false) => "inspector-input-lifetime",
        ("lifetime", true) => "inspector-input-lifetime-description",
        ("speed", false) => "inspector-input-speed",
        ("speed", true) => "inspector-input-speed-description",
        ("direction_degrees", false) => "inspector-input-direction",
        ("direction_degrees", true) => "inspector-input-direction-description",
        ("spread_degrees", false) => "inspector-input-spread",
        ("spread_degrees", true) => "inspector-input-spread-description",
        ("angular_velocity", false) => "inspector-input-angular-velocity",
        ("angular_velocity", true) => "inspector-input-angular-velocity-description",
        ("gravity", false) => "inspector-input-gravity",
        ("gravity", true) => "inspector-input-gravity-description",
        ("drag", false) => "inspector-input-drag",
        ("drag", true) => "inspector-input-drag-description",
        ("turbulence", false) => "inspector-input-turbulence",
        ("turbulence", true) => "inspector-input-turbulence-description",
        ("size", false) => "inspector-input-size-over-life",
        ("size", true) => "inspector-input-size-over-life-description",
        ("opacity", false) => "inspector-input-opacity-over-life",
        ("opacity", true) => "inspector-input-opacity-over-life-description",
        ("color", false) => "inspector-input-color-over-life",
        ("color", true) => "inspector-input-color-over-life-description",
        _ => return fallback.to_owned(),
    };
    localizer.text(message)
}

fn spawn_inspector_integer_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    description: &str,
) {
    let InputControl::Number { step, min, max } = input.control else {
        return;
    };
    parent
        .spawn((
            InspectorHelp(description.to_owned()),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|container| {
                container
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_integer_input())
                    .insert((
                        InspectorNumberControl {
                            module,
                            parameter: input.name,
                            component: 0,
                            kind: InspectorNumberKind::U32,
                            step,
                            min,
                            max,
                        },
                        AccessibleLabel(title.to_owned()),
                    ));
            });
            if let Some(unit) = input.unit {
                row.spawn_empty().apply_scene(label_dim(unit));
            }
        });
}

fn spawn_inspector_number_controls(
    parent: &mut ChildSpawnerCommands,
    input: &InputMetadata,
    title: &str,
    description: &str,
    control: InspectorNumberControl,
    values: &[(&'static str, f32, u8)],
) {
    parent
        .spawn((
            InspectorHelp(description.to_owned()),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|controls| {
                for (axis, _value, component) in values {
                    let sigil = match *axis {
                        "X" => tokens::TEXT_INPUT_X_AXIS,
                        "Y" => tokens::TEXT_INPUT_Y_AXIS,
                        "Z" => tokens::TEXT_INPUT_Z_AXIS,
                        _ => tokens::TEXT_INPUT_BG,
                    };
                    controls
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            min_width: Val::Px(44.0),
                            ..default()
                        })
                        .with_children(|wrapper| {
                            let mut input_entity = wrapper.spawn_empty();
                            if axis.is_empty() {
                                input_entity.apply_scene(ui_shell::feathers_scalar_input());
                            } else {
                                input_entity.apply_scene(ui_shell::feathers_labeled_scalar_input(
                                    axis, sigil,
                                ));
                            }
                            input_entity.insert((
                                InspectorNumberControl {
                                    component: *component,
                                    ..control
                                },
                                AccessibleLabel(if axis.is_empty() {
                                    title.to_owned()
                                } else {
                                    format!("{title} {axis}")
                                }),
                            ));
                        });
                }
            });
            if let Some(unit) = input.unit {
                row.spawn_empty().apply_scene(label_dim(unit));
            }
        });
}

fn spawn_inspector_property_label(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        ThemedText,
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        Node {
            width: Val::Percent(36.0),
            min_width: Val::Px(82.0),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn spawn_inspector_toggle_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: &InputMetadata,
    title: &str,
    description: &str,
    value: bool,
) {
    parent
        .spawn((
            InspectorHelp(description.to_owned()),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                InspectorToggleControl {
                    module,
                    parameter: input.name,
                },
                AccessibleLabel(title.to_owned()),
            ));
            if value {
                checkbox.insert(Checked);
            }
        });
}

fn spawn_inspector_choice_control(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    title: &str,
    description: &str,
    value: &Value,
) {
    let Value::Shape(shape) = value else {
        spawn_inspector_read_only_control(parent, title, &format_value(value.clone()));
        return;
    };
    let current = shape_label(*shape);
    let selected = shape_index(*shape);
    let options = ["Point", "Circle", "Ring", "Cone"]
        .into_iter()
        .enumerate()
        .map(|(choice, label)| ComboOption {
            label: label.to_owned(),
            selected: choice == selected,
            action: EditorAction::SetModuleChoice {
                module,
                input,
                choice: choice as u8,
            },
        })
        .collect::<Vec<_>>();
    spawn_inspector_combo_row(parent, title, current, &options, Some(description));
}

fn spawn_inspector_combo_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    current: &str,
    options: &[ComboOption],
    description: Option<&str>,
) {
    let mut row = parent.spawn(Node {
        width: Val::Percent(100.0),
        min_height: Val::Px(27.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        ..default()
    });
    if let Some(description) = description {
        row.insert((
            InspectorHelp(description.to_owned()),
            RelativeCursorPosition::default(),
        ));
    }
    row.with_children(|row| {
        spawn_inspector_property_label(row, title);
        row.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        spawn_combo_control(row, current, title, options, 150.0);
    });
}

fn spawn_renderer_scalar_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    unit: Option<&str>,
    control: RendererNumberControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn(Node {
                width: Val::Px(112.0),
                ..default()
            })
            .with_children(|input| {
                input
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert((control, AccessibleLabel(title.to_owned())));
            });
            if let Some(unit) = unit {
                row.spawn_empty().apply_scene(label_dim(unit.to_owned()));
            }
        });
}

fn spawn_renderer_toggle_control(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    enabled: bool,
    control: RendererToggleControl,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            let mut checkbox = row.spawn_empty();
            checkbox
                .apply_scene(ui_shell::feathers_checkbox())
                .insert((control, AccessibleLabel(title.to_owned())));
            if enabled {
                checkbox.insert(Checked);
            }
        });
}

fn shape_index(shape: EmitterShape) -> usize {
    match shape {
        EmitterShape::Point => 0,
        EmitterShape::Circle { .. } => 1,
        EmitterShape::Ring { .. } => 2,
        EmitterShape::Cone { .. } => 3,
    }
}

fn shape_label(shape: EmitterShape) -> &'static str {
    match shape {
        EmitterShape::Point => "Point",
        EmitterShape::Circle { .. } => "Circle",
        EmitterShape::Ring { .. } => "Ring",
        EmitterShape::Cone { .. } => "Cone",
    }
}

fn spawn_inspector_read_only_control(parent: &mut ChildSpawnerCommands, title: &str, value: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(27.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_inspector_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn_empty().apply_scene(label_dim(value.to_owned()));
        });
}

fn unit_suffix(input: &InputMetadata) -> String {
    input
        .unit
        .map_or_else(String::new, |unit| format!(" {unit}"))
}

fn spawn_renderer_card(
    parent: &mut ChildSpawnerCommands,
    renderer: &aestra_bevy::RendererInstance,
    diagnostic_path: &str,
    session: &EditorSession,
    collapsed: bool,
) {
    let display_name = match renderer.properties {
        RendererProperties::Sprite => "Sprite Renderer",
        RendererProperties::Flipbook { .. } => "Flipbook Renderer",
        _ => "Renderer",
    };
    let base_border = if session.selection.primary == SemanticTarget::Renderer(renderer.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    parent
        .spawn((
            InspectorSemanticTarget {
                target: SemanticTarget::Renderer(renderer.id),
                base_border,
            },
            Node {
                width: Val::Auto,
                margin: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(if collapsed { 3.0 } else { 5.0 })),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(if collapsed { 0.0 } else { 2.0 }),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(base_border),
        ))
        .with_children(|card| {
            card.spawn((
                InspectorSelectionTarget(SemanticTarget::Renderer(renderer.id)),
                Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                },
            ))
            .with_children(|header| {
                spawn_inspector_disclosure(
                    header,
                    InspectorSection::Renderer(renderer.id),
                    collapsed,
                    display_name,
                    renderer.enabled,
                    Some("Controls how this emitter is drawn."),
                );
                let mut enabled = header.spawn_empty();
                enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                    RendererEnabledControl(renderer.id),
                    AccessibleLabel("Enable renderer".into()),
                ));
                if renderer.enabled {
                    enabled.insert(Checked);
                }
                spawn_action_menu(
                    header,
                    "Renderer actions",
                    &[
                        ComboOption {
                            label: "Duplicate".into(),
                            selected: false,
                            action: EditorAction::DuplicateRenderer(renderer.id),
                        },
                        ComboOption {
                            label: "Delete…".into(),
                            selected: false,
                            action: EditorAction::DeleteRenderer(renderer.id),
                        },
                    ],
                );
            });
            if collapsed {
                return;
            }
            let Some(material) = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)
            else {
                spawn_inspector_read_only_control(card, "Material", "Missing");
                spawn_inline_diagnostics(card, diagnostic_path, session);
                return;
            };
            let material_options = session
                .effect
                .materials
                .iter()
                .enumerate()
                .map(|(index, candidate)| ComboOption {
                    label: candidate.name.clone(),
                    selected: candidate.id == material.id,
                    action: EditorAction::SetRendererMaterial(renderer.id, index),
                })
                .collect::<Vec<_>>();
            spawn_inspector_combo_row(card, "Material", &material.name, &material_options, None);
            let blend_options = [BlendMode::Alpha, BlendMode::Additive, BlendMode::Multiply]
                .into_iter()
                .map(|blend| ComboOption {
                    label: format!("{blend:?}"),
                    selected: blend == material.blend,
                    action: EditorAction::SetRendererBlend(renderer.id, blend),
                })
                .collect::<Vec<_>>();
            spawn_inspector_combo_row(
                card,
                "Blend",
                &format!("{:?}", material.blend),
                &blend_options,
                None,
            );
            let MaterialProperties::Sprite {
                softness, texture, ..
            } = &material.properties;
            match softness {
                MaterialInput::Constant(_) => spawn_renderer_scalar_control(
                    card,
                    "Softness",
                    None,
                    RendererNumberControl::Softness(renderer.id),
                ),
                MaterialInput::Parameter(parameter) => spawn_inspector_read_only_control(
                    card,
                    "Softness",
                    &format!("Parameter {parameter}"),
                ),
            }
            match &renderer.properties {
                RendererProperties::Sprite => {
                    let texture_name = texture
                        .and_then(|id| session.effect.assets.iter().find(|asset| asset.id == id))
                        .map_or("Procedural", |asset| asset.name.as_str());
                    let mut texture_options = vec![ComboOption {
                        label: "Procedural".into(),
                        selected: texture.is_none(),
                        action: EditorAction::SetRendererTexture(renderer.id, None),
                    }];
                    texture_options.extend(
                        session
                            .effect
                            .assets
                            .iter()
                            .enumerate()
                            .filter(|(_, asset)| asset.kind == aestra_bevy::AssetKind::Texture)
                            .map(|(index, asset)| ComboOption {
                                label: asset.name.clone(),
                                selected: Some(asset.id) == *texture,
                                action: EditorAction::SetRendererTexture(renderer.id, Some(index)),
                            }),
                    );
                    spawn_inspector_combo_row(
                        card,
                        "Texture",
                        texture_name,
                        &texture_options,
                        None,
                    );
                    if texture.is_some() {
                        for (label, component) in [
                            ("UV Min X", 0),
                            ("UV Min Y", 1),
                            ("UV Max X", 2),
                            ("UV Max Y", 3),
                        ] {
                            spawn_renderer_scalar_control(
                                card,
                                label,
                                None,
                                RendererNumberControl::Uv(renderer.id, component),
                            );
                        }
                    }
                }
                RendererProperties::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => {
                    let definition = session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|item| item.id == *flipbook);
                    let flipbook_options = session
                        .effect
                        .flipbooks
                        .iter()
                        .enumerate()
                        .map(|(index, candidate)| ComboOption {
                            label: candidate.name.clone(),
                            selected: candidate.id == *flipbook,
                            action: EditorAction::SetRendererFlipbook(renderer.id, index),
                        })
                        .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Flipbook",
                        definition.map_or("Missing", |item| item.name.as_str()),
                        &flipbook_options,
                        None,
                    );
                    if let Some(definition) = definition {
                        spawn_renderer_scalar_control(
                            card,
                            "Frame Rate",
                            Some("FPS"),
                            RendererNumberControl::FlipbookFrameRate(renderer.id),
                        );
                        spawn_renderer_toggle_control(
                            card,
                            "Looping",
                            definition.looping,
                            RendererToggleControl::FlipbookLooping(renderer.id),
                        );
                    }
                    let time_source_options = [
                        FlipbookTimeSource::ParticleAge,
                        FlipbookTimeSource::EffectTime,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *time_source,
                        action: EditorAction::SetFlipbookTimeSource(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Time Source",
                        &format!("{time_source:?}"),
                        &time_source_options,
                        None,
                    );
                    let playback_options = [
                        FlipbookPlaybackMode::Forward,
                        FlipbookPlaybackMode::Reverse,
                        FlipbookPlaybackMode::PingPong,
                    ]
                    .into_iter()
                    .map(|candidate| ComboOption {
                        label: format!("{candidate:?}"),
                        selected: candidate == *playback,
                        action: EditorAction::SetFlipbookPlayback(renderer.id, candidate),
                    })
                    .collect::<Vec<_>>();
                    spawn_inspector_combo_row(
                        card,
                        "Playback",
                        &format!("{playback:?}"),
                        &playback_options,
                        None,
                    );
                    spawn_renderer_toggle_control(
                        card,
                        "Random Start",
                        *random_start,
                        RendererToggleControl::FlipbookRandomStart(renderer.id),
                    );
                }
                _ => {}
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        });
}

fn spawn_inline_diagnostics(
    parent: &mut ChildSpawnerCommands,
    path: &str,
    session: &EditorSession,
) {
    for diagnostic in session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path.starts_with(path))
    {
        let color = match diagnostic.severity {
            DiagnosticSeverity::Error => Color::srgb(1.0, 0.38, 0.32),
            DiagnosticSeverity::Warning => Color::srgb(1.0, 0.72, 0.28),
            DiagnosticSeverity::Info => theme::TEXT_MUTED,
        };
        parent.spawn((
            Text::new(format!("{:?}: {}", diagnostic.code, diagnostic.message)),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(color),
        ));
    }
}

fn spawn_stage_diagnostics(
    parent: &mut ChildSpawnerCommands,
    stage: StackStage,
    path: &str,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
) {
    for diagnostic in session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path == path)
        .filter(|diagnostic| {
            if stage == StackStage::Render {
                return true;
            }
            if diagnostic.code != DiagnosticCode::MissingModule {
                return stage == StackStage::EmitterUpdate;
            }
            registry.0.iter().any(|metadata| {
                metadata.stages.contains(
                    &stage
                        .semantic()
                        .expect("non-render stages have semantic stages"),
                ) && diagnostic.message.contains(&metadata.type_id.0)
            })
        })
    {
        parent.spawn((
            Text::new(format!("⚠ {}", diagnostic.message)),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.38, 0.32)),
            Node {
                margin: UiRect::horizontal(Val::Px(10.0)),
                ..default()
            },
        ));
    }
}

fn spawn_module_palette(
    parent: &mut ChildSpawnerCommands,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
) {
    parent
        .spawn((
            GlobalZIndex(120),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.0),
                top: Val::Px(76.0),
                width: Val::Px(360.0),
                max_height: Val::Px(430.0),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::ACCENT_DIM),
        ))
        .with_children(|popup| {
            popup
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header.spawn((
                        Text::new(format!("ADD TO {}", palette.stage.title())),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    stack_button(header, "×", EditorAction::CloseModulePalette, 28.0);
                });
            popup.spawn((
                Text::new(format!(
                    "Search: {}▏",
                    if palette.query.is_empty() {
                        "type to filter"
                    } else {
                        &palette.query
                    }
                )),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(32.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
                BorderColor::all(theme::BORDER_BRIGHT),
            ));
            let query = palette.query.to_lowercase();
            let mut results = 0;
            if palette.stage == StackStage::Render
                && (query.is_empty() || "sprite renderer render".contains(&query))
            {
                palette_result(
                    popup,
                    "Sprite Renderer",
                    "Render · translucent particle sprites",
                    EditorAction::AddSpriteRenderer,
                );
                results += 1;
            }
            if palette.stage == StackStage::Render
                && (query.is_empty() || "flipbook renderer animation render".contains(&query))
            {
                palette_result(
                    popup,
                    "Flipbook Renderer",
                    "Render · animated imported sprite sheet",
                    EditorAction::AddFlipbookRenderer,
                );
                results += 1;
            }
            for (index, metadata) in registry.0.iter().enumerate() {
                let Some(stage) = palette.stage.semantic() else {
                    continue;
                };
                if !metadata.stages.contains(&stage) || !module_matches(metadata, &query) {
                    continue;
                }
                palette_result(
                    popup,
                    metadata.display_name,
                    &format!("{} · {}", metadata.category, metadata.description),
                    EditorAction::AddModule(index),
                );
                results += 1;
            }
            if results == 0 {
                popup.spawn((
                    Text::new("No modules match this stage and search."),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

fn palette_result(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    subtitle: &str,
    action: EditorAction,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(title.to_owned()),
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: Val::Px(2.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
            button.spawn((
                Text::new(subtitle),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                ThemeTextColor(tokens::TEXT_DIM),
                Pickable::IGNORE,
            ));
        });
}

fn stack_button(parent: &mut ChildSpawnerCommands, label: &str, action: EditorAction, width: f32) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                width: Val::Px(width),
                height: Val::Px(21.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

fn module_matches(metadata: &ModuleMetadata, query: &str) -> bool {
    query.is_empty()
        || metadata.display_name.to_lowercase().contains(query)
        || metadata.category.to_lowercase().contains(query)
        || metadata.type_id.0.to_lowercase().contains(query)
        || metadata.tags.iter().any(|tag| tag.contains(query))
}

fn module_parameter(module: &ModuleInstance, name: &str) -> Option<Value> {
    match (&module.parameters, name) {
        (
            ModuleParameters::Emission {
                spawn_rate,
                burst_count: _,
            },
            "spawn_rate",
        ) => Some(Value::Scalar(*spawn_rate)),
        (ModuleParameters::Emission { burst_count, .. }, "burst_count") => {
            Some(Value::U32(*burst_count))
        }
        (ModuleParameters::Shape { shape }, "shape") => Some(Value::Shape(*shape)),
        (ModuleParameters::Initialize { lifetime, .. }, "lifetime") => {
            Some(Value::Range(*lifetime))
        }
        (ModuleParameters::Initialize { speed, .. }, "speed") => Some(Value::Range(*speed)),
        (
            ModuleParameters::Initialize {
                direction_degrees, ..
            },
            "direction_degrees",
        ) => Some(Value::Scalar(*direction_degrees)),
        (ModuleParameters::Initialize { spread_degrees, .. }, "spread_degrees") => {
            Some(Value::Scalar(*spread_degrees))
        }
        (
            ModuleParameters::Initialize {
                angular_velocity, ..
            },
            "angular_velocity",
        ) => Some(Value::Range(*angular_velocity)),
        (ModuleParameters::Motion { gravity, .. }, "gravity") => Some(Value::Vec2(*gravity)),
        (ModuleParameters::Motion { drag, .. }, "drag") => Some(Value::Scalar(*drag)),
        (ModuleParameters::Motion { turbulence, .. }, "turbulence") => {
            Some(Value::Scalar(*turbulence))
        }
        (ModuleParameters::Appearance { size, .. }, "size") => Some(Value::Curve(size.clone())),
        (ModuleParameters::Appearance { opacity, .. }, "opacity") => {
            Some(Value::Curve(opacity.clone()))
        }
        (ModuleParameters::Appearance { color, .. }, "color") => {
            Some(Value::Gradient(color.clone()))
        }
        (ModuleParameters::Custom(values), name) => values.get(name).cloned(),
        _ => None,
    }
}

fn format_value(value: Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::U32(value) => value.to_string(),
        Value::Scalar(value) => format!("{value:.2}"),
        Value::Vec2(value) => format!("[{:.1}, {:.1}]", value[0], value[1]),
        Value::Vec3(value) => format!("[{:.1}, {:.1}, {:.1}]", value[0], value[1], value[2]),
        Value::Vec4(value) => format!(
            "[{:.1}, {:.1}, {:.1}, {:.1}]",
            value[0], value[1], value[2], value[3]
        ),
        Value::Text(value) => value,
        Value::Range(value) => format!("{:.2} – {:.2}", value.min, value.max),
        Value::Curve(value) => format!("Curve · {} keys", value.keys.len()),
        Value::Gradient(value) => format!("Gradient · {} keys", value.keys.len()),
        Value::Shape(EmitterShape::Point) => "Point".into(),
        Value::Shape(EmitterShape::Circle { radius }) => format!("Circle · r {radius:.1}"),
        Value::Shape(EmitterShape::Ring { radius }) => format!("Ring · r {radius:.1}"),
        Value::Shape(EmitterShape::Cone { radius, depth }) => {
            format!("Cone · r {radius:.1} d {depth:.1}")
        }
        Value::Parameter(id) => format!("Parameter {id}"),
        Value::Asset(id) => format!("Asset {id}"),
        Value::Material(id) => format!("Material {id}"),
    }
}

fn spawn_timeline(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|timeline| {
            timeline
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("00:00.000  /  00:02.800"),
                        TimeLabel,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    mini_button(header, "<", EditorAction::StepFrame(-1));
                    mini_button(header, ">", EditorAction::StepFrame(1));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(format!(
                            "{} Hz  ·  Seed {:016x}",
                            session.clock.tick_rate(),
                            session.preview_seed
                        )),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    mini_button(header, "-", EditorAction::AdjustPreviewSeed(-1));
                    mini_button(header, "+", EditorAction::AdjustPreviewSeed(1));
                    header.spawn((
                        Text::new(format!("Duration {:.2}s", session.playback_duration())),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    mini_button(header, "-", EditorAction::EffectDuration(-0.25));
                    mini_button(header, "+", EditorAction::EffectDuration(0.25));
                });
            timeline
                .spawn(Node {
                    flex_grow: 1.0,
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Px(224.0),
                            height: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::top(Val::Px(25.0)),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|labels| {
                        for (index, layer) in session.effect.emitters.iter().enumerate() {
                            labels.spawn((
                                Text::new(format!("  {:02}   {}", index + 1, layer.name)),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT_MUTED),
                                Node {
                                    height: Val::Px(31.0),
                                    padding: UiRect::top(Val::Px(8.0)),
                                    ..default()
                                },
                            ));
                        }
                    });
                    body.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineCanvas,
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            position_type: PositionType::Relative,
                            padding: UiRect::top(Val::Px(25.0)),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(theme::TIMELINE_BG),
                    ))
                    .with_children(|tracks| {
                        spawn_ruler(tracks, session.playback_duration());
                        for (index, layer) in session.effect.emitters.iter().enumerate() {
                            tracks
                                .spawn(Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(31.0),
                                    position_type: PositionType::Relative,
                                    border: UiRect::bottom(Val::Px(1.0)),
                                    ..default()
                                })
                                .with_children(|track| {
                                    let duration = session.playback_duration();
                                    let start = layer.start_time / duration * 100.0;
                                    let width = layer.duration / duration * 100.0;
                                    track.spawn((
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: Val::Percent(start),
                                            top: Val::Px(5.0),
                                            width: Val::Percent(width.min(100.0 - start)),
                                            height: Val::Px(21.0),
                                            border_radius: BorderRadius::all(Val::Px(3.0)),
                                            border: UiRect::all(Val::Px(1.0)),
                                            ..default()
                                        },
                                        BackgroundColor(layer_color_alpha(index, 0.28)),
                                        BorderColor::all(layer_color(index)),
                                    ));
                                });
                        }
                        tracks.spawn((
                            Playhead,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::PLAYHEAD),
                        ));
                    });
                });
        });
}

fn spawn_curves_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &WorkspaceState,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|workspace_panel| {
            workspace_panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new("NORMALIZED LIFETIME  ·  DRAG KEYS TO EDIT"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                });
            workspace_panel
                .spawn(Node {
                    flex_grow: 1.0,
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    spawn_complex_input_list(body, session, registry, workspace);
                    let Some(selection) = workspace.complex else {
                        body.spawn((
                            Text::new("Choose a curve or gradient from the list."),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                            Node {
                                margin: UiRect::all(Val::Px(28.0)),
                                ..default()
                            },
                        ));
                        return;
                    };
                    let Some((module, input, value)) =
                        resolve_complex_input(session, registry, selection)
                    else {
                        body.spawn((
                            Text::new("The selected property no longer exists."),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(Color::srgb(1.0, 0.38, 0.32)),
                            Node {
                                margin: UiRect::all(Val::Px(28.0)),
                                ..default()
                            },
                        ));
                        return;
                    };
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|editor| match value {
                        Value::Curve(curve) => spawn_curve_graph(
                            editor,
                            module.id,
                            selection.input,
                            input,
                            &curve,
                            selection.key,
                        ),
                        Value::Gradient(gradient) => spawn_gradient_graph(
                            editor,
                            module.id,
                            selection.input,
                            input,
                            &gradient,
                            selection.key,
                        ),
                        _ => {}
                    });
                });
        });
}

fn spawn_generated_code_workspace(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    let compiled = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().as_ref());
    let (state_label, state_color) = generated_code_status(session, compiled.is_some());

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(9.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("COMPILED PLAN"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(state_color),
                    ));
                    header.spawn((
                        Text::new(state_label),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(state_color),
                    ));
                });

            let Some(compiled) = compiled else {
                spawn_diagnostics_empty_state(
                    panel,
                    "NO COMPILED ARTIFACT",
                    "Resolve compiler diagnostics to inspect the executable plan.",
                    Color::srgb(1.0, 0.38, 0.32),
                );
                return;
            };

            spawn_compiled_summary(panel, compiled);
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::GeneratedCode,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(10.0),
                            ..default()
                        },
                        |content| {
                            spawn_compiled_layout(content, compiled);
                            spawn_compiled_parameters(content, compiled, session);
                            for (emitter_index, emitter) in compiled.emitters.iter().enumerate() {
                                spawn_compiled_emitter(
                                    content,
                                    compiled,
                                    emitter,
                                    emitter_index,
                                    session,
                                );
                            }
                            spawn_wesl_backend(content);
                        },
                    );
                });
        });
}

fn spawn_profiler_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &ProfilerState,
) {
    let status = if session
        .pending_change
        .as_ref()
        .is_some_and(|pending| !pending.can_apply)
    {
        "CPU REFERENCE  ·  LAST VALID EFFECT"
    } else {
        "CPU REFERENCE  ·  LIVE"
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(9.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("EFFECT PROFILE"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.35, 0.88, 0.57)),
                    ));
                    header.spawn((
                        Text::new(status),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    spawn_profiler_reset_button(header);
                });

            let Some(profile) = &state.profile else {
                spawn_diagnostics_empty_state(
                    panel,
                    "WAITING FOR PREVIEW",
                    "Profiler data appears after the first evaluated frame.",
                    theme::TEXT_MUTED,
                );
                return;
            };

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Profiler,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(9.0),
                            ..default()
                        },
                        |content| {
                            spawn_profiler_metric_grid(content, profile);
                            spawn_profiler_history(content, state);
                            spawn_profiler_emitters(content, profile);
                            spawn_profiler_availability(content, profile);
                        },
                    );
                });
        });
}

fn spawn_profiler_reset_button(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            EditorAction::ResetProfilerPeaks,
            FeathersActionButton,
            AccessibleLabel("Reset profiler peaks".into()),
            Node {
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("RESET PEAKS"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_profiler_metric_grid(parent: &mut ChildSpawnerCommands, profile: &EffectProfile) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|grid| {
            for metric in [
                ProfilerMetric::CpuTime,
                ProfilerMetric::GpuTime,
                ProfilerMetric::AliveParticles,
                ProfilerMetric::PeakParticles,
                ProfilerMetric::ParticleCapacity,
                ProfilerMetric::Emitters,
                ProfilerMetric::DrawCalls,
                ProfilerMetric::Dispatches,
                ProfilerMetric::BufferMemory,
            ] {
                spawn_profiler_metric_card(grid, profile, metric);
            }
        });
}

fn spawn_profiler_metric_card(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    metric: ProfilerMetric,
) {
    let (value, source) = profiler_metric_display(profile, metric);
    parent
        .spawn((
            Node {
                width: Val::Px(132.0),
                min_height: Val::Px(70.0),
                flex_grow: 1.0,
                padding: UiRect::all(Val::Px(9.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                ProfilerMetricText {
                    metric,
                    part: ProfilerMetricPart::Value,
                },
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
            card.spawn((
                Text::new(profiler_metric_name(metric)),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            card.spawn((
                ProfilerMetricText {
                    metric,
                    part: ProfilerMetricPart::Source,
                },
                Text::new(profile_source_label(source)),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(profile_source_color(source)),
            ));
        });
}

fn spawn_profiler_history(parent: &mut ChildSpawnerCommands, state: &ProfilerState) {
    spawn_compiled_section(parent, "CPU UPDATE HISTORY", |section| {
        section.spawn((
            ProfilerHistorySummary,
            Text::new(profiler_history_summary(&state.cpu_history_ns)),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
        ));
        section
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(82.0),
                align_items: AlignItems::End,
                column_gap: Val::Px(1.0),
                padding: UiRect::top(Val::Px(7.0)),
                ..default()
            })
            .with_children(|graph| {
                for index in 0..PROFILER_HISTORY_SAMPLES {
                    graph.spawn((
                        ProfilerHistoryBar(index),
                        Node {
                            height: Val::Px(1.0),
                            min_width: Val::Px(1.0),
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT_DIM),
                    ));
                }
            });
    });
}

fn spawn_profiler_emitters(parent: &mut ChildSpawnerCommands, profile: &EffectProfile) {
    spawn_compiled_section(parent, "EMITTERS", |section| {
        for (index, emitter) in profile.emitters.iter().enumerate() {
            section
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(35.0),
                        padding: UiRect::horizontal(Val::Px(7.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(format!("E{index:02}")),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                        Node {
                            width: Val::Px(38.0),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Text::new(&emitter.name),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    row.spawn((
                        ProfilerEmitterValue(index),
                        Text::new(profiler_emitter_value(emitter)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                });
        }
    });
}

fn spawn_profiler_availability(parent: &mut ChildSpawnerCommands, profile: &EffectProfile) {
    spawn_compiled_section(parent, "MEASUREMENT AVAILABILITY", |section| {
        spawn_compiled_label_value(
            section,
            "MEASURED",
            "CPU update time, live particles, and peak particles",
        );
        spawn_compiled_label_value(
            section,
            "ESTIMATED",
            "draw calls, dispatches, and runtime buffer memory from the compiled plan",
        );
        if profile.gpu_time_ns.source() == ProfileValueSource::Unavailable {
            spawn_compiled_label_value(
                section,
                "UNAVAILABLE",
                "GPU time, overdraw, and collision timing require backend instrumentation",
            );
        }
    });
}

fn profiler_metric_name(metric: ProfilerMetric) -> &'static str {
    match metric {
        ProfilerMetric::CpuTime => "CPU UPDATE",
        ProfilerMetric::GpuTime => "GPU TIME",
        ProfilerMetric::AliveParticles => "LIVE PARTICLES",
        ProfilerMetric::PeakParticles => "PEAK PARTICLES",
        ProfilerMetric::ParticleCapacity => "CAPACITY",
        ProfilerMetric::Emitters => "EMITTERS",
        ProfilerMetric::DrawCalls => "DRAW CALLS",
        ProfilerMetric::Dispatches => "DISPATCHES",
        ProfilerMetric::BufferMemory => "BUFFER MEMORY",
    }
}

fn profiler_metric_display(
    profile: &EffectProfile,
    metric: ProfilerMetric,
) -> (String, ProfileValueSource) {
    match metric {
        ProfilerMetric::CpuTime => format_profile_duration(profile.cpu_time_ns),
        ProfilerMetric::GpuTime => format_profile_duration(profile.gpu_time_ns),
        ProfilerMetric::AliveParticles => format_profile_count(profile.alive_particles),
        ProfilerMetric::PeakParticles => format_profile_count(profile.peak_particles),
        ProfilerMetric::ParticleCapacity => format_profile_count(profile.particle_capacity),
        ProfilerMetric::Emitters => format_profile_count(profile.emitter_count),
        ProfilerMetric::DrawCalls => format_profile_count(profile.draw_calls),
        ProfilerMetric::Dispatches => format_profile_count(profile.dispatch_count),
        ProfilerMetric::BufferMemory => format_profile_memory(profile.buffer_memory_bytes),
    }
}

fn format_profile_duration(value: ProfileValue<u64>) -> (String, ProfileValueSource) {
    let source = value.source();
    let Some(nanoseconds) = value.value() else {
        return ("—".into(), source);
    };
    let display = if nanoseconds >= 1_000_000 {
        format!("{:.3} ms", nanoseconds as f64 / 1_000_000.0)
    } else if nanoseconds >= 1_000 {
        format!("{:.1} µs", nanoseconds as f64 / 1_000.0)
    } else {
        format!("{nanoseconds} ns")
    };
    (display, source)
}

fn format_profile_count(value: ProfileValue<u32>) -> (String, ProfileValueSource) {
    (
        value
            .value()
            .map_or_else(|| "—".into(), |value| value.to_string()),
        value.source(),
    )
}

fn format_profile_memory(value: ProfileValue<u64>) -> (String, ProfileValueSource) {
    let source = value.source();
    let Some(bytes) = value.value() else {
        return ("—".into(), source);
    };
    let display = if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    };
    (display, source)
}

fn profile_source_label(source: ProfileValueSource) -> &'static str {
    match source {
        ProfileValueSource::Measured => "MEASURED",
        ProfileValueSource::Estimated => "ESTIMATED",
        ProfileValueSource::Unavailable => "UNAVAILABLE",
    }
}

fn profile_source_color(source: ProfileValueSource) -> Color {
    match source {
        ProfileValueSource::Measured => Color::srgb(0.35, 0.88, 0.57),
        ProfileValueSource::Estimated => Color::srgb(1.0, 0.74, 0.30),
        ProfileValueSource::Unavailable => theme::TEXT_FAINT,
    }
}

fn profiler_emitter_value(emitter: &aestra_runtime::EmitterProfile) -> String {
    format!(
        "{} LIVE  ·  {} PEAK  ·  {} CAP",
        emitter.alive_particles, emitter.peak_particles, emitter.particle_capacity
    )
}

fn profiler_history_summary(history: &VecDeque<u64>) -> String {
    if history.is_empty() {
        return "Collecting samples…".into();
    }
    let total = history.iter().copied().map(u128::from).sum::<u128>();
    let average = (total / history.len() as u128).min(u128::from(u64::MAX)) as u64;
    let maximum = history.iter().copied().max().unwrap_or_default();
    format!(
        "{} FRAMES  ·  AVG {}  ·  MAX {}",
        history.len(),
        format_profile_duration(ProfileValue::Measured(average)).0,
        format_profile_duration(ProfileValue::Measured(maximum)).0
    )
}

fn spawn_settings_workspace(
    parent: &mut ChildSpawnerCommands,
    settings: &EditorSettings,
    state: &SettingsPanelState,
    persistence: &SettingsPersistence,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn_empty()
                .apply_scene(pane_header())
                .with_children(|header| {
                    header.spawn((
                        Text::new(localizer.text("settings-editor-settings")),
                        ThemedText,
                    ));
                    header
                        .spawn_empty()
                        .apply_scene(label_dim(persistence.path().display().to_string()));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_feathers_action_button(
                        header,
                        &localizer.text("common-reset-settings"),
                        EditorAction::ResetEditorSettings,
                        false,
                    );
                });
            if let Some(diagnostic) = persistence.diagnostic() {
                panel.spawn((
                    Text::new(diagnostic),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.74, 0.30)),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ));
            }
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Px(156.0),
                            height: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            padding: UiRect::all(Val::Px(8.0)),
                            row_gap: Val::Px(3.0),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        ThemeBackgroundColor(tokens::PANE_BODY_BG),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|categories| {
                        for category in SettingsCategory::ALL {
                            spawn_settings_category_button(
                                categories,
                                category,
                                state.category == category,
                                localizer,
                            );
                        }
                    });
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|content| {
                        spawn_vertical_scroll_area(
                            content,
                            ScrollMemoryKey::Settings,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(18.0)),
                                row_gap: Val::Px(8.0),
                                ..default()
                            },
                            |settings_body| {
                                spawn_settings_category(
                                    settings_body,
                                    settings,
                                    state.category,
                                    localizer,
                                );
                            },
                        );
                    });
                });
        });
}

fn spawn_settings_category_button(
    parent: &mut ChildSpawnerCommands,
    category: SettingsCategory,
    selected: bool,
    localizer: &Localizer,
) {
    let mut button = parent.spawn_empty();
    if selected {
        button.apply_scene(ui_shell::feathers_primary_button());
    } else {
        button.apply_scene(ui_shell::feathers_plain_button());
    }
    button
        .insert((
            EditorAction::SelectSettingsCategory(category),
            SettingsCategoryButton(category),
            FeathersActionButton,
            AccessibleLabel(localizer.text(category.message_id())),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(localizer.text(category.message_id())),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_settings_category(
    parent: &mut ChildSpawnerCommands,
    settings: &EditorSettings,
    category: SettingsCategory,
    localizer: &Localizer,
) {
    spawn_settings_heading(parent, &localizer.text(category.message_id()));
    match category {
        SettingsCategory::General => {
            spawn_settings_toggle(
                parent,
                &localizer.text("settings-confirm-unsaved"),
                &localizer.text("settings-confirm-unsaved-description"),
                settings.general.confirm_unsaved_changes,
                SettingsToggle::ConfirmUnsavedChanges,
                localizer,
            );
        }
        SettingsCategory::Preview => {
            spawn_settings_toggle(
                parent,
                &localizer.text("settings-viewport-grid"),
                &localizer.text("settings-viewport-grid-description"),
                settings.preview.show_grid,
                SettingsToggle::ShowGrid,
                localizer,
            );
            spawn_settings_toggle(
                parent,
                &localizer.text("settings-play-on-open"),
                &localizer.text("settings-play-on-open-description"),
                settings.preview.play_on_open,
                SettingsToggle::PlayOnOpen,
                localizer,
            );
        }
        SettingsCategory::Performance => {
            spawn_settings_integer(
                parent,
                &localizer.text("settings-preview-particle-limit"),
                &localizer.text("settings-preview-particle-limit-description"),
                SettingsNumber::PreviewParticleLimit,
                None,
            );
        }
        SettingsCategory::Capture => {
            spawn_settings_integer(
                parent,
                &localizer.text("settings-capture-frame-rate"),
                &localizer.text("settings-capture-frame-rate-description"),
                SettingsNumber::CaptureFrameRate,
                Some("FPS"),
            );
            spawn_settings_integer(
                parent,
                &localizer.text("settings-contact-sheet-columns"),
                &localizer.text("settings-contact-sheet-columns-description"),
                SettingsNumber::ContactSheetColumns,
                None,
            );
        }
        SettingsCategory::Appearance => {
            spawn_settings_scalar(
                parent,
                &localizer.text("settings-interface-scale"),
                &localizer.text("settings-interface-scale-description"),
                SettingsNumber::UiScale,
                Some("%"),
            );
        }
        SettingsCategory::Language => {
            spawn_settings_locale(
                parent,
                &localizer.text("settings-editor-language"),
                &localizer.text("settings-language-description"),
                localizer,
            );
        }
        SettingsCategory::Keybindings => {
            for (command, binding) in [
                ("settings-binding-play-pause", "Space"),
                ("settings-binding-restart", "R"),
                ("settings-binding-save", "Ctrl+S"),
                ("settings-binding-undo", "Ctrl+Z"),
                ("settings-binding-redo", "Ctrl+Y"),
                ("settings-binding-add-emitter", "Ctrl+Enter"),
            ] {
                spawn_settings_read_only(
                    parent,
                    &localizer.text(command),
                    binding,
                    &localizer.text("settings-keybinding-description"),
                );
            }
        }
    }
}

fn spawn_settings_heading(parent: &mut ChildSpawnerCommands, title: &str) {
    parent
        .spawn_empty()
        .apply_scene(label(title.to_owned()))
        .insert(Node {
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        });
}

fn settings_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn_empty()
        .apply_scene(group())
        .with_children(|card| {
            card.spawn_empty()
                .apply_scene(group_header())
                .with_children(|header| {
                    header.spawn((Text::new(title), ThemedText));
                });
            card.spawn_empty()
                .apply_scene(group_body())
                .with_children(|body| {
                    body.spawn_empty()
                        .apply_scene(label_dim(description.to_owned()));
                    body.spawn(Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(controls);
                });
        });
}

fn spawn_settings_toggle(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    enabled: bool,
    setting: SettingsToggle,
    localizer: &Localizer,
) {
    settings_row(parent, title, description, |controls| {
        let mut checkbox = controls.spawn_empty();
        checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
            SettingsToggleControl(setting),
            AccessibleLabel(localizer.text(if enabled { "common-on" } else { "common-off" })),
        ));
        if enabled {
            checkbox.insert(Checked);
        }
    });
}

fn spawn_settings_locale(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    localizer: &Localizer,
) {
    settings_row(parent, title, description, |controls| {
        let options = SUPPORTED_LOCALES
            .iter()
            .enumerate()
            .map(|(index, locale)| ComboOption {
                label: localizer.locale_name(locale),
                selected: *locale == localizer.locale(),
                action: EditorAction::SetLocale(index),
            })
            .collect::<Vec<_>>();
        spawn_combo_control(
            controls,
            &localizer.locale_name(localizer.locale()),
            title,
            &options,
            180.0,
        );
    });
}

fn spawn_settings_integer(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    setting: SettingsNumber,
    unit: Option<&str>,
) {
    settings_row(parent, title, description, |controls| {
        controls
            .spawn(Node {
                width: Val::Px(112.0),
                ..default()
            })
            .with_children(|input| {
                input
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_integer_input())
                    .insert((
                        SettingsNumberControl(setting),
                        AccessibleLabel(title.to_owned()),
                    ));
            });
        if let Some(unit) = unit {
            controls
                .spawn_empty()
                .apply_scene(label_dim(unit.to_owned()));
        }
    });
}

fn spawn_settings_scalar(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    description: &str,
    setting: SettingsNumber,
    unit: Option<&str>,
) {
    settings_row(parent, title, description, |controls| {
        controls
            .spawn(Node {
                width: Val::Px(112.0),
                ..default()
            })
            .with_children(|input| {
                input
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert((
                        SettingsNumberControl(setting),
                        AccessibleLabel(title.to_owned()),
                    ));
            });
        if let Some(unit) = unit {
            controls
                .spawn_empty()
                .apply_scene(label_dim(unit.to_owned()));
        }
    });
}

fn spawn_settings_read_only(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    value: &str,
    description: &str,
) {
    settings_row(parent, title, description, |controls| {
        controls
            .spawn_empty()
            .apply_scene(label_dim(value.to_owned()));
    });
}

fn spawn_feathers_action_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: EditorAction,
    primary: bool,
) {
    let mut button = parent.spawn_empty();
    if primary {
        button.apply_scene(ui_shell::feathers_primary_button());
    } else {
        button.apply_scene(ui_shell::feathers_button());
    }
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

struct ComboOption {
    label: String,
    selected: bool,
    action: EditorAction,
}

fn spawn_combo_control(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    accessible_label: &str,
    options: &[ComboOption],
    width: f32,
) {
    parent
        .spawn(Node {
            width: Val::Px(width),
            min_width: Val::Px(112.0),
            ..default()
        })
        .with_children(|wrapper| {
            wrapper
                .spawn_empty()
                .apply_scene(ui_shell::feathers_menu())
                .with_children(|menu| {
                    menu.spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_button())
                        .insert((
                            AccessibleLabel(accessible_label.to_owned()),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(28.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                ..default()
                            },
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new(value),
                                ThemedText,
                                Pickable::IGNORE,
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            button
                                .spawn_empty()
                                .apply_scene(icon(icons::CHEVRON_DOWN))
                                .insert(Pickable::IGNORE);
                        });
                    menu.spawn_empty()
                        .apply_scene(ui_shell::feathers_menu_popup())
                        .with_children(|popup| {
                            for option in options {
                                spawn_combo_option(popup, option);
                            }
                        });
                });
        });
}

fn spawn_combo_option(parent: &mut ChildSpawnerCommands, option: &ComboOption) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_item())
        .insert((
            Interaction::None,
            option.action,
            FeathersActionButton,
            AccessibleLabel(option.label.clone()),
        ))
        .with_children(|item| {
            item.spawn((
                Node {
                    width: Val::Px(18.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|indicator| {
                if option.selected {
                    indicator.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT),
                        Pickable::IGNORE,
                    ));
                }
            });
            item.spawn((
                Text::new(option.label.clone()),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_action_menu(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption],
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu())
        .with_children(|menu| {
            menu.spawn_empty()
                .apply_scene(ui_shell::feathers_menu_button())
                .insert((
                    AccessibleLabel(accessible_label.to_owned()),
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button
                        .spawn((
                            Node {
                                width: Val::Px(4.0),
                                height: Val::Px(16.0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|dots| {
                            for _ in 0..3 {
                                dots.spawn((
                                    Node {
                                        width: Val::Px(3.0),
                                        height: Val::Px(3.0),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(theme::TEXT_MUTED),
                                    Pickable::IGNORE,
                                ));
                            }
                        });
                });
            menu.spawn_empty()
                .apply_scene(ui_shell::feathers_menu_popup())
                .insert((
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Left,
                                align: PopoverAlign::Start,
                                gap: 2.0,
                            },
                        ],
                        window_margin: 8.0,
                    },
                    OverrideClip,
                ))
                .with_children(|popup| {
                    for option in options {
                        spawn_combo_option(popup, option);
                    }
                });
        });
}

fn handle_settings_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&SettingsToggleControl>,
    mut commands: Commands,
    mut settings: ResMut<EditorSettings>,
    mut menu: ResMut<MenuState>,
    mut persistence: ResMut<SettingsPersistence>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let changed = apply_settings_toggle(&mut settings, &mut menu, control.0, change.value);
    if changed {
        session.ui_revision += 1;
        persist_editor_settings(&settings, &mut persistence, &mut session);
    }
}

fn apply_settings_toggle(
    settings: &mut EditorSettings,
    menu: &mut MenuState,
    setting: SettingsToggle,
    value: bool,
) -> bool {
    match setting {
        SettingsToggle::ConfirmUnsavedChanges => {
            let changed = settings.general.confirm_unsaved_changes != value;
            settings.general.confirm_unsaved_changes = value;
            changed
        }
        SettingsToggle::ShowGrid => {
            let changed = settings.preview.show_grid != value;
            settings.preview.show_grid = value;
            menu.show_grid = value;
            changed
        }
        SettingsToggle::PlayOnOpen => {
            let changed = settings.preview.play_on_open != value;
            settings.preview.play_on_open = value;
            changed
        }
    }
}

fn handle_settings_integer_change(
    change: On<ValueChange<i32>>,
    controls: Query<&SettingsNumberControl>,
    mut settings: ResMut<EditorSettings>,
    mut persistence: ResMut<SettingsPersistence>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let changed = apply_settings_integer(&mut settings, control.0, change.value);
    if changed {
        session.ui_revision += 1;
        persist_editor_settings(&settings, &mut persistence, &mut session);
    }
}

fn apply_settings_integer(
    settings: &mut EditorSettings,
    setting: SettingsNumber,
    value: i32,
) -> bool {
    match setting {
        SettingsNumber::PreviewParticleLimit => {
            let value = value.clamp(64, MAX_PREVIEW_PARTICLE_LIMIT as i32) as usize;
            let changed = settings.performance.preview_particle_limit != value;
            settings.performance.preview_particle_limit = value;
            changed
        }
        SettingsNumber::CaptureFrameRate => {
            let value = value.clamp(1, 240) as u16;
            let changed = settings.capture.frame_rate != value;
            settings.capture.frame_rate = value;
            changed
        }
        SettingsNumber::ContactSheetColumns => {
            let value = value.clamp(1, 16) as u8;
            let changed = settings.capture.contact_sheet_columns != value;
            settings.capture.contact_sheet_columns = value;
            changed
        }
        SettingsNumber::UiScale => false,
    }
}

fn handle_settings_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&SettingsNumberControl>,
    mut settings: ResMut<EditorSettings>,
    mut persistence: ResMut<SettingsPersistence>,
    mut session: ResMut<EditorSession>,
    mut ui_scale: ResMut<UiScale>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.0 != SettingsNumber::UiScale {
        return;
    }
    if apply_settings_scalar(&mut settings, control.0, change.value) {
        ui_scale.0 = settings.appearance.ui_scale;
        session.ui_revision += 1;
        persist_editor_settings(&settings, &mut persistence, &mut session);
    }
}

fn handle_inspector_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&InspectorToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let Some(current) = inspector_module_parameter(&session, control.module, control.parameter)
    else {
        session.status = "Inspector target is no longer available".into();
        return;
    };
    let value = Value::Bool(change.value);
    if current != value {
        session.set_module_parameter(control.module, control.parameter, value);
    }
}

fn handle_module_enabled_change(
    change: On<ValueChange<bool>>,
    controls: Query<&ModuleEnabledControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let enabled = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == control.0)
        .map(|module| module.enabled);
    if enabled.is_some_and(|enabled| enabled != change.value) {
        session.toggle_module(control.0);
    }
}

fn handle_renderer_enabled_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererEnabledControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    let enabled = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == control.0)
        .map(|renderer| renderer.enabled);
    if enabled.is_some_and(|enabled| enabled != change.value) {
        session.toggle_renderer(control.0);
    }
}

fn handle_renderer_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&RendererNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    match *control {
        RendererNumberControl::Softness(renderer) => {
            session.set_renderer_softness(renderer, change.value)
        }
        RendererNumberControl::Uv(renderer, component) => {
            session.set_renderer_uv(renderer, component, change.value)
        }
        RendererNumberControl::FlipbookFrameRate(renderer) => {
            session.set_flipbook_frame_rate(renderer, change.value)
        }
    }
}

fn handle_emitter_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&EmitterNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final || !change.value.is_finite() {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let current = emitter_number_input_value(&session, *control);
    match control {
        EmitterNumberControl::Start => {
            session.adjust_selected_start(change.value.max(0.0) - current);
        }
        EmitterNumberControl::Duration => {
            session.adjust_selected_duration(change.value.max(0.05) - current);
        }
    }
}

fn handle_inspector_numeric_scroll(
    mut scroll: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    inspector_controls: Query<&InspectorNumberControl>,
    emitter_controls: Query<&EmitterNumberControl>,
    renderer_controls: Query<&RendererNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    let direction = scroll.y.signum();
    if direction == 0.0 {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let multiplier = numeric_scroll_multiplier(shift, control);

    if let Ok(number) = inspector_controls.get(scroll.entity) {
        adjust_inspector_number_from_scroll(&mut session, *number, direction, multiplier);
        scroll.propagate(false);
        return;
    }
    if let Ok(number) = emitter_controls.get(scroll.entity) {
        let step = 0.05 * multiplier * direction;
        match number {
            EmitterNumberControl::Start => session.adjust_selected_start(step),
            EmitterNumberControl::Duration => session.adjust_selected_duration(step),
        }
        scroll.propagate(false);
        return;
    }
    if let Ok(number) = renderer_controls.get(scroll.entity) {
        let Some(current) = renderer_number_input_value(&session, *number) else {
            return;
        };
        let value = current + renderer_number_step(*number) * multiplier * direction;
        match number {
            RendererNumberControl::Softness(renderer) => {
                session.set_renderer_softness(*renderer, value)
            }
            RendererNumberControl::Uv(renderer, component) => {
                session.set_renderer_uv(*renderer, *component, value)
            }
            RendererNumberControl::FlipbookFrameRate(renderer) => {
                session.set_flipbook_frame_rate(*renderer, value)
            }
        }
        scroll.propagate(false);
    }
}

fn numeric_scroll_multiplier(shift: bool, control: bool) -> f32 {
    if shift {
        0.1
    } else if control {
        10.0
    } else {
        1.0
    }
}

fn renderer_number_step(control: RendererNumberControl) -> f32 {
    match control {
        RendererNumberControl::Softness(_) => 0.1,
        RendererNumberControl::Uv(_, _) => 0.05,
        RendererNumberControl::FlipbookFrameRate(_) => 1.0,
    }
}

fn adjust_inspector_number_from_scroll(
    session: &mut EditorSession,
    control: InspectorNumberControl,
    direction: f32,
    multiplier: f32,
) -> bool {
    let Some(current) = inspector_number_input_value(session, control) else {
        return false;
    };
    let current = match current {
        NumberInputValue::I32(value) => value as f32,
        NumberInputValue::F32(value) => value,
        NumberInputValue::I64(value) => value as f32,
        NumberInputValue::F64(value) => value as f32,
    };
    apply_inspector_number(
        session,
        control,
        current + control.step * multiplier * direction,
    )
}

fn handle_renderer_toggle_change(
    change: On<ValueChange<bool>>,
    controls: Query<&RendererToggleControl>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    match *control {
        RendererToggleControl::FlipbookLooping(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { flipbook, .. } => session
                        .effect
                        .flipbooks
                        .iter()
                        .find(|definition| definition.id == flipbook)
                        .map(|definition| definition.looping),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_looping(renderer_id);
            }
        }
        RendererToggleControl::FlipbookRandomStart(renderer_id) => {
            let current = session
                .selected_layer()
                .renderers
                .iter()
                .find(|renderer| renderer.id == renderer_id)
                .and_then(|renderer| match renderer.properties {
                    RendererProperties::Flipbook { random_start, .. } => Some(random_start),
                    _ => None,
                });
            if current.is_some_and(|current| current != change.value) {
                session.toggle_flipbook_random_start(renderer_id);
            }
        }
    }
}

fn handle_inspector_integer_change(
    change: On<ValueChange<i32>>,
    controls: Query<&InspectorNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind != InspectorNumberKind::U32 {
        return;
    }
    apply_inspector_number(&mut session, *control, change.value as f32);
}

fn handle_inspector_scalar_change(
    change: On<ValueChange<f32>>,
    controls: Query<&InspectorNumberControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if control.kind == InspectorNumberKind::U32 {
        return;
    }
    apply_inspector_number(&mut session, *control, change.value);
}

fn inspector_module_parameter(
    session: &EditorSession,
    module: ModuleId,
    parameter: &str,
) -> Option<Value> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    module_parameter(module, parameter)
}

fn apply_inspector_number(
    session: &mut EditorSession,
    control: InspectorNumberControl,
    raw_value: f32,
) -> bool {
    let Some(value) = clamp_inspector_number(raw_value, control.min, control.max) else {
        session.status = format!("{} requires a finite number", control.parameter);
        return false;
    };
    let Some(current) = inspector_module_parameter(session, control.module, control.parameter)
    else {
        session.status = "Inspector target is no longer available".into();
        return false;
    };
    let updated = match (control.kind, current.clone()) {
        (InspectorNumberKind::U32, Value::U32(_)) => {
            Value::U32(value.max(0.0).round().min(u32::MAX as f32) as u32)
        }
        (InspectorNumberKind::Scalar, Value::Scalar(_)) => Value::Scalar(value),
        (InspectorNumberKind::Vector, Value::Vec2(mut vector)) => {
            let Some(component) = vector.get_mut(control.component as usize) else {
                return false;
            };
            *component = value;
            Value::Vec2(vector)
        }
        (InspectorNumberKind::Vector, Value::Vec3(mut vector)) => {
            let Some(component) = vector.get_mut(control.component as usize) else {
                return false;
            };
            *component = value;
            Value::Vec3(vector)
        }
        (InspectorNumberKind::Vector, Value::Vec4(mut vector)) => {
            let Some(component) = vector.get_mut(control.component as usize) else {
                return false;
            };
            *component = value;
            Value::Vec4(vector)
        }
        (InspectorNumberKind::Range, Value::Range(mut range)) => {
            if control.component == 0 {
                range.min = value.min(range.max);
            } else {
                range.max = value.max(range.min);
            }
            Value::Range(range)
        }
        _ => {
            session.status = format!("{} has incompatible Inspector metadata", control.parameter);
            return false;
        }
    };
    if updated == current {
        return false;
    }
    session.set_module_parameter(control.module, control.parameter, updated);
    true
}

fn clamp_inspector_number(value: f32, min: Option<f32>, max: Option<f32>) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }
    let value = min.map_or(value, |min| value.max(min));
    Some(max.map_or(value, |max| value.min(max)))
}

fn apply_settings_scalar(
    settings: &mut EditorSettings,
    setting: SettingsNumber,
    value: f32,
) -> bool {
    if setting != SettingsNumber::UiScale {
        return false;
    }
    let value = ((value / 100.0).clamp(0.75, 1.5) * 20.0).round() / 20.0;
    let changed = settings.appearance.ui_scale != value;
    settings.appearance.ui_scale = value;
    changed
}

fn sync_settings_number_inputs(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    controls: Query<(Entity, &SettingsNumberControl), Added<SettingsNumberControl>>,
) {
    for (entity, control) in &controls {
        let value = settings_number_input_value(&settings, control.0);
        commands.trigger(UpdateNumberInput { entity, value });
    }
}

fn sync_emitter_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &EmitterNumberControl), Added<EmitterNumberControl>>,
) {
    for (entity, control) in &controls {
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(emitter_number_input_value(&session, *control)),
        });
    }
}

fn emitter_number_input_value(session: &EditorSession, control: EmitterNumberControl) -> f32 {
    match control {
        EmitterNumberControl::Start => session.selected_layer().start_time,
        EmitterNumberControl::Duration => session.selected_layer().duration,
    }
}

fn sync_inspector_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &InspectorNumberControl), Added<InspectorNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = inspector_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput { entity, value });
    }
}

fn sync_renderer_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RendererNumberControl), Added<RendererNumberControl>>,
) {
    for (entity, control) in &controls {
        let Some(value) = renderer_number_input_value(&session, *control) else {
            continue;
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(value),
        });
    }
}

fn renderer_number_input_value(
    session: &EditorSession,
    control: RendererNumberControl,
) -> Option<f32> {
    let renderer_id = match control {
        RendererNumberControl::Softness(renderer)
        | RendererNumberControl::Uv(renderer, _)
        | RendererNumberControl::FlipbookFrameRate(renderer) => renderer,
    };
    let renderer = session
        .selected_layer()
        .renderers
        .iter()
        .find(|renderer| renderer.id == renderer_id)?;
    match control {
        RendererNumberControl::Softness(_) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { softness, .. } = &material.properties;
            match softness {
                MaterialInput::Constant(value) => Some(*value),
                MaterialInput::Parameter(_) => None,
            }
        }
        RendererNumberControl::Uv(_, component) => {
            let material = session
                .effect
                .materials
                .iter()
                .find(|material| material.id == renderer.material)?;
            let MaterialProperties::Sprite { uv, .. } = &material.properties;
            match component {
                0 => Some(uv.min[0]),
                1 => Some(uv.min[1]),
                2 => Some(uv.max[0]),
                3 => Some(uv.max[1]),
                _ => None,
            }
        }
        RendererNumberControl::FlipbookFrameRate(_) => {
            let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
                return None;
            };
            session
                .effect
                .flipbooks
                .iter()
                .find(|definition| definition.id == flipbook)
                .map(|definition| definition.frame_rate)
        }
    }
}

fn inspector_number_input_value(
    session: &EditorSession,
    control: InspectorNumberControl,
) -> Option<NumberInputValue> {
    let value = inspector_module_parameter(session, control.module, control.parameter)?;
    match (control.kind, value) {
        (InspectorNumberKind::U32, Value::U32(value)) => {
            Some(NumberInputValue::I32(value.min(i32::MAX as u32) as i32))
        }
        (InspectorNumberKind::Scalar, Value::Scalar(value)) => Some(NumberInputValue::F32(value)),
        (InspectorNumberKind::Vector, Value::Vec2(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Vector, Value::Vec3(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Vector, Value::Vec4(value)) => value
            .get(control.component as usize)
            .copied()
            .map(NumberInputValue::F32),
        (InspectorNumberKind::Range, Value::Range(value)) => {
            Some(NumberInputValue::F32(if control.component == 0 {
                value.min
            } else {
                value.max
            }))
        }
        _ => None,
    }
}

fn settings_number_input_value(
    settings: &EditorSettings,
    setting: SettingsNumber,
) -> NumberInputValue {
    match setting {
        SettingsNumber::PreviewParticleLimit => {
            NumberInputValue::I32(settings.performance.preview_particle_limit as i32)
        }
        SettingsNumber::CaptureFrameRate => {
            NumberInputValue::I32(i32::from(settings.capture.frame_rate))
        }
        SettingsNumber::ContactSheetColumns => {
            NumberInputValue::I32(i32::from(settings.capture.contact_sheet_columns))
        }
        SettingsNumber::UiScale => NumberInputValue::F32(settings.appearance.ui_scale * 100.0),
    }
}

fn spawn_vertical_scroll_area(
    parent: &mut ChildSpawnerCommands,
    memory: ScrollMemoryKey,
    mut viewport: Node,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    viewport.overflow = Overflow::scroll_y();
    viewport.scrollbar_width = 0.0;
    let target = parent
        .spawn((viewport, ScrollArea, PersistedScroll(memory)))
        .with_children(content)
        .id();
    spawn_vertical_scrollbar(parent, target);
    target
}

fn spawn_vertical_scrollbar(parent: &mut ChildSpawnerCommands, target: Entity) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_vertical_scrollbar(target))
        .insert(Node {
            width: Val::Px(10.0),
            height: Val::Percent(100.0),
            display: Display::None,
            padding: UiRect::horizontal(Val::Px(3.0)),
            ..default()
        });
}

fn remember_scroll_positions(
    mut memory: ResMut<ScrollMemoryState>,
    scroll_areas: Query<(&PersistedScroll, &ScrollPosition)>,
) {
    for (marker, position) in &scroll_areas {
        memory.0.insert(marker.0, position.0);
    }
}

fn restore_scroll_positions(
    memory: Res<ScrollMemoryState>,
    mut scroll_areas: Query<(&PersistedScroll, &mut ScrollPosition), Added<PersistedScroll>>,
) {
    for (marker, mut position) in &mut scroll_areas {
        if let Some(saved) = memory.0.get(&marker.0) {
            position.0 = *saved;
        }
    }
}

fn vertical_scrollbar_needed(viewport_height: f32, content_height: f32) -> bool {
    content_height > viewport_height + 0.5
}

fn update_scrollbar_visibility(
    scroll_areas: Query<&ComputedNode, With<ScrollArea>>,
    mut scrollbars: Query<(&Scrollbar, &mut Node), Without<ScrollArea>>,
) {
    for (scrollbar, mut node) in &mut scrollbars {
        let Ok(viewport) = scroll_areas.get(scrollbar.target) else {
            continue;
        };
        node.display = if vertical_scrollbar_needed(viewport.size().y, viewport.content_size().y) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn generated_code_status(session: &EditorSession, has_artifact: bool) -> (&'static str, Color) {
    if session
        .pending_change
        .as_ref()
        .is_some_and(|pending| !pending.can_apply)
        && has_artifact
    {
        (
            "PREVIEW BLOCKED  ·  LAST VALID COMPILE",
            Color::srgb(1.0, 0.74, 0.30),
        )
    } else if session.pending_change.is_some() && has_artifact {
        ("PENDING PREVIEW  ·  LIVE", theme::ACCENT)
    } else if has_artifact {
        ("WORKING EFFECT  ·  LIVE", Color::srgb(0.35, 0.88, 0.57))
    } else {
        ("COMPILE UNAVAILABLE", Color::srgb(1.0, 0.38, 0.32))
    }
}

fn spawn_compiled_summary(parent: &mut ChildSpawnerCommands, compiled: &CompiledEffect) {
    let instruction_count = compiled
        .emitters
        .iter()
        .map(|emitter| {
            emitter.execution.emitter_update.len()
                + emitter.execution.particle_spawn.len()
                + emitter.execution.particle_update.len()
        })
        .sum::<usize>();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(42.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|summary| {
            spawn_compiled_metric(summary, "EMITTERS", compiled.emitters.len());
            spawn_compiled_metric(summary, "OPS", instruction_count);
            spawn_compiled_metric(
                summary,
                "ATTRIBUTES",
                compiled.particle_layout.attributes.len(),
            );
            spawn_compiled_metric(summary, "PARAMETERS", compiled.parameters.len());
            spawn_compiled_metric(summary, "CAPACITY", compiled.max_particles);
            summary.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            summary.spawn((
                Text::new(format!(
                    "{}  ·  {:.2}s  ·  {:?}",
                    compiled.name.to_uppercase(),
                    compiled.duration,
                    compiled.seek_mode
                )),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_compiled_metric(parent: &mut ChildSpawnerCommands, label: &str, value: usize) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|metric| {
            metric.spawn((
                Text::new(value.to_string()),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
            metric.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_compiled_layout(parent: &mut ChildSpawnerCommands, compiled: &CompiledEffect) {
    spawn_compiled_section(parent, "PARTICLE LAYOUT", |section| {
        spawn_compiled_label_value(
            section,
            "STORED",
            &format_particle_attributes(&compiled.particle_layout.attributes),
        );
        spawn_compiled_label_value(
            section,
            "TRANSIENT",
            &format_particle_attributes(&compiled.particle_layout.transient_attributes),
        );
        spawn_compiled_label_value(
            section,
            "OPTIMIZED",
            &format!(
                "{} constants  ·  {} runtime reads  ·  {} attributes removed",
                compiled.optimizations.constant_expressions,
                compiled.optimizations.runtime_parameter_reads,
                compiled.optimizations.eliminated_attributes
            ),
        );
    });
}

fn spawn_compiled_parameters(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    session: &EditorSession,
) {
    spawn_compiled_section(parent, "PARAMETER TABLE", |section| {
        if compiled.parameters.is_empty() {
            spawn_compiled_muted_line(section, "No runtime parameter slots retained.");
            return;
        }
        for (index, parameter) in compiled.parameters.iter().enumerate() {
            spawn_compiled_target_row(
                section,
                SemanticTarget::Parameter(parameter.source),
                session.selection.primary == SemanticTarget::Parameter(parameter.source),
                &format!("P{index:03}"),
                &parameter.name,
                &format!("{:?}  ·  {:?}", parameter.value_type, parameter.default),
            );
        }
    });
}

fn spawn_compiled_emitter(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    emitter: &CompiledEmitter,
    emitter_index: usize,
    session: &EditorSession,
) {
    spawn_compiled_section(
        parent,
        &format!(
            "EMITTER {emitter_index:02}  ·  {}",
            emitter.name.to_uppercase()
        ),
        |section| {
            spawn_compiled_target_row(
                section,
                SemanticTarget::Emitter(emitter.source),
                session.selection.primary == SemanticTarget::Emitter(emitter.source),
                &format!("E{emitter_index:02}"),
                if emitter.enabled {
                    "ENABLED"
                } else {
                    "DISABLED"
                },
                &format!(
                    "start {:.2}s  ·  duration {:.2}s  ·  capacity {}  ·  {}",
                    emitter.start_time, emitter.duration, emitter.max_particles, emitter.source
                ),
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::EmitterUpdate,
                &emitter.execution.emitter_update,
                session,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleSpawn,
                &emitter.execution.particle_spawn,
                session,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleUpdate,
                &emitter.execution.particle_update,
                session,
            );
            if emitter.renderers.is_empty() {
                spawn_compiled_stage_heading(section, "RENDERERS", 0);
            } else {
                spawn_compiled_stage_heading(section, "RENDERERS", emitter.renderers.len());
                for (index, renderer) in emitter.renderers.iter().enumerate() {
                    let material = compiled
                        .material(renderer.material)
                        .expect("compiled renderer material must exist");
                    let texture = material
                        .texture
                        .and_then(|id| compiled.assets.iter().find(|asset| asset.source == id))
                        .map_or("procedural".to_string(), |asset| {
                            format!("texture {}", asset.name)
                        });
                    spawn_compiled_target_row(
                        section,
                        SemanticTarget::Renderer(renderer.source),
                        session.selection.primary == SemanticTarget::Renderer(renderer.source),
                        &format!("R{index:03}"),
                        "SPRITE DRAW",
                        &format!(
                            "material {}  ·  {:?} blend  ·  softness {:?}  ·  {texture}  ·  {}",
                            material.name, material.blend, material.softness, renderer.source,
                        ),
                    );
                }
            }
        },
    );
}

fn spawn_compiled_stage(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    _emitter: &CompiledEmitter,
    emitter_index: usize,
    stage: RuntimeStage,
    instructions: &[Instruction],
    session: &EditorSession,
) {
    spawn_compiled_stage_heading(parent, runtime_stage_label(stage), instructions.len());
    for (instruction_index, instruction) in instructions.iter().enumerate() {
        let module = instruction.source();
        let source_name = authored_module_name(compiled_source_effect(session), module);
        let location =
            compiled
                .source_map
                .get(&module)
                .copied()
                .unwrap_or(aestra_runtime::IrLocation {
                    emitter_index,
                    stage,
                    instruction_index,
                });
        spawn_compiled_target_row(
            parent,
            SemanticTarget::Module(module),
            session.selection.primary == SemanticTarget::Module(module),
            &format!(
                "E{:02}/{}{:03}",
                location.emitter_index,
                runtime_stage_code(location.stage),
                location.instruction_index
            ),
            &format!("{}  ·  {source_name}", instruction_opcode(instruction)),
            &instruction_summary(instruction),
        );
    }
}

fn spawn_compiled_stage_heading(parent: &mut ChildSpawnerCommands, title: &str, count: usize) {
    parent.spawn((
        Text::new(format!("{title}  ·  {count} OPS")),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(8.0), Val::Px(2.0)),
            ..default()
        },
    ));
}

fn spawn_wesl_backend(parent: &mut ChildSpawnerCommands) {
    spawn_compiled_section(parent, "WESL BACKEND", |section| {
        spawn_compiled_label_value(
            section,
            "SIMULATION",
            "aestra_simulation.wesl  ·  reset @compute(1)  ·  simulate @compute(64)",
        );
        spawn_compiled_label_value(
            section,
            "SPRITE",
            "aestra_sprite_render.wesl  ·  vertex  ·  fragment_alpha  ·  fragment_additive",
        );
        spawn_compiled_muted_line(
            section,
            "The effect plan supplies typed buffers to these WESL entry points; Aestra does not store generated WGSL.",
        );
    });
}

fn spawn_compiled_section(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(4.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|section| {
            section.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    margin: UiRect::bottom(Val::Px(3.0)),
                    ..default()
                },
            ));
            content(section);
        });
}

fn spawn_compiled_target_row(
    parent: &mut ChildSpawnerCommands,
    target: SemanticTarget,
    selected: bool,
    address: &str,
    opcode: &str,
    detail: &str,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            CompiledPlanRow,
            EditorAction::SelectCompiledTarget(target),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(31.0),
                padding: UiRect::axes(Val::Px(7.0), Val::Px(5.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(9.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL
            }),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(address),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Node {
                    width: Val::Px(80.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn((
                Text::new(opcode),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    width: Val::Px(190.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn((
                Text::new(detail),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

fn spawn_compiled_label_value(parent: &mut ChildSpawnerCommands, label: &str, value: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(24.0),
            padding: UiRect::horizontal(Val::Px(5.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Node {
                    width: Val::Px(82.0),
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
        });
}

fn spawn_compiled_muted_line(parent: &mut ChildSpawnerCommands, value: &str) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
        Node {
            margin: UiRect::horizontal(Val::Px(5.0)),
            ..default()
        },
    ));
}

fn format_particle_attributes(attributes: &[aestra_runtime::ParticleAttribute]) -> String {
    if attributes.is_empty() {
        return "none".into();
    }
    attributes
        .iter()
        .map(|attribute| format!("{attribute:?}").to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("  ·  ")
}

fn authored_module_name(effect: &EffectAsset, module: ModuleId) -> String {
    effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|candidate| candidate.id == module)
        .map(|module| module.module_type.0.to_ascii_uppercase())
        .unwrap_or_else(|| "UNKNOWN MODULE".into())
}

fn compiled_source_effect(session: &EditorSession) -> &EffectAsset {
    session
        .pending_change
        .as_ref()
        .filter(|pending| pending.can_apply)
        .map(|pending| pending.preview.candidate())
        .unwrap_or(&session.effect)
}

fn runtime_stage_label(stage: RuntimeStage) -> &'static str {
    match stage {
        RuntimeStage::EmitterUpdate => "EMITTER UPDATE",
        RuntimeStage::ParticleSpawn => "PARTICLE SPAWN",
        RuntimeStage::ParticleUpdate => "PARTICLE UPDATE",
    }
}

fn runtime_stage_code(stage: RuntimeStage) -> &'static str {
    match stage {
        RuntimeStage::EmitterUpdate => "EU",
        RuntimeStage::ParticleSpawn => "PS",
        RuntimeStage::ParticleUpdate => "PU",
    }
}

fn instruction_opcode(instruction: &Instruction) -> &'static str {
    match instruction {
        Instruction::Emit { .. } => "EMIT",
        Instruction::SampleShape { .. } => "SAMPLE SHAPE",
        Instruction::Initialize { .. } => "INITIALIZE",
        Instruction::Motion { .. } => "MOTION",
        Instruction::Appearance { .. } => "APPEARANCE",
    }
}

fn instruction_summary(instruction: &Instruction) -> String {
    match instruction {
        Instruction::Emit {
            spawn_rate,
            burst_count,
            ..
        } => format!("rate {spawn_rate:?}  ·  burst {burst_count:?}"),
        Instruction::SampleShape { shape, .. } => format!("shape {shape:?}"),
        Instruction::Initialize {
            lifetime,
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
            ..
        } => format!(
            "life {lifetime:?}  ·  speed {speed:?}  ·  direction {direction_degrees:?}  ·  spread {spread_degrees:?}  ·  angular {angular_velocity:?}"
        ),
        Instruction::Motion {
            gravity,
            drag,
            turbulence,
            ..
        } => format!("gravity {gravity:?}  ·  drag {drag:?}  ·  turbulence {turbulence:?}"),
        Instruction::Appearance {
            size,
            opacity,
            color,
            ..
        } => format!("size {size:?}  ·  opacity {opacity:?}  ·  color {color:?}"),
    }
}

fn spawn_diagnostics_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &DiagnosticsPanelState,
) {
    let current = &session.diagnostics.diagnostics;
    let pending = session
        .pending_change
        .as_ref()
        .map(|pending| pending.diagnostics.diagnostics.as_slice())
        .unwrap_or_default();
    let all = current.iter().chain(pending.iter());
    let errors = all
        .clone()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let warnings = all
        .clone()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count();
    let info = all
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Info)
        .count();
    let visible = current
        .iter()
        .chain(pending.iter())
        .filter(|diagnostic| state.filter.matches(diagnostic.severity))
        .count();

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("VALIDATION"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_diagnostic_count(header, errors, "ERRORS", Color::srgb(1.0, 0.38, 0.32));
                    spawn_diagnostic_count(
                        header,
                        warnings,
                        "WARNINGS",
                        Color::srgb(1.0, 0.74, 0.30),
                    );
                    spawn_diagnostic_count(header, info, "INFO", Color::srgb(0.45, 0.70, 1.0));
                });
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(36.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(10.0)),
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|filters| {
                    for filter in DiagnosticsFilter::ALL {
                        let count = match filter {
                            DiagnosticsFilter::All => errors + warnings + info,
                            DiagnosticsFilter::Errors => errors,
                            DiagnosticsFilter::Warnings => warnings,
                            DiagnosticsFilter::Info => info,
                        };
                        spawn_diagnostics_filter_button(
                            filters,
                            filter,
                            state.filter == filter,
                            count,
                        );
                    }
                });

            if errors + warnings + info == 0 {
                spawn_diagnostics_empty_state(
                    panel,
                    "NO ISSUES",
                    "The working effect passes semantic and compiler validation.",
                    Color::srgb(0.35, 0.88, 0.57),
                );
                return;
            }
            if visible == 0 {
                spawn_diagnostics_empty_state(
                    panel,
                    "NO MATCHES",
                    "No diagnostics match the selected severity filter.",
                    theme::TEXT_MUTED,
                );
                return;
            }

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Diagnostics,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(8.0)),
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                        |list| {
                            spawn_diagnostic_section(
                                list,
                                "WORKING EFFECT",
                                &session.diagnostics,
                                DiagnosticSource::Current,
                                state.filter,
                            );
                            if let Some(pending) = &session.pending_change {
                                spawn_diagnostic_section(
                                    list,
                                    "PENDING TRANSACTION",
                                    &pending.diagnostics,
                                    DiagnosticSource::Pending,
                                    state.filter,
                                );
                            }
                        },
                    );
                });
        });
}

fn spawn_diagnostics_filter_button(
    parent: &mut ChildSpawnerCommands,
    filter: DiagnosticsFilter,
    selected: bool,
    count: usize,
) {
    let mut button = parent.spawn_empty();
    if selected {
        button.apply_scene(ui_shell::feathers_primary_button());
    } else {
        button.apply_scene(ui_shell::feathers_button());
    }
    button
        .insert((
            EditorAction::SetDiagnosticsFilter(filter),
            DiagnosticsFilterButton(filter),
            FeathersActionButton,
            AccessibleLabel(format!("{} {count}", filter.label())),
            Node {
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(format!("{} {count}", filter.label())),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(match filter {
                    DiagnosticsFilter::Errors => Color::srgb(1.0, 0.38, 0.32),
                    DiagnosticsFilter::Warnings => Color::srgb(1.0, 0.74, 0.30),
                    DiagnosticsFilter::All | DiagnosticsFilter::Info => theme::TEXT_MUTED,
                }),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_diagnostic_section(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    report: &ValidationReport,
    source: DiagnosticSource,
    filter: DiagnosticsFilter,
) {
    if !report
        .diagnostics
        .iter()
        .any(|diagnostic| filter.matches(diagnostic.severity))
    {
        return;
    }
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
        Node {
            margin: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
            ..default()
        },
    ));
    for (index, diagnostic) in report.diagnostics.iter().enumerate() {
        if !filter.matches(diagnostic.severity) {
            continue;
        }
        spawn_diagnostic_row(parent, diagnostic, source, index);
    }
}

fn spawn_diagnostic_row(
    parent: &mut ChildSpawnerCommands,
    diagnostic: &Diagnostic,
    source: DiagnosticSource,
    index: usize,
) {
    let (label, color) = diagnostic_severity_style(diagnostic.severity);
    parent
        .spawn((
            Button,
            EditorNativeControl,
            EditorAction::SelectDiagnostic { source, index },
            DiagnosticRow,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(64.0),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(9.0),
                align_items: AlignItems::Stretch,
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(4.0),
                    min_height: Val::Px(48.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|content| {
                content.spawn((
                    Text::new(format!("{label}  ·  {:?}", diagnostic.code)),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(color),
                    Pickable::IGNORE,
                ));
                content.spawn((
                    Text::new(&diagnostic.message),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
                content.spawn((
                    Text::new(&diagnostic.path),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Pickable::IGNORE,
                ));
            });
        });
}

fn spawn_diagnostics_empty_state(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    message: &str,
    color: Color,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|empty| {
            empty.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(color),
            ));
            empty.spawn((
                Text::new(message),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
        });
}

fn diagnostic_severity_style(severity: DiagnosticSeverity) -> (&'static str, Color) {
    match severity {
        DiagnosticSeverity::Error => ("ERROR", Color::srgb(1.0, 0.38, 0.32)),
        DiagnosticSeverity::Warning => ("WARNING", Color::srgb(1.0, 0.74, 0.30)),
        DiagnosticSeverity::Info => ("INFO", Color::srgb(0.45, 0.70, 1.0)),
    }
}

fn spawn_diagnostic_count(
    parent: &mut ChildSpawnerCommands,
    count: usize,
    label: &str,
    active_color: Color,
) {
    parent.spawn((
        Text::new(format!("{count} {label}")),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(if count == 0 {
            theme::TEXT_FAINT
        } else {
            active_color
        }),
    ));
}

fn spawn_changes_workspace(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    let summary = session.pending_change.as_ref().map_or_else(
                        || "NO TRANSACTION PENDING".to_string(),
                        |pending| {
                            format!(
                                "{}  ·  {} CHANGES",
                                pending.preview.transaction().label.to_uppercase(),
                                pending.preview.diff().changes.len()
                            )
                        },
                    );
                    header.spawn((
                        Text::new(summary),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                });

            let Some(pending) = &session.pending_change else {
                panel.spawn((
                    Text::new(
                        "No proposed changes. Structural deletions open here for review before they modify the effect.",
                    ),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Node {
                        margin: UiRect::all(Val::Px(28.0)),
                        ..default()
                    },
                ));
                return;
            };

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Percent(66.0),
                            height: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|column| {
                        spawn_vertical_scroll_area(
                            column,
                            ScrollMemoryKey::ChangesList,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(8.0)),
                                row_gap: Val::Px(4.0),
                                ..default()
                            },
                            |changes| {
                        for change in &pending.preview.diff().changes {
                            let (kind, color) = change_kind_style(change.kind);
                            let values = match (&change.before, &change.after) {
                                (Some(before), Some(after)) => format!("{before}  →  {after}"),
                                (Some(before), None) => before.clone(),
                                (None, Some(after)) => after.clone(),
                                (None, None) => String::new(),
                            };
                            changes
                                .spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        min_height: Val::Px(30.0),
                                        align_items: AlignItems::Center,
                                        padding: UiRect::horizontal(Val::Px(8.0)),
                                        column_gap: Val::Px(8.0),
                                        border_radius: BorderRadius::all(Val::Px(3.0)),
                                        ..default()
                                    },
                                    BackgroundColor(theme::PANEL),
                                ))
                                .with_children(|row| {
                                    row.spawn((
                                        Text::new(kind),
                                        TextFont {
                                            font_size: FontSize::Px(9.0),
                                            ..default()
                                        },
                                        TextColor(color),
                                        Node {
                                            width: Val::Px(58.0),
                                            ..default()
                                        },
                                    ));
                                    row.spawn((
                                        Text::new(change.path.clone()),
                                        TextFont {
                                            font_size: FontSize::Px(10.0),
                                            ..default()
                                        },
                                        TextColor(theme::TEXT),
                                        Node {
                                            width: Val::Percent(42.0),
                                            ..default()
                                        },
                                    ));
                                    row.spawn((
                                        Text::new(values),
                                        TextFont {
                                            font_size: FontSize::Px(9.0),
                                            ..default()
                                        },
                                        TextColor(theme::TEXT_MUTED),
                                    ));
                                });
                        }
                            },
                        );
                    });
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|column| {
                        spawn_vertical_scroll_area(
                            column,
                            ScrollMemoryKey::ChangesReview,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(10.0)),
                                row_gap: Val::Px(6.0),
                                ..default()
                            },
                            |review| {
                        let errors = pending
                            .diagnostics
                            .diagnostics
                            .iter()
                            .filter(|item| item.severity == DiagnosticSeverity::Error)
                            .count();
                        review.spawn((
                            Text::new(if pending.can_apply {
                                "VALIDATED · READY TO APPLY".to_string()
                            } else {
                                format!("BLOCKED · {errors} COMPILER ERROR(S)")
                            }),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(if pending.can_apply {
                                Color::srgb(0.35, 0.88, 0.57)
                            } else {
                                Color::srgb(1.0, 0.38, 0.32)
                            }),
                        ));
                        for diagnostic in &pending.diagnostics.diagnostics {
                            review.spawn((
                                Text::new(format!(
                                    "{:?} · {}\n{}",
                                    diagnostic.code, diagnostic.path, diagnostic.message
                                )),
                                TextFont {
                                    font_size: FontSize::Px(9.0),
                                    ..default()
                                },
                                TextColor(match diagnostic.severity {
                                    DiagnosticSeverity::Error => Color::srgb(1.0, 0.38, 0.32),
                                    DiagnosticSeverity::Warning => Color::srgb(1.0, 0.74, 0.30),
                                    DiagnosticSeverity::Info => theme::TEXT_MUTED,
                                }),
                            ));
                        }
                        review.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        review
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(32.0),
                                justify_content: JustifyContent::FlexEnd,
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|actions| {
                                inspector_action_button(
                                    actions,
                                    "Discard",
                                    EditorAction::DiscardPendingChange,
                                    None,
                                );
                                inspector_action_button(
                                    actions,
                                    if pending.can_apply { "Apply" } else { "Apply blocked" },
                                    EditorAction::ApplyPendingChange,
                                    None,
                                );
                            });
                            },
                        );
                    });
                });
        });
}

fn change_kind_style(kind: ChangeKind) -> (&'static str, Color) {
    match kind {
        ChangeKind::Added => ("ADDED", Color::srgb(0.35, 0.88, 0.57)),
        ChangeKind::Removed => ("REMOVED", Color::srgb(1.0, 0.38, 0.32)),
        ChangeKind::Modified => ("MODIFIED", Color::srgb(0.45, 0.70, 1.0)),
        ChangeKind::Moved => ("MOVED", Color::srgb(1.0, 0.74, 0.30)),
    }
}

fn spawn_complex_input_list(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &WorkspaceState,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(224.0),
                height: Val::Percent(100.0),
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|column| {
            spawn_vertical_scroll_area(
                column,
                ScrollMemoryKey::Curves,
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(7.0)),
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                |list| {
                    for module in &session.selected_layer().modules {
                        let Some(metadata) = registry.0.get(&module.module_type) else {
                            continue;
                        };
                        for (input_index, input) in metadata.inputs.iter().enumerate() {
                            if !matches!(
                                input.control,
                                InputControl::Curve { .. } | InputControl::Gradient
                            ) {
                                continue;
                            }
                            let selected = workspace.complex.is_some_and(|selection| {
                                selection.module == module.id
                                    && selection.input == input_index as u8
                            });
                            parent_list_button(
                                list,
                                &format!("{} / {}", metadata.display_name, input.display_name),
                                EditorAction::EditComplexInput(module.id, input_index as u8),
                                selected,
                            );
                        }
                    }
                },
            );
        });
}

fn parent_list_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: EditorAction,
    selected: bool,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            action,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(28.0),
                padding: UiRect::horizontal(Val::Px(7.0)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::BUTTON
            }),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(if selected {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                }),
            ));
        });
}

fn resolve_complex_input<'a>(
    session: &'a EditorSession,
    registry: &'a EditorModuleRegistry,
    selection: ComplexSelection,
) -> Option<(&'a ModuleInstance, &'a InputMetadata, Value)> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == selection.module)?;
    let input = registry
        .0
        .get(&module.module_type)?
        .inputs
        .get(selection.input as usize)?;
    let value = module_parameter(module, input.name)?;
    Some((module, input, value))
}

fn spawn_curve_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    curve: &aestra_bevy::Curve,
    selected_key: usize,
) {
    let InputControl::Curve { step, min, max } = input.control else {
        return;
    };
    parent.spawn((
        Text::new(format!("{}  ·  {}", input.display_name, input.description)),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
    ));
    parent
        .spawn((
            CurveGraph,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(112.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::TIMELINE_BG),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|graph| {
            for index in 0..64 {
                let time = index as f32 / 63.0;
                let normalized = ((curve.sample(time) - min) / (max - min)).clamp(0.0, 1.0);
                graph.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(time * 100.0),
                        top: Val::Percent((1.0 - normalized) * 100.0),
                        width: Val::Px(2.0),
                        height: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(theme::ACCENT_DIM),
                ));
            }
            for (key_index, key) in curve.keys.iter().enumerate() {
                let normalized = ((key.value - min) / (max - min)).clamp(0.0, 1.0);
                let parameter = input.name;
                graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        UiTransform::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            top: Val::Percent((1.0 - normalized) * 100.0),
                            width: Val::Px(11.0),
                            height: Val::Px(11.0),
                            border: UiRect::all(Val::Px(if key_index == selected_key {
                                2.0
                            } else {
                                1.0
                            })),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT),
                        BorderColor::all(if key_index == selected_key {
                            Color::WHITE
                        } else {
                            theme::ACCENT_DIM
                        }),
                    ))
                    .observe(
                        move |click: On<Pointer<Click>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<WorkspaceState>| {
                            if click.button == PointerButton::Primary {
                                workspace.complex = Some(ComplexSelection {
                                    module,
                                    input: input_index,
                                    key: key_index,
                                });
                                session.ui_revision += 1;
                            }
                        },
                    )
                    .observe(
                        |drag: On<Pointer<Drag>>, mut transforms: Query<&mut UiTransform>| {
                            if drag.button == PointerButton::Primary
                                && let Ok(mut transform) = transforms.get_mut(drag.entity)
                            {
                                transform.translation = Val2::px(drag.distance.x, drag.distance.y);
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut transforms: Query<&mut UiTransform>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<WorkspaceState>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            if let Ok(mut transform) = transforms.get_mut(drag.entity) {
                                transform.translation = Val2::ZERO;
                            }
                            let graph_size = graph.size() * graph.inverse_scale_factor;
                            let Some(Value::Curve(curve)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| module_parameter(item, parameter))
                            else {
                                return;
                            };
                            let Some(mut key) = curve.keys.get(key_index).copied() else {
                                return;
                            };
                            let previous = key_index
                                .checked_sub(1)
                                .and_then(|index| curve.keys.get(index))
                                .map_or(0.0, |key| key.time + 0.001);
                            let next = curve
                                .keys
                                .get(key_index + 1)
                                .map_or(1.0, |key| key.time - 0.001);
                            key.time =
                                (key.time + drag.distance.x / graph_size.x).clamp(previous, next);
                            key.value = (key.value - drag.distance.y / graph_size.y * (max - min))
                                .clamp(min, max);
                            session.set_curve_key(module, parameter, key_index, key);
                            workspace.complex = Some(ComplexSelection {
                                module,
                                input: input_index,
                                key: key_index,
                            });
                        },
                    );
            }
        });
    spawn_complex_controls(parent, curve.keys.get(selected_key).copied(), None, step);
}

fn spawn_gradient_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    gradient: &aestra_bevy::Gradient,
    selected_key: usize,
) {
    parent.spawn((
        Text::new(format!("{}  ·  {}", input.display_name, input.description)),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
    ));
    parent
        .spawn((
            CurveGraph,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(82.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::TIMELINE_BG),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|graph| {
            for index in 0..64 {
                let time = index as f32 / 63.0;
                let color = gradient.sample(time);
                graph.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(time * 100.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0 / 64.0 + 0.1),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(color[0], color[1], color[2], color[3])),
                ));
            }
            for (key_index, key) in gradient.keys.iter().enumerate() {
                let parameter = input.name;
                graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        UiTransform::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            bottom: Val::Px(4.0),
                            width: Val::Px(13.0),
                            height: Val::Px(20.0),
                            border: UiRect::all(Val::Px(if key_index == selected_key {
                                2.0
                            } else {
                                1.0
                            })),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(
                            key.color[0],
                            key.color[1],
                            key.color[2],
                            key.color[3],
                        )),
                        BorderColor::all(if key_index == selected_key {
                            Color::WHITE
                        } else {
                            theme::BORDER_BRIGHT
                        }),
                    ))
                    .observe(
                        move |click: On<Pointer<Click>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<WorkspaceState>| {
                            if click.button == PointerButton::Primary {
                                workspace.complex = Some(ComplexSelection {
                                    module,
                                    input: input_index,
                                    key: key_index,
                                });
                                session.ui_revision += 1;
                            }
                        },
                    )
                    .observe(
                        |drag: On<Pointer<Drag>>, mut transforms: Query<&mut UiTransform>| {
                            if drag.button == PointerButton::Primary
                                && let Ok(mut transform) = transforms.get_mut(drag.entity)
                            {
                                transform.translation = Val2::px(drag.distance.x, 0.0);
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut transforms: Query<&mut UiTransform>,
                              mut session: ResMut<EditorSession>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            if let Ok(mut transform) = transforms.get_mut(drag.entity) {
                                transform.translation = Val2::ZERO;
                            }
                            let width = graph.size().x * graph.inverse_scale_factor;
                            let Some(Value::Gradient(gradient)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| module_parameter(item, parameter))
                            else {
                                return;
                            };
                            let Some(mut key) = gradient.keys.get(key_index).copied() else {
                                return;
                            };
                            let previous = key_index
                                .checked_sub(1)
                                .and_then(|index| gradient.keys.get(index))
                                .map_or(0.0, |key| key.time + 0.001);
                            let next = gradient
                                .keys
                                .get(key_index + 1)
                                .map_or(1.0, |key| key.time - 0.001);
                            key.time = (key.time + drag.distance.x / width).clamp(previous, next);
                            session.set_gradient_key(module, parameter, key_index, key);
                        },
                    );
            }
        });
    spawn_complex_controls(parent, None, gradient.keys.get(selected_key).copied(), 0.05);
}

fn spawn_complex_controls(
    parent: &mut ChildSpawnerCommands,
    curve_key: Option<CurveKey>,
    color_key: Option<ColorKey>,
    value_step: f32,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(34.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|controls| {
            stack_button(controls, "+ KEY", EditorAction::AddComplexKey, 48.0);
            stack_button(controls, "− KEY", EditorAction::DeleteComplexKey, 48.0);
            let time = curve_key
                .map(|key| key.time)
                .or_else(|| color_key.map(|key| key.time));
            if let Some(time) = time {
                controls.spawn((
                    Text::new(format!("Time {time:.3}")),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
                mini_button(controls, "−", EditorAction::AdjustComplexTime(-1));
                mini_button(controls, "+", EditorAction::AdjustComplexTime(1));
            }
            if let Some(key) = curve_key {
                controls.spawn((
                    Text::new(format!("Value {:.3}  step {value_step:.2}", key.value)),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
                mini_button(controls, "−", EditorAction::AdjustCurveValue(-1));
                mini_button(controls, "+", EditorAction::AdjustCurveValue(1));
            }
            if let Some(key) = color_key {
                for (channel, label) in ["R", "G", "B", "A"].into_iter().enumerate() {
                    controls.spawn((
                        Text::new(format!("{label}{:.2}", key.color[channel])),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    mini_button(
                        controls,
                        "−",
                        EditorAction::AdjustGradientChannel(channel as u8, -1),
                    );
                    mini_button(
                        controls,
                        "+",
                        EditorAction::AdjustGradientChannel(channel as u8, 1),
                    );
                }
            }
        });
}

fn spawn_ruler(parent: &mut ChildSpawnerCommands, duration: f32) {
    for index in 0..=7 {
        parent.spawn((
            Text::new(format!("{:.1}", index as f32 / 7.0 * duration)),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(index as f32 / 7.0 * 100.0),
                top: Val::Px(5.0),
                ..default()
            },
        ));
    }
}

fn spawn_status_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                grid_row: GridPlacement::start(4),
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|bar| {
            let (compile_status, compile_color) = compile_status(session);
            bar.spawn_empty()
                .apply_scene(ui_shell::feathers_plain_button())
                .insert((
                    CompileStatusButton,
                    EditorAction::ShowDockPanel(DockPanel::Diagnostics),
                    FeathersActionButton,
                    AccessibleLabel(localizer.text(compile_status)),
                    Node {
                        height: Val::Px(20.0),
                        padding: UiRect::horizontal(Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button.spawn((
                        CompileStatusDot,
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            margin: UiRect::right(Val::Px(7.0)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(compile_color),
                        Pickable::IGNORE,
                    ));
                    button.spawn((
                        CompileStatusLabel,
                        Text::new(localizer.text(compile_status)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(compile_color),
                        Pickable::IGNORE,
                    ));
                });
        });
}

fn compile_status(session: &EditorSession) -> (&'static str, Color) {
    let current_errors = session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .count();
    let pending_errors = session.pending_change.as_ref().map_or(0, |pending| {
        pending
            .diagnostics
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    });
    let warnings = session
        .diagnostics
        .diagnostics
        .iter()
        .chain(
            session
                .pending_change
                .iter()
                .flat_map(|pending| pending.diagnostics.diagnostics.iter()),
        )
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .count();

    if current_errors > 0 {
        ("compile-failed", Color::srgb(1.0, 0.38, 0.32))
    } else if pending_errors > 0 {
        ("compile-preview-blocked", Color::srgb(1.0, 0.74, 0.30))
    } else if warnings > 0 {
        ("compile-with-warnings", Color::srgb(1.0, 0.74, 0.30))
    } else {
        ("compile-compiled", Color::srgb(0.35, 0.88, 0.57))
    }
}

fn panel_heading(parent: &mut ChildSpawnerCommands, title: &str, meta: &str) {
    parent
        .spawn((
            Node {
                height: Val::Px(34.0),
                width: Val::Percent(100.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn((
                Text::new(meta),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn mini_button(parent: &mut ChildSpawnerCommands, label: &str, action: EditorAction) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

fn inspector_action_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: EditorAction,
    help: Option<&str>,
) {
    let mut button = parent.spawn_empty();
    button.apply_scene(ui_shell::feathers_button()).insert((
        action,
        FeathersActionButton,
        AccessibleLabel(label.to_owned()),
        Node {
            width: Val::Auto,
            height: Val::Px(28.0),
            margin: UiRect::horizontal(Val::Px(12.0)),
            padding: UiRect::horizontal(Val::Px(10.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));
    if let Some(help) = help {
        button.insert((
            InspectorHelp(help.to_owned()),
            RelativeCursorPosition::default(),
        ));
    }
    button.with_children(|button| {
        button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
    });
}

fn localized_action_button(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    action: EditorAction,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(message_id)),
            Node {
                width: Val::Auto,
                height: Val::Px(28.0),
                margin: UiRect::horizontal(Val::Px(12.0)),
                padding: UiRect::horizontal(Val::Px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                LocalizedText(message_id),
                Text::new(localizer.text(message_id)),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn module_palette_keyboard(
    mut input: MessageReader<KeyboardInput>,
    mut palette: ResMut<ModulePaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if !palette.open {
        return;
    }
    let mut changed = false;
    for event in input.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match event.key_code {
            KeyCode::Escape => {
                palette.open = false;
                changed = true;
            }
            KeyCode::Backspace => {
                changed |= palette.query.pop().is_some();
            }
            _ => {
                if let Some(text) = &event.text {
                    let clean = text.chars().filter(|character| !character.is_control());
                    let previous = palette.query.len();
                    palette.query.extend(clean);
                    changed |= palette.query.len() != previous;
                }
            }
        }
    }
    if changed {
        session.ui_revision += 1;
    }
}

fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    palette: Res<ModulePaletteState>,
    mut workspace: ResMut<WorkspaceState>,
    mut layout: ResMut<WorkspaceLayout>,
    settings_resources: (ResMut<EditorSettings>, ResMut<SettingsPersistence>),
) {
    let (mut settings, mut settings_persistence) = settings_resources;
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if keys.just_pressed(KeyCode::Escape) {
        let context_was_open = menu.tab_context.take().is_some();
        menu.open = None;
        menu.panels_open = false;
        menu.show_about = false;
        if context_was_open {
            session.ui_revision += 1;
        }
    }
    if control && keys.just_pressed(KeyCode::KeyN) && confirm_discard(&session, &settings) {
        session.new_effect();
        workspace.complex = None;
    }
    if control && keys.just_pressed(KeyCode::KeyO) {
        open_effect_dialog(&mut session, &settings);
        workspace.complex = None;
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        save_session(&mut session, shift);
    }
    if control && keys.just_pressed(KeyCode::KeyZ) {
        session.undo();
    }
    if control && keys.just_pressed(KeyCode::KeyY) {
        session.redo();
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        session.duplicate_selected_layer();
    }
    if control && keys.just_pressed(KeyCode::Enter) {
        session.add_layer();
    }
    if keys.just_pressed(KeyCode::Delete) && preview_selected_layer_deletion(&mut session) {
        reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
        workspace.complex = None;
    }
    if keys.just_pressed(KeyCode::Space) {
        session.playing = !session.playing;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        session.restart();
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        session.step_frame(-1);
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        session.step_frame(1);
    }
    if keys.just_pressed(KeyCode::KeyG) && !control {
        menu.show_grid = !menu.show_grid;
        settings.preview.show_grid = menu.show_grid;
        session.ui_revision += 1;
        persist_editor_settings(&settings, &mut settings_persistence, &mut session);
    }
}

fn queue_feathers_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<EditorAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn audit_editor_action_controls(controls: Query<Entity, UnclassifiedEditorActionControl>) {
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "editor action control {entity:?} must use FeathersActionButton or be explicitly \
             marked EditorNativeControl"
        );
    }
}

#[allow(clippy::type_complexity)]
fn handle_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &EditorAction,
            Option<&LayerRow>,
            Option<&DockTab>,
            Option<&DockCloseButton>,
            Option<&DiagnosticsFilterButton>,
            Option<&SettingsCategoryButton>,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Option<&CompiledPlanRow>,
            Option<&CompileStatusButton>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    editor_resources: (
        Res<EffectCatalog>,
        Res<EditorModuleRegistry>,
        ResMut<ModulePaletteState>,
        ResMut<WorkspaceState>,
        ResMut<WorkspaceLayout>,
        ResMut<DiagnosticsPanelState>,
        ResMut<InspectorFocus>,
        ResMut<ProfilerState>,
        ResMut<SettingsPanelState>,
        ResMut<EditorSettings>,
        ResMut<SettingsPersistence>,
        ResMut<UiScale>,
        ResMut<Localizer>,
    ),
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let (
        catalog,
        registry,
        mut palette,
        mut workspace,
        mut layout,
        mut diagnostics_panel,
        mut inspector_focus,
        mut profiler,
        mut settings_panel,
        mut settings,
        mut settings_persistence,
        mut ui_scale,
        mut localizer,
    ) = editor_resources;
    for (
        entity,
        interaction,
        action,
        layer_row,
        dock_tab,
        dock_close,
        diagnostics_filter,
        settings_category,
        feathers_action,
        pending_feathers_activation,
        compiled_plan_row,
        compile_status,
        disabled,
        mut background,
    ) in &mut buttons
    {
        if disabled.is_some() {
            if feathers_action.is_none() {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match *interaction {
            Interaction::Hovered => {
                if feathers_action.is_none() {
                    background.0 = theme::BUTTON_HOVER;
                }
                if let EditorAction::ToggleMenu(kind) = *action
                    && let Some(next) = menu_after_hover(menu.open, kind)
                    && menu.open != Some(next)
                {
                    menu.open = Some(next);
                    menu.panels_open = false;
                }
            }
            Interaction::None => {
                if feathers_action.is_none() {
                    background.0 = if let Some(row) = layer_row {
                        if row.0 == session.selected_layer_index() {
                            theme::SELECTION
                        } else {
                            theme::PANEL_DARK
                        }
                    } else if let Some(tab) = dock_tab {
                        let active = layout.is_active(tab.0);
                        if active {
                            theme::PANEL
                        } else {
                            theme::PANEL_DARK
                        }
                    } else if dock_close.is_some() {
                        Color::NONE
                    } else if let Some(filter) = diagnostics_filter {
                        if diagnostics_panel.filter == filter.0 {
                            theme::SELECTION
                        } else {
                            theme::BUTTON
                        }
                    } else if let Some(category) = settings_category {
                        if settings_panel.category == category.0 {
                            theme::SELECTION
                        } else {
                            theme::PANEL_DARK
                        }
                    } else if compiled_plan_row.is_some() {
                        if matches!(
                            *action,
                            EditorAction::SelectCompiledTarget(target)
                                if target == session.selection.primary
                        ) {
                            theme::SELECTION
                        } else {
                            theme::PANEL
                        }
                    } else if compile_status.is_some() {
                        theme::PANEL_DARK
                    } else {
                        theme::BUTTON
                    };
                }
            }
            Interaction::Pressed => {
                if feathers_action.is_some() {
                    if pending_feathers_activation.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                if let EditorAction::ToggleMenu(kind) = *action {
                    if menu.tab_context.take().is_some() {
                        session.ui_revision += 1;
                    }
                    menu.panels_open = false;
                    menu.open = if menu.open == Some(kind) {
                        None
                    } else {
                        Some(kind)
                    };
                    continue;
                }
                if matches!(*action, EditorAction::TogglePanelsSubmenu) {
                    menu.panels_open = !menu.panels_open;
                    continue;
                }
                let keep_view_menu_open = matches!(*action, EditorAction::ToggleDockPanel(_));
                if !keep_view_menu_open {
                    menu.open = None;
                    menu.panels_open = false;
                }
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                match *action {
                    EditorAction::NewEffect => {
                        if confirm_discard(&session, &settings) {
                            session.new_effect();
                            workspace.complex = None;
                        }
                    }
                    EditorAction::OpenEffect => {
                        open_effect_dialog(&mut session, &settings);
                        workspace.complex = None;
                    }
                    EditorAction::OpenCatalog(index) => {
                        if confirm_discard(&session, &settings) {
                            if let Some(entry) = catalog.entries.get(index)
                                && let Err(error) = session.open(&entry.path)
                            {
                                session.status = format!("Open failed: {error}");
                            }
                            workspace.complex = None;
                        } else {
                            session.status = "Open cancelled".into();
                        }
                    }
                    EditorAction::TogglePlayback => session.playing = !session.playing,
                    EditorAction::Restart => session.restart(),
                    EditorAction::StepFrame(direction) => session.step_frame(direction),
                    EditorAction::AdjustPreviewSeed(direction) => {
                        session.adjust_preview_seed(direction);
                    }
                    EditorAction::Save => save_session(&mut session, false),
                    EditorAction::SaveAs => save_session(&mut session, true),
                    EditorAction::Exit => {
                        if confirm_discard(&session, &settings) {
                            commands.write_message(AppExit::Success);
                        } else {
                            session.status = "Exit cancelled".into();
                        }
                    }
                    EditorAction::Undo => session.undo(),
                    EditorAction::Redo => session.redo(),
                    EditorAction::AddLayer => session.add_layer(),
                    EditorAction::AddSpriteMaterial => session.add_sprite_material(),
                    EditorAction::AddGridFlipbook => session.add_grid_flipbook(),
                    EditorAction::DuplicateLayer => {
                        session.duplicate_selected_layer();
                        workspace.complex = None;
                    }
                    EditorAction::DeleteLayer => {
                        if preview_selected_layer_deletion(&mut session) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.complex = None;
                        }
                    }
                    EditorAction::SelectLayer(index) => {
                        session.select_layer(index);
                        workspace.complex = None;
                    }
                    EditorAction::ToggleInspectorSection(section) => {
                        if toggle_persisted_inspector_section(&session, &mut settings, section) {
                            session.ui_revision += 1;
                            persist_editor_settings(
                                &settings,
                                &mut settings_persistence,
                                &mut session,
                            );
                        }
                    }
                    EditorAction::EffectDuration(delta) => {
                        session.adjust_effect_duration(delta);
                    }
                    EditorAction::OpenModulePalette(stage) => {
                        palette.open = true;
                        palette.stage = stage;
                        palette.query.clear();
                        session.ui_revision += 1;
                    }
                    EditorAction::CloseModulePalette => {
                        palette.open = false;
                        session.ui_revision += 1;
                    }
                    EditorAction::AddModule(index) => {
                        let module = registry
                            .0
                            .iter()
                            .nth(index)
                            .and_then(|metadata| registry.0.instantiate(&metadata.type_id));
                        if let Some(module) = module {
                            session.add_module(module);
                            palette.open = false;
                        } else {
                            session.status = "Module is unavailable in the registry".into();
                        }
                    }
                    EditorAction::AddSpriteRenderer => {
                        session.add_sprite_renderer();
                        palette.open = false;
                    }
                    EditorAction::AddFlipbookRenderer => {
                        session.add_flipbook_renderer();
                        palette.open = false;
                    }
                    EditorAction::SetModuleChoice {
                        module,
                        input,
                        choice,
                    } => set_module_choice(&mut session, &registry.0, module, input, choice),
                    EditorAction::EditComplexInput(module, input) => {
                        reveal_dock_panel(&mut layout, &mut session, DockPanel::Curves);
                        workspace.complex = Some(ComplexSelection {
                            module,
                            input,
                            key: 0,
                        });
                        session.ui_revision += 1;
                    }
                    EditorAction::AddComplexKey => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut workspace,
                        ComplexKeyEdit::Add,
                    ),
                    EditorAction::DeleteComplexKey => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut workspace,
                        ComplexKeyEdit::Delete,
                    ),
                    EditorAction::AdjustComplexTime(direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut workspace,
                        ComplexKeyEdit::Time(direction),
                    ),
                    EditorAction::AdjustCurveValue(direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut workspace,
                        ComplexKeyEdit::CurveValue(direction),
                    ),
                    EditorAction::AdjustGradientChannel(channel, direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut workspace,
                        ComplexKeyEdit::GradientChannel(channel, direction),
                    ),
                    EditorAction::MoveModule(id, direction) => {
                        session.move_module(id, direction);
                    }
                    EditorAction::DuplicateModule(id) => session.duplicate_module(id),
                    EditorAction::DeleteModule(id) => {
                        if preview_module_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.complex = None;
                        }
                    }
                    EditorAction::SetRendererMaterial(id, index) => {
                        if let Some(material) = session
                            .effect
                            .materials
                            .get(index)
                            .map(|material| material.id)
                        {
                            session.set_renderer_material(id, material);
                        }
                    }
                    EditorAction::SetRendererBlend(id, blend) => {
                        session.set_renderer_blend(id, blend);
                    }
                    EditorAction::SetRendererTexture(id, index) => {
                        let texture = index
                            .and_then(|index| session.effect.assets.get(index))
                            .filter(|asset| asset.kind == aestra_bevy::AssetKind::Texture)
                            .map(|asset| asset.id);
                        session.set_renderer_texture(id, texture);
                    }
                    EditorAction::SetRendererFlipbook(id, index) => {
                        if let Some(flipbook) = session
                            .effect
                            .flipbooks
                            .get(index)
                            .map(|flipbook| flipbook.id)
                        {
                            session.set_renderer_flipbook(id, flipbook);
                        }
                    }
                    EditorAction::SetFlipbookTimeSource(id, value) => {
                        session.set_flipbook_time_source(id, value);
                    }
                    EditorAction::SetFlipbookPlayback(id, value) => {
                        session.set_flipbook_playback(id, value);
                    }
                    EditorAction::DuplicateRenderer(id) => session.duplicate_renderer(id),
                    EditorAction::DeleteRenderer(id) => {
                        if preview_renderer_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.complex = None;
                        }
                    }
                    EditorAction::ApplyPendingChange => {
                        session.apply_pending_change();
                    }
                    EditorAction::DiscardPendingChange => {
                        session.discard_pending_change();
                    }
                    EditorAction::SetDiagnosticsFilter(filter) => {
                        if diagnostics_panel.filter != filter {
                            diagnostics_panel.filter = filter;
                            session.ui_revision += 1;
                        }
                    }
                    EditorAction::SelectDiagnostic { source, index } => {
                        if navigate_to_diagnostic(&mut session, source, index) {
                            workspace.complex = None;
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Inspector);
                        }
                    }
                    EditorAction::SelectCompiledTarget(target) => {
                        workspace.complex = None;
                        if focus_compiled_target(&mut session, &mut inspector_focus, target) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Inspector);
                        } else {
                            session.status =
                                "Compiled target exists only in the pending transaction".into();
                            session.ui_revision += 1;
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                        }
                    }
                    EditorAction::ResetProfilerPeaks => {
                        profiler.reset_peaks();
                        session.status = "Profiler peaks and history reset".into();
                    }
                    EditorAction::SelectDockPanel(panel) => {
                        if layout.activate(panel) {
                            session.ui_revision += 1;
                            if let Err(error) = layout.save() {
                                warn!("failed to save editor workspace layout: {error}");
                            }
                        }
                    }
                    EditorAction::CloseDockPanel(panel) => {
                        if layout.close(panel) {
                            session.ui_revision += 1;
                            session.status = format!(
                                "Closed {} panel · reopen it from View",
                                panel.title().to_ascii_lowercase()
                            );
                            if let Err(error) = layout.save() {
                                warn!("failed to save editor workspace layout: {error}");
                            }
                        }
                    }
                    EditorAction::ShowDockPanel(panel) => {
                        if layout.show(panel) {
                            session.ui_revision += 1;
                            session.status =
                                format!("Showing {} panel", panel.title().to_ascii_lowercase());
                            if let Err(error) = layout.save() {
                                warn!("failed to save editor workspace layout: {error}");
                            }
                        }
                    }
                    EditorAction::ToggleDockPanel(panel) => {
                        let was_visible = layout.is_visible(panel);
                        let changed = if was_visible {
                            layout.close(panel)
                        } else {
                            layout.show(panel)
                        };
                        if changed {
                            session.ui_revision += 1;
                            session.status = format!(
                                "{} {} panel",
                                if was_visible { "Hid" } else { "Showing" },
                                panel.title().to_ascii_lowercase()
                            );
                            if let Err(error) = layout.save() {
                                warn!("failed to save editor workspace layout: {error}");
                            }
                        }
                    }
                    EditorAction::FloatDockPanel(panel, pointer_position) => {
                        let available_size = [window.width(), (window.height() - 108.0).max(180.0)];
                        let origin = match window.position {
                            WindowPosition::At(position) => position,
                            _ => IVec2::new(80, 80),
                        };
                        let scale = window.scale_factor();
                        let position = [
                            origin.x as f32 + (pointer_position[0] - 92.0) * scale,
                            origin.y as f32 + (pointer_position[1] + 68.0) * scale,
                        ];
                        if layout.float_panel(panel, position, available_size) {
                            if let Err(error) = layout.save() {
                                warn!("failed to save editor workspace layout: {error}");
                            }
                            session.ui_revision += 1;
                            session.status =
                                format!("Floated {} panel", panel.title().to_ascii_lowercase());
                        }
                    }
                    EditorAction::ToggleGrid => {
                        menu.show_grid = !menu.show_grid;
                        settings.preview.show_grid = menu.show_grid;
                        persist_editor_settings(&settings, &mut settings_persistence, &mut session);
                    }
                    EditorAction::ResetWorkspaceLayout => {
                        *layout = WorkspaceLayout::default();
                        if let Err(error) = layout.save() {
                            warn!("failed to save editor workspace layout: {error}");
                        }
                        session.ui_revision += 1;
                        session.status = "Workspace layout reset".into();
                    }
                    EditorAction::SelectSettingsCategory(category) => {
                        if settings_panel.category != category {
                            settings_panel.category = category;
                            session.ui_revision += 1;
                        }
                    }
                    EditorAction::SetLocale(index) => {
                        if let Some(locale) = SUPPORTED_LOCALES.get(index)
                            && localizer.set_locale(locale)
                        {
                            settings.language.locale = localizer.locale().into();
                            session.ui_revision += 1;
                            persist_editor_settings(
                                &settings,
                                &mut settings_persistence,
                                &mut session,
                            );
                        }
                    }
                    EditorAction::ResetEditorSettings => {
                        match settings_persistence.replace_with_defaults() {
                            Ok(defaults) => {
                                *settings = defaults;
                                menu.show_grid = settings.preview.show_grid;
                                ui_scale.0 = settings.appearance.ui_scale;
                                localizer.set_locale(&settings.language.locale);
                                session.ui_revision += 1;
                                session.status = "Editor settings reset".into();
                            }
                            Err(error) => {
                                session.status = format!("Settings reset failed: {error}");
                            }
                        }
                    }
                    EditorAction::ShowAbout => menu.show_about = true,
                    EditorAction::CloseAbout => menu.show_about = false,
                    EditorAction::ToggleMenu(_) => unreachable!(),
                    EditorAction::TogglePanelsSubmenu => unreachable!(),
                }
            }
        }
    }
}

fn navigate_to_diagnostic(
    session: &mut EditorSession,
    source: DiagnosticSource,
    index: usize,
) -> bool {
    let diagnostic = match source {
        DiagnosticSource::Current => session.diagnostics.diagnostics.get(index),
        DiagnosticSource::Pending => session
            .pending_change
            .as_ref()
            .and_then(|pending| pending.diagnostics.diagnostics.get(index)),
    };
    let Some(diagnostic) = diagnostic else {
        session.status = "Diagnostic no longer exists".into();
        return false;
    };
    let path = diagnostic.path.clone();
    let code = diagnostic.code;
    let Some(target) = semantic_target_for_diagnostic_path(&session.effect, &path) else {
        session.status = format!("Diagnostic target no longer exists · {path}");
        return false;
    };
    if matches!(
        target,
        SemanticTarget::Emitter(_) | SemanticTarget::Module(_) | SemanticTarget::Renderer(_)
    ) {
        session.selection.primary = target;
    }
    session.status = format!("Selected {code:?} diagnostic · {path}");
    session.ui_revision += 1;
    true
}

fn semantic_target_exists(effect: &EffectAsset, target: SemanticTarget) -> bool {
    match target {
        SemanticTarget::Effect(id) => effect.id == id,
        SemanticTarget::Parameter(id) => effect.parameters.iter().any(|value| value.id == id),
        SemanticTarget::Emitter(id) => effect.emitters.iter().any(|emitter| emitter.id == id),
        SemanticTarget::Module(id) => effect
            .emitters
            .iter()
            .flat_map(|emitter| emitter.modules.iter())
            .any(|module| module.id == id),
        SemanticTarget::Renderer(id) => effect
            .emitters
            .iter()
            .flat_map(|emitter| emitter.renderers.iter())
            .any(|renderer| renderer.id == id),
        SemanticTarget::Event(id) => effect.events.iter().any(|event| event.id == id),
        SemanticTarget::Curve(_) | SemanticTarget::Gradient(_) => false,
    }
}

fn focus_compiled_target(
    session: &mut EditorSession,
    focus: &mut InspectorFocus,
    target: SemanticTarget,
) -> bool {
    if !semantic_target_exists(&session.effect, target) {
        return false;
    }
    if matches!(
        target,
        SemanticTarget::Emitter(_) | SemanticTarget::Module(_) | SemanticTarget::Renderer(_)
    ) {
        session.selection.primary = target;
    }
    focus.target = Some(target);
    focus.wait_frames = 2;
    focus.highlight = Some(target);
    focus.highlight_remaining = INSPECTOR_HIGHLIGHT_DURATION;
    session.status = format!("Selected compiled {target}");
    session.ui_revision += 1;
    true
}

fn semantic_target_for_diagnostic_path(effect: &EffectAsset, path: &str) -> Option<SemanticTarget> {
    if let Some(emitter_index) = diagnostic_collection_index(path, "emitters") {
        let emitter = effect.emitters.get(emitter_index)?;
        if let Some(module_index) = diagnostic_collection_index(path, "modules") {
            return emitter
                .modules
                .get(module_index)
                .map(|module| SemanticTarget::Module(module.id));
        }
        if let Some(renderer_index) = diagnostic_collection_index(path, "renderers") {
            return emitter
                .renderers
                .get(renderer_index)
                .map(|renderer| SemanticTarget::Renderer(renderer.id));
        }
        return Some(SemanticTarget::Emitter(emitter.id));
    }
    if let Some(parameter_index) = diagnostic_collection_index(path, "parameters") {
        return effect
            .parameters
            .get(parameter_index)
            .map(|parameter| SemanticTarget::Parameter(parameter.id));
    }
    if let Some(event_index) = diagnostic_collection_index(path, "events") {
        return effect
            .events
            .get(event_index)
            .map(|event| SemanticTarget::Event(event.id));
    }
    path.starts_with("effect")
        .then_some(SemanticTarget::Effect(effect.id))
}

fn diagnostic_collection_index(path: &str, collection: &str) -> Option<usize> {
    let marker = format!("{collection}[");
    let start = path.find(&marker)? + marker.len();
    let end = start + path[start..].find(']')?;
    path[start..end].parse().ok()
}

fn reveal_dock_panel(layout: &mut WorkspaceLayout, session: &mut EditorSession, panel: DockPanel) {
    if !layout.show(panel) {
        return;
    }
    session.ui_revision += 1;
    if let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}

fn dismiss_open_menus(
    buttons: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
    menu_surfaces: Query<&RelativeCursorPosition, With<MenuSurface>>,
    menu_buttons: Query<(&Interaction, Has<Pressed>), With<MenuButton>>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if menu.tab_context.take().is_some() {
        session.ui_revision += 1;
    }
    if menu.open.is_none() {
        return;
    }
    let pointer_over_menu = menu_surfaces
        .iter()
        .any(RelativeCursorPosition::cursor_over);
    let menu_button_pressed = menu_buttons.iter().any(|(interaction, feathers_pressed)| {
        *interaction == Interaction::Pressed || feathers_pressed
    });
    if should_dismiss_open_menu(pointer_over_menu, menu_button_pressed) {
        menu.open = None;
        menu.panels_open = false;
    }
}

fn should_dismiss_open_menu(pointer_over_menu: bool, menu_button_pressed: bool) -> bool {
    !pointer_over_menu && !menu_button_pressed
}

fn menu_after_hover(open: Option<MenuKind>, hovered: MenuKind) -> Option<MenuKind> {
    open.map(|_| hovered)
}

fn preview_selected_layer_deletion(session: &mut EditorSession) -> bool {
    if session.effect.emitters.len() <= 1 {
        session.status = "An effect must keep at least one layer".into();
        return false;
    }
    let id = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete emitter layer",
        EffectCommand::RemoveEmitter { id },
    ))
}

fn preview_module_deletion(session: &mut EditorSession, module: ModuleId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete module",
        EffectCommand::RemoveModule { emitter, module },
    ))
}

fn preview_renderer_deletion(session: &mut EditorSession, renderer: RendererId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete renderer",
        EffectCommand::RemoveRenderer { emitter, renderer },
    ))
}

fn set_module_choice(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    choice: u8,
) {
    let Some(module) = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == module_id)
    else {
        session.status = "Module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
    else {
        session.status = "Module input metadata is unavailable".into();
        return;
    };
    if !matches!(input.control, InputControl::Choice) {
        session.status = format!("{} is not a choice input", input.display_name);
        return;
    }
    let current = module_parameter(module, input.name);
    let shape = match choice {
        0 => EmitterShape::Point,
        1 => match current {
            Some(Value::Shape(EmitterShape::Circle { radius })) => EmitterShape::Circle { radius },
            _ => EmitterShape::Circle { radius: 12.0 },
        },
        2 => match current {
            Some(Value::Shape(EmitterShape::Ring { radius })) => EmitterShape::Ring { radius },
            _ => EmitterShape::Ring { radius: 12.0 },
        },
        3 => match current {
            Some(Value::Shape(EmitterShape::Cone { radius, depth })) => {
                EmitterShape::Cone { radius, depth }
            }
            _ => EmitterShape::Cone {
                radius: 12.0,
                depth: 24.0,
            },
        },
        _ => {
            session.status = "Choice is no longer available".into();
            return;
        }
    };
    session.set_module_parameter(module_id, input.name, Value::Shape(shape));
}

#[derive(Clone, Copy)]
enum ComplexKeyEdit {
    Add,
    Delete,
    Time(i8),
    CurveValue(i8),
    GradientChannel(u8, i8),
}

fn edit_complex_key(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    workspace: &mut WorkspaceState,
    edit: ComplexKeyEdit,
) {
    let Some(selection) = workspace.complex else {
        session.status = "Select a curve or gradient first".into();
        return;
    };
    let Some(module) = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == selection.module)
    else {
        session.status = "The selected module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(selection.input as usize))
    else {
        session.status = "The selected input metadata no longer exists".into();
        return;
    };
    let parameter = input.name;
    let control = input.control;
    let Some(value) = module_parameter(module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };

    match (value, edit) {
        (Value::Curve(curve), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let value = curve.sample(time);
            session.add_curve_key(
                selection.module,
                parameter,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let color = gradient.sample(time);
            session.add_gradient_key(
                selection.module,
                parameter,
                index,
                ColorKey::new(time, color),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Delete) => {
            if curve.keys.len() <= 2 {
                session.status = "A curve must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(curve.keys.len() - 1);
            session.remove_curve_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(curve.keys.len() - 2),
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Delete) => {
            if gradient.keys.len() <= 2 {
                session.status = "A gradient must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(gradient.keys.len() - 1);
            session.remove_gradient_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(gradient.keys.len() - 2),
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        (Value::Curve(curve), ComplexKeyEdit::CurveValue(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            let InputControl::Curve { step, min, max } = control else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Gradient(gradient), ComplexKeyEdit::GradientChannel(channel, direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            let Some(value) = key.color.get_mut(channel as usize) else {
                return;
            };
            *value = (*value + direction as f32 * 0.05).clamp(0.0, 1.0);
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        _ => session.status = "This edit does not apply to the selected property".into(),
    }
}

fn insertion_time(times: &[f32], selected: usize) -> (usize, f32) {
    if times.is_empty() {
        return (0, 0.5);
    }
    let selected = selected.min(times.len() - 1);
    if let Some(next) = times.get(selected + 1) {
        (selected + 1, (times[selected] + next) * 0.5)
    } else if selected > 0 {
        (selected, (times[selected - 1] + times[selected]) * 0.5)
    } else {
        (1, (times[0] + 1.0) * 0.5)
    }
}

fn bounded_key_time(times: &[f32], index: usize, value: f32) -> f32 {
    let previous = index
        .checked_sub(1)
        .and_then(|index| times.get(index))
        .map_or(0.0, |time| time + 0.001);
    let next = times.get(index + 1).map_or(1.0, |time| time - 0.001);
    value.clamp(previous, next)
}

fn open_effect_dialog(session: &mut EditorSession, settings: &EditorSettings) {
    if !confirm_discard(session, settings) {
        session.status = "Open cancelled".into();
        return;
    }
    let mut dialog = FileDialog::new().add_filter("Aestra effect", &["ron"]);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.pick_file() else {
        session.status = "Open cancelled".into();
        return;
    };
    if let Err(error) = session.open(&path) {
        session.status = format!("Open failed: {error}");
    }
}

fn confirm_discard(session: &EditorSession, settings: &EditorSettings) -> bool {
    if !session.dirty || !settings.general.confirm_unsaved_changes {
        return true;
    }
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description("Discard the unsaved changes to the current effect?")
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    )
}

fn persist_editor_settings(
    settings: &EditorSettings,
    persistence: &mut SettingsPersistence,
    session: &mut EditorSession,
) {
    match persistence.persist(settings) {
        Ok(()) => session.status = "Editor settings saved".into(),
        Err(error) => session.status = format!("Settings save failed: {error}"),
    }
}

fn save_session(session: &mut EditorSession, save_as: bool) {
    if !save_as && session.source_path.is_some() {
        if let Err(error) = session.save() {
            session.status = format!("Save failed: {error}");
        }
        return;
    }

    let file_name = format!("{}.aestra.ron", session.effect.id);
    let mut dialog = FileDialog::new()
        .add_filter("Aestra effect", &["ron"])
        .set_file_name(file_name);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.save_file() else {
        session.status = "Save cancelled".into();
        return;
    };
    if let Err(error) = session.save_as(path) {
        session.status = format!("Save failed: {error}");
    }
}

fn scrub_timeline(
    timeline: Query<(&Interaction, &RelativeCursorPosition), With<TimelineCanvas>>,
    mut session: ResMut<EditorSession>,
) {
    for (interaction, cursor) in &timeline {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(position) = cursor.normalized else {
            continue;
        };
        let time = position.x.clamp(0.0, 1.0) * session.playback_duration();
        session.seek_time(time);
    }
}

#[allow(clippy::type_complexity)]
fn update_menu_visibility(
    menu: Res<MenuState>,
    mut dropdowns: Query<(&MenuDropdown, &mut Node, &mut Visibility)>,
    mut panels_submenus: Query<
        &mut Node,
        (
            With<PanelsSubmenu>,
            Without<MenuDropdown>,
            Without<AboutOverlay>,
        ),
    >,
    mut about: Query<
        &mut Node,
        (
            With<AboutOverlay>,
            Without<MenuDropdown>,
            Without<PanelsSubmenu>,
        ),
    >,
) {
    if !menu.is_changed() {
        return;
    }
    for (dropdown, mut node, mut visibility) in &mut dropdowns {
        let visible = menu.open == Some(dropdown.0);
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut node in &mut panels_submenus {
        node.display = if menu.open == Some(MenuKind::View) && menu.panels_open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut about {
        node.display = if menu.show_about {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn scroll_inspector_to_focus(
    mut commands: Commands,
    mut focus: ResMut<InspectorFocus>,
    targets: Query<(Entity, &InspectorSemanticTarget)>,
) {
    let Some(target) = focus.target else {
        return;
    };
    if focus.wait_frames > 0 {
        focus.wait_frames -= 1;
        return;
    }
    if let Some((entity, _)) = targets
        .iter()
        .find(|(_, candidate)| candidate.target == target)
    {
        commands.trigger(ScrollIntoView { entity });
    }
    focus.target = None;
}

fn update_inspector_highlight(
    time: Res<Time>,
    mut focus: ResMut<InspectorFocus>,
    mut targets: Query<(&InspectorSemanticTarget, &mut BorderColor)>,
) {
    let Some(highlight) = focus.highlight else {
        return;
    };
    focus.highlight_remaining = (focus.highlight_remaining - time.delta_secs()).max(0.0);
    let strength = (focus.highlight_remaining / INSPECTOR_HIGHLIGHT_DURATION)
        .clamp(0.0, 1.0)
        .powi(2);
    for (target, mut border) in &mut targets {
        if target.target == highlight {
            *border = BorderColor::all(target.base_border.mix(&theme::ACCENT, strength));
        }
    }
    if focus.highlight_remaining == 0.0 {
        focus.highlight = None;
    }
}

fn begin_inspector_tooltip(
    over: On<Pointer<Over>>,
    helps: Query<(), With<InspectorHelp>>,
    mut state: ResMut<InspectorTooltipState>,
    mut commands: Commands,
) {
    if !helps.contains(over.entity) || state.target == Some(over.entity) {
        return;
    }
    if let Some(popup) = state.popup.take() {
        commands.entity(popup).despawn();
    }
    state.target = Some(over.entity);
    state.hovered_at = Some(Instant::now());
}

fn update_inspector_tooltip(
    mut commands: Commands,
    mut state: ResMut<InspectorTooltipState>,
    helps: Query<(&InspectorHelp, &RelativeCursorPosition)>,
    popups: Query<(), With<InspectorTooltipPopup>>,
) {
    if state.popup.is_some_and(|popup| !popups.contains(popup)) {
        state.popup = None;
    }
    let Some(target) = state.target else {
        return;
    };
    let Ok((help, cursor)) = helps.get(target) else {
        clear_inspector_tooltip(&mut commands, &mut state);
        return;
    };
    if !cursor.cursor_over() {
        clear_inspector_tooltip(&mut commands, &mut state);
        return;
    }
    if state.popup.is_some()
        || state
            .hovered_at
            .is_none_or(|started| started.elapsed() < INSPECTOR_TOOLTIP_DELAY)
    {
        return;
    }

    let text = help.0.clone();
    let mut popup = None;
    commands.entity(target).with_children(|target| {
        popup = Some(
            target
                .spawn((
                    InspectorTooltipPopup,
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Left,
                                align: PopoverAlign::Center,
                                gap: 8.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Right,
                                align: PopoverAlign::Center,
                                gap: 8.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 6.0,
                            },
                        ],
                        window_margin: 10.0,
                    },
                    OverrideClip,
                    GlobalZIndex(300),
                    Pickable::IGNORE,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(280.0),
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.65),
                        Val::Px(0.0),
                        Val::Px(2.0),
                        Val::Px(3.0),
                        Val::Px(5.0),
                    ),
                ))
                .with_children(|tooltip| {
                    tooltip.spawn((
                        Text::new(text),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                })
                .id(),
        );
    });
    state.popup = popup;
}

fn clear_inspector_tooltip(commands: &mut Commands, state: &mut InspectorTooltipState) {
    if let Some(popup) = state.popup.take() {
        commands.entity(popup).despawn();
    }
    state.target = None;
    state.hovered_at = None;
}

fn update_panel_visibility_labels(
    layout: Res<WorkspaceLayout>,
    localizer: Res<Localizer>,
    mut labels: Query<(&PanelVisibilityLabel, &mut Text)>,
) {
    if !layout.is_changed() && !localizer.is_changed() {
        return;
    }
    for (label, mut text) in &mut labels {
        text.0 = panel_visibility_label(&localizer, label.0, layout.is_visible(label.0));
    }
}

fn update_floating_window_titles(
    localizer: Res<Localizer>,
    mut windows: Query<(&NativeFloatingWindow, &mut Window)>,
) {
    if !localizer.is_changed() {
        return;
    }
    for (floating, mut window) in &mut windows {
        window.title = format!("{} — Aestra", localizer.text(floating.0.message_id()));
    }
}

fn handle_window_close_requests(
    mut close_requests: MessageReader<WindowCloseRequested>,
    primary: Single<Entity, With<PrimaryWindow>>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    for request in close_requests.read() {
        if request.window == *primary {
            if confirm_discard(&session, &settings) {
                commands.write_message(AppExit::Success);
            } else {
                session.status = "Exit cancelled".into();
            }
            continue;
        }
        let Ok(floating) = floating_windows.get(request.window) else {
            continue;
        };
        if layout.redock(floating.0) {
            if let Err(error) = layout.save() {
                warn!("failed to save editor workspace layout: {error}");
            }
            session.ui_revision += 1;
            session.status = format!(
                "Docked {} panel after closing its window",
                floating.0.title().to_ascii_lowercase()
            );
        }
    }
}

fn persist_native_window_geometry(
    mut moved: MessageReader<WindowMoved>,
    mut resized: MessageReader<WindowResized>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let mut changed = false;
    for event in moved.read() {
        let Ok(floating) = floating_windows.get(event.window) else {
            continue;
        };
        changed |= layout.update_floating_geometry(
            floating.0,
            Some([event.position.x as f32, event.position.y as f32]),
            None,
        );
    }
    for event in resized.read() {
        let Ok(floating) = floating_windows.get(event.window) else {
            continue;
        };
        changed |=
            layout.update_floating_geometry(floating.0, None, Some([event.width, event.height]));
    }
    if changed && let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}

fn sync_native_floating_windows(
    mut commands: Commands,
    session: Res<EditorSession>,
    editor_resources: UiBuildResources,
    windows: Query<(Entity, &NativeFloatingWindow)>,
    cameras: Query<(Entity, &NativeFloatingCamera)>,
    roots: Query<(Entity, &NativeFloatingUi)>,
) {
    for (entity, native) in &windows {
        if editor_resources
            .layout
            .floating
            .iter()
            .all(|floating| floating.panel != native.0)
        {
            commands.entity(entity).despawn();
            for (camera_entity, camera) in &cameras {
                if camera.0 == native.0 {
                    commands.entity(camera_entity).despawn();
                }
            }
            for (root_entity, root) in &roots {
                if root.panel == native.0 {
                    commands.entity(root_entity).despawn();
                }
            }
        }
    }

    let sources = PanelSources {
        session: &session,
        catalog: &editor_resources.catalog,
        registry: &editor_resources.registry,
        palette: &editor_resources.palette,
        diagnostics_panel: &editor_resources.diagnostics_panel,
        profiler: &editor_resources.profiler,
        settings: &editor_resources.settings,
        settings_panel: &editor_resources.settings_panel,
        settings_persistence: &editor_resources.settings_persistence,
        localizer: &editor_resources.localizer,
        preview_camera: *editor_resources.preview_camera,
    };
    for floating in &editor_resources.layout.floating {
        if windows.iter().any(|(_, native)| native.0 == floating.panel) {
            continue;
        }
        let window = commands
            .spawn((
                Window {
                    title: format!(
                        "{} — Aestra",
                        editor_resources.localizer.text(floating.panel.message_id())
                    ),
                    resolution: WindowResolution::new(
                        floating.size[0].round() as u32,
                        floating.size[1].round() as u32,
                    ),
                    position: WindowPosition::At(IVec2::new(
                        floating.position[0].round() as i32,
                        floating.position[1].round() as i32,
                    )),
                    resize_constraints: WindowResizeConstraints {
                        min_width: 260.0,
                        min_height: 180.0,
                        ..default()
                    },
                    resizable: true,
                    ..default()
                },
                CursorIcon::default(),
                NativeFloatingWindow(floating.panel),
            ))
            .id();
        let camera = commands
            .spawn((
                Camera2d,
                RenderTarget::Window(WindowRef::Entity(window)),
                RenderLayers::layer(31),
                NativeFloatingCamera(floating.panel),
            ))
            .id();
        spawn_native_floating_ui(
            &mut commands,
            floating.panel,
            window,
            camera,
            &editor_resources.workspace,
            sources,
        );
    }
}

fn rebuild_editor_ui(
    mut commands: Commands,
    session: Res<EditorSession>,
    editor_resources: UiBuildResources,
    mut rendered: ResMut<RenderedUiRevision>,
    root: Single<Entity, With<EditorRoot>>,
    contents: Query<Entity, With<EditorContent>>,
    floating_roots: Query<(Entity, &NativeFloatingUi)>,
) {
    if rendered.0 == session.ui_revision {
        return;
    }
    for content in &contents {
        commands.entity(content).despawn();
    }
    let sources = PanelSources {
        session: &session,
        catalog: &editor_resources.catalog,
        registry: &editor_resources.registry,
        palette: &editor_resources.palette,
        diagnostics_panel: &editor_resources.diagnostics_panel,
        profiler: &editor_resources.profiler,
        settings: &editor_resources.settings,
        settings_panel: &editor_resources.settings_panel,
        settings_persistence: &editor_resources.settings_persistence,
        localizer: &editor_resources.localizer,
        preview_camera: *editor_resources.preview_camera,
    };
    commands.entity(*root).with_children(|root| {
        spawn_editor_content(
            root,
            &editor_resources.menu,
            &editor_resources.workspace,
            &editor_resources.layout,
            sources,
        );
    });
    for (entity, floating_root) in &floating_roots {
        commands.entity(entity).despawn();
        if editor_resources
            .layout
            .floating
            .iter()
            .any(|floating| floating.panel == floating_root.panel)
        {
            spawn_native_floating_ui(
                &mut commands,
                floating_root.panel,
                floating_root.window,
                floating_root.camera,
                &editor_resources.workspace,
                sources,
            );
        }
    }
    rendered.0 = session.ui_revision;
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    session.advance_playback(time.delta_secs());
}

fn sync_rendered_preview(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut players: Query<(Entity, &mut EffectPlayer, &Transform), With<PreviewEffectPlayer>>,
) {
    let desired = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().clone());
    let Some(desired) = desired else {
        for (entity, _, _) in &mut players {
            commands.entity(entity).despawn();
        }
        return;
    };

    let Some((entity, mut player, transform)) = players.iter_mut().next() else {
        spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
        return;
    };
    if !std::sync::Arc::ptr_eq(player.effect(), &desired) {
        let transform = *transform;
        commands.entity(entity).despawn();
        spawn_preview_effect_player(&mut commands, &session, transform);
        return;
    }

    player.playing = false;
    player.speed = session.speed;
    if player.instance.seed() != session.preview_seed {
        player.set_seed(session.preview_seed);
    }
    if player.frame() != session.frame() {
        player.seek_frame(session.frame());
    }
}

fn update_preview(mut session: ResMut<EditorSession>, mut profiler: ResMut<ProfilerState>) {
    let compiled = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().clone());
    let mut samples = std::mem::take(&mut session.samples);
    let started = Instant::now();
    session.evaluate_preview(&mut samples);
    let elapsed = started.elapsed();
    session.samples = samples;
    if let Some(compiled) = compiled
        && profiler.record_cpu_frame(&compiled, &session.samples, elapsed)
    {
        session.ui_revision += 1;
    }
}

#[allow(clippy::type_complexity)]
fn update_profiler_labels(
    profiler: Res<ProfilerState>,
    mut labels: Query<
        (
            &mut Text,
            &mut TextColor,
            Option<&ProfilerMetricText>,
            Option<&ProfilerEmitterValue>,
            Option<&ProfilerHistorySummary>,
        ),
        Or<(
            With<ProfilerMetricText>,
            With<ProfilerEmitterValue>,
            With<ProfilerHistorySummary>,
        )>,
    >,
    mut bars: Query<(&ProfilerHistoryBar, &mut Node, &mut BackgroundColor)>,
) {
    if !profiler.is_changed() {
        return;
    }
    if let Some(profile) = &profiler.profile {
        for (mut text, mut color, metric, emitter, summary) in &mut labels {
            if let Some(metric) = metric {
                let (value, source) = profiler_metric_display(profile, metric.metric);
                match metric.part {
                    ProfilerMetricPart::Value => {
                        text.0 = value;
                        color.0 = theme::TEXT;
                    }
                    ProfilerMetricPart::Source => {
                        text.0 = profile_source_label(source).into();
                        color.0 = profile_source_color(source);
                    }
                }
            } else if let Some(emitter) = emitter {
                if let Some(profile) = profile.emitters.get(emitter.0) {
                    text.0 = profiler_emitter_value(profile);
                }
            } else if summary.is_some() {
                text.0 = profiler_history_summary(&profiler.cpu_history_ns);
            }
        }
    }

    let history_len = profiler.cpu_history_ns.len().min(PROFILER_HISTORY_SAMPLES);
    let first_bar = PROFILER_HISTORY_SAMPLES - history_len;
    let maximum = profiler
        .cpu_history_ns
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    for (bar, mut node, mut color) in &mut bars {
        if bar.0 < first_bar {
            node.height = Val::Px(1.0);
            color.0 = theme::ACCENT_DIM;
            continue;
        }
        let history_index = bar.0 - first_bar;
        let value = profiler
            .cpu_history_ns
            .get(history_index)
            .copied()
            .unwrap_or_default();
        node.height = Val::Px(2.0 + 72.0 * value as f32 / maximum as f32);
        color.0 = if bar.0 + 1 == PROFILER_HISTORY_SAMPLES {
            theme::ACCENT
        } else {
            theme::ACCENT_DIM
        };
    }
}

fn update_localized_text(
    localizer: Res<Localizer>,
    mut labels: Query<(&LocalizedText, &mut Text), Without<AboutDescription>>,
    mut about: Query<&mut Text, (With<AboutDescription>, Without<LocalizedText>)>,
) {
    if !localizer.is_changed() {
        return;
    }
    for (message, mut text) in &mut labels {
        text.0 = localizer.text(message.0);
    }
    let mut args = FluentArgs::new();
    args.set("version", env!("CARGO_PKG_VERSION"));
    for mut text in &mut about {
        text.0 = localizer.text_with("about-description", &args);
    }
}

#[allow(clippy::type_complexity)]
fn update_editor_labels(
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    preview_runtime: Query<Ref<EffectRuntimeStatus>, With<PreviewEffectPlayer>>,
    mut labels: Query<(
        &mut Text,
        Option<&PlaybackLabel>,
        Option<&TimeLabel>,
        Option<&InspectorTitle>,
        Option<&ParticleCountLabel>,
        Option<&DocumentMenuLabel>,
        Option<&DocumentToolbarLabel>,
    )>,
) {
    let runtime_changed = preview_runtime
        .iter()
        .any(|runtime| runtime.is_added() || runtime.is_changed());
    if !session.is_changed() && !localizer.is_changed() && !runtime_changed {
        return;
    }
    let backend = preview_runtime
        .iter()
        .next()
        .map_or("DETECTING GPU", |runtime| match runtime.active {
            ActiveBackend::Pending => "DETECTING GPU",
            ActiveBackend::Gpu => "NATIVE GPU",
            ActiveBackend::GpuReadback => "GPU READBACK",
            ActiveBackend::CpuReference => "CPU FALLBACK",
        });
    let layer = session.selected_layer();
    for (mut text, playback, time, title, count, document_menu, document_toolbar) in &mut labels {
        if playback.is_some() {
            text.0 = localizer.text(if session.playing {
                "toolbar-pause"
            } else {
                "toolbar-play"
            });
        } else if time.is_some() {
            text.0 = format!(
                "F{:05}  ·  {:02}:{:06.3}  /  00:{:06.3}  ·  {}",
                session.frame(),
                0,
                session.time(),
                session.playback_duration(),
                session.seek_status()
            );
        } else if title.is_some() {
            text.0 = layer.name.clone();
        } else if count.is_some() {
            text.0 = format!("{} LIVE PARTICLES  |  {backend}", session.samples.len());
        } else if document_menu.is_some() {
            let file = session
                .source_path
                .as_ref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled");
            text.0 = format!(
                "{}{}  |  {}",
                if session.dirty { "* " } else { "" },
                session.effect.name,
                file
            );
        } else if document_toolbar.is_some() {
            text.0 = format!(
                "{}  /  {}",
                session.effect.name.to_uppercase(),
                localizer.text("toolbar-choreography")
            );
        }
    }
}

fn update_compile_status(
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    mut labels: Query<(&mut Text, &mut TextColor), With<CompileStatusLabel>>,
    mut dots: Query<&mut BackgroundColor, With<CompileStatusDot>>,
) {
    if !session.is_changed() && !localizer.is_changed() {
        return;
    }
    let (label, color) = compile_status(&session);
    for (mut text, mut text_color) in &mut labels {
        text.0 = localizer.text(label);
        text_color.0 = color;
    }
    for mut background in &mut dots {
        background.0 = color;
    }
}

#[allow(clippy::type_complexity)]
fn update_history_actions(
    session: Res<EditorSession>,
    mut commands: Commands,
    items: Query<
        (Entity, Has<UndoMenuItem>, Has<RedoMenuItem>),
        Or<(With<UndoMenuItem>, With<RedoMenuItem>)>,
    >,
) {
    if !session.is_changed() {
        return;
    }
    for (entity, undo, redo) in &items {
        let enabled = (undo && session.can_undo()) || (redo && session.can_redo());
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

fn update_playhead(session: Res<EditorSession>, mut playhead: Query<&mut Node, With<Playhead>>) {
    if let Ok(mut node) = playhead.single_mut() {
        node.left = Val::Percent(session.time() / session.playback_duration() * 100.0);
    }
}

fn update_layer_selection(
    session: Res<EditorSession>,
    mut rows: Query<(&LayerRow, &mut BackgroundColor)>,
) {
    if !session.is_changed() {
        return;
    }
    for (row, mut color) in &mut rows {
        color.0 = if row.0 == session.selected_layer_index() {
            theme::SELECTION
        } else {
            theme::PANEL_DARK
        };
    }
}

fn layer_color(index: usize) -> Color {
    match index % 4 {
        0 => Color::srgb(0.48, 0.31, 0.98),
        1 => Color::srgb(0.17, 0.75, 0.95),
        2 => Color::srgb(0.98, 0.47, 0.21),
        _ => Color::srgb(0.84, 0.29, 0.72),
    }
}

fn layer_color_alpha(index: usize, alpha: f32) -> Color {
    layer_color(index).with_alpha(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_action_refresh_does_not_disable_unrelated_ui() {
        let mut app = App::new();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ));
        app.add_systems(Update, update_history_actions);

        let particle_color = Color::srgba(0.8, 0.4, 1.0, 0.75);
        let particle = app.world_mut().spawn(BackgroundColor(particle_color)).id();
        let undo = app
            .world_mut()
            .spawn((UndoMenuItem, BackgroundColor(theme::PANEL)))
            .id();

        app.update();

        let world = app.world();
        assert_eq!(
            world.get::<BackgroundColor>(particle).unwrap().0,
            particle_color
        );
        assert!(!world.entity(particle).contains::<InteractionDisabled>());
        assert!(world.entity(undo).contains::<InteractionDisabled>());
    }

    #[test]
    fn dock_drag_state_clears_even_if_the_dragged_tab_was_rebuilt() {
        let mut app = App::new();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        app.insert_resource(buttons);
        app.insert_resource(DockDragState(Some(DockPanel::Inspector)));
        app.add_systems(Update, clear_finished_dock_drag);
        let tab = app
            .world_mut()
            .spawn((
                DockTab(DockPanel::Inspector),
                UiTransform {
                    translation: Val2::px(20.0, 10.0),
                    ..default()
                },
                GlobalZIndex(160),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().resource::<DockDragState>().0,
            Some(DockPanel::Inspector)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();

        assert_eq!(app.world().resource::<DockDragState>().0, None);
        assert_eq!(
            app.world().get::<UiTransform>(tab).unwrap().translation,
            Val2::ZERO
        );
        assert!(!app.world().entity(tab).contains::<GlobalZIndex>());
    }

    #[test]
    fn panel_scroll_position_is_restored_after_a_rebuild() {
        let saved = Vec2::new(0.0, 184.0);
        let mut memory = ScrollMemoryState::default();
        memory.0.insert(ScrollMemoryKey::Inspector, saved);
        let mut app = App::new();
        app.insert_resource(memory);
        app.add_systems(Update, restore_scroll_positions);
        let rebuilt = app
            .world_mut()
            .spawn((
                PersistedScroll(ScrollMemoryKey::Inspector),
                ScrollPosition::default(),
            ))
            .id();

        app.update();

        assert_eq!(app.world().get::<ScrollPosition>(rebuilt).unwrap().0, saved);
    }

    #[test]
    fn bundled_effect_is_valid() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).expect("bundled effect should parse");
        assert_eq!(effect.format_version, 2);
        assert_eq!(effect.emitters.len(), 4);
    }

    #[test]
    fn editor_asset_root_contains_bundled_textures() {
        let asset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EDITOR_ASSET_ROOT);
        for source in [
            include_str!("../../assets/effects/ember_sigil.aestra.ron"),
            include_str!("../../assets/effects/plasma_burst.aestra.ron"),
        ] {
            let effect = EffectAsset::from_ron(source).unwrap();
            for asset in effect.assets {
                assert!(
                    asset_root.join(&asset.path).is_file(),
                    "missing bundled asset {}",
                    asset.path
                );
            }
        }
    }

    #[test]
    fn editor_preview_player_uses_the_compiled_effect_timeline_and_seed() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.preview_seed = 42;
        session.clock.seek_frame(37, session.playback_duration());

        let player = configured_preview_player(&session).unwrap();

        assert!(std::sync::Arc::ptr_eq(
            player.effect(),
            session.preview.as_ref().unwrap().effect()
        ));
        assert_eq!(player.frame(), session.frame());
        assert_eq!(player.instance.seed(), 42);
        assert!(!player.playing);
    }

    #[test]
    fn palette_search_uses_registry_names_categories_ids_and_tags() {
        let registry = ModuleRegistry::builtin();
        let motion = registry
            .iter()
            .find(|metadata| metadata.type_id.0.ends_with("motion"))
            .unwrap();
        assert!(module_matches(motion, "motion"));
        assert!(module_matches(motion, "forces"));
        assert!(module_matches(motion, "force"));
        assert!(!module_matches(motion, "color"));
    }

    #[test]
    fn inspector_choice_selects_the_requested_shape_directly() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "shape").is_some())
            .unwrap()
            .id;
        let registry = ModuleRegistry::builtin();

        set_module_choice(&mut session, &registry, module, 0, 3);

        assert_eq!(
            inspector_module_parameter(&session, module, "shape"),
            Some(Value::Shape(EmitterShape::Cone {
                radius: 12.0,
                depth: 24.0,
            }))
        );
    }

    #[test]
    fn inspector_number_edit_is_clamped_semantic_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spawn_rate").unwrap();
        let control = InspectorNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: Some(30.0),
        };

        assert!(apply_inspector_number(&mut session, control, 300.0));
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(30.0))
        );
        assert!(session.can_undo());

        session.undo();
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(original)
        );
    }

    #[test]
    fn inspector_sections_use_compact_defaults_and_persist_type_preferences() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut settings = EditorSettings::default();
        let emission = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.stage == StageKind::EmitterUpdate)
            .unwrap();
        let motion = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.stage == StageKind::ParticleUpdate)
            .unwrap();

        assert!(!inspector_module_collapsed(&settings, emission));
        assert!(inspector_module_collapsed(&settings, motion));

        assert!(toggle_persisted_inspector_section(
            &session,
            &mut settings,
            InspectorSection::Module(motion.id),
        ));
        assert!(!inspector_module_collapsed(&settings, motion));
        assert_eq!(
            settings
                .inspector
                .section_expansion
                .get(&inspector_module_key(motion)),
            Some(&true)
        );
    }

    #[test]
    fn inspector_wheel_uses_metadata_steps_and_modifier_precision() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let control = InspectorNumberControl {
            module,
            parameter: "spawn_rate",
            component: 0,
            kind: InspectorNumberKind::Scalar,
            step: 5.0,
            min: Some(0.0),
            max: None,
        };
        let Value::Scalar(initial) =
            inspector_module_parameter(&session, module, "spawn_rate").unwrap()
        else {
            panic!("spawn rate should be a scalar");
        };

        assert!(adjust_inspector_number_from_scroll(
            &mut session,
            control,
            1.0,
            numeric_scroll_multiplier(false, false),
        ));
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(initial + 5.0))
        );

        assert!(adjust_inspector_number_from_scroll(
            &mut session,
            control,
            -1.0,
            numeric_scroll_multiplier(true, false),
        ));
        assert_eq!(
            inspector_module_parameter(&session, module, "spawn_rate"),
            Some(Value::Scalar(initial + 4.5))
        );
        assert_eq!(numeric_scroll_multiplier(false, true), 10.0);
    }

    #[test]
    fn inspector_range_edit_preserves_ordering() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "lifetime").is_some())
            .unwrap()
            .id;
        let control = InspectorNumberControl {
            module,
            parameter: "lifetime",
            component: 0,
            kind: InspectorNumberKind::Range,
            step: 0.1,
            min: Some(0.05),
            max: None,
        };

        assert!(apply_inspector_number(&mut session, control, 99.0));
        let Value::Range(range) = inspector_module_parameter(&session, module, "lifetime").unwrap()
        else {
            panic!("lifetime should remain a range");
        };
        assert_eq!(range.min, range.max);
    }

    #[test]
    fn inspector_typing_does_not_rebuild_or_commit_until_final() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module_parameter(module, "spawn_rate").is_some())
            .unwrap()
            .id;
        let original = inspector_module_parameter(&session, module, "spawn_rate").unwrap();
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session);
        app.add_observer(handle_inspector_scalar_change);
        let control = app
            .world_mut()
            .spawn(InspectorNumberControl {
                module,
                parameter: "spawn_rate",
                component: 0,
                kind: InspectorNumberKind::Scalar,
                step: 5.0,
                min: Some(0.0),
                max: None,
            })
            .id();

        app.world_mut().trigger(ValueChange {
            source: control,
            value: 123.0_f32,
            is_final: false,
        });
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            inspector_module_parameter(session, module, "spawn_rate"),
            Some(original)
        );
        assert_eq!(session.ui_revision, revision);
    }

    #[test]
    fn inspector_number_rejects_non_finite_values() {
        assert_eq!(clamp_inspector_number(f32::INFINITY, None, None), None);
        assert_eq!(
            clamp_inspector_number(-5.0, Some(0.0), Some(10.0)),
            Some(0.0)
        );
    }

    #[test]
    fn inspector_input_localization_uses_fluent_and_preserves_custom_metadata() {
        let localizer = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localized_inspector_input(&localizer, "spawn_rate", "Spawn Rate", false),
            "Taux d’émission"
        );
        assert_eq!(
            localized_inspector_input(&localizer, "custom_gain", "Custom Gain", false),
            "Custom Gain"
        );
    }

    #[test]
    fn complex_key_helpers_preserve_ordering() {
        assert_eq!(insertion_time(&[0.0, 0.4, 1.0], 1), (2, 0.7));
        assert_eq!(insertion_time(&[0.0, 1.0], 1), (1, 0.5));
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, 2.0), 0.999);
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, -1.0), 0.001);
    }

    #[test]
    fn diagnostic_filters_match_only_the_selected_severity() {
        assert!(DiagnosticsFilter::All.matches(DiagnosticSeverity::Warning));
        assert!(DiagnosticsFilter::Errors.matches(DiagnosticSeverity::Error));
        assert!(!DiagnosticsFilter::Errors.matches(DiagnosticSeverity::Info));
        assert!(DiagnosticsFilter::Warnings.matches(DiagnosticSeverity::Warning));
        assert!(DiagnosticsFilter::Info.matches(DiagnosticSeverity::Info));
    }

    #[test]
    fn diagnostic_paths_resolve_to_semantic_targets() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        let emitter = &effect.emitters[1];
        assert_eq!(
            semantic_target_for_diagnostic_path(&effect, "effect.emitters[1].duration"),
            Some(SemanticTarget::Emitter(emitter.id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(
                &effect,
                "effect.emitters[1].modules[2].parameters.drag",
            ),
            Some(SemanticTarget::Module(emitter.modules[2].id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(
                &effect,
                "effect.emitters[1].renderers[0].renderer_type",
            ),
            Some(SemanticTarget::Renderer(emitter.renderers[0].id))
        );
        assert_eq!(
            semantic_target_for_diagnostic_path(&effect, "not-a-semantic-path"),
            None
        );
    }

    #[test]
    fn diagnostic_navigation_selects_the_owning_module() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let expected = session.effect.emitters[2].modules[1].id;
        session.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidValue,
            "effect.emitters[2].modules[1].parameters",
            "invalid test value",
        ));

        assert!(navigate_to_diagnostic(
            &mut session,
            DiagnosticSource::Current,
            0,
        ));
        assert_eq!(session.selection.primary, SemanticTarget::Module(expected));
        assert_eq!(session.selected_layer_index(), 2);
    }

    #[test]
    fn compile_footer_reports_success_and_failure() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let localizer = Localizer::new("en-US").unwrap();
        assert_eq!(localizer.text(compile_status(&session).0), "COMPILED");

        session.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidDuration,
            "effect.duration",
            "invalid test duration",
        ));
        assert_eq!(localizer.text(compile_status(&session).0), "COMPILE FAILED");
    }

    #[test]
    fn generated_code_uses_the_live_compiler_artifact() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let compiled = session.preview.as_ref().unwrap().effect();
        let instruction_count = compiled
            .emitters
            .iter()
            .flat_map(|emitter| {
                emitter
                    .execution
                    .emitter_update
                    .iter()
                    .chain(emitter.execution.particle_spawn.iter())
                    .chain(emitter.execution.particle_update.iter())
            })
            .count();

        assert_eq!(
            generated_code_status(&session, true).0,
            "WORKING EFFECT  ·  LIVE"
        );
        assert_eq!(compiled.emitters.len(), session.effect.emitters.len());
        assert_eq!(instruction_count, compiled.source_map.len());
        assert!(!compiled.particle_layout.attributes.is_empty());
        assert_eq!(
            instruction_opcode(&compiled.emitters[0].execution.emitter_update[0]),
            "EMIT"
        );
    }

    #[test]
    fn compiled_navigation_focuses_the_exact_inspector_target() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = SemanticTarget::Module(session.effect.emitters[3].modules[2].id);
        let mut focus = InspectorFocus::default();

        assert!(focus_compiled_target(&mut session, &mut focus, target));
        assert_eq!(session.selection.primary, target);
        assert_eq!(focus.target, Some(target));
        assert_eq!(focus.wait_frames, 2);
        assert_eq!(focus.highlight, Some(target));
        assert_eq!(focus.highlight_remaining, INSPECTOR_HIGHLIGHT_DURATION);
        assert_eq!(session.selected_layer_index(), 3);
    }

    #[test]
    fn profile_collection_preserves_deterministic_preview_state() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut baseline = session.preview.as_ref().unwrap().clone();
        let mut profiled = baseline.clone();
        baseline.seek(1.25);
        profiled.seek(1.25);
        let mut baseline_samples = Vec::new();
        let mut profiled_samples = Vec::new();
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);

        let mut profile = EffectProfile::from_compiled(profiled.effect());
        let time_before = profiled.time();
        let seed_before = profiled.seed();
        profile.record_cpu_frame(Duration::from_micros(125), &profiled_samples);
        assert_eq!(profiled.time(), time_before);
        assert_eq!(profiled.seed(), seed_before);
        assert_eq!(
            profile.alive_particles,
            ProfileValue::Measured(profiled_samples.len() as u32)
        );
        assert_eq!(
            profile
                .emitters
                .iter()
                .map(|emitter| emitter.alive_particles)
                .sum::<u32>(),
            profiled_samples.len() as u32
        );

        baseline.advance(1.0 / 60.0);
        profiled.advance(1.0 / 60.0);
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);
    }

    #[test]
    fn profiler_history_is_bounded_and_resettable() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let compiled = session.preview.as_ref().unwrap().effect();
        let mut profiler = ProfilerState::default();
        for frame in 0..(PROFILER_HISTORY_SAMPLES + 12) {
            profiler.record_cpu_frame(
                compiled,
                &session.samples,
                Duration::from_micros(frame as u64 + 1),
            );
        }
        assert_eq!(profiler.cpu_history_ns.len(), PROFILER_HISTORY_SAMPLES);
        profiler.reset_peaks();
        assert!(profiler.cpu_history_ns.is_empty());
    }

    #[test]
    fn scrollbar_only_appears_for_overflowing_content() {
        assert!(!vertical_scrollbar_needed(320.0, 320.0));
        assert!(!vertical_scrollbar_needed(320.0, 320.4));
        assert!(vertical_scrollbar_needed(320.0, 321.0));
    }

    #[test]
    fn feathers_settings_controls_apply_persisted_constraints() {
        let mut settings = EditorSettings::default();
        let mut menu = MenuState::default();

        assert!(apply_settings_toggle(
            &mut settings,
            &mut menu,
            SettingsToggle::ShowGrid,
            false,
        ));
        assert!(!settings.preview.show_grid);
        assert!(!menu.show_grid);

        assert!(apply_settings_integer(
            &mut settings,
            SettingsNumber::CaptureFrameRate,
            500,
        ));
        assert_eq!(settings.capture.frame_rate, 240);
        assert!(apply_settings_integer(
            &mut settings,
            SettingsNumber::ContactSheetColumns,
            0,
        ));
        assert_eq!(settings.capture.contact_sheet_columns, 1);

        assert!(apply_settings_scalar(
            &mut settings,
            SettingsNumber::UiScale,
            127.0,
        ));
        assert_eq!(settings.appearance.ui_scale, 1.25);
        assert_eq!(
            settings_number_input_value(&settings, SettingsNumber::UiScale),
            NumberInputValue::F32(125.0)
        );
    }

    #[test]
    fn open_menu_only_dismisses_for_clicks_outside_its_surfaces() {
        assert!(should_dismiss_open_menu(false, false));
        assert!(!should_dismiss_open_menu(true, false));
        assert!(!should_dismiss_open_menu(false, true));
    }

    #[test]
    fn feathers_menu_button_press_does_not_immediately_dismiss_its_menu() {
        let mut app = App::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        app.insert_resource(mouse);
        app.insert_resource(MenuState {
            open: Some(MenuKind::File),
            ..default()
        });
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ));
        app.world_mut()
            .spawn((MenuButton, Interaction::None, Pressed));
        app.add_systems(Update, dismiss_open_menus);

        app.update();

        assert_eq!(
            app.world().resource::<MenuState>().open,
            Some(MenuKind::File)
        );
    }

    #[test]
    fn hovering_switches_between_open_top_level_menus() {
        assert_eq!(menu_after_hover(None, MenuKind::Edit), None);
        assert_eq!(
            menu_after_hover(Some(MenuKind::File), MenuKind::Edit),
            Some(MenuKind::Edit)
        );
        assert_eq!(
            menu_after_hover(Some(MenuKind::View), MenuKind::View),
            Some(MenuKind::View)
        );
    }

    #[test]
    fn feathers_activation_queues_one_editor_action() {
        let mut app = App::new();
        app.add_observer(queue_feathers_action_activation);
        let action = app
            .world_mut()
            .spawn((
                EditorAction::Restart,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn menu_state_controls_feathers_popup_display_and_visibility() {
        let mut app = App::new();
        app.insert_resource(MenuState {
            open: Some(MenuKind::File),
            ..default()
        });
        app.add_systems(Update, update_menu_visibility);
        let file = app
            .world_mut()
            .spawn((
                MenuDropdown(MenuKind::File),
                Node::default(),
                Visibility::Hidden,
            ))
            .id();
        let edit = app
            .world_mut()
            .spawn((
                MenuDropdown(MenuKind::Edit),
                Node::default(),
                Visibility::Visible,
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<Node>(file).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Visibility>(file),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Node>(edit).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Visibility>(edit),
            Some(&Visibility::Hidden)
        );
    }

    #[test]
    fn panel_visibility_labels_use_checkbox_notation() {
        let localizer = Localizer::new("en-US").unwrap();
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::Diagnostics, true),
            "[x]  DIAGNOSTICS"
        );
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::Diagnostics, false),
            "[ ]  DIAGNOSTICS"
        );
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::GeneratedCode, true),
            "[x]  GENERATED CODE"
        );
        assert_eq!(
            panel_visibility_label(&localizer, DockPanel::Profiler, true),
            "[x]  PROFILER"
        );
    }

    #[test]
    fn localized_text_updates_when_the_locale_changes() {
        let mut app = App::new();
        app.insert_resource(Localizer::new("en-US").unwrap());
        app.add_systems(Update, update_localized_text);
        let label = app
            .world_mut()
            .spawn((LocalizedText("menu-file"), Text::new("stale")))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "File");

        app.world_mut()
            .resource_mut::<Localizer>()
            .set_locale("fr-FR");
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "Fichier");
    }
}
