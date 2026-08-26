mod session;
mod theme;

use aestra_bevy::EffectAsset;
use aestra_bevy::evaluate;
use bevy::{prelude::*, ui::RelativeCursorPosition, window::WindowResolution};
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
    SpawnRate(f32),
    Burst(i32),
    Lifetime(f32),
    LayerStart(f32),
    LayerDuration(f32),
    EffectDuration(f32),
    CurveValue {
        curve: CurveTarget,
        key: usize,
        delta: f32,
    },
    ColorPreset,
    ToggleLayer,
    ToggleMenu(MenuKind),
    ToggleGrid,
    ShowAbout,
    CloseAbout,
}

#[derive(Clone, Copy)]
enum CurveTarget {
    Size,
    Opacity,
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
struct InspectorValues;

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
    mut rendered: ResMut<RenderedUiRevision>,
) {
    commands.spawn(Camera2d);
    spawn_editor_ui(&mut commands, &session, &menu, &catalog);
    rendered.0 = session.ui_revision;
}

fn spawn_editor_ui(
    commands: &mut Commands,
    session: &EditorSession,
    menu: &MenuState,
    catalog: &EffectCatalog,
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
                spawn_inspector(main, session);
            });
            spawn_timeline(root, session);
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

fn spawn_inspector(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    let layer = session.selected_layer();
    parent
        .spawn((
            Node {
                width: Val::Px(286.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|panel| {
            panel_heading(panel, "INSPECTOR", "EMITTER");
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
            inspector_section(panel, "EMISSION");
            property_stepper(
                panel,
                &format!("Spawn rate  {:.1}/s", layer.spawn_rate()),
                EditorAction::SpawnRate(-5.0),
                EditorAction::SpawnRate(5.0),
            );
            property_stepper(
                panel,
                &format!("Burst  {}", layer.burst_count()),
                EditorAction::Burst(-4),
                EditorAction::Burst(4),
            );
            property_stepper(
                panel,
                &format!(
                    "Lifetime  {:.2}-{:.2}s",
                    layer.lifetime().min,
                    layer.lifetime().max
                ),
                EditorAction::Lifetime(-0.1),
                EditorAction::Lifetime(0.1),
            );
            inspector_section(panel, "CHOREOGRAPHY");
            property_stepper(
                panel,
                &format!("Start  {:.2}s", layer.start_time),
                EditorAction::LayerStart(-0.05),
                EditorAction::LayerStart(0.05),
            );
            property_stepper(
                panel,
                &format!("Duration  {:.2}s", layer.duration),
                EditorAction::LayerDuration(-0.05),
                EditorAction::LayerDuration(0.05),
            );

            inspector_section(panel, "CURVES & COLOR");
            spawn_curve_preview(panel, layer.size_curve());
            spawn_gradient_preview(panel, layer.color_gradient());
            let size_middle = layer.size_curve().keys.len() / 2;
            let size_end = layer.size_curve().keys.len().saturating_sub(1);
            let opacity_middle = layer.opacity_curve().keys.len() / 2;
            property_stepper(
                panel,
                &format!("Size start  {:.1}", layer.size_curve().keys[0].value),
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: 0,
                    delta: -1.0,
                },
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: 0,
                    delta: 1.0,
                },
            );
            property_stepper(
                panel,
                &format!(
                    "Size peak  {:.1}",
                    layer.size_curve().keys[size_middle].value
                ),
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: size_middle,
                    delta: -1.0,
                },
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: size_middle,
                    delta: 1.0,
                },
            );
            property_stepper(
                panel,
                &format!("Size end  {:.1}", layer.size_curve().keys[size_end].value),
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: size_end,
                    delta: -1.0,
                },
                EditorAction::CurveValue {
                    curve: CurveTarget::Size,
                    key: size_end,
                    delta: 1.0,
                },
            );
            property_stepper(
                panel,
                &format!(
                    "Opacity peak  {:.2}",
                    layer.opacity_curve().keys[opacity_middle].value
                ),
                EditorAction::CurveValue {
                    curve: CurveTarget::Opacity,
                    key: opacity_middle,
                    delta: -0.1,
                },
                EditorAction::CurveValue {
                    curve: CurveTarget::Opacity,
                    key: opacity_middle,
                    delta: 0.1,
                },
            );
            inspector_action_button(panel, "Cycle color gradient", EditorAction::ColorPreset);
            inspector_action_button(
                panel,
                if layer.enabled {
                    "Layer visible"
                } else {
                    "Layer hidden"
                },
                EditorAction::ToggleLayer,
            );
        });
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
                    header.spawn((
                        Text::new("TIMELINE"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Node {
                            width: Val::Px(190.0),
                            ..default()
                        },
                    ));
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

fn inspector_section(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(27.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL),
    ));
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

