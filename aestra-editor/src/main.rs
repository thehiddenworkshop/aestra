mod docking;
mod session;
mod theme;
mod ui_shell;

use aestra_authoring::{ChangeKind, EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    ColorKey, CurveKey, Diagnostic, DiagnosticCode, DiagnosticSeverity, EffectAsset, EmitterShape,
    ModuleId, ModuleInstance, ModuleParameters, RendererId, RendererProperties, StageKind,
    ValidationReport, Value,
};
use aestra_compiler::{InputControl, InputMetadata, ModuleMetadata, ModuleRegistry};
use bevy::{
    camera::RenderTarget,
    ecs::system::SystemParam,
    input::{ButtonState, keyboard::KeyboardInput, mouse::MouseScrollUnit},
    picking::events::{Click, Drag, DragDrop, DragEnd, DragStart, Out, Over, Pointer, Scroll},
    picking::pointer::PointerButton,
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
    window::{
        CursorIcon, PrimaryWindow, SystemCursorIcon, WindowCloseRequested, WindowMoved,
        WindowPosition, WindowRef, WindowResizeConstraints, WindowResized, WindowResolution,
    },
};
use docking::{DockAxis, DockDrop, DockNode, DockNodeId, DockPanel, DockStack, WorkspaceLayout};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use session::EditorSession;
use std::{fs, path::PathBuf};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const PARTICLE_POOL_SIZE: usize = 384;

fn main() {
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ))
        .insert_resource(EffectCatalog::scan())
        .init_resource::<MenuState>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<DiagnosticsPanelState>()
        .init_resource::<WorkspaceState>()
        .init_resource::<DockDragState>()
        .init_resource::<ResizeState>()
        .insert_resource(WorkspaceLayout::load())
        .init_resource::<RenderedUiRevision>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            close_when_requested: false,
            primary_window: Some(Window {
                title: "Aestra — VFX Choreography Editor".into(),
                resolution: WindowResolution::new(1440, 900),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, (setup_window_cursor, setup_editor))
        .add_systems(
            Update,
            (
                (
                    module_palette_keyboard,
                    keyboard_shortcuts,
                    handle_buttons,
                    handle_window_close_requests,
                    persist_native_window_geometry,
                    dismiss_tab_context_menu,
                    scrub_timeline,
                    advance_playback,
                    update_preview,
                    update_editor_labels,
                    update_compile_status,
                    update_history_actions,
                )
                    .chain(),
                (
                    update_playhead,
                    update_layer_selection,
                    update_menu_visibility,
                    update_panel_visibility_labels,
                    update_preview_grid_visibility,
                    clear_finished_dock_drag,
                    sync_dock_drop_hints,
                    sync_tab_reorder_hints,
                    sync_tab_append_hint,
                    update_dock_zone_style,
                    rebuild_editor_ui,
                    sync_native_floating_windows,
                )
                    .chain(),
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
    Restart,
    StepFrame(i8),
    AdjustPreviewSeed(i8),
    Save,
    SaveAs,
    Undo,
    Redo,
    AddLayer,
    DuplicateLayer,
    DeleteLayer,
    SelectLayer(usize),
    LayerStart(f32),
    LayerDuration(f32),
    EffectDuration(f32),
    OpenModulePalette(StackStage),
    CloseModulePalette,
    AddModule(usize),
    AddSpriteRenderer,
    AdjustModuleInput {
        module: ModuleId,
        input: u8,
        component: u8,
        direction: i8,
    },
    EditComplexInput(ModuleId, u8),
    AddComplexKey,
    DeleteComplexKey,
    AdjustComplexTime(i8),
    AdjustCurveValue(i8),
    AdjustGradientChannel(u8, i8),
    ToggleModule(ModuleId),
    MoveModule(ModuleId, i8),
    DuplicateModule(ModuleId),
    DeleteModule(ModuleId),
    ToggleRenderer(RendererId),
    CycleRendererBlend(RendererId),
    AdjustRendererSoftness(RendererId, i8),
    DuplicateRenderer(RendererId),
    DeleteRenderer(RendererId),
    ApplyPendingChange,
    DiscardPendingChange,
    SetDiagnosticsFilter(DiagnosticsFilter),
    SelectDiagnostic {
        source: DiagnosticSource,
        index: usize,
    },
    SelectDockPanel(DockPanel),
    CloseDockPanel(DockPanel),
    ShowDockPanel(DockPanel),
    ToggleDockPanel(DockPanel),
    FloatDockPanel(DockPanel, [f32; 2]),
    ToggleMenu(MenuKind),
    TogglePanelsSubmenu,
    ToggleGrid,
    ResetWorkspaceLayout,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticSource {
    Current,
    Pending,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
    View,
    Help,
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
struct DiagnosticRow;

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
struct PreviewGrid;

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
struct PreviewParticle(usize);

#[derive(Component)]
struct PreviewCanvas;

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
}

#[derive(SystemParam)]
struct UiBuildResources<'w> {
    catalog: Res<'w, EffectCatalog>,
    layout: Res<'w, WorkspaceLayout>,
    menu: Res<'w, MenuState>,
    registry: Res<'w, EditorModuleRegistry>,
    palette: Res<'w, ModulePaletteState>,
    diagnostics_panel: Res<'w, DiagnosticsPanelState>,
    workspace: Res<'w, WorkspaceState>,
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
    editor_resources: (
        Res<EditorModuleRegistry>,
        Res<ModulePaletteState>,
        Res<WorkspaceState>,
        Res<DiagnosticsPanelState>,
    ),
    mut rendered: ResMut<RenderedUiRevision>,
) {
    let (registry, palette, workspace, diagnostics_panel) = editor_resources;
    commands.spawn(Camera2d);
    let sources = PanelSources {
        session: &session,
        catalog: &catalog,
        registry: &registry,
        palette: &palette,
        diagnostics_panel: &diagnostics_panel,
    };
    spawn_editor_ui(&mut commands, &menu, &workspace, &layout, sources);
    rendered.0 = session.ui_revision;
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
            spawn_menu_bar(root, sources.session, layout);
            spawn_toolbar(root, sources.session);
            spawn_editor_content(root, menu, workspace, layout, sources);
            spawn_status_bar(root, sources.session);
            spawn_about_overlay(root, menu.show_about);
        });
}

