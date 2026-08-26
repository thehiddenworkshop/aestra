use aestra_bevy::{EffectAsset, ParticleSample, evaluate};
use bevy::{prelude::*, window::WindowResolution};
use std::path::PathBuf;

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const PARTICLE_POOL_SIZE: usize = 384;
const PREVIEW_WIDTH: f32 = 680.0;
const PREVIEW_HEIGHT: f32 = 430.0;

fn main() {
    App::new()
        .insert_resource(ClearColor(theme::APP_BG))
        .insert_resource(EditorSession::from_embedded_sample())
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
                advance_playback,
                update_preview,
                update_editor_labels,
                update_playhead,
                update_layer_selection,
            )
                .chain(),
        )
        .run();
}

#[derive(Resource)]
struct EditorSession {
    effect: EffectAsset,
    source_path: PathBuf,
    selected_layer: usize,
    time: f32,
    playing: bool,
    speed: f32,
    dirty: bool,
    status: String,
    samples: Vec<ParticleSample>,
}

impl EditorSession {
    fn from_embedded_sample() -> Self {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE)
            .expect("the bundled Prism Bloom sample must always be valid");
        Self {
            effect,
            source_path: EFFECT_PATH.into(),
            selected_layer: 0,
            time: 0.0,
            playing: true,
            speed: 1.0,
            dirty: false,
            status: "Previewing embedded Prism Bloom".into(),
            samples: Vec::with_capacity(PARTICLE_POOL_SIZE),
        }
    }

    fn restart(&mut self) {
        self.time = 0.0;
        self.playing = true;
        self.status = "Choreography restarted".into();
    }

    fn save(&mut self) {
        match self.effect.save_ron(&self.source_path) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Saved {}", self.source_path.display());
            }
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }
}

#[derive(Component, Clone, Copy)]
enum EditorAction {
    TogglePlayback,
    Restart,
    Save,
    SelectLayer(usize),
    SpawnRate(f32),
    Burst(i32),
    Lifetime(f32),
    ToggleLayer,
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

fn setup_editor(mut commands: Commands, session: Res<EditorSession>) {
    commands.spawn(Camera2d);

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::APP_BG),
        ))
        .with_children(|root| {
            spawn_toolbar(root);
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
                spawn_asset_browser(main, &session);
                spawn_preview(main);
                spawn_inspector(main, &session);
            });
            spawn_timeline(root, &session);
            spawn_status_bar(root);
        });
}

