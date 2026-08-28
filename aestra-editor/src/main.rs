// Bevy ECS systems express disjoint resources and queries in their signatures. Keeping those
// dependencies explicit is clearer than hiding them behind editor-specific parameter bundles.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod assets;
mod changes;
mod compiler_inspector;
mod curves;
mod diagnostics;
mod dock_ui;
mod docking;
mod feathers;
mod history;
mod inspector;
mod localization;
mod menus;
mod persistence;
mod profiler;
mod recovery;
mod session;
mod settings;
mod settings_ui;
mod theme;
mod timeline;
mod transport;
mod viewport;

use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    AestraPlugin, BlendMode, DiagnosticCode, DiagnosticSeverity, EffectAsset, EmitterId,
    EmitterShape, EmitterTransform, EventId, EventTrigger, FlipbookPlaybackMode,
    FlipbookTimeSource, MaterialInput, MaterialProperties, ModuleId, ModuleInstance,
    ModuleParameters, RendererId, RendererProperties, StageKind, Value,
};
use aestra_compiler::ModuleMetadata;
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
        WindowMoved, WindowRef, WindowResizeConstraints, WindowResized, WindowResolution,
    },
};
pub(crate) use changes::spawn_changes_workspace;
use changes::{ChangesSet, EditorChangesPlugin};
pub(crate) use compiler_inspector::spawn_compiler_inspector_workspace;
use compiler_inspector::{CompilerInspectorSet, EditorCompilerInspectorPlugin};
pub(crate) use curves::{CurvesAction, CurvesState, spawn_curves_workspace};
use curves::{CurvesSet, EditorCurvesPlugin};
pub(crate) use diagnostics::{DiagnosticsPanelState, spawn_diagnostics_workspace};
use diagnostics::{DiagnosticsSet, EditorDiagnosticsPlugin, spawn_compile_status};
#[cfg(test)]
use dock_ui::{clear_finished_dock_drag, dock_pane_background};
#[cfg(test)]
use docking::DockDragState;
#[cfg(test)]
use docking::DockTab;
use docking::{
    DockPanel, DockTreeHost, DockingPlugin, DockingSet, NativeFloatingWindow, WorkspaceLayout,
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
    text_input::spawn_text_input,
    tooltip::EditorTooltip,
};
use fluent_bundle::FluentArgs;
pub(crate) use history::HistoryAction;
use history::{EditorHistoryPlugin, HistorySet};
use inspector::*;
use localization::{EditorLocalizationPlugin, LocalizationSet};
pub(crate) use localization::{LocalizedText, Localizer};
pub(crate) use menus::{DocumentMenuLabel, MenuState, TabContextMenu};
use menus::{EditorMenusPlugin, spawn_about_overlay, spawn_menu_bar, spawn_tab_context_menu};
pub(crate) use persistence::persist_editor_settings;
use persistence::{DocumentAction, EditorPersistencePlugin, PersistenceSet};
use profiler::{EditorProfilerPlugin, ProfilerSet};
pub(crate) use profiler::{ProfilerState, spawn_profiler_workspace};
use session::EditorSession;
use settings::{EditorSettings, SettingsPersistence};
use settings_ui::EditorSettingsUiPlugin;
pub(crate) use settings_ui::{SettingsPanelState, spawn_settings_workspace};
use std::collections::HashMap;
use timeline::{TimelinePlugin, TimelineSet, TimelineSnapMode, TimelineState};
pub(crate) use transport::TransportAction;
use transport::{EditorTransportPlugin, TransportSet, spawn_transport_controls};
use viewport::{
    EmitterTransformGizmoInteraction, EmitterTransformGizmoProxy, PreviewCameraController,
    PreviewDisplayMode, PreviewDisplayState, ViewportPlugin, ViewportSet,
    emitter_transform_from_bevy,
};