fn spawn_tab_context_menu(parent: &mut ChildSpawnerCommands, context: Option<TabContextMenu>) {
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
            menu.spawn((
                Button,
                EditorAction::FloatDockPanel(context.panel, context.position),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(30.0),
                    padding: UiRect::horizontal(Val::Px(9.0)),
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL),
            ))
            .with_children(|item| {
                item.spawn((
                    Text::new("Float Panel"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
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
            spawn_tab_context_menu(content, menu.tab_context);
        });
}

fn spawn_menu_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    layout: &WorkspaceLayout,
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
            BackgroundColor(theme::MENU),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|bar| {
            menu_button(bar, "File", MenuKind::File);
            menu_button(bar, "Edit", MenuKind::Edit);
            menu_button(bar, "View", MenuKind::View);
            menu_button(bar, "Help", MenuKind::Help);
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

            spawn_dropdown(
                bar,
                MenuKind::File,
                0.0,
                &[
                    ("New Effect", "Ctrl+N", EditorAction::NewEffect),
                    ("Open...", "Ctrl+O", EditorAction::OpenEffect),
                    ("Save", "Ctrl+S", EditorAction::Save),
                    ("Save As...", "Ctrl+Shift+S", EditorAction::SaveAs),
                ],
            );
            spawn_dropdown(
                bar,
                MenuKind::Edit,
                52.0,
                &[
                    ("Undo", "Ctrl+Z", EditorAction::Undo),
                    ("Redo", "Ctrl+Y", EditorAction::Redo),
                    ("Add Emitter", "Ctrl+Enter", EditorAction::AddLayer),
                    ("Duplicate Emitter", "Ctrl+D", EditorAction::DuplicateLayer),
                    ("Delete Emitter", "Delete", EditorAction::DeleteLayer),
                ],
            );
            spawn_view_dropdown(bar, layout);
            spawn_dropdown(
                bar,
                MenuKind::Help,
                164.0,
                &[("About Aestra", "", EditorAction::ShowAbout)],
            );
        });
}

