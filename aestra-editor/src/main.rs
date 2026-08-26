mod session;
mod theme;

use aestra_authoring::{ChangeKind, EffectCommand, EffectTransaction};
use aestra_bevy::{
    ColorKey, CurveKey, DiagnosticCode, DiagnosticSeverity, EffectAsset, EmitterShape, ModuleId,
    ModuleInstance, ModuleParameters, RendererId, RendererProperties, StageKind, Value,
};
use aestra_compiler::{InputControl, InputMetadata, ModuleMetadata, ModuleRegistry};
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput, mouse::MouseScrollUnit},
    picking::events::{Click, Drag, DragEnd, Pointer, Scroll},
    prelude::*,
    ui::RelativeCursorPosition,
    window::WindowResolution,
};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use session::EditorSession;
use std::{fs, path::PathBuf};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const PARTICLE_POOL_SIZE: usize = 384;
const PREVIEW_WIDTH: f32 = 680.0;
const PREVIEW_HEIGHT: f32 = 430.0;

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
        .init_resource::<WorkspaceState>()
        .init_resource::<RenderedUiRevision>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Aestra — VFX Choreography Editor".into(),
                resolution: WindowResolution::new(1440, 900),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_editor)
        .add_systems(
            Update,
            (
                module_palette_keyboard,
                keyboard_shortcuts,
                handle_buttons,
                scrub_timeline,
                advance_playback,
                update_preview,
                update_editor_labels,
                update_playhead,
                update_layer_selection,
                update_menu_visibility,
                update_preview_grid_visibility,
                rebuild_editor_ui,
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
    SelectWorkspace(WorkspaceTab),
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
    ToggleMenu(MenuKind),
    ToggleGrid,
    ShowAbout,
    CloseAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceTab {
    Timeline,
    Curves,
    Changes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComplexSelection {
    module: ModuleId,
    input: u8,
    key: usize,
}

#[derive(Resource)]
struct WorkspaceState {
    tab: WorkspaceTab,
    complex: Option<ComplexSelection>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            tab: WorkspaceTab::Timeline,
            complex: None,
        }
    }
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
    show_grid: bool,
    show_about: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: None,
            show_grid: true,
            show_about: false,
        }
    }
}

#[derive(Resource, Default)]
struct RenderedUiRevision(u64);

#[derive(Component)]
struct EditorRoot;

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
struct PlaybackLabel;

#[derive(Component)]
struct TimeLabel;

#[derive(Component)]
struct StatusLabel;

#[derive(Component)]
struct InspectorTitle;

#[derive(Component)]
struct ParticleCountLabel;

#[derive(Component)]
struct Playhead;

#[derive(Component)]
struct LayerRow(usize);

fn setup_editor(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    catalog: Res<EffectCatalog>,
    editor_resources: (
        Res<EditorModuleRegistry>,
        Res<ModulePaletteState>,
        Res<WorkspaceState>,
    ),
    mut rendered: ResMut<RenderedUiRevision>,
) {
    let (registry, palette, workspace) = editor_resources;
    commands.spawn(Camera2d);
    spawn_editor_ui(
        &mut commands,
        &session,
        &menu,
        &catalog,
        &registry,
        &palette,
        &workspace,
    );
    rendered.0 = session.ui_revision;
}

fn spawn_editor_ui(
    commands: &mut Commands,
    session: &EditorSession,
    menu: &MenuState,
    catalog: &EffectCatalog,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
    workspace: &WorkspaceState,
) {
    commands
        .spawn((
            EditorRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::APP_BG),
        ))
        .with_children(|root| {
            spawn_menu_bar(root, session);
            spawn_toolbar(root, session);
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(360.0),
                    ..default()
                },
                BackgroundColor(theme::APP_BG),
            ))
            .with_children(|main| {
                spawn_asset_browser(main, session, catalog);
                spawn_preview(main);
                spawn_inspector(main, session, registry, palette);
            });
            spawn_workspace(root, session, registry, workspace);
            spawn_status_bar(root);
            spawn_about_overlay(root, menu.show_about);
        });
}

