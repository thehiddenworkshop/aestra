mod dock_ui;
mod docking;
mod feathers;
mod inspector;
mod localization;
mod recovery;
mod session;
mod settings;
mod theme;
mod timeline;
mod viewport;

use aestra_authoring::{ChangeKind, EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    AestraPlugin, BlendMode, ColorKey, CurveKey, Diagnostic, DiagnosticCode, DiagnosticSeverity,
    EffectAsset, EmitterShape, EmitterTransform, FlipbookPlaybackMode, FlipbookTimeSource,
    MaterialInput, MaterialProperties, ModuleId, ModuleInstance, ModuleParameters, RendererId,
    RendererProperties, StageKind, ValidationReport, Value,
};
use aestra_compiler::{InputControl, InputMetadata, ModuleMetadata, ModuleRegistry};
use aestra_runtime::{CompiledEffect, CompiledEmitter, Instruction, RuntimeStage};
use aestra_runtime::{EffectProfile, ProfileValue, ProfileValueSource};
#[cfg(test)]
use bevy::ui_widgets::Activate;
use bevy::{
    asset::AssetPlugin,
    camera::{RenderTarget, visibility::RenderLayers},
    feathers::{
        constants::fonts,
        containers::{group, group_body, group_header, pane_header},
        controls::{NumberInputValue, UpdateNumberInput},
        cursor::{EntityCursor, OverrideCursor},
        display::{label, label_dim},
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemedText},
        tokens,
    },
    gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings, TransformGizmoState},
    input::{ButtonState, keyboard::KeyboardInput},
    picking::events::{Click, Drag, DragDrop, DragEnd, DragStart, Out, Over, Pointer},
    picking::pointer::PointerButton,
    prelude::*,
    text::{EditableText, FontSource, TextEdit},
    ui::{Checked, InteractionDisabled, Pressed, RelativeCursorPosition},
    ui_widgets::{ScrollIntoView, ValueChange},
    window::{
        CursorIcon, CursorOptions, PrimaryWindow, SystemCursorIcon, WindowCloseRequested,
        WindowMoved, WindowPosition, WindowRef, WindowResizeConstraints, WindowResized,
        WindowResolution,
    },
};
#[cfg(test)]
use dock_ui::{clear_finished_dock_drag, dock_pane_background};
#[cfg(test)]
use docking::DockDragState;
use docking::{
    DockCloseButton, DockPanel, DockTab, DockTreeHost, DockingPlugin, DockingSet,
    NativeFloatingWindow, WorkspaceLayout,
};
#[cfg(test)]
use feathers::button::queue_action_activation as queue_feathers_action_activation;
pub(crate) use feathers::scenes as ui_shell;
#[cfg(test)]
use feathers::scroll::vertical_scrollbar_needed;
use feathers::{
    AestraFeathersPlugin, AestraFeathersSet,
    button::{
        EditorNativeControl, FeathersActionButton, PendingFeathersActivation,
        spawn_action_button as spawn_feathers_action_button, spawn_tool_button as mini_button,
    },
    combo_box::{ComboOption, spawn_action_menu, spawn_combo_control},
    panel::spawn_panel_heading as panel_heading,
    scroll::{PersistedScroll, spawn_vertical_scroll_area},
    tooltip::EditorTooltip,
};
use fluent_bundle::FluentArgs;
use inspector::*;
use localization::{Localizer, SUPPORTED_LOCALES};
use recovery::{RecoveryCandidate, RecoveryPersistence};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use session::EditorSession;
use settings::{EditorSettings, SettingsPersistence};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use timeline::{TimelinePlugin, TimelineSet, TimelineSnapMode, TimelineState};
use viewport::{
    EmitterTransformGizmoInteraction, EmitterTransformGizmoProxy, PreviewCameraController,
    PreviewDisplayMode, PreviewDisplayState, ViewportPlugin, ViewportSet,
    emitter_transform_from_bevy,
};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const EDITOR_ASSET_ROOT: &str = "../assets";
const MAX_PREVIEW_PARTICLE_LIMIT: usize = 384;
const RECOVERY_CLEANUP_RETRY_DELAY: Duration = Duration::from_secs(1);
const PROFILER_HISTORY_SAMPLES: usize = 96;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EditorSet {
    Setup,
    PreViewport,
    MainUpdate,
    UiRebuild,
    UiSync,
}