fn menu_button(parent: &mut ChildSpawnerCommands, label: &str, menu: MenuKind) {
    parent
        .spawn((
            Button,
            EditorAction::ToggleMenu(menu),
            Node {
                width: Val::Px(52.0),
                height: Val::Px(28.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::MENU),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
        });
}

fn spawn_dropdown(
    parent: &mut ChildSpawnerCommands,
    menu: MenuKind,
    left: f32,
    items: &[(&str, &str, EditorAction)],
) {
    parent
        .spawn((
            MenuDropdown(menu),
            GlobalZIndex(100),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(29.0),
                width: Val::Px(218.0),
                padding: UiRect::all(Val::Px(5.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|dropdown| {
            for (label, shortcut, action) in items {
                let mut item = dropdown.spawn((
                    Button,
                    *action,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(29.0),
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ));
                match action {
                    EditorAction::Undo => {
                        item.insert(UndoMenuItem);
                    }
                    EditorAction::Redo => {
                        item.insert(RedoMenuItem);
                    }
                    _ => {}
                }
                item.with_children(|item| {
                    item.spawn((
                        Text::new(*label),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                    item.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    item.spawn((
                        Text::new(*shortcut),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                });
            }
        });
}

fn spawn_view_dropdown(parent: &mut ChildSpawnerCommands, layout: &WorkspaceLayout) {
    parent
        .spawn((
            MenuDropdown(MenuKind::View),
            GlobalZIndex(100),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: Val::Px(104.0),
                top: Val::Px(29.0),
                width: Val::Px(218.0),
                padding: UiRect::all(Val::Px(5.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|dropdown| {
            spawn_view_menu_item(dropdown, "Toggle Grid", "G", EditorAction::ToggleGrid);
            spawn_view_menu_item(dropdown, "Restart Preview", "R", EditorAction::Restart);
            spawn_view_menu_item(dropdown, "Panels", ">", EditorAction::TogglePanelsSubmenu);
            spawn_view_menu_item(
                dropdown,
                "Reset Workspace",
                "",
                EditorAction::ResetWorkspaceLayout,
            );

            dropdown
                .spawn((
                    PanelsSubmenu,
                    GlobalZIndex(101),
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        left: Val::Px(212.0),
                        top: Val::Px(63.0),
                        width: Val::Px(206.0),
                        padding: UiRect::all(Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|submenu| {
                    for panel in DockPanel::ALL {
                        let visible = layout.is_visible(panel);
                        let mut item = submenu.spawn((
                            Button,
                            EditorAction::ToggleDockPanel(panel),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(29.0),
                                padding: UiRect::horizontal(Val::Px(9.0)),
                                align_items: AlignItems::Center,
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL),
                        ));
                        if !panel.closable() {
                            item.insert(InteractionDisabled);
                        }
                        item.with_children(|row| {
                            row.spawn((
                                PanelVisibilityLabel(panel),
                                Text::new(panel_visibility_label(panel, visible)),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(if panel.closable() {
                                    theme::TEXT
                                } else {
                                    theme::TEXT_FAINT
                                }),
                                Pickable::IGNORE,
                            ));
                        });
                    }
                });
        });
}

fn spawn_view_menu_item(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    shortcut: &str,
    action: EditorAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(29.0),
                padding: UiRect::horizontal(Val::Px(9.0)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
        ))
        .with_children(|item| {
            item.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            item.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            item.spawn((
                Text::new(shortcut),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Pickable::IGNORE,
            ));
        });
}

fn panel_visibility_label(panel: DockPanel, visible: bool) -> String {
    format!("[{}]  {}", if visible { "x" } else { " " }, panel.title())
}

fn spawn_about_overlay(parent: &mut ChildSpawnerCommands, visible: bool) {
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
                    dialog.spawn((
                        Text::new("Bevy-native VFX choreography toolkit\nVersion 0.1.0"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::justify(Justify::Center),
                    ));
                    inspector_action_button(dialog, "Close", EditorAction::CloseAbout);
                });
        });
}

fn spawn_toolbar(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
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
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
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
            toolbar_button(bar, "Play", EditorAction::TogglePlayback, PlaybackLabel);
            toolbar_button(bar, "Restart", EditorAction::Restart, PlainMarker);
            toolbar_button(bar, "Save", EditorAction::Save, PlainMarker);
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
                    "{}  /  VFX CHOREOGRAPHY",
                    session.effect.name.to_uppercase()
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
                Text::new("BEVY 0.19  |  CPU REFERENCE"),
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
    label: &str,
    action: EditorAction,
    marker: M,
) {
    parent
        .spawn((
            Button,
            action,
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
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                marker,
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
            spawn_dock_tab_bar(pane, node, stack);
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
        DockPanel::Viewport => spawn_preview(parent),
        DockPanel::Assets => spawn_asset_browser(parent, sources.session, sources.catalog),
        DockPanel::Inspector => {
            spawn_inspector(parent, sources.session, sources.registry, sources.palette);
        }
        DockPanel::Timeline => spawn_timeline(parent, sources.session),
        DockPanel::Curves => {
            spawn_curves_workspace(parent, sources.session, sources.registry, workspace);
        }
        DockPanel::Diagnostics => {
            spawn_diagnostics_workspace(parent, sources.session, sources.diagnostics_panel);
        }
        DockPanel::Changes => spawn_changes_workspace(parent, sources.session),
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

fn spawn_dock_tab_bar(parent: &mut ChildSpawnerCommands, node: DockNodeId, stack: &DockStack) {
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
                spawn_dock_tab(bar, *panel, stack.active == Some(*panel));
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

fn spawn_dock_tab(parent: &mut ChildSpawnerCommands, panel: DockPanel, selected: bool) {
    parent
        .spawn((
            Button,
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
                Text::new(panel.title()),
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
                "LAYERS",
                &format!("{} ACTIVE", session.effect.emitters.len()),
            );
            toolbar_button(panel, "+ Add Emitter", EditorAction::AddLayer, PlainMarker);
            for (index, layer) in session.effect.emitters.iter().enumerate() {
                let selected = index == session.selected_layer_index();
                panel
                    .spawn((
                        Button,
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

fn spawn_preview(parent: &mut ChildSpawnerCommands) {
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
                    BackgroundColor(theme::VIEWPORT),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|canvas| {
                    spawn_preview_grid(canvas);
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(50.0),
                            top: Val::Percent(50.0),
                            width: Val::Px(4.0),
                            height: Val::Px(4.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::TEXT_FAINT),
                    ));
                    for index in 0..PARTICLE_POOL_SIZE {
                        canvas.spawn((
                            PreviewParticle(index),
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                width: Val::Px(4.0),
                                height: Val::Px(4.0),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(Color::WHITE),
                        ));
                    }
                    canvas.spawn((
                        Text::new("PERSPECTIVE  |  LIT  |  LOCAL SPACE"),
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

fn spawn_preview_grid(parent: &mut ChildSpawnerCommands) {
    for i in 1..8 {
        parent.spawn((
            PreviewGrid,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(i as f32 * 12.5),
                top: Val::Px(0.0),
                width: Val::Px(1.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(theme::GRID),
        ));
    }
    for i in 1..6 {
        parent.spawn((
            PreviewGrid,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Percent(i as f32 * 16.6667),
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(theme::GRID),
        ));
    }
}

fn spawn_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
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
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        padding: UiRect::bottom(Val::Px(12.0)),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .observe(
                    |scroll: On<Pointer<Scroll>>,
                     mut nodes: Query<(&mut ScrollPosition, &ComputedNode)>| {
                        if let Ok((mut position, computed)) = nodes.get_mut(scroll.entity) {
                            let delta = match scroll.unit {
                                MouseScrollUnit::Line => scroll.y * 24.0,
                                MouseScrollUnit::Pixel => scroll.y,
                            };
                            let range = (computed.content_size().y - computed.size().y).max(0.0)
                                * computed.inverse_scale_factor;
                            position.y = (position.y - delta).clamp(0.0, range);
                        }
                    },
                )
                .with_children(|stack| {
                    property_stepper(
                        stack,
                        &format!("Start  {:.2}s", layer.start_time),
                        EditorAction::LayerStart(-0.05),
                        EditorAction::LayerStart(0.05),
                    );
                    property_stepper(
                        stack,
                        &format!("Duration  {:.2}s", layer.duration),
                        EditorAction::LayerDuration(-0.05),
                        EditorAction::LayerDuration(0.05),
                    );
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
                });
            if palette.open {
                spawn_module_palette(panel, registry, palette);
            }
        });
}

fn spawn_stage_header(parent: &mut ChildSpawnerCommands, stage: StackStage) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                margin: UiRect::top(Val::Px(5.0)),
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

fn spawn_module_card(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let meta = metadata.map_or_else(
        || "Unknown module".to_string(),
        |item| format!("{}  ·  cost {}", item.category, item.approximate_cost),
    );
    parent
        .spawn((
            Node {
                width: Val::Auto,
                margin: UiRect::axes(Val::Px(9.0), Val::Px(3.0)),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if module.enabled {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(
                if session
                    .diagnostics
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.path.starts_with(diagnostic_path))
                {
                    Color::srgb(0.82, 0.28, 0.24)
                } else {
                    theme::BORDER_BRIGHT
                },
            ),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new(display_name),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(if module.enabled {
                        theme::TEXT
                    } else {
                        theme::TEXT_FAINT
                    }),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                stack_button(
                    header,
                    if module.enabled { "ON" } else { "OFF" },
                    EditorAction::ToggleModule(module.id),
                    34.0,
                );
                stack_button(header, "↑", EditorAction::MoveModule(module.id, -1), 24.0);
                stack_button(header, "↓", EditorAction::MoveModule(module.id, 1), 24.0);
                stack_button(
                    header,
                    "DUP",
                    EditorAction::DuplicateModule(module.id),
                    34.0,
                );
                stack_button(header, "×", EditorAction::DeleteModule(module.id), 24.0);
            });
            card.spawn((
                Text::new(meta),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            if let Some(metadata) = metadata {
                for (input_index, input) in metadata.inputs.iter().enumerate() {
                    spawn_input_control(card, module, input, input_index as u8);
                }
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        });
}

fn spawn_input_control(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
) {
    let Some(value) = module_parameter(module, input.name) else {
        property_stepper(
            parent,
            &format!("{}  missing", input.display_name),
            EditorAction::AdjustModuleInput {
                module: module.id,
                input: input_index,
                component: 0,
                direction: -1,
            },
            EditorAction::AdjustModuleInput {
                module: module.id,
                input: input_index,
                component: 0,
                direction: 1,
            },
        );
        return;
    };
    match (&input.control, &value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => inspector_action_button(
            parent,
            &format!("{}  ·  {} keys  →", input.display_name, curve.keys.len()),
            EditorAction::EditComplexInput(module.id, input_index),
        ),
        (InputControl::Gradient, Value::Gradient(gradient)) => inspector_action_button(
            parent,
            &format!(
                "{}  ·  {} color keys  →",
                input.display_name,
                gradient.keys.len()
            ),
            EditorAction::EditComplexInput(module.id, input_index),
        ),
        (InputControl::Vector { .. }, Value::Vec2(vector)) => {
            for (component, axis) in ["X", "Y"].into_iter().enumerate() {
                metadata_stepper(
                    parent,
                    module.id,
                    input_index,
                    component as u8,
                    &format!(
                        "{} {axis}  {:.2}{}",
                        input.display_name,
                        vector[component],
                        unit_suffix(input)
                    ),
                );
            }
        }
        (InputControl::Range { .. }, Value::Range(range)) => {
            metadata_stepper(
                parent,
                module.id,
                input_index,
                0,
                &format!(
                    "{} Min  {:.2}{}",
                    input.display_name,
                    range.min,
                    unit_suffix(input)
                ),
            );
            metadata_stepper(
                parent,
                module.id,
                input_index,
                1,
                &format!(
                    "{} Max  {:.2}{}",
                    input.display_name,
                    range.max,
                    unit_suffix(input)
                ),
            );
        }
        _ => metadata_stepper(
            parent,
            module.id,
            input_index,
            0,
            &format!(
                "{}  {}{}",
                input.display_name,
                format_value(value),
                unit_suffix(input)
            ),
        ),
    }
    parent.spawn((
        Text::new(input.description),
        TextFont {
            font_size: FontSize::Px(7.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
        Node {
            margin: UiRect::horizontal(Val::Px(12.0)),
            ..default()
        },
    ));
}

fn metadata_stepper(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input: u8,
    component: u8,
    label: &str,
) {
    property_stepper(
        parent,
        label,
        EditorAction::AdjustModuleInput {
            module,
            input,
            component,
            direction: -1,
        },
        EditorAction::AdjustModuleInput {
            module,
            input,
            component,
            direction: 1,
        },
    );
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
) {
    parent
        .spawn((
            Node {
                width: Val::Auto,
                margin: UiRect::axes(Val::Px(9.0), Val::Px(3.0)),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new("Sprite Renderer"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                stack_button(
                    header,
                    if renderer.enabled { "ON" } else { "OFF" },
                    EditorAction::ToggleRenderer(renderer.id),
                    34.0,
                );
                stack_button(
                    header,
                    "DUP",
                    EditorAction::DuplicateRenderer(renderer.id),
                    34.0,
                );
                stack_button(header, "×", EditorAction::DeleteRenderer(renderer.id), 24.0);
            });
            inspector_action_button(
                card,
                &format!("Blend  {:?}", renderer.blend),
                EditorAction::CycleRendererBlend(renderer.id),
            );
            let RendererProperties::Sprite { softness } = renderer.properties else {
                spawn_inline_diagnostics(card, diagnostic_path, session);
                return;
            };
            property_stepper(
                card,
                &format!("Softness  {softness:.2}"),
                EditorAction::AdjustRendererSoftness(renderer.id, -1),
                EditorAction::AdjustRendererSoftness(renderer.id, 1),
            );
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
                    &format!(
                        "{} · {} · cost {}",
                        metadata.category, metadata.type_id.0, metadata.approximate_cost
                    ),
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
        .spawn((
            Button,
            action,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: Val::Px(2.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::BUTTON),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
            button.spawn((
                Text::new(subtitle),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn stack_button(parent: &mut ChildSpawnerCommands, label: &str, action: EditorAction, width: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(width),
                height: Val::Px(21.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::BUTTON),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
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

fn adjusted_module_value(
    module: &ModuleInstance,
    input: &InputMetadata,
    component: u8,
    direction: i8,
) -> Option<Value> {
    let direction = direction as f32;
    let value = module_parameter(module, input.name)?;
    Some(match (&input.control, value) {
        (InputControl::Toggle, Value::Bool(value)) => Value::Bool(!value),
        (InputControl::Number { step, min, max }, Value::U32(value)) => {
            let delta = (*step).round().max(1.0) as u32;
            let value = if direction < 0.0 {
                value.saturating_sub(delta)
            } else {
                value.saturating_add(delta)
            };
            Value::U32(clamp_number(value as f32, *min, *max).round() as u32)
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            Value::Scalar(clamp_number(value + direction * step, *min, *max))
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(mut value)) => {
            let target = value.get_mut(component as usize)?;
            *target = clamp_number(*target + direction * step, *min, *max);
            Value::Vec2(value)
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(mut value)) => {
            let target = value.get_mut(component as usize)?;
            *target = clamp_number(*target + direction * step, *min, *max);
            Value::Vec3(value)
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(mut value)) => {
            let target = value.get_mut(component as usize)?;
            *target = clamp_number(*target + direction * step, *min, *max);
            Value::Vec4(value)
        }
        (InputControl::Range { step, min, max }, Value::Range(mut value)) => {
            if component == 0 {
                value.min = clamp_number(value.min + direction * step, *min, *max).min(value.max);
            } else {
                value.max = clamp_number(value.max + direction * step, *min, *max).max(value.min);
            }
            Value::Range(value)
        }
        (InputControl::Choice, Value::Shape(shape)) => {
            let index = match shape {
                EmitterShape::Point => 0,
                EmitterShape::Circle { .. } => 1,
                EmitterShape::Ring { .. } => 2,
                EmitterShape::Cone { .. } => 3,
            };
            let next = (index as i8 + direction.signum() as i8).rem_euclid(4);
            Value::Shape(match next {
                0 => EmitterShape::Point,
                1 => EmitterShape::Circle { radius: 12.0 },
                2 => EmitterShape::Ring { radius: 12.0 },
                _ => EmitterShape::Cone {
                    radius: 12.0,
                    depth: 24.0,
                },
            })
        }
        _ => return None,
    })
}

fn clamp_number(mut value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    if let Some(min) = min {
        value = value.max(min);
    }
    if let Some(max) = max {
        value = value.min(max);
    }
    value
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
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .observe(
                    |scroll: On<Pointer<Scroll>>,
                     mut nodes: Query<(&mut ScrollPosition, &ComputedNode)>| {
                        if let Ok((mut position, computed)) = nodes.get_mut(scroll.entity) {
                            let delta = match scroll.unit {
                                MouseScrollUnit::Line => scroll.y * 24.0,
                                MouseScrollUnit::Pixel => scroll.y,
                            };
                            let range = (computed.content_size().y - computed.size().y).max(0.0)
                                * computed.inverse_scale_factor;
                            position.y = (position.y - delta).clamp(0.0, range);
                        }
                    },
                )
                .with_children(|list| {
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
                });
        });
}

fn spawn_diagnostics_filter_button(
    parent: &mut ChildSpawnerCommands,
    filter: DiagnosticsFilter,
    selected: bool,
    count: usize,
) {
    parent
        .spawn((
            Button,
            EditorAction::SetDiagnosticsFilter(filter),
            DiagnosticsFilterButton(filter),
            Node {
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(8.0)),
                            row_gap: Val::Px(4.0),
                            overflow: Overflow::scroll_y(),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                        ScrollPosition::default(),
                    ))
                    .with_children(|changes| {
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
                    });
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(10.0)),
                        row_gap: Val::Px(6.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    })
                    .with_children(|review| {
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
                                );
                                inspector_action_button(
                                    actions,
                                    if pending.can_apply { "Apply" } else { "Apply blocked" },
                                    EditorAction::ApplyPendingChange,
                                );
                            });
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
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(7.0)),
                row_gap: Val::Px(4.0),
                border: UiRect::right(Val::Px(1.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
            ScrollPosition::default(),
        ))
        .with_children(|list| {
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
                        selection.module == module.id && selection.input == input_index as u8
                    });
                    parent_list_button(
                        list,
                        &format!("{} / {}", metadata.display_name, input.display_name),
                        EditorAction::EditComplexInput(module.id, input_index as u8),
                        selected,
                    );
                }
            }
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

fn spawn_status_bar(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
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
            bar.spawn((
                Button,
                CompileStatusButton,
                EditorAction::ShowDockPanel(DockPanel::Diagnostics),
                Node {
                    height: Val::Px(20.0),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
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
                    Text::new(compile_status),
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

fn compile_status(session: &EditorSession) -> (String, Color) {
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
        ("COMPILE FAILED".into(), Color::srgb(1.0, 0.38, 0.32))
    } else if pending_errors > 0 {
        ("PREVIEW BLOCKED".into(), Color::srgb(1.0, 0.74, 0.30))
    } else if warnings > 0 {
        (
            "COMPILED WITH WARNINGS".into(),
            Color::srgb(1.0, 0.74, 0.30),
        )
    } else {
        ("COMPILED".into(), Color::srgb(0.35, 0.88, 0.57))
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

fn property_stepper(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    decrease: EditorAction,
    increase: EditorAction,
) {
    parent
        .spawn(Node {
            height: Val::Px(34.0),
            width: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            mini_button(row, "-", decrease);
            mini_button(row, "+", increase);
        });
}

fn mini_button(parent: &mut ChildSpawnerCommands, label: &str, action: EditorAction) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(theme::BUTTON),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
        });
}

fn inspector_action_button(parent: &mut ChildSpawnerCommands, label: &str, action: EditorAction) {
    parent
        .spawn((
            Button,
            action,
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
            BackgroundColor(theme::BUTTON),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
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
) {
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
    if control && keys.just_pressed(KeyCode::KeyN) && confirm_discard(&session) {
        session.new_effect();
        workspace.complex = None;
    }
    if control && keys.just_pressed(KeyCode::KeyO) {
        open_effect_dialog(&mut session);
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
    }
}

#[allow(clippy::type_complexity)]
fn handle_buttons(
    mut buttons: Query<
        (
            &Interaction,
            &EditorAction,
            Option<&LayerRow>,
            Option<&DockTab>,
            Option<&DockCloseButton>,
            Option<&DiagnosticsFilterButton>,
            Option<&CompileStatusButton>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (Changed<Interaction>, With<Button>),
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
    ),
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let (catalog, registry, mut palette, mut workspace, mut layout, mut diagnostics_panel) =
        editor_resources;
    for (
        interaction,
        action,
        layer_row,
        dock_tab,
        dock_close,
        diagnostics_filter,
        compile_status,
        disabled,
        mut background,
    ) in &mut buttons
    {
        if disabled.is_some() {
            background.0 = theme::PANEL_DARK;
            continue;
        }
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
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
                } else if compile_status.is_some() {
                    theme::PANEL_DARK
                } else {
                    theme::BUTTON
                };
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
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
                        if confirm_discard(&session) {
                            session.new_effect();
                            workspace.complex = None;
                        }
                    }
                    EditorAction::OpenEffect => {
                        open_effect_dialog(&mut session);
                        workspace.complex = None;
                    }
                    EditorAction::OpenCatalog(index) => {
                        if confirm_discard(&session) {
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
                    EditorAction::Undo => session.undo(),
                    EditorAction::Redo => session.redo(),
                    EditorAction::AddLayer => session.add_layer(),
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
                    EditorAction::LayerStart(delta) => session.adjust_selected_start(delta),
                    EditorAction::LayerDuration(delta) => {
                        session.adjust_selected_duration(delta);
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
                    EditorAction::AdjustModuleInput {
                        module,
                        input,
                        component,
                        direction,
                    } => adjust_module_input(
                        &mut session,
                        &registry.0,
                        module,
                        input,
                        component,
                        direction,
                    ),
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
                    EditorAction::ToggleModule(id) => session.toggle_module(id),
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
                    EditorAction::ToggleRenderer(id) => session.toggle_renderer(id),
                    EditorAction::CycleRendererBlend(id) => session.cycle_renderer_blend(id),
                    EditorAction::AdjustRendererSoftness(id, direction) => {
                        session.adjust_renderer_softness(id, direction as f32 * 0.1);
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
                    EditorAction::ToggleGrid => menu.show_grid = !menu.show_grid,
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

fn dismiss_tab_context_menu(
    buttons: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    if buttons.just_pressed(MouseButton::Left) && menu.tab_context.take().is_some() {
        session.ui_revision += 1;
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

fn adjust_module_input(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    component: u8,
    direction: i8,
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
    let parameter = input.name;
    let Some(value) = adjusted_module_value(module, input, component, direction) else {
        session.status = format!("{} needs a dedicated editor", input.display_name);
        return;
    };
    session.set_module_parameter(module_id, parameter, value);
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

fn open_effect_dialog(session: &mut EditorSession) {
    if !confirm_discard(session) {
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

fn confirm_discard(session: &EditorSession) -> bool {
    if !session.dirty {
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
    mut dropdowns: Query<(&MenuDropdown, &mut Node)>,
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
    for (dropdown, mut node) in &mut dropdowns {
        node.display = if menu.open == Some(dropdown.0) {
            Display::Flex
        } else {
            Display::None
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

fn update_panel_visibility_labels(
    layout: Res<WorkspaceLayout>,
    mut labels: Query<(&PanelVisibilityLabel, &mut Text)>,
) {
    if !layout.is_changed() {
        return;
    }
    for (label, mut text) in &mut labels {
        text.0 = panel_visibility_label(label.0, layout.is_visible(label.0));
    }
}

fn update_preview_grid_visibility(
    menu: Res<MenuState>,
    mut grid: Query<&mut Visibility, With<PreviewGrid>>,
) {
    if !menu.is_changed() {
        return;
    }
    for mut visibility in &mut grid {
        *visibility = if menu.show_grid {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn handle_window_close_requests(
    mut close_requests: MessageReader<WindowCloseRequested>,
    primary: Single<Entity, With<PrimaryWindow>>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
) {
    for request in close_requests.read() {
        if request.window == *primary {
            commands.write_message(AppExit::Success);
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
    };
    for floating in &editor_resources.layout.floating {
        if windows.iter().any(|(_, native)| native.0 == floating.panel) {
            continue;
        }
        let window = commands
            .spawn((
                Window {
                    title: format!("{} — Aestra", floating.panel.title()),
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

fn update_preview(
    mut session: ResMut<EditorSession>,
    mut particles: Query<(&PreviewParticle, &mut Node, &mut BackgroundColor)>,
    canvas: Single<&ComputedNode, With<PreviewCanvas>>,
) {
    let mut samples = std::mem::take(&mut session.samples);
    session.evaluate_preview(&mut samples);
    session.samples = samples;
    let canvas_size = canvas.size() * canvas.inverse_scale_factor;
    for (marker, mut node, mut background) in &mut particles {
        let Some(sample) = session.samples.get(marker.0) else {
            node.display = Display::None;
            continue;
        };
        let scale = sample.size.clamp(1.0, 38.0);
        node.display = Display::Flex;
        node.left = Val::Px(canvas_size.x * 0.5 + sample.position[0] - scale * 0.5);
        node.top = Val::Px(canvas_size.y * 0.5 - sample.position[1] - scale * 0.5);
        node.width = Val::Px(scale);
        node.height = Val::Px(scale);
        background.0 = Color::srgba(
            sample.color[0],
            sample.color[1],
            sample.color[2],
            sample.color[3],
        );
    }
}

#[allow(clippy::type_complexity)]
fn update_editor_labels(
    session: Res<EditorSession>,
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
    if !session.is_changed() {
        return;
    }
    let layer = session.selected_layer();
    for (mut text, playback, time, title, count, document_menu, document_toolbar) in &mut labels {
        if playback.is_some() {
            text.0 = if session.playing { "Pause" } else { "Play" }.into();
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
            text.0 = format!("{} LIVE PARTICLES  |  60 FPS", session.samples.len());
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
                "{}  /  VFX CHOREOGRAPHY",
                session.effect.name.to_uppercase()
            );
        }
    }
}

fn update_compile_status(
    session: Res<EditorSession>,
    mut labels: Query<(&mut Text, &mut TextColor), With<CompileStatusLabel>>,
    mut dots: Query<&mut BackgroundColor, With<CompileStatusDot>>,
) {
    if !session.is_changed() {
        return;
    }
    let (label, color) = compile_status(&session);
    for (mut text, mut text_color) in &mut labels {
        text.0 = label.clone();
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
    mut items: Query<
        (
            Entity,
            Has<UndoMenuItem>,
            Has<RedoMenuItem>,
            &mut BackgroundColor,
        ),
        Or<(With<UndoMenuItem>, With<RedoMenuItem>)>,
    >,
) {
    if !session.is_changed() {
        return;
    }
    for (entity, undo, redo, mut background) in &mut items {
        let enabled = (undo && session.can_undo()) || (redo && session.can_redo());
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
            background.0 = theme::PANEL;
        } else {
            commands.entity(entity).insert(InteractionDisabled);
            background.0 = theme::PANEL_DARK;
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
        let particle = app
            .world_mut()
            .spawn((PreviewParticle(0), BackgroundColor(particle_color)))
            .id();
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
    fn bundled_effect_is_valid() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).expect("bundled effect should parse");
        assert_eq!(effect.format_version, 2);
        assert_eq!(effect.emitters.len(), 4);
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
    fn metadata_control_adjusts_builtin_values() {
        let module = ModuleInstance::emission(20.0, 4);
        let registry = ModuleRegistry::builtin();
        let metadata = registry.get(&module.module_type).unwrap();
        assert_eq!(
            adjusted_module_value(&module, &metadata.inputs[0], 0, 1),
            Some(Value::Scalar(25.0))
        );
        assert_eq!(
            adjusted_module_value(&module, &metadata.inputs[1], 0, -1),
            Some(Value::U32(0))
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
        assert_eq!(compile_status(&session).0, "COMPILED");

        session.diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidDuration,
            "effect.duration",
            "invalid test duration",
        ));
        assert_eq!(compile_status(&session).0, "COMPILE FAILED");
    }

    #[test]
    fn panel_visibility_labels_use_checkbox_notation() {
        assert_eq!(
            panel_visibility_label(DockPanel::Diagnostics, true),
            "[x]  DIAGNOSTICS"
        );
        assert_eq!(
            panel_visibility_label(DockPanel::Diagnostics, false),
            "[ ]  DIAGNOSTICS"
        );
    }
}