const EFFECT_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const EFFECT_PATH: &str = "assets/effects/prism_bloom.aestra.ron";
const EDITOR_ASSET_ROOT: &str = "../assets";

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
        .add_plugins(EditorChangesPlugin)
        .add_plugins(EditorCompilerInspectorPlugin)
        .add_plugins(EditorCurvesPlugin)
        .add_plugins(EditorDiagnosticsPlugin)
        .add_plugins(EditorHistoryPlugin)
        .add_plugins(EditorProfilerPlugin)
        .add_plugins(EditorSettingsUiPlugin)
        .add_plugins(EditorPersistencePlugin)
        .add_plugins(AestraPlugin)
        .add_plugins(DockingPlugin)
        .add_plugins(InspectorPlugin)
        .add_plugins(TimelinePlugin)
        .add_plugins(EditorTransportPlugin)
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
                (apply_editor_fonts, keyboard_shortcuts, handle_buttons)
                    .chain()
                    .in_set(EditorSet::PreViewport),
                update_editor_labels.in_set(EditorSet::MainUpdate),
                (remember_scroll_positions, rebuild_editor_ui)
                    .chain()
                    .in_set(EditorSet::UiRebuild),
                restore_scroll_positions.in_set(EditorSet::UiSync),
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
                (TransportSet::Input, HistorySet::Input).chain(),
                TimelineSet::Input,
                InspectorSet::Input,
                DockingSet::Input,
                AestraFeathersSet::Input,
                (
                    AssetsSet::Actions,
                    ChangesSet::Actions,
                    CompilerInspectorSet::Actions,
                    CurvesSet::Actions,
                    DiagnosticsSet::Actions,
                    DockingSet::Actions,
                    HistorySet::Actions,
                    InspectorSet::Actions,
                    ProfilerSet::Actions,
                    PersistenceSet::Actions,
                    TransportSet::Actions,
                )
                    .chain(),
                EditorSet::PreViewport,
                TransportSet::Playback,
                PersistenceSet::Lifecycle,
                ViewportSet::Update,
                LocalizationSet::Sync,
                EditorSet::MainUpdate,
                DockingSet::Reconcile,
                EditorSet::UiRebuild,
                TimelineSet::Visuals,
                (
                    AssetsSet::Sync,
                    DiagnosticsSet::Sync,
                    HistorySet::Sync,
                    ProfilerSet::Sync,
                    InspectorSet::Sync,
                )
                    .chain(),
                DockingSet::Sync,
                AestraFeathersSet::Sync,
                TransportSet::Sync,
                EditorSet::UiSync,
            )
                .chain(),
        )
        .run();
}

#[derive(Component, Clone, Copy)]
enum EditorAction {
    AddLayer,
    DuplicateLayer,
    DeleteLayer,
    EffectDuration(f32),
    SetTimelineSnap(TimelineSnapMode),
    FrameTimeline,
    ToggleGrid,
    FramePreview,
    SetTransformGizmoMode(TransformGizmoMode),
    SetPreviewDisplayMode(PreviewDisplayMode),
    ShowAbout,
    CloseAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScrollMemoryKey {
    Inspector,
    CompilerInspector,
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
            spawn_transport_controls(bar, session, localizer);
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

fn remember_scroll_positions(
    mut memory: ResMut<ScrollMemoryState>,
    scroll_areas: Query<(Ref<PersistedScroll>, &ScrollPosition)>,
) {
    for (marker, position) in &scroll_areas {
        // Dock content is populated after the editor rebuild set. Never let a replacement
        // scroll area's provisional zero overwrite the offset captured from the old panel.
        if marker.is_added() {
            continue;
        }
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

fn spawn_status_bar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn(feathers::status_bar::status_bar())
        .with_children(|bar| spawn_compile_status(bar, session, localizer));
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
    localizer: Res<Localizer>,
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
    if keys.just_pressed(KeyCode::KeyG) && !control {
        menu.show_grid = !menu.show_grid;
        settings.preview.show_grid = menu.show_grid;
        session.ui_revision += 1;
        persist_editor_settings(
            &settings,
            &mut settings_persistence,
            &mut session,
            &localizer,
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
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
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
        ResMut<CurvesState>,
        ResMut<WorkspaceLayout>,
        ResMut<EditorSettings>,
        ResMut<SettingsPersistence>,
        ResMut<PreviewCameraController>,
        ResMut<PreviewDisplayState>,
    ),
    mut timeline_state: ResMut<TimelineState>,
    mut transform_gizmo_settings: ResMut<TransformGizmoSettings>,
    localizer: Res<Localizer>,
) {
    let (
        mut workspace,
        mut layout,
        mut settings,
        mut settings_persistence,
        mut preview_camera,
        mut preview_display,
    ) = editor_resources;
    for (
        entity,
        interaction,
        action,
        feathers_action,
        pending_feathers_activation,
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
                    background.0 = theme::BUTTON;
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
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                match *action {
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
                    EditorAction::ToggleGrid => {
                        menu.show_grid = !menu.show_grid;
                        settings.preview.show_grid = menu.show_grid;
                        persist_editor_settings(
                            &settings,
                            &mut settings_persistence,
                            &mut session,
                            &localizer,
                        );
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
                    EditorAction::ShowAbout => menu.show_about = true,
                    EditorAction::CloseAbout => menu.show_about = false,
                }
            }
        }
    }
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
    fn new_scroll_area_keeps_memory_until_same_frame_restore() {
        let saved = Vec2::new(0.0, 184.0);
        let mut memory = ScrollMemoryState::default();
        memory.0.insert(ScrollMemoryKey::Inspector, saved);
        let mut app = App::new();
        app.insert_resource(memory);
        app.add_systems(
            Update,
            (remember_scroll_positions, restore_scroll_positions).chain(),
        );
        let rebuilt = app
            .world_mut()
            .spawn((
                PersistedScroll(ScrollMemoryKey::Inspector),
                ScrollPosition::default(),
            ))
            .id();

        app.update();

        assert_eq!(app.world().get::<ScrollPosition>(rebuilt).unwrap().0, saved);
        assert_eq!(
            app.world()
                .resource::<ScrollMemoryState>()
                .0
                .get(&ScrollMemoryKey::Inspector),
            Some(&saved)
        );
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
                EditorAction::ShowAbout,
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