fn main() {
    let (mut settings, persistence) = SettingsPersistence::load();
    let localizer =
        Localizer::new(&settings.language.locale).expect("embedded Fluent catalogs must be valid");
    settings.language.locale = localizer.locale().into();
    let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
    let (mut recovery, recovery_candidate, recovery_diagnostic) = RecoveryPersistence::discover();
    if let Some(candidate) = recovery_candidate {
        recover_startup_session(&mut session, &mut recovery, candidate);
    } else if let Some(diagnostic) = recovery_diagnostic {
        session.status = diagnostic;
    }
    session.playing = settings.preview.play_on_open;
    if let Some(diagnostic) = persistence.diagnostic() {
        session.status = diagnostic.into();
    }
    let menu = MenuState {
        show_grid: settings.preview.show_grid,
        ..default()
    };
    let ui_scale = settings.appearance.ui_scale;
    let autosave = AutosaveState::new(&session, settings.general.autosave_enabled);
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(session)
        .insert_resource(settings)
        .insert_resource(persistence)
        .insert_resource(recovery)
        .insert_resource(localizer)
        .insert_resource(UiScale(ui_scale))
        .insert_resource(EffectCatalog::scan())
        .insert_resource(menu)
        .insert_resource(autosave)
        .init_resource::<DiagnosticsPanelState>()
        .init_resource::<ProfilerState>()
        .init_resource::<SettingsPanelState>()
        .init_resource::<ScrollMemoryState>()
        .init_resource::<WorkspaceState>()
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
        .add_plugins(AestraFeathersPlugin)
        .add_plugins(AestraPlugin)
        .add_plugins(DockingPlugin)
        .add_plugins(InspectorPlugin)
        .add_plugins(TimelinePlugin)
        .add_plugins(ViewportPlugin)
        .add_observer(handle_settings_toggle_change)
        .add_observer(handle_settings_integer_change)
        .add_observer(handle_settings_scalar_change)
        .add_systems(
            Startup,
            (setup_window_cursor, setup_editor_fonts, setup_editor)
                .chain()
                .in_set(EditorSet::Setup),
        )
        .add_systems(
            Update,
            (
                (
                    apply_editor_fonts,
                    keyboard_shortcuts,
                    handle_buttons,
                    handle_window_close_requests,
                    autosave_recovery,
                    dismiss_open_menus,
                    advance_playback,
                )
                    .chain()
                    .in_set(EditorSet::PreViewport),
                (
                    update_profiler_labels,
                    update_localized_text,
                    update_editor_labels,
                    update_transport_icons,
                    update_compile_status,
                    update_history_actions,
                )
                    .chain()
                    .in_set(EditorSet::MainUpdate),
                (
                    update_layer_selection,
                    update_menu_visibility,
                    update_grid_menu_check,
                    update_panel_visibility_labels,
                    remember_scroll_positions,
                    rebuild_editor_ui,
                    restore_scroll_positions,
                )
                    .chain()
                    .in_set(EditorSet::UiRebuild),
                sync_settings_number_inputs.in_set(EditorSet::UiSync),
            ),
        )
        .configure_sets(Startup, (ViewportSet::Setup, EditorSet::Setup).chain())
        .configure_sets(
            Update,
            (
                TimelineSet::Input,
                InspectorSet::Input,
                DockingSet::Input,
                AestraFeathersSet::Input,
                EditorSet::PreViewport,
                ViewportSet::Update,
                EditorSet::MainUpdate,
                DockingSet::Reconcile,
                EditorSet::UiRebuild,
                TimelineSet::Visuals,
                InspectorSet::Sync,
                DockingSet::Sync,
                AestraFeathersSet::Sync,
                EditorSet::UiSync,
            )
                .chain(),
        )
        .run();
}