fn spawn_toolbar(parent: &mut ChildSpawnerCommands) {
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
                Text::new("PRISM BLOOM  /  VFX CHOREOGRAPHY"),
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

fn spawn_asset_browser(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
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
            panel_heading(panel, "EFFECT LIBRARY", "1 ASSET");
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
                        Text::new("example / energy"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                });

            panel_heading(
                panel,
                "LAYERS",
                &format!("{} ACTIVE", session.effect.layers.len()),
            );
            for (index, layer) in session.effect.layers.iter().enumerate() {
                let selected = index == session.selected_layer;
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
    let layer = &session.effect.layers[session.selected_layer];
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
                    margin: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
                    ..default()
                },
            ));
            inspector_section(panel, "EMISSION");
            property_stepper(
                panel,
                "Spawn rate",
                EditorAction::SpawnRate(-5.0),
                EditorAction::SpawnRate(5.0),
            );
            property_stepper(
                panel,
                "Burst",
                EditorAction::Burst(-4),
                EditorAction::Burst(4),
            );
            property_stepper(
                panel,
                "Lifetime",
                EditorAction::Lifetime(-0.1),
                EditorAction::Lifetime(0.1),
            );
            panel.spawn((
                Text::new(inspector_text(layer)),
                InspectorValues,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    margin: UiRect::all(Val::Px(14.0)),
                    ..default()
                },
            ));
            inspector_section(panel, "RENDERER");
            info_row(panel, "Blend", &format!("{:?}", layer.blend));
            info_row(panel, "Facing", "Camera billboard");
            inspector_section(panel, "LAYER");
            panel
                .spawn((
                    Button,
                    EditorAction::ToggleLayer,
                    Node {
                        height: Val::Px(32.0),
                        margin: UiRect::all(Val::Px(12.0)),
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
                        Text::new("Toggle visibility"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                });
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
                        for (index, layer) in session.effect.layers.iter().enumerate() {
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
                        spawn_ruler(tracks);
                        for (index, layer) in session.effect.layers.iter().enumerate() {
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

fn spawn_ruler(parent: &mut ChildSpawnerCommands) {
    for index in 0..=7 {
        parent.spawn((
            Text::new(format!("{:.1}", index as f32 * 0.4)),
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

fn info_row(parent: &mut ChildSpawnerCommands, label: &str, value: &str) {
    parent
        .spawn(Node {
            height: Val::Px(30.0),
            width: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
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
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
        });
}

fn keyboard_shortcuts(keys: Res<ButtonInput<KeyCode>>, mut session: ResMut<EditorSession>) {
    if keys.just_pressed(KeyCode::Space) {
        session.playing = !session.playing;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        session.restart();
    }
    if keys.just_pressed(KeyCode::KeyS)
        && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
    {
        session.save();
    }
}

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
) {
    for (interaction, action, layer_row, mut background) in &mut buttons {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = layer_row.map_or(theme::BUTTON, |row| {
                    if row.0 == session.selected_layer {
                        theme::SELECTION
                    } else {
                        theme::PANEL_DARK
                    }
                });
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
                match *action {
                    EditorAction::TogglePlayback => session.playing = !session.playing,
                    EditorAction::Restart => session.restart(),
                    EditorAction::Save => session.save(),
                    EditorAction::SelectLayer(index) => session.selected_layer = index,
                    EditorAction::SpawnRate(delta) => {
                        let selected = session.selected_layer;
                        let layer = &mut session.effect.layers[selected];
                        layer.emitter.spawn_rate = (layer.emitter.spawn_rate + delta).max(0.0);
                        session.dirty = true;
                    }
                    EditorAction::Burst(delta) => {
                        let selected = session.selected_layer;
                        let layer = &mut session.effect.layers[selected];
                        layer.emitter.burst_count = if delta.is_negative() {
                            layer
                                .emitter
                                .burst_count
                                .saturating_sub(delta.unsigned_abs())
                        } else {
                            layer.emitter.burst_count.saturating_add(delta as u32)
                        };
                        session.dirty = true;
                    }
                    EditorAction::Lifetime(delta) => {
                        let selected = session.selected_layer;
                        let layer = &mut session.effect.layers[selected];
                        layer.emitter.lifetime.min = (layer.emitter.lifetime.min + delta).max(0.05);
                        layer.emitter.lifetime.max =
                            (layer.emitter.lifetime.max + delta).max(layer.emitter.lifetime.min);
                        session.dirty = true;
                    }
                    EditorAction::ToggleLayer => {
                        let selected = session.selected_layer;
                        let layer = &mut session.effect.layers[selected];
                        layer.enabled = !layer.enabled;
                        session.dirty = true;
                    }
                }
            }
        }
    }
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
    let layer = &session.effect.layers[session.selected_layer];
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
        color.0 = if row.0 == session.selected_layer {
            theme::SELECTION
        } else {
            theme::PANEL_DARK
        };
    }
}

fn inspector_text(layer: &aestra_bevy::EffectLayer) -> String {
    format!(
        "Spawn rate       {:>7.1} /s\nBurst            {:>7}\nLifetime      {:>4.2}-{:>4.2} s\nMax particles    {:>7}\nTurbulence       {:>7.1}",
        layer.emitter.spawn_rate,
        layer.emitter.burst_count,
        layer.emitter.lifetime.min,
        layer.emitter.lifetime.max,
        layer.emitter.max_particles,
        layer.emitter.turbulence,
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

mod theme {
    use bevy::prelude::Color;

    pub const APP_BG: Color = Color::srgb(0.027, 0.031, 0.047);
    pub const PANEL_DARK: Color = Color::srgb(0.039, 0.045, 0.066);
    pub const PANEL: Color = Color::srgb(0.055, 0.062, 0.087);
    pub const PANEL_LIGHT: Color = Color::srgb(0.070, 0.078, 0.105);
    pub const VIEWPORT_FRAME: Color = Color::srgb(0.020, 0.024, 0.038);
    pub const VIEWPORT: Color = Color::srgb(0.013, 0.017, 0.030);
    pub const TIMELINE_BG: Color = Color::srgb(0.030, 0.035, 0.052);
    pub const BORDER: Color = Color::srgb(0.105, 0.116, 0.151);
    pub const BORDER_BRIGHT: Color = Color::srgb(0.148, 0.164, 0.211);
    pub const GRID: Color = Color::srgba(0.20, 0.23, 0.31, 0.18);
    pub const BUTTON: Color = Color::srgb(0.085, 0.095, 0.128);
    pub const BUTTON_HOVER: Color = Color::srgb(0.135, 0.143, 0.190);
    pub const SELECTION: Color = Color::srgb(0.100, 0.089, 0.173);
    pub const ACCENT: Color = Color::srgb(0.61, 0.47, 1.0);
    pub const ACCENT_DIM: Color = Color::srgb(0.31, 0.23, 0.53);
    pub const PLAYHEAD: Color = Color::srgb(0.95, 0.44, 0.78);
    pub const TEXT: Color = Color::srgb(0.88, 0.90, 0.96);
    pub const TEXT_MUTED: Color = Color::srgb(0.59, 0.62, 0.70);
    pub const TEXT_FAINT: Color = Color::srgb(0.36, 0.39, 0.47);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_effect_is_valid() {
        let effect = EffectAsset::from_ron(EFFECT_SOURCE).expect("bundled effect should parse");
        assert_eq!(effect.id, "aestra.example.prism_bloom");
        assert_eq!(effect.layers.len(), 4);
    }
}
