mod assets;
mod curves;
mod dock_ui;
mod docking;
mod feathers;
mod inspector;
mod localization;
mod menus;
mod persistence;
mod recovery;
mod session;
mod settings;
mod settings_ui;
mod theme;
mod timeline;
mod viewport;

use aestra_authoring::{ChangeKind, EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    AestraPlugin, BlendMode, Diagnostic, DiagnosticCode, DiagnosticSeverity, EffectAsset,
    EmitterShape, EmitterTransform, FlipbookPlaybackMode, FlipbookTimeSource, MaterialInput,
    MaterialProperties, ModuleId, ModuleInstance, ModuleParameters, RendererId, RendererProperties,
    StageKind, ValidationReport, Value,
};
use aestra_compiler::ModuleMetadata;
use aestra_runtime::{CompiledEffect, CompiledEmitter, Instruction, RuntimeStage};
use aestra_runtime::{EffectProfile, ProfileValue, ProfileValueSource};
use assets::{AssetsSet, EditorAssetsPlugin};
pub(crate) use assets::{EffectCatalog, layer_color, spawn_asset_browser};
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
pub(crate) use curves::{CurvesAction, CurvesState, spawn_curves_workspace};
use curves::{CurvesSet, EditorCurvesPlugin};
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
use localization::{EditorLocalizationPlugin, LocalizationSet};
pub(crate) use localization::{LocalizedText, Localizer};
pub(crate) use menus::{DocumentMenuLabel, MenuState, RedoMenuItem, TabContextMenu, UndoMenuItem};
use menus::{EditorMenusPlugin, spawn_about_overlay, spawn_menu_bar, spawn_tab_context_menu};
pub(crate) use persistence::persist_editor_settings;
use persistence::{DocumentAction, EditorPersistencePlugin, PersistenceSet};
use session::EditorSession;
use settings::{EditorSettings, SettingsPersistence};
use settings_ui::EditorSettingsUiPlugin;
pub(crate) use settings_ui::{SettingsPanelState, spawn_settings_workspace};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
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
    let localization = EditorLocalizationPlugin::new(&settings.language.locale);
    settings.language.locale = localization.locale().into();
    let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
    let show_grid = settings.preview.show_grid;
    let ui_scale = settings.appearance.ui_scale;
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(session)
        .insert_resource(settings)
        .insert_resource(persistence)
        .insert_resource(UiScale(ui_scale))
        .init_resource::<DiagnosticsPanelState>()
        .init_resource::<ProfilerState>()
        .init_resource::<ScrollMemoryState>()
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
        .add_plugins(localization)
        .add_plugins(EditorMenusPlugin::new(show_grid))
        .add_plugins(EditorAssetsPlugin)
        .add_plugins(EditorCurvesPlugin)
        .add_plugins(EditorSettingsUiPlugin)
        .add_plugins(EditorPersistencePlugin)
        .add_plugins(AestraPlugin)
        .add_plugins(DockingPlugin)
        .add_plugins(InspectorPlugin)
        .add_plugins(TimelinePlugin)
        .add_plugins(ViewportPlugin)
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
                    advance_playback,
                )
                    .chain()
                    .in_set(EditorSet::PreViewport),
                (
                    update_profiler_labels,
                    update_editor_labels,
                    update_transport_icons,
                    update_compile_status,
                    update_history_actions,
                )
                    .chain()
                    .in_set(EditorSet::MainUpdate),
                (
                    remember_scroll_positions,
                    rebuild_editor_ui,
                    restore_scroll_positions,
                )
                    .chain()
                    .in_set(EditorSet::UiRebuild),
            ),
        )
        .configure_sets(
            Startup,
            (
                PersistenceSet::Startup,
                ViewportSet::Setup,
                EditorSet::Setup,
            )
                .chain(),
        )
        .configure_sets(
            Update,
            (
                TimelineSet::Input,
                InspectorSet::Input,
                DockingSet::Input,
                AestraFeathersSet::Input,
                AssetsSet::Actions,
                CurvesSet::Actions,
                PersistenceSet::Actions,
                EditorSet::PreViewport,
                PersistenceSet::Lifecycle,
                ViewportSet::Update,
                LocalizationSet::Sync,
                EditorSet::MainUpdate,
                DockingSet::Reconcile,
                EditorSet::UiRebuild,
                TimelineSet::Visuals,
                AssetsSet::Sync,
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
    TogglePlayback,
    StopPlayback,
    Restart,
    StepFrame(i8),
    AdjustPreviewSeed(i8),
    Undo,
    Redo,
    AddLayer,
    DuplicateLayer,
    DeleteLayer,
    EffectDuration(f32),
    SetTimelineSnap(TimelineSnapMode),
    FrameTimeline,
    OpenModulePalette(StackStage),
    CloseModulePalette,
    AddModule(usize),
    AddSpriteRenderer,
    AddFlipbookRenderer,
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
    ToggleGrid,
    FramePreview,
    SetTransformGizmoMode(TransformGizmoMode),
    SetPreviewDisplayMode(PreviewDisplayMode),
    ResetWorkspaceLayout,
    ToggleInspectorSection(InspectorSection),
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
    ShowAbout,
    CloseAbout,
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

    fn message_id(self) -> &'static str {
        match self {
            Self::All => "diagnostics-filter-all",
            Self::Errors => "diagnostics-errors",
            Self::Warnings => "diagnostics-warnings",
            Self::Info => "diagnostics-info",
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
struct DocumentToolbarLabel;

#[derive(Component)]
struct DiagnosticsFilterButton(DiagnosticsFilter);

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
struct PlaybackPlayIcon;

#[derive(Component)]
struct PlaybackPauseIcon;

#[derive(Component)]
struct CompileStatusLabel;

#[derive(Component)]
struct CompileStatusButton;

#[derive(Component)]
struct CompileStatusDot;

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

fn spawn_generated_code_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
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
                        Text::new(localizer.text("generated-compiled-plan")),
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
                        Text::new(localizer.text(state_label)),
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
                    &localizer.text("generated-no-artifact"),
                    &localizer.text("generated-no-artifact-description"),
                    Color::srgb(1.0, 0.38, 0.32),
                );
                return;
            };

            spawn_compiled_summary(panel, compiled, localizer);
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
                            spawn_compiled_layout(content, compiled, localizer);
                            spawn_compiled_parameters(content, compiled, session, localizer);
                            for (emitter_index, emitter) in compiled.emitters.iter().enumerate() {
                                spawn_compiled_emitter(
                                    content,
                                    compiled,
                                    emitter,
                                    emitter_index,
                                    session,
                                    localizer,
                                );
                            }
                            spawn_wesl_backend(content, localizer);
                        },
                    );
                });
        });
}