#[derive(Component, Clone, Copy)]
enum EditorAction {
    NewEffect,
    OpenEffect,
    OpenCatalog(usize),
    TogglePlayback,
    StopPlayback,
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
    SetTimelineSnap(TimelineSnapMode),
    FrameTimeline,
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
    FramePreview,
    SetTransformGizmoMode(TransformGizmoMode),
    SetPreviewDisplayMode(PreviewDisplayMode),
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

#[derive(Resource, Default)]
struct DiagnosticsPanelState {
    filter: DiagnosticsFilter,
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
        profile.record_submitted_frame(effect, samples);
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
    AutosaveEnabled,
    ShowGrid,
    PlayOnOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsNumber {
    AutosaveInterval,
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
struct AutosaveState {
    document_key: String,
    observed_revision: u64,
    written_revision: Option<u64>,
    write_after: Instant,
    cleanup_after: Instant,
    enabled: bool,
    suspended: bool,
}

impl AutosaveState {
    fn new(session: &EditorSession, enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            document_key: recovery_document_key(session),
            observed_revision: session.document_revision(),
            written_revision: session.dirty.then_some(session.document_revision()),
            write_after: now,
            cleanup_after: now,
            enabled,
            suspended: false,
        }
    }
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
struct DiagnosticsFilterButton(DiagnosticsFilter);

#[derive(Component)]
struct SettingsCategoryButton(SettingsCategory);

#[derive(Component)]
struct SettingsToggleControl(SettingsToggle);

#[derive(Component)]
struct SettingsNumberControl(SettingsNumber);

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
    SubmittedInstances,
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
struct PanelsSubmenu;

#[derive(Component)]
struct PanelVisibilityLabel(DockPanel);

#[derive(Component)]
struct MenuDropdown(MenuKind);

#[derive(Component)]
struct MenuButton;

#[derive(Component)]
struct GridMenuCheck;

#[derive(Component)]
struct MenuSurface;

#[derive(Component)]
struct AboutOverlay;

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
struct PlaybackPlayIcon;

#[derive(Component)]
struct PlaybackPauseIcon;

#[derive(Component)]
struct CompileStatusLabel;

#[derive(Component)]
struct CompileStatusButton;

#[derive(Component)]
struct CompileStatusDot;

#[derive(Component)]
struct LayerRow(usize);

fn setup_editor(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    layout: Res<WorkspaceLayout>,
    localizer: Res<Localizer>,
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
    spawn_editor_ui(&mut commands, &menu, &layout, &session, &localizer);
    rendered.0 = session.ui_revision;
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
    layout: &WorkspaceLayout,
    session: &EditorSession,
    localizer: &Localizer,
) {
    commands
        .spawn(EditorRoot)
        .apply_scene(ui_shell::editor_root())
        .with_children(|root| {
            spawn_menu_bar(root, session, menu, layout, localizer);
            spawn_toolbar(root, session, localizer);
            spawn_editor_content(root, menu, localizer);
            spawn_status_bar(root, session, localizer);
            spawn_about_overlay(root, menu.show_about, localizer);
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
    localizer: &Localizer,
) {
    parent
        .spawn((EditorContent, RelativeCursorPosition::default()))
        .apply_scene(ui_shell::editor_content())
        .with_children(|content| {
            content.spawn(DockTreeHost);
            spawn_tab_context_menu(content, menu.tab_context, localizer);
        });
}

fn spawn_menu_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    menu: &MenuState,
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
            spawn_view_menu(bar, layout, menu.show_grid, localizer);
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
    show_grid: bool,
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
                    spawn_checkable_menu_item(
                        dropdown,
                        "view-toggle-grid",
                        "G",
                        EditorAction::ToggleGrid,
                        show_grid,
                        localizer,
                    );
                    for (message_id, shortcut, action) in [
                        ("view-frame-effect", "F", EditorAction::FramePreview),
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

fn spawn_checkable_menu_item(
    parent: &mut ChildSpawnerCommands,
    message_id: &'static str,
    shortcut: &str,
    action: EditorAction,
    checked: bool,
    localizer: &Localizer,
) {
    let label = localizer.text(message_id);
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_menu_item())
        .insert((
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
        ))
        .with_children(|item| {
            item.spawn((
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    margin: UiRect::right(Val::Px(7.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(theme::TEXT_MUTED),
                Pickable::IGNORE,
            ))
            .with_child((
                GridMenuCheck,
                Node {
                    width: Val::Px(6.0),
                    height: Val::Px(6.0),
                    border_radius: BorderRadius::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(if checked { theme::ACCENT } else { Color::NONE }),
                Pickable::IGNORE,
            ));
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
            bar.spawn((
                Node {
                    height: Val::Px(34.0),
                    padding: UiRect::all(Val::Px(2.0)),
                    column_gap: Val::Px(2.0),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                ThemeBackgroundColor(tokens::PANE_HEADER_BG),
                ThemeBorderColor(tokens::PANE_HEADER_BORDER),
            ))
            .with_children(|transport| {
                transport_button(
                    transport,
                    "toolbar-play",
                    EditorAction::TogglePlayback,
                    localizer,
                )
                .with_children(|button| {
                    spawn_play_icon(button, !session.playing);
                    spawn_pause_icon(button, session.playing);
                });
                transport_button(
                    transport,
                    "toolbar-stop",
                    EditorAction::StopPlayback,
                    localizer,
                )
                .with_child((
                    Node {
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        border_radius: BorderRadius::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
            });
            bar.spawn(Node {
                width: Val::Px(11.0),
                height: Val::Px(26.0),
                padding: UiRect::horizontal(Val::Px(5.0)),
                ..default()
            })
            .with_child(feathers::separator::separator(
                feathers::separator::SeparatorProps::vertical(),
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

fn transport_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    message_id: &'static str,
    action: EditorAction,
    localizer: &Localizer,
) -> EntityCommands<'a> {
    let mut button = parent.spawn_empty();
    button
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(message_id)),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(28.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ));
    button
}

fn spawn_play_icon(parent: &mut ChildSpawnerCommands, visible: bool) {
    parent
        .spawn((
            PlaybackPlayIcon,
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(8.0),
                height: Val::Px(14.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_child((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-5.0),
                top: Val::Px(2.0),
                width: Val::Px(10.0),
                height: Val::Px(10.0),
                border_radius: BorderRadius::all(Val::Px(1.0)),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_4)),
            BackgroundColor(theme::TEXT),
            Pickable::IGNORE,
        ));
}

fn spawn_pause_icon(parent: &mut ChildSpawnerCommands, visible: bool) {
    parent
        .spawn((
            PlaybackPauseIcon,
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(11.0),
                height: Val::Px(13.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(3.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|icon| {
            for _ in 0..2 {
                icon.spawn((
                    Node {
                        width: Val::Px(3.0),
                        height: Val::Px(12.0),
                        border_radius: BorderRadius::all(Val::Px(0.5)),
                        ..default()
                    },
                    BackgroundColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
            }
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
        Value::Shape(EmitterShape::Sphere { radius }) => format!("Sphere · r {radius:.1}"),
        Value::Shape(EmitterShape::Hemisphere { radius }) => {
            format!("Hemisphere · r {radius:.1}")
        }
        Value::Shape(EmitterShape::Box { half_extents }) => format!(
            "Box · {:.1} × {:.1} × {:.1}",
            half_extents[0] * 2.0,
            half_extents[1] * 2.0,
            half_extents[2] * 2.0
        ),
        Value::Shape(EmitterShape::Cylinder { radius, depth }) => {
            format!("Cylinder · r {radius:.1} h {depth:.1}")
        }
        Value::Shape(EmitterShape::Cone { radius, depth }) => {
            format!("Cone · r {radius:.1} d {depth:.1}")
        }
        Value::Parameter(id) => format!("Parameter {id}"),
        Value::Asset(id) => format!("Asset {id}"),
        Value::Material(id) => format!("Material {id}"),
    }
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
                ProfilerMetric::SubmittedInstances,
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
            "CPU update time, live particles, submitted instances, and peak particles",
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
        ProfilerMetric::SubmittedInstances => "SUBMITTED INSTANCES",
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
        ProfilerMetric::SubmittedInstances => format_profile_count(profile.submitted_instances),
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
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::APP_BG),
        ))
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
    parent.spawn(feathers::separator::separator(
        feathers::separator::SeparatorProps::horizontal().with_alpha(0.12),
    ));
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
            spawn_settings_toggle(
                parent,
                &localizer.text("settings-autosave-enabled"),
                &localizer.text("settings-autosave-enabled-description"),
                settings.general.autosave_enabled,
                SettingsToggle::AutosaveEnabled,
                localizer,
            );
            spawn_settings_integer(
                parent,
                &localizer.text("settings-autosave-interval"),
                &localizer.text("settings-autosave-interval-description"),
                SettingsNumber::AutosaveInterval,
                Some("s"),
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
        SettingsToggle::AutosaveEnabled => {
            let changed = settings.general.autosave_enabled != value;
            settings.general.autosave_enabled = value;
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
        SettingsNumber::AutosaveInterval => {
            let value = value.clamp(5, 600) as u16;
            let changed = settings.general.autosave_interval_seconds != value;
            settings.general.autosave_interval_seconds = value;
            changed
        }
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

fn settings_number_input_value(
    settings: &EditorSettings,
    setting: SettingsNumber,
) -> NumberInputValue {
    match setting {
        SettingsNumber::AutosaveInterval => {
            NumberInputValue::I32(i32::from(settings.general.autosave_interval_seconds))
        }
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
                    "start {:.2}s  ·  duration {:.2}s  ·  position {:?}  ·  scale {:?}  ·  capacity {}  ·  {}",
                    emitter.start_time,
                    emitter.duration,
                    emitter.transform.translation,
                    emitter.transform.scale,
                    emitter.max_particles,
                    emitter.source
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
            direction,
            spread_degrees,
            angular_velocity,
            ..
        } => format!(
            "life {lifetime:?}  ·  speed {speed:?}  ·  direction {direction:?}  ·  spread {spread_degrees:?}  ·  angular {angular_velocity:?}"
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

fn spawn_status_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn(feathers::status_bar::status_bar())
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
        button.insert(EditorTooltip::titled(label, help));
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
        ResMut<PreviewCameraController>,
        ResMut<PreviewDisplayState>,
    ),
    mut timeline_state: ResMut<TimelineState>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut transform_gizmo_settings: ResMut<TransformGizmoSettings>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
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
        mut preview_camera,
        mut preview_display,
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
                            session.playing = settings.preview.play_on_open;
                            workspace.complex = None;
                        }
                    }
                    EditorAction::OpenEffect => {
                        open_effect_dialog(&mut session, &settings);
                        workspace.complex = None;
                    }
                    EditorAction::OpenCatalog(index) => {
                        if confirm_discard(&session, &settings) {
                            if let Some(entry) = catalog.entries.get(index) {
                                match session.open(&entry.path) {
                                    Ok(()) => {
                                        session.playing = settings.preview.play_on_open;
                                    }
                                    Err(error) => {
                                        session.status = format!("Open failed: {error}");
                                    }
                                }
                            }
                            workspace.complex = None;
                        } else {
                            session.status = "Open cancelled".into();
                        }
                    }
                    EditorAction::TogglePlayback => session.playing = !session.playing,
                    EditorAction::StopPlayback => session.stop(),
                    EditorAction::Restart => session.restart(),
                    EditorAction::StepFrame(direction) => session.step_frame(direction),
                    EditorAction::AdjustPreviewSeed(direction) => {
                        session.adjust_preview_seed(direction);
                    }
                    EditorAction::Save => save_session(&mut session, false),
                    EditorAction::SaveAs => save_session(&mut session, true),
                    EditorAction::Exit => {
                        if confirm_discard(&session, &settings) {
                            autosave.suspended = true;
                            discard_active_recovery(&mut recovery);
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
                    EditorAction::SetTimelineSnap(mode) => {
                        if timeline_state.set_snap(mode) {
                            session.ui_revision += 1;
                        }
                    }
                    EditorAction::FrameTimeline => {
                        timeline_state.frame_all(session.playback_duration());
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
                    EditorAction::FramePreview => {
                        preview_camera.request_frame();
                    }
                    EditorAction::SetTransformGizmoMode(mode) => {
                        transform_gizmo_settings.mode = mode;
                    }
                    EditorAction::SetPreviewDisplayMode(mode) => {
                        preview_display.set_mode(mode);
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

fn recover_startup_session(
    session: &mut EditorSession,
    persistence: &mut RecoveryPersistence,
    candidate: RecoveryCandidate,
) {
    let source = candidate.source_path().map_or_else(
        || "an unsaved effect".to_string(),
        |path| path.display().to_string(),
    );
    let restore = matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Recover unsaved effect")
            .set_description(format!(
                "A newer recovery snapshot was found for {source}.\n\nRestore it? Yes restores the unsaved work; No discards the snapshot."
            ))
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    );
    if restore {
        session.restore_recovery(
            candidate.effect().clone(),
            candidate.source_path().map(Path::to_owned),
        );
        persistence.activate(&candidate);
    } else {
        match persistence.discard_candidate(&candidate) {
            Ok(()) => session.status = "Discarded recovery snapshot".into(),
            Err(error) => session.status = format!("Recovery discard failed: {error}"),
        }
    }
}

fn recovery_document_key(session: &EditorSession) -> String {
    format!(
        "{}|{}",
        session.effect.id,
        session.source_path.as_deref().map_or_else(
            || "<untitled>".to_string(),
            |path| path.display().to_string()
        )
    )
}

fn autosave_recovery(
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut persistence: ResMut<RecoveryPersistence>,
    mut state: ResMut<AutosaveState>,
) {
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        Instant::now(),
    );
}

fn autosave_recovery_at(
    session: &mut EditorSession,
    settings: &EditorSettings,
    persistence: &mut RecoveryPersistence,
    state: &mut AutosaveState,
    now: Instant,
) {
    if state.suspended {
        return;
    }
    let interval = Duration::from_secs(u64::from(settings.general.autosave_interval_seconds));
    if state.enabled != settings.general.autosave_enabled {
        state.enabled = settings.general.autosave_enabled;
        state.write_after = now + interval;
        state.cleanup_after = now;
        if state.enabled {
            state.written_revision = None;
        }
    }
    if !state.enabled {
        try_clear_tracked_recovery(persistence, state, now, "disabled recovery snapshot");
        return;
    }
    let document_key = recovery_document_key(session);
    if document_key != state.document_key {
        if !try_clear_tracked_recovery(
            persistence,
            state,
            now,
            "previous document recovery snapshot",
        ) {
            return;
        }
        state.document_key = document_key;
        state.observed_revision = session.document_revision();
        state.written_revision = None;
        state.write_after = now + interval;
    }

    if !session.dirty {
        try_clear_tracked_recovery(persistence, state, now, "saved effect recovery snapshot");
        return;
    }

    let revision = session.document_revision();
    if revision != state.observed_revision {
        state.observed_revision = revision;
        state.written_revision = None;
        state.write_after = now + interval;
        return;
    }
    if state.written_revision == Some(revision) || now < state.write_after {
        return;
    }

    match persistence.persist(&session.effect, session.source_path.as_deref()) {
        Ok(_) => {
            state.written_revision = Some(revision);
            state.cleanup_after = now;
        }
        Err(error) => {
            error!("failed to write recovery snapshot: {error}");
            session.status = format!("Recovery autosave failed: {error}");
            state.write_after = now + interval;
        }
    }
}

fn try_clear_tracked_recovery(
    persistence: &mut RecoveryPersistence,
    state: &mut AutosaveState,
    now: Instant,
    context: &str,
) -> bool {
    if !persistence.has_active() {
        state.written_revision = None;
        state.cleanup_after = now;
        return true;
    }
    if now < state.cleanup_after {
        return false;
    }
    match persistence.clear_active() {
        Ok(()) => {
            state.written_revision = None;
            state.cleanup_after = now;
            true
        }
        Err(error) => {
            warn!("failed to clear the {context}: {error}");
            state.cleanup_after = now + RECOVERY_CLEANUP_RETRY_DELAY;
            false
        }
    }
}

fn discard_active_recovery(persistence: &mut RecoveryPersistence) {
    if let Err(error) = persistence.clear_active() {
        warn!("failed to discard recovery snapshot: {error}");
    }
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
    match session.open(&path) {
        Ok(()) => session.playing = settings.preview.play_on_open,
        Err(error) => session.status = format!("Open failed: {error}"),
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

fn update_grid_menu_check(
    menu: Res<MenuState>,
    mut checks: Query<&mut BackgroundColor, With<GridMenuCheck>>,
) {
    if !menu.is_changed() {
        return;
    }
    let color = if menu.show_grid {
        theme::ACCENT
    } else {
        Color::NONE
    };
    for mut background in &mut checks {
        background.0 = color;
    }
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

fn handle_window_close_requests(
    mut close_requests: MessageReader<WindowCloseRequested>,
    primary: Single<Entity, With<PrimaryWindow>>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
    mut commands: Commands,
) {
    for request in close_requests.read() {
        if request.window == *primary {
            if confirm_discard(&session, &settings) {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
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

fn rebuild_editor_ui(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    localizer: Res<Localizer>,
    mut rendered: ResMut<RenderedUiRevision>,
    root: Single<Entity, With<EditorRoot>>,
    contents: Query<Entity, With<EditorContent>>,
) {
    if rendered.0 == session.ui_revision {
        return;
    }
    for content in &contents {
        commands.entity(content).despawn();
    }
    commands.entity(*root).with_children(|root| {
        spawn_editor_content(root, &menu, &localizer);
    });
    rendered.0 = session.ui_revision;
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    session.advance_playback(time.delta_secs());
}

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
    mut labels: Query<(
        &mut Text,
        Option<&InspectorTitle>,
        Option<&DocumentMenuLabel>,
        Option<&DocumentToolbarLabel>,
    )>,
) {
    if !session.is_changed() && !localizer.is_changed() {
        return;
    }
    let layer = session.selected_layer();
    for (mut text, title, document_menu, document_toolbar) in &mut labels {
        if title.is_some() {
            text.0 = layer.name.clone();
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

fn update_transport_icons(
    session: Res<EditorSession>,
    mut play_icons: Query<&mut Node, (With<PlaybackPlayIcon>, Without<PlaybackPauseIcon>)>,
    mut pause_icons: Query<&mut Node, (With<PlaybackPauseIcon>, Without<PlaybackPlayIcon>)>,
) {
    if !session.is_changed() {
        return;
    }
    for mut node in &mut play_icons {
        node.display = if session.playing {
            Display::None
        } else {
            Display::Flex
        };
    }
    for mut node in &mut pause_icons {
        node.display = if session.playing {
            Display::Flex
        } else {
            Display::None
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_a_document_clears_its_tracked_recovery_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let source_path = temporary.path().join("saved-effect.aestra.ron");
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.save_as(&source_path).unwrap();
        session.adjust_effect_duration(0.25);
        let mut persistence = RecoveryPersistence::for_test(temporary.path().into(), None);
        let recovery_path = persistence
            .persist(&session.effect, session.source_path.as_deref())
            .unwrap();
        let mut state = AutosaveState::new(&session, true);
        session.save().unwrap();

        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            Instant::now(),
        );

        assert!(!recovery_path.exists());
        assert!(!persistence.has_active());
        assert!(state.written_revision.is_none());
    }

    #[test]
    fn disabling_autosave_clears_a_snapshot_even_without_a_written_revision_marker() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut persistence = RecoveryPersistence::for_test(temporary.path().into(), None);
        let recovery_path = persistence
            .persist(&session.effect, session.source_path.as_deref())
            .unwrap();
        let mut state = AutosaveState::new(&session, true);
        assert!(state.written_revision.is_none());
        let settings = EditorSettings {
            general: settings::GeneralSettings {
                autosave_enabled: false,
                ..default()
            },
            ..default()
        };

        autosave_recovery_at(
            &mut session,
            &settings,
            &mut persistence,
            &mut state,
            Instant::now(),
        );

        assert!(!recovery_path.exists());
        assert!(!persistence.has_active());
        assert!(!state.enabled);
    }

    #[test]
    fn document_switch_waits_for_failed_cleanup_and_retries() {
        let temporary = tempfile::tempdir().unwrap();
        let blocked_path = temporary.path().join("blocked.recovery.ron");
        fs::create_dir(&blocked_path).unwrap();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut state = AutosaveState::new(&session, true);
        let previous_document_key = state.document_key.clone();
        let mut persistence =
            RecoveryPersistence::for_test(temporary.path().into(), Some(blocked_path.clone()));
        session.new_effect();
        let next_document_key = recovery_document_key(&session);
        let now = Instant::now();

        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            now,
        );

        assert!(persistence.has_active());
        assert_eq!(state.document_key, previous_document_key);

        fs::remove_dir(&blocked_path).unwrap();
        fs::write(&blocked_path, "pending snapshot").unwrap();
        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            now + RECOVERY_CLEANUP_RETRY_DELAY,
        );

        assert!(!blocked_path.exists());
        assert!(!persistence.has_active());
        assert_eq!(state.document_key, next_document_key);
    }

    #[test]
    fn viewport_dock_is_a_transparent_cutout_for_the_preview_camera() {
        assert_eq!(dock_pane_background(Some(DockPanel::Viewport)), Color::NONE);
        assert_eq!(
            dock_pane_background(Some(DockPanel::Inspector)),
            theme::PANEL_DARK
        );
        assert_eq!(dock_pane_background(None), theme::PANEL_DARK);
    }

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
        assert_eq!(effect.format_version, 3);
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
        profile.record_submitted_frame(profiled.effect(), &profiled_samples);
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
        let expected_submissions = profiled_samples.iter().fold(0_u32, |total, sample| {
            total.saturating_add(
                profiled.effect().emitters[sample.emitter_index]
                    .renderers
                    .len() as u32,
            )
        });
        assert_eq!(
            profile.submitted_instances,
            ProfileValue::Measured(expected_submissions)
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

        assert!(apply_settings_toggle(
            &mut settings,
            &mut menu,
            SettingsToggle::AutosaveEnabled,
            false,
        ));
        assert!(!settings.general.autosave_enabled);

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
        assert!(apply_settings_integer(
            &mut settings,
            SettingsNumber::AutosaveInterval,
            900,
        ));
        assert_eq!(settings.general.autosave_interval_seconds, 600);
        assert_eq!(
            settings_number_input_value(&settings, SettingsNumber::AutosaveInterval),
            NumberInputValue::I32(600)
        );

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
    fn grid_menu_check_tracks_the_persisted_visibility_state() {
        let mut app = App::new();
        app.insert_resource(MenuState {
            show_grid: true,
            ..default()
        });
        let check = app
            .world_mut()
            .spawn((GridMenuCheck, BackgroundColor(Color::NONE)))
            .id();
        app.add_systems(Update, update_grid_menu_check);

        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(check).unwrap().0,
            theme::ACCENT
        );

        app.world_mut().resource_mut::<MenuState>().show_grid = false;
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(check).unwrap().0,
            Color::NONE
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