fn spawn_menu_bar(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn((
            Node {
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
            let undo_label = if session.can_undo() {
                "Undo"
            } else {
                "Undo (empty)"
            };
            let redo_label = if session.can_redo() {
                "Redo"
            } else {
                "Redo (empty)"
            };
            spawn_dropdown(
                bar,
                MenuKind::Edit,
                52.0,
                &[
                    (undo_label, "Ctrl+Z", EditorAction::Undo),
                    (redo_label, "Ctrl+Y", EditorAction::Redo),
                    ("Add Emitter", "Ctrl+Enter", EditorAction::AddLayer),
                    ("Duplicate Emitter", "Ctrl+D", EditorAction::DuplicateLayer),
                    ("Delete Emitter", "Delete", EditorAction::DeleteLayer),
                ],
            );
            spawn_dropdown(
                bar,
                MenuKind::View,
                104.0,
                &[
                    ("Toggle Grid", "G", EditorAction::ToggleGrid),
                    ("Restart Preview", "R", EditorAction::Restart),
                ],
            );
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
                dropdown
                    .spawn((
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
                    ))
                    .with_children(|item| {
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

fn spawn_asset_browser(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &EffectCatalog,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(224.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
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
        .spawn((
            Node {
                flex_grow: 1.0,
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                min_width: Val::Px(500.0),
                ..default()
            },
            BackgroundColor(theme::VIEWPORT_FRAME),
        ))
        .with_children(|column| {
            column
                .spawn((
                    Node {
                        width: Val::Px(PREVIEW_WIDTH),
                        max_width: Val::Percent(100.0),
                        height: Val::Px(PREVIEW_HEIGHT),
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
                            left: Val::Px(PREVIEW_WIDTH * 0.5 - 2.0),
                            top: Val::Px(PREVIEW_HEIGHT * 0.5 - 2.0),
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
        .spawn((
            Node {
                width: Val::Px(390.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
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

fn spawn_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &WorkspaceState,
) {
    match workspace.tab {
        WorkspaceTab::Timeline => spawn_timeline(parent, session),
        WorkspaceTab::Curves => spawn_curves_workspace(parent, session, registry, workspace),
        WorkspaceTab::Changes => spawn_changes_workspace(parent, session),
    }
}

fn spawn_timeline(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(226.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
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
                    workspace_tab_button(header, "TIMELINE", WorkspaceTab::Timeline, true);
                    workspace_tab_button(header, "CURVES", WorkspaceTab::Curves, false);
                    workspace_tab_button(header, "CHANGES", WorkspaceTab::Changes, false);
                    header.spawn((
                        Text::new("00:00.000  /  00:02.800"),
                        TimeLabel,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(format!("Duration {:.2}s", session.effect.duration)),
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
                        spawn_ruler(tracks, session.effect.duration);
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
                                    let start = layer.start_time / session.effect.duration * 100.0;
                                    let width = layer.duration / session.effect.duration * 100.0;
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

fn workspace_tab_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    tab: WorkspaceTab,
    selected: bool,
) {
    parent
        .spawn((
            Button,
            EditorAction::SelectWorkspace(tab),
            Node {
                width: Val::Px(82.0),
                height: Val::Px(28.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL_LIGHT
            }),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.0),
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

fn spawn_curves_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &WorkspaceState,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(226.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
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
                    workspace_tab_button(header, "TIMELINE", WorkspaceTab::Timeline, false);
                    workspace_tab_button(header, "CURVES", WorkspaceTab::Curves, true);
                    workspace_tab_button(header, "CHANGES", WorkspaceTab::Changes, false);
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

fn spawn_changes_workspace(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(226.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
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
                    workspace_tab_button(header, "TIMELINE", WorkspaceTab::Timeline, false);
                    workspace_tab_button(header, "CURVES", WorkspaceTab::Curves, false);
                    workspace_tab_button(header, "CHANGES", WorkspaceTab::Changes, true);
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

fn spawn_status_bar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|bar| {
            bar.spawn((
                Text::new("READY"),
                StatusLabel,
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            bar.spawn((
                Text::new("SPACE Play/Pause   |   CTRL+S Save   |   R Restart"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
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
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if keys.just_pressed(KeyCode::Escape) {
        menu.open = None;
        menu.show_about = false;
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
        workspace.tab = WorkspaceTab::Changes;
        workspace.complex = None;
    }
    if keys.just_pressed(KeyCode::Space) {
        session.playing = !session.playing;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        session.restart();
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
            &mut BackgroundColor,
        ),
        (Changed<Interaction>, With<Button>),
    >,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    catalog: Res<EffectCatalog>,
    registry: Res<EditorModuleRegistry>,
    mut palette: ResMut<ModulePaletteState>,
    mut workspace: ResMut<WorkspaceState>,
) {
    for (interaction, action, layer_row, mut background) in &mut buttons {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = layer_row.map_or(theme::BUTTON, |row| {
                    if row.0 == session.selected_layer_index() {
                        theme::SELECTION
                    } else {
                        theme::PANEL_DARK
                    }
                });
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
                if let EditorAction::ToggleMenu(kind) = *action {
                    menu.open = if menu.open == Some(kind) {
                        None
                    } else {
                        Some(kind)
                    };
                    continue;
                }
                menu.open = None;
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
                            workspace.tab = WorkspaceTab::Changes;
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
                        workspace.tab = WorkspaceTab::Curves;
                        workspace.complex = Some(ComplexSelection {
                            module,
                            input,
                            key: 0,
                        });
                        session.ui_revision += 1;
                    }
                    EditorAction::SelectWorkspace(tab) => {
                        workspace.tab = tab;
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
                            workspace.tab = WorkspaceTab::Changes;
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
                            workspace.tab = WorkspaceTab::Changes;
                            workspace.complex = None;
                        }
                    }
                    EditorAction::ApplyPendingChange => {
                        session.apply_pending_change();
                    }
                    EditorAction::DiscardPendingChange => {
                        session.discard_pending_change();
                    }
                    EditorAction::ToggleGrid => menu.show_grid = !menu.show_grid,
                    EditorAction::ShowAbout => menu.show_about = true,
                    EditorAction::CloseAbout => menu.show_about = false,
                    EditorAction::ToggleMenu(_) => unreachable!(),
                }
            }
        }
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
        session.time = position.x.clamp(0.0, 1.0) * session.effect.duration;
        session.playing = false;
        session.status = format!("Scrubbed to {:.3}s", session.time);
    }
}

fn update_menu_visibility(
    menu: Res<MenuState>,
    mut dropdowns: Query<(&MenuDropdown, &mut Node)>,
    mut about: Query<&mut Node, (With<AboutOverlay>, Without<MenuDropdown>)>,
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
    for mut node in &mut about {
        node.display = if menu.show_about {
            Display::Flex
        } else {
            Display::None
        };
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

fn rebuild_editor_ui(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    mut catalog: ResMut<EffectCatalog>,
    editor_resources: (
        Res<EditorModuleRegistry>,
        Res<ModulePaletteState>,
        Res<WorkspaceState>,
    ),
    mut rendered: ResMut<RenderedUiRevision>,
    roots: Query<Entity, With<EditorRoot>>,
) {
    let (registry, palette, workspace) = editor_resources;
    if rendered.0 == session.ui_revision {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    *catalog = EffectCatalog::scan();
    spawn_editor_ui(
        &mut commands,
        &session,
        &menu,
        &catalog,
        &registry,
        &palette,
        &workspace,
    );
    rendered.0 = session.ui_revision;
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    if !session.playing {
        return;
    }
    session.time += time.delta_secs() * session.speed;
    if session.time > session.effect.duration {
        if session.effect.looping {
            session.time = session.time.rem_euclid(session.effect.duration);
        } else {
            session.time = session.effect.duration;
            session.playing = false;
        }
    }
}

fn update_preview(
    mut session: ResMut<EditorSession>,
    mut particles: Query<(&PreviewParticle, &mut Node, &mut BackgroundColor)>,
) {
    let time = session.time;
    let mut samples = std::mem::take(&mut session.samples);
    if let Some(preview) = &mut session.preview {
        preview.seek(time);
        preview.evaluate(&mut samples);
    } else {
        samples.clear();
    }
    session.samples = samples;
    for (marker, mut node, mut background) in &mut particles {
        let Some(sample) = session.samples.get(marker.0) else {
            node.display = Display::None;
            continue;
        };
        let scale = sample.size.clamp(1.0, 38.0);
        node.display = Display::Flex;
        node.left = Val::Px(PREVIEW_WIDTH * 0.5 + sample.position[0] - scale * 0.5);
        node.top = Val::Px(PREVIEW_HEIGHT * 0.5 - sample.position[1] - scale * 0.5);
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
        Option<&StatusLabel>,
        Option<&InspectorTitle>,
        Option<&ParticleCountLabel>,
    )>,
) {
    if !session.is_changed() {
        return;
    }
    let layer = session.selected_layer();
    for (mut text, playback, time, status, title, count) in &mut labels {
        if playback.is_some() {
            text.0 = if session.playing { "Pause" } else { "Play" }.into();
        } else if time.is_some() {
            text.0 = format!(
                "{:02}:{:06.3}  /  00:{:06.3}",
                0, session.time, session.effect.duration
            );
        } else if status.is_some() {
            text.0 = format!(
                "{}{}",
                if session.dirty { "*  " } else { "" },
                session.status
            );
        } else if title.is_some() {
            text.0 = layer.name.clone();
        } else if count.is_some() {
            text.0 = format!("{} LIVE PARTICLES  |  60 FPS", session.samples.len());
        }
    }
}

fn update_playhead(session: Res<EditorSession>, mut playhead: Query<&mut Node, With<Playhead>>) {
    if let Ok(mut node) = playhead.single_mut() {
        node.left = Val::Percent(session.time / session.effect.duration * 100.0);
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
}