fn spawn_profiler_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &ProfilerState,
    localizer: &Localizer,
) {
    let status = if session
        .pending_change
        .as_ref()
        .is_some_and(|pending| !pending.can_apply)
    {
        "profiler-status-last-valid"
    } else {
        "profiler-status-live"
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
                        Text::new(localizer.text("profiler-effect-profile")),
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
                        Text::new(localizer.text(status)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    spawn_profiler_reset_button(header, localizer);
                });

            let Some(profile) = &state.profile else {
                spawn_diagnostics_empty_state(
                    panel,
                    &localizer.text("profiler-waiting"),
                    &localizer.text("profiler-waiting-description"),
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
                            spawn_profiler_metric_grid(content, profile, localizer);
                            spawn_profiler_history(content, state, localizer);
                            spawn_profiler_emitters(content, profile, localizer);
                            spawn_profiler_availability(content, profile, localizer);
                        },
                    );
                });
        });
}

fn spawn_profiler_reset_button(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    let label = localizer.text("profiler-reset-peaks");
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            EditorAction::ResetProfilerPeaks,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
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
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_profiler_metric_grid(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
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
                spawn_profiler_metric_card(grid, profile, metric, localizer);
            }
        });
}

fn spawn_profiler_metric_card(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    metric: ProfilerMetric,
    localizer: &Localizer,
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
                Text::new(localizer.text(profiler_metric_message(metric))),
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
                Text::new(profile_source_label(source, localizer)),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(profile_source_color(source)),
            ));
        });
}