fn spawn_curve_preview(parent: &mut ChildSpawnerCommands, curve: &aestra_bevy::Curve) {
    let maximum = curve
        .keys
        .iter()
        .map(|key| key.value)
        .fold(0.001_f32, f32::max);
    parent
        .spawn((
            Node {
                width: Val::Auto,
                height: Val::Px(26.0),
                margin: UiRect::horizontal(Val::Px(12.0)),
                padding: UiRect::all(Val::Px(2.0)),
                align_items: AlignItems::End,
                column_gap: Val::Px(1.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::TIMELINE_BG),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|preview| {
            for index in 0..24 {
                let value = curve.sample(index as f32 / 23.0) / maximum;
                preview.spawn((
                    Node {
                        flex_grow: 1.0,
                        height: Val::Percent((value * 100.0).clamp(2.0, 100.0)),
                        ..default()
                    },
                    BackgroundColor(theme::ACCENT_DIM),
                ));
            }
        });
}

fn spawn_gradient_preview(parent: &mut ChildSpawnerCommands, gradient: &aestra_bevy::Gradient) {
    parent
        .spawn((
            Node {
                width: Val::Auto,
                height: Val::Px(14.0),
                margin: UiRect::horizontal(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|preview| {
            for index in 0..24 {
                let color = gradient.sample(index as f32 / 23.0);
                preview.spawn((
                    Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(color[0], color[1], color[2], color[3])),
                ));
            }
        });
}

fn keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if keys.just_pressed(KeyCode::Escape) {
        menu.open = None;
        menu.show_about = false;
    }
    if control && keys.just_pressed(KeyCode::KeyN) && confirm_discard(&session) {
        session.new_effect();
    }
    if control && keys.just_pressed(KeyCode::KeyO) {
        open_effect_dialog(&mut session);
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
    if keys.just_pressed(KeyCode::Delete) {
        session.delete_selected_layer();
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
                        }
                    }
                    EditorAction::OpenEffect => open_effect_dialog(&mut session),
                    EditorAction::OpenCatalog(index) => {
                        if confirm_discard(&session) {
                            if let Some(entry) = catalog.entries.get(index)
                                && let Err(error) = session.open(&entry.path)
                            {
                                session.status = format!("Open failed: {error}");
                            }
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
                    EditorAction::DuplicateLayer => session.duplicate_selected_layer(),
                    EditorAction::DeleteLayer => session.delete_selected_layer(),
                    EditorAction::SelectLayer(index) => session.select_layer(index),
                    EditorAction::SpawnRate(delta) => session.adjust_spawn_rate(delta),
                    EditorAction::Burst(delta) => session.adjust_burst_count(delta),
                    EditorAction::Lifetime(delta) => session.adjust_lifetime(delta),
                    EditorAction::LayerStart(delta) => session.adjust_selected_start(delta),
                    EditorAction::LayerDuration(delta) => {
                        session.adjust_selected_duration(delta);
                    }
                    EditorAction::EffectDuration(delta) => {
                        session.adjust_effect_duration(delta);
                    }
                    EditorAction::CurveValue { curve, key, delta } => {
                        let (parameter, range) = match curve {
                            CurveTarget::Size => ("size", 0.0..=f32::MAX),
                            CurveTarget::Opacity => ("opacity", 0.0..=1.0),
                        };
                        session.adjust_curve_key(parameter, key, delta, range);
                    }
                    EditorAction::ColorPreset => {
                        let gradient = cycled_color_gradient(session.selected_layer());
                        session.set_color_gradient(gradient);
                    }
                    EditorAction::ToggleLayer => session.toggle_selected_layer(),
                    EditorAction::ToggleGrid => menu.show_grid = !menu.show_grid,
                    EditorAction::ShowAbout => menu.show_about = true,
                    EditorAction::CloseAbout => menu.show_about = false,
                    EditorAction::ToggleMenu(_) => unreachable!(),
                }
            }
        }
    }
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

fn cycled_color_gradient(layer: &aestra_bevy::Emitter) -> aestra_bevy::Gradient {
    let mut gradient = layer.color_gradient().clone();
    let first = layer
        .color_gradient()
        .keys
        .first()
        .map_or([0.4, 0.4, 1.0, 1.0], |key| key.color);
    let palette = if first[0] > 0.72 {
        [
            [0.25, 1.0, 0.68, 1.0],
            [0.10, 0.62, 0.82, 1.0],
            [0.02, 0.18, 0.28, 0.0],
        ]
    } else if first[1] > 0.85 {
        [
            [0.38, 0.72, 1.0, 1.0],
            [0.68, 0.28, 1.0, 1.0],
            [0.18, 0.05, 0.42, 0.0],
        ]
    } else {
        [
            [1.0, 0.82, 0.28, 1.0],
            [1.0, 0.24, 0.08, 1.0],
            [0.35, 0.02, 0.01, 0.0],
        ]
    };
    let key_count = gradient.keys.len();
    for (index, key) in gradient.keys.iter_mut().enumerate() {
        let palette_index = if key_count <= 1 {
            0
        } else {
            (index * 2 / (key_count - 1)).min(2)
        };
        key.color = palette[palette_index];
    }
    gradient
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
    mut rendered: ResMut<RenderedUiRevision>,
    roots: Query<Entity, With<EditorRoot>>,
) {
    if rendered.0 == session.ui_revision {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    *catalog = EffectCatalog::scan();
    spawn_editor_ui(&mut commands, &session, &menu, &catalog);
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
    evaluate(&session.effect, time, &mut samples);
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
        Option<&InspectorValues>,
        Option<&ParticleCountLabel>,
    )>,
) {
    if !session.is_changed() {
        return;
    }
    let layer = session.selected_layer();
    for (mut text, playback, time, status, title, values, count) in &mut labels {
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
        } else if values.is_some() {
            text.0 = inspector_text(layer);
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

fn inspector_text(layer: &aestra_bevy::Emitter) -> String {
    format!(
        "Spawn rate       {:>7.1} /s\nBurst            {:>7}\nLifetime      {:>4.2}-{:>4.2} s\nMax particles    {:>7}\nTurbulence       {:>7.1}",
        layer.spawn_rate(),
        layer.burst_count(),
        layer.lifetime().min,
        layer.lifetime().max,
        layer.max_particles,
        layer.turbulence(),
    )
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
}