fn spawn_profiler_history(
    parent: &mut ChildSpawnerCommands,
    state: &ProfilerState,
    localizer: &Localizer,
) {
    spawn_compiled_section(parent, &localizer.text("profiler-cpu-history"), |section| {
        section.spawn((
            ProfilerHistorySummary,
            Text::new(profiler_history_summary(&state.cpu_history_ns, localizer)),
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

fn spawn_profiler_emitters(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
    spawn_compiled_section(parent, &localizer.text("profiler-emitters"), |section| {
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
                        Text::new(profiler_emitter_value(emitter, localizer)),
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

fn spawn_profiler_availability(
    parent: &mut ChildSpawnerCommands,
    profile: &EffectProfile,
    localizer: &Localizer,
) {
    spawn_compiled_section(
        parent,
        &localizer.text("profiler-measurement-availability"),
        |section| {
            spawn_compiled_label_value(
                section,
                &localizer.text("profiler-source-measured"),
                &localizer.text("profiler-measured-description"),
            );
            spawn_compiled_label_value(
                section,
                &localizer.text("profiler-source-estimated"),
                &localizer.text("profiler-estimated-description"),
            );
            if profile.gpu_time_ns.source() == ProfileValueSource::Unavailable {
                spawn_compiled_label_value(
                    section,
                    &localizer.text("profiler-source-unavailable"),
                    &localizer.text("profiler-unavailable-description"),
                );
            }
        },
    );
}

fn profiler_metric_message(metric: ProfilerMetric) -> &'static str {
    match metric {
        ProfilerMetric::CpuTime => "profiler-metric-cpu-update",
        ProfilerMetric::GpuTime => "profiler-metric-gpu-time",
        ProfilerMetric::AliveParticles => "profiler-metric-live-particles",
        ProfilerMetric::SubmittedInstances => "profiler-metric-submitted-instances",
        ProfilerMetric::PeakParticles => "profiler-metric-peak-particles",
        ProfilerMetric::ParticleCapacity => "profiler-metric-capacity",
        ProfilerMetric::Emitters => "profiler-metric-emitters",
        ProfilerMetric::DrawCalls => "profiler-metric-draw-calls",
        ProfilerMetric::Dispatches => "profiler-metric-dispatches",
        ProfilerMetric::BufferMemory => "profiler-metric-buffer-memory",
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

fn profile_source_label(source: ProfileValueSource, localizer: &Localizer) -> String {
    localizer.text(match source {
        ProfileValueSource::Measured => "profiler-source-measured",
        ProfileValueSource::Estimated => "profiler-source-estimated",
        ProfileValueSource::Unavailable => "profiler-source-unavailable",
    })
}

fn profile_source_color(source: ProfileValueSource) -> Color {
    match source {
        ProfileValueSource::Measured => Color::srgb(0.35, 0.88, 0.57),
        ProfileValueSource::Estimated => Color::srgb(1.0, 0.74, 0.30),
        ProfileValueSource::Unavailable => theme::TEXT_FAINT,
    }
}

fn profiler_emitter_value(
    emitter: &aestra_runtime::EmitterProfile,
    localizer: &Localizer,
) -> String {
    let mut args = FluentArgs::new();
    args.set("live", emitter.alive_particles);
    args.set("peak", emitter.peak_particles);
    args.set("capacity", emitter.particle_capacity);
    localizer.text_with("profiler-emitter-summary", &args)
}

fn profiler_history_summary(history: &VecDeque<u64>, localizer: &Localizer) -> String {
    if history.is_empty() {
        return localizer.text("profiler-history-collecting");
    }
    let total = history.iter().copied().map(u128::from).sum::<u128>();
    let average = (total / history.len() as u128).min(u128::from(u64::MAX)) as u64;
    let maximum = history.iter().copied().max().unwrap_or_default();
    let mut args = FluentArgs::new();
    args.set("count", history.len());
    args.set(
        "average",
        format_profile_duration(ProfileValue::Measured(average)).0,
    );
    args.set(
        "maximum",
        format_profile_duration(ProfileValue::Measured(maximum)).0,
    );
    localizer.text_with("profiler-history-summary", &args)
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
        ("generated-status-last-valid", Color::srgb(1.0, 0.74, 0.30))
    } else if session.pending_change.is_some() && has_artifact {
        ("generated-status-pending", theme::ACCENT)
    } else if has_artifact {
        ("generated-status-live", Color::srgb(0.35, 0.88, 0.57))
    } else {
        ("generated-status-unavailable", Color::srgb(1.0, 0.38, 0.32))
    }
}

fn spawn_compiled_summary(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    localizer: &Localizer,
) {
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
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-emitters"),
                compiled.emitters.len(),
            );
            spawn_compiled_metric(summary, &localizer.text("generated-ops"), instruction_count);
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-attributes"),
                compiled.particle_layout.attributes.len(),
            );
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-parameters"),
                compiled.parameters.len(),
            );
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-capacity"),
                compiled.max_particles,
            );
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

fn spawn_compiled_layout(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    localizer: &Localizer,
) {
    spawn_compiled_section(
        parent,
        &localizer.text("generated-particle-layout"),
        |section| {
            spawn_compiled_label_value(
                section,
                &localizer.text("generated-stored"),
                &format_particle_attributes(&compiled.particle_layout.attributes),
            );
            spawn_compiled_label_value(
                section,
                &localizer.text("generated-transient"),
                &format_particle_attributes(&compiled.particle_layout.transient_attributes),
            );
            spawn_compiled_label_value(section, &localizer.text("generated-optimized"), &{
                let mut args = FluentArgs::new();
                args.set("constants", compiled.optimizations.constant_expressions);
                args.set("reads", compiled.optimizations.runtime_parameter_reads);
                args.set("removed", compiled.optimizations.eliminated_attributes);
                localizer.text_with("generated-optimization-summary", &args)
            });
        },
    );
}

fn spawn_compiled_parameters(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_compiled_section(
        parent,
        &localizer.text("generated-parameter-table"),
        |section| {
            if compiled.parameters.is_empty() {
                spawn_compiled_muted_line(
                    section,
                    &localizer.text("generated-no-runtime-parameters"),
                );
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
        },
    );
}

fn spawn_compiled_emitter(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    emitter: &CompiledEmitter,
    emitter_index: usize,
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_compiled_section(
        parent,
        &format!(
            "EMITTER {emitter_index:02}  ·  {}",
            emitter.name.to_uppercase()
        ),
        |section| {
            let enabled = localizer.text(if emitter.enabled {
                "generated-enabled"
            } else {
                "generated-disabled"
            });
            spawn_compiled_target_row(
                section,
                SemanticTarget::Emitter(emitter.source),
                session.selection.primary == SemanticTarget::Emitter(emitter.source),
                &format!("E{emitter_index:02}"),
                &enabled,
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
                localizer,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleSpawn,
                &emitter.execution.particle_spawn,
                session,
                localizer,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleUpdate,
                &emitter.execution.particle_update,
                session,
                localizer,
            );
            if emitter.renderers.is_empty() {
                spawn_compiled_stage_heading(
                    section,
                    &localizer.text("generated-renderers"),
                    0,
                    localizer,
                );
            } else {
                spawn_compiled_stage_heading(
                    section,
                    &localizer.text("generated-renderers"),
                    emitter.renderers.len(),
                    localizer,
                );
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
                        &localizer.text("generated-sprite-draw"),
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
    localizer: &Localizer,
) {
    spawn_compiled_stage_heading(
        parent,
        &localizer.text(runtime_stage_message_id(stage)),
        instructions.len(),
        localizer,
    );
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

fn spawn_compiled_stage_heading(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    count: usize,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(format!(
            "{title}  ·  {count} {}",
            localizer.text("generated-ops")
        )),
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

fn spawn_wesl_backend(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    spawn_compiled_section(
        parent,
        &localizer.text("generated-wesl-backend"),
        |section| {
            spawn_compiled_label_value(
                section,
                &localizer.text("generated-simulation"),
                "aestra_simulation.wesl  ·  reset @compute(1)  ·  simulate @compute(64)",
            );
            spawn_compiled_label_value(
                section,
                &localizer.text("assets-sprite"),
                "aestra_sprite_render.wesl  ·  vertex  ·  fragment_alpha  ·  fragment_additive",
            );
            spawn_compiled_muted_line(section, &localizer.text("generated-wesl-description"));
        },
    );
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

fn runtime_stage_message_id(stage: RuntimeStage) -> &'static str {
    match stage {
        RuntimeStage::EmitterUpdate => "generated-stage-emitter-update",
        RuntimeStage::ParticleSpawn => "generated-stage-particle-spawn",
        RuntimeStage::ParticleUpdate => "generated-stage-particle-update",
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
    localizer: &Localizer,
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
                        Text::new(localizer.text("diagnostics-validation")),
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
                    spawn_diagnostic_count(
                        header,
                        errors,
                        "diagnostics-errors",
                        Color::srgb(1.0, 0.38, 0.32),
                        localizer,
                    );
                    spawn_diagnostic_count(
                        header,
                        warnings,
                        "diagnostics-warnings",
                        Color::srgb(1.0, 0.74, 0.30),
                        localizer,
                    );
                    spawn_diagnostic_count(
                        header,
                        info,
                        "diagnostics-info",
                        Color::srgb(0.45, 0.70, 1.0),
                        localizer,
                    );
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
                            localizer,
                        );
                    }
                });

            if errors + warnings + info == 0 {
                spawn_diagnostics_empty_state(
                    panel,
                    &localizer.text("diagnostics-no-issues"),
                    &localizer.text("diagnostics-no-issues-description"),
                    Color::srgb(0.35, 0.88, 0.57),
                );
                return;
            }
            if visible == 0 {
                spawn_diagnostics_empty_state(
                    panel,
                    &localizer.text("diagnostics-no-matches"),
                    &localizer.text("diagnostics-no-matches-description"),
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
                                &localizer.text("diagnostics-working-effect"),
                                &session.diagnostics,
                                DiagnosticSource::Current,
                                state.filter,
                                localizer,
                            );
                            if let Some(pending) = &session.pending_change {
                                spawn_diagnostic_section(
                                    list,
                                    &localizer.text("diagnostics-pending-transaction"),
                                    &pending.diagnostics,
                                    DiagnosticSource::Pending,
                                    state.filter,
                                    localizer,
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
    localizer: &Localizer,
) {
    let label = format!("{} {count}", localizer.text(filter.message_id()));
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
            AccessibleLabel(label.clone()),
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
                Text::new(label),
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
    localizer: &Localizer,
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
        spawn_diagnostic_row(parent, diagnostic, source, index, localizer);
    }
}

fn spawn_diagnostic_row(
    parent: &mut ChildSpawnerCommands,
    diagnostic: &Diagnostic,
    source: DiagnosticSource,
    index: usize,
    localizer: &Localizer,
) {
    let (label, color) = diagnostic_severity_style(diagnostic.severity, localizer);
    let code = localizer.text(diagnostic_code_message(diagnostic.code));
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
                    Text::new(format!("{label}  ·  {code}")),
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

fn diagnostic_severity_style(
    severity: DiagnosticSeverity,
    localizer: &Localizer,
) -> (String, Color) {
    let (message, color) = match severity {
        DiagnosticSeverity::Error => ("diagnostics-severity-error", Color::srgb(1.0, 0.38, 0.32)),
        DiagnosticSeverity::Warning => {
            ("diagnostics-severity-warning", Color::srgb(1.0, 0.74, 0.30))
        }
        DiagnosticSeverity::Info => ("diagnostics-severity-info", Color::srgb(0.45, 0.70, 1.0)),
    };
    (localizer.text(message), color)
}

fn diagnostic_code_message(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::UnsupportedFormat => "diagnostics-code-unsupported-format",
        DiagnosticCode::NilId => "diagnostics-code-nil-id",
        DiagnosticCode::DuplicateId => "diagnostics-code-duplicate-id",
        DiagnosticCode::InvalidDuration => "diagnostics-code-invalid-duration",
        DiagnosticCode::InvalidTiming => "diagnostics-code-invalid-timing",
        DiagnosticCode::InvalidCapacity => "diagnostics-code-invalid-capacity",
        DiagnosticCode::MissingModule => "diagnostics-code-missing-module",
        DiagnosticCode::DuplicateModule => "diagnostics-code-duplicate-module",
        DiagnosticCode::StageMismatch => "diagnostics-code-stage-mismatch",
        DiagnosticCode::InvalidValue => "diagnostics-code-invalid-value",
        DiagnosticCode::MissingRenderer => "diagnostics-code-missing-renderer",
        DiagnosticCode::InvalidReference => "diagnostics-code-invalid-reference",
        DiagnosticCode::UnknownModule => "diagnostics-code-unknown-module",
        DiagnosticCode::UnsupportedRenderer => "diagnostics-code-unsupported-renderer",
        DiagnosticCode::MissingAttribute => "diagnostics-code-missing-attribute",
        DiagnosticCode::UnknownParameter => "diagnostics-code-unknown-parameter",
        DiagnosticCode::ParameterTypeMismatch => "diagnostics-code-parameter-type-mismatch",
    }
}

fn spawn_diagnostic_count(
    parent: &mut ChildSpawnerCommands,
    count: usize,
    message_id: &str,
    active_color: Color,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(format!("{count} {}", localizer.text(message_id))),
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

fn spawn_changes_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
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
                        || localizer.text("changes-none-pending"),
                        |pending| {
                            let mut args = FluentArgs::new();
                            args.set(
                                "transaction",
                                pending.preview.transaction().label.to_uppercase(),
                            );
                            args.set("count", pending.preview.diff().changes.len());
                            localizer.text_with("changes-summary", &args)
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
                    Text::new(localizer.text("changes-empty-description")),
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
                                    let (kind, color) = change_kind_style(change.kind, localizer);
                                    let values = match (&change.before, &change.after) {
                                        (Some(before), Some(after)) => {
                                            format!("{before}  →  {after}")
                                        }
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
                                        localizer.text("changes-ready")
                                    } else {
                                        let mut args = FluentArgs::new();
                                        args.set("count", errors);
                                        localizer.text_with("changes-blocked", &args)
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
                                            DiagnosticSeverity::Error => {
                                                Color::srgb(1.0, 0.38, 0.32)
                                            }
                                            DiagnosticSeverity::Warning => {
                                                Color::srgb(1.0, 0.74, 0.30)
                                            }
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
                                            &localizer.text("changes-discard"),
                                            EditorAction::DiscardPendingChange,
                                            None,
                                        );
                                        inspector_action_button(
                                            actions,
                                            &localizer.text(if pending.can_apply {
                                                "changes-apply"
                                            } else {
                                                "changes-apply-blocked"
                                            }),
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

fn change_kind_style(kind: ChangeKind, localizer: &Localizer) -> (String, Color) {
    match kind {
        ChangeKind::Added => (
            localizer.text("changes-kind-added"),
            Color::srgb(0.35, 0.88, 0.57),
        ),
        ChangeKind::Removed => (
            localizer.text("changes-kind-removed"),
            Color::srgb(1.0, 0.38, 0.32),
        ),
        ChangeKind::Modified => (
            localizer.text("changes-kind-modified"),
            Color::srgb(0.45, 0.70, 1.0),
        ),
        ChangeKind::Moved => (
            localizer.text("changes-kind-moved"),
            Color::srgb(1.0, 0.74, 0.30),
        ),
    }
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

fn inspector_action_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
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
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    palette: Res<ModulePaletteState>,
    workspace_resources: (ResMut<CurvesState>, ResMut<WorkspaceLayout>),
    settings_resources: (ResMut<EditorSettings>, ResMut<SettingsPersistence>),
) {
    let (mut workspace, mut layout) = workspace_resources;
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
    if control && keys.just_pressed(KeyCode::KeyN) {
        commands.trigger(DocumentAction::New);
    }
    if control && keys.just_pressed(KeyCode::KeyO) {
        commands.trigger(DocumentAction::Open);
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        commands.trigger(if shift {
            DocumentAction::SaveAs
        } else {
            DocumentAction::Save
        });
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
        workspace.clear();
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
            Option<&DockTab>,
            Option<&DockCloseButton>,
            Option<&DiagnosticsFilterButton>,
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
        Res<EditorModuleRegistry>,
        ResMut<ModulePaletteState>,
        ResMut<CurvesState>,
        ResMut<WorkspaceLayout>,
        ResMut<DiagnosticsPanelState>,
        ResMut<InspectorFocus>,
        ResMut<ProfilerState>,
        ResMut<EditorSettings>,
        ResMut<SettingsPersistence>,
        Res<Localizer>,
        ResMut<PreviewCameraController>,
        ResMut<PreviewDisplayState>,
    ),
    mut timeline_state: ResMut<TimelineState>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut transform_gizmo_settings: ResMut<TransformGizmoSettings>,
) {
    let (
        registry,
        mut palette,
        mut workspace,
        mut layout,
        mut diagnostics_panel,
        mut inspector_focus,
        mut profiler,
        mut settings,
        mut settings_persistence,
        localizer,
        mut preview_camera,
        mut preview_display,
    ) = editor_resources;
    for (
        entity,
        interaction,
        action,
        dock_tab,
        dock_close,
        diagnostics_filter,
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
            }
            Interaction::None => {
                if feathers_action.is_none() {
                    background.0 = if let Some(tab) = dock_tab {
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
                let keep_view_menu_open = matches!(*action, EditorAction::ToggleDockPanel(_));
                if !keep_view_menu_open {
                    menu.open = None;
                    menu.panels_open = false;
                }
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                match *action {
                    EditorAction::TogglePlayback => session.playing = !session.playing,
                    EditorAction::StopPlayback => session.stop(),
                    EditorAction::Restart => session.restart(),
                    EditorAction::StepFrame(direction) => session.step_frame(direction),
                    EditorAction::AdjustPreviewSeed(direction) => {
                        session.adjust_preview_seed(direction);
                    }
                    EditorAction::Undo => session.undo(),
                    EditorAction::Redo => session.redo(),
                    EditorAction::AddLayer => session.add_layer(),
                    EditorAction::DuplicateLayer => {
                        session.duplicate_selected_layer();
                        workspace.clear();
                    }
                    EditorAction::DeleteLayer => {
                        if preview_selected_layer_deletion(&mut session) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
                        }
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
                    EditorAction::MoveModule(id, direction) => {
                        session.move_module(id, direction);
                    }
                    EditorAction::DuplicateModule(id) => session.duplicate_module(id),
                    EditorAction::DeleteModule(id) => {
                        if preview_module_deletion(&mut session, id) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                            workspace.clear();
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
                            workspace.clear();
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
                            workspace.clear();
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Inspector);
                        }
                    }
                    EditorAction::SelectCompiledTarget(target) => {
                        workspace.clear();
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
                        session.status = localizer.text("profiler-reset-status");
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
                    EditorAction::ShowAbout => menu.show_about = true,
                    EditorAction::CloseAbout => menu.show_about = false,
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

pub(crate) fn reveal_dock_panel(
    layout: &mut WorkspaceLayout,
    session: &mut EditorSession,
    panel: DockPanel,
) {
    if !layout.show(panel) {
        return;
    }
    session.ui_revision += 1;
    if let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
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
    localizer: Res<Localizer>,
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
    if !profiler.is_changed() && !localizer.is_changed() {
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
                        text.0 = profile_source_label(source, &localizer);
                        color.0 = profile_source_color(source);
                    }
                }
            } else if let Some(emitter) = emitter {
                if let Some(profile) = profile.emitters.get(emitter.0) {
                    text.0 = profiler_emitter_value(profile, &localizer);
                }
            } else if summary.is_some() {
                text.0 = profiler_history_summary(&profiler.cpu_history_ns, &localizer);
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

#[cfg(test)]
mod tests {
    use super::*;

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
            "generated-status-live"
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
}
