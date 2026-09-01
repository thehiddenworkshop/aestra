//! Root editor chrome, global shortcuts, and UI rebuild lifecycle.

use crate::feathers::{
    breadcrumb::{BreadcrumbItem, BreadcrumbProps, spawn_breadcrumb},
    icon::load_svg_icon,
};
use crate::timeline::{EffectClipChildSelection, TimelineState, resolve_effect_clip_path};
use crate::*;
use bevy_resvg::prelude::{SvgColor, UiSvg};
use std::collections::HashMap;

pub(crate) struct EditorShellPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EditorSet {
    Setup,
    PreViewport,
    MainUpdate,
    UiRebuild,
    UiSync,
}

impl Plugin for EditorShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScrollMemoryState>()
            .init_resource::<RenderedUiRevision>()
            .init_resource::<RenderedGlobalSourceNavigation>()
            .add_systems(First, activate_staged_editor_ui)
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
                    (update_editor_labels, sync_global_source_navigation)
                        .in_set(EditorSet::MainUpdate),
                    (remember_scroll_positions, stage_editor_ui_rebuild)
                        .chain()
                        .in_set(EditorSet::UiRebuild),
                    restore_scroll_positions.in_set(EditorSet::UiSync),
                ),
            );
    }
}

#[derive(Component, Clone, Copy)]
pub(crate) enum EditorAction {
    ShowAbout,
    CloseAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ScrollMemoryKey {
    Library,
    LibraryRelations,
    LibraryDeletion,
    Properties,
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

/// A replacement workspace that is built off-screen for one frame before it becomes active.
///
/// Editor panels contain text whose final font is applied by [`apply_editor_fonts`] and SVGs that
/// are rasterized by `bevy_resvg` after they are spawned. Replacing the visible workspace in the
/// same frame therefore exposes those transient, incomplete controls and makes unrelated labels
/// and icons blink. Keeping the previous workspace visible while this tree initializes makes a
/// rebuild an atomic visual swap.
#[derive(Component)]
struct StagedEditorContent(u64);

#[derive(Resource)]
struct EditorFonts {
    mono: Handle<Font>,
}

#[derive(Component)]
struct GlobalSourceNavigation;

#[derive(Component)]
struct GlobalSourceNavigationItem;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GlobalSourceNavigationView {
    ancestors: Vec<String>,
    current_id: EffectAssetRef,
    current_name: String,
    current_emitter: Option<(EmitterId, String)>,
    dirty: bool,
    can_go_forward: bool,
}

#[derive(Resource, Default)]
struct RenderedGlobalSourceNavigation(Option<GlobalSourceNavigationView>);

fn setup_editor(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    layout: Res<WorkspaceLayout>,
    localizer: Res<Localizer>,
    protection: Res<DocumentProtectionState>,
    library_asset_operation: Res<LibraryAssetOperationState>,
    navigation: Res<SourceNavigationState>,
    timeline: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    mut rendered: ResMut<RenderedUiRevision>,
    mut rendered_navigation: ResMut<RenderedGlobalSourceNavigation>,
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
    spawn_editor_ui(
        &mut commands,
        &menu,
        &layout,
        &session,
        &localizer,
        &asset_server,
        &protection,
        &library_asset_operation,
        &navigation,
        &timeline,
        &catalog,
    );
    rendered.0 = session.ui_revision;
    rendered_navigation.0 = Some(global_source_navigation_view(
        &session,
        &navigation,
        &timeline,
        &catalog,
    ));
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
    asset_server: &AssetServer,
    protection: &DocumentProtectionState,
    library_asset_operation: &LibraryAssetOperationState,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
) {
    commands
        .spawn(EditorRoot)
        .apply_scene(ui_shell::editor_root())
        .with_children(|root| {
            spawn_menu_bar(root, session, menu, layout, localizer);
            spawn_toolbar(
                root,
                session,
                navigation,
                timeline,
                catalog,
                localizer,
                asset_server,
            );
            spawn_editor_content(root, menu, localizer, None);
            spawn_status_bar(root, session, localizer);
            spawn_about_overlay(root, menu.show_about, localizer);
            spawn_document_protection_overlay(root, protection, localizer);
            spawn_library_asset_operation_overlay(
                root,
                library_asset_operation,
                catalog,
                localizer,
            );
        });
}

fn spawn_editor_content(
    parent: &mut ChildSpawnerCommands,
    menu: &MenuState,
    localizer: &Localizer,
    staged_revision: Option<u64>,
) {
    let mut content = parent.spawn((EditorContent, RelativeCursorPosition::default()));
    if let Some(revision) = staged_revision {
        content.insert((StagedEditorContent(revision), Visibility::Hidden));
    }
    content
        .apply_scene(ui_shell::editor_content())
        .with_children(|content| {
            content.spawn(DockTreeHost);
            spawn_tab_context_menu(content, menu.tab_context, localizer);
        });
}

fn spawn_toolbar(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    asset_server: &AssetServer,
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
            spawn_transport_controls(bar, session, localizer, asset_server);
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
                GlobalSourceNavigation,
                EditorTooltip::description(source_navigation_path(
                    session, navigation, timeline, catalog,
                )),
                Node {
                    min_width: Val::Px(0.0),
                    max_width: Val::Percent(55.0),
                    height: Val::Px(28.0),
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|navigation_root| {
                spawn_global_source_navigation_items(
                    navigation_root,
                    session,
                    navigation,
                    timeline,
                    catalog,
                    localizer,
                    asset_server,
                );
            });
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

fn global_source_navigation_view(
    session: &EditorSession,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
) -> GlobalSourceNavigationView {
    let mut breadcrumb = navigation.breadcrumb(&session.effect.name);
    let current_name = breadcrumb
        .pop()
        .unwrap_or_else(|| session.effect.name.clone());
    GlobalSourceNavigationView {
        ancestors: breadcrumb,
        current_id: EffectAssetRef::new(session.effect.id),
        current_name,
        current_emitter: inspected_emitter(session, timeline, catalog),
        dirty: session.dirty,
        can_go_forward: navigation.can_go_forward(),
    }
}

fn source_navigation_path(
    session: &EditorSession,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
) -> String {
    source_navigation_breadcrumb(session, navigation, timeline, catalog).join(" › ")
}

fn inspected_emitter(
    session: &EditorSession,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
) -> Option<(EmitterId, String)> {
    match timeline.inspected_child.as_ref() {
        Some(EffectClipChildSelection::Emitter { path, emitter }) => {
            let (_, source) = resolve_effect_clip_path(session, catalog, path)?;
            source
                .emitters
                .iter()
                .find(|candidate| candidate.id == *emitter)
                .map(|candidate| (candidate.id, candidate.name.clone()))
        }
        Some(EffectClipChildSelection::EffectClip { .. }) => None,
        None => {
            let emitter = session.selection.emitter(&session.effect)?;
            session
                .effect
                .emitters
                .iter()
                .find(|candidate| candidate.id == emitter)
                .map(|candidate| (candidate.id, candidate.name.clone()))
        }
    }
}

fn source_navigation_breadcrumb(
    session: &EditorSession,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
) -> Vec<String> {
    let mut breadcrumb = navigation.breadcrumb(&session.effect.name);
    if let Some((_, name)) = inspected_emitter(session, timeline, catalog) {
        breadcrumb.push(name);
    }
    breadcrumb
}

fn spawn_global_source_navigation_items(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    navigation: &SourceNavigationState,
    timeline: &TimelineState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    spawn_global_navigation_button(
        parent,
        -1,
        DocumentAction::BackToSource,
        &localizer.text("toolbar-source-back"),
        !navigation.can_go_back(),
        asset_server,
    );
    spawn_global_navigation_button(
        parent,
        1,
        DocumentAction::ForwardToSource,
        &localizer.text("toolbar-source-forward"),
        !navigation.can_go_forward(),
        asset_server,
    );

    let mut breadcrumb = source_navigation_breadcrumb(session, navigation, timeline, catalog);
    let depth = navigation.depth();
    let full_path = breadcrumb.join(" › ");
    if session.dirty {
        breadcrumb[depth] = format!("* {}", breadcrumb[depth]);
    }
    let items = breadcrumb
        .into_iter()
        .enumerate()
        .map(|(index, label)| BreadcrumbItem {
            label,
            action: (index < depth).then_some(DocumentAction::NavigateSourceAncestor(index)),
        })
        .collect::<Vec<_>>();
    let breadcrumb = spawn_breadcrumb(
        parent,
        &items,
        BreadcrumbProps {
            height: 28.0,
            font: fonts::MONO,
            font_size: 11.0,
            text_offset_y: 2.0,
            uppercase: true,
            flex_grow: 0.0,
            max_ancestor_width: 132.0,
            max_current_width: 164.0,
            ancestor_color: theme::TEXT_MUTED,
            current_color: theme::ACCENT,
            compact_ancestors: false,
            overflow_label: &localizer.text("toolbar-source-hidden-ancestors"),
            current_tooltip: Some(&full_path),
            ancestor_tooltips: true,
        },
        asset_server,
    );
    parent
        .commands()
        .entity(breadcrumb)
        .insert(GlobalSourceNavigationItem);
}

fn spawn_global_navigation_button(
    parent: &mut ChildSpawnerCommands,
    direction: i8,
    action: DocumentAction,
    label: &str,
    disabled: bool,
    asset_server: &AssetServer,
) {
    let mut button = parent.spawn_empty();
    button
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            GlobalSourceNavigationItem,
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                margin: UiRect::right(Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ));
    if disabled {
        button.insert(InteractionDisabled);
    }
    button.with_child((
        Node {
            width: Val::Px(16.0),
            height: Val::Px(16.0),
            ..default()
        },
        UiSvg(load_svg_icon(asset_server, "icons/chevron-right.svg")),
        SvgColor(if disabled {
            theme::TEXT_FAINT
        } else {
            theme::TEXT
        }),
        UiTransform::from_rotation(if direction < 0 {
            Rot2::radians(std::f32::consts::PI)
        } else {
            Rot2::IDENTITY
        }),
        Pickable::IGNORE,
    ));
}

fn sync_global_source_navigation(
    mut commands: Commands,
    session: Res<EditorSession>,
    navigation: Res<SourceNavigationState>,
    timeline: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    asset_server: Res<AssetServer>,
    root: Single<Entity, With<GlobalSourceNavigation>>,
    items: Query<Entity, With<GlobalSourceNavigationItem>>,
    mut rendered: ResMut<RenderedGlobalSourceNavigation>,
) {
    let view = global_source_navigation_view(&session, &navigation, &timeline, &catalog);
    if rendered.0.as_ref() == Some(&view) && !localizer.is_changed() {
        return;
    }
    for item in &items {
        commands.entity(item).despawn();
    }
    commands
        .entity(*root)
        .insert(EditorTooltip::description(source_navigation_path(
            &session,
            &navigation,
            &timeline,
            &catalog,
        )));
    commands.entity(*root).with_children(|parent| {
        spawn_global_source_navigation_items(
            parent,
            &session,
            &navigation,
            &timeline,
            &catalog,
            &localizer,
            &asset_server,
        );
    });
    rendered.0 = Some(view);
}

pub(crate) fn format_value(value: Value) -> String {
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
        Value::Vec3Range(value) => format!(
            "[{:.1}, {:.1}, {:.1}] – [{:.1}, {:.1}, {:.1}]",
            value.min[0], value.min[1], value.min[2], value.max[0], value.max[1], value.max[2]
        ),
        Value::Curve(value) => format!("Curve · {} keys", value.keys.len()),
        Value::Vec3Curve(value) => format!(
            "XYZ curves · {}/{}/{} keys",
            value.curves[0].keys.len(),
            value.curves[1].keys.len(),
            value.curves[2].keys.len()
        ),
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

pub(crate) fn properties_action_button<A: Component>(
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

pub(crate) fn localized_action_button(
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
    navigation: Res<SourceNavigationState>,
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

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
    if alt && keys.just_pressed(KeyCode::ArrowLeft) && navigation.can_go_back() {
        commands.trigger(DocumentAction::BackToSource);
    }
    if alt && keys.just_pressed(KeyCode::ArrowRight) && navigation.can_go_forward() {
        commands.trigger(DocumentAction::ForwardToSource);
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn handle_buttons(
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
) {
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

fn activate_staged_editor_ui(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut rendered: ResMut<RenderedUiRevision>,
    contents: Query<(Entity, Option<&StagedEditorContent>), With<EditorContent>>,
) {
    let Some(staged) = contents.iter().find_map(|(entity, staged)| {
        (staged.is_some_and(|staged| staged.0 == session.ui_revision)).then_some(entity)
    }) else {
        return;
    };
    for (content, _) in &contents {
        if content != staged {
            commands.entity(content).despawn();
        }
    }
    commands
        .entity(staged)
        .remove::<StagedEditorContent>()
        .insert(Visibility::Inherited);
    rendered.0 = session.ui_revision;
}

fn stage_editor_ui_rebuild(
    mut commands: Commands,
    session: Res<EditorSession>,
    menu: Res<MenuState>,
    localizer: Res<Localizer>,
    rendered: Res<RenderedUiRevision>,
    root: Single<Entity, With<EditorRoot>>,
    contents: Query<(Entity, Option<&StagedEditorContent>), With<EditorContent>>,
) {
    if rendered.0 == session.ui_revision
        || contents
            .iter()
            .any(|(_, staged)| staged.is_some_and(|staged| staged.0 == session.ui_revision))
    {
        return;
    }

    // A newer edit may arrive while a replacement is initializing. Discard only the obsolete
    // hidden trees; the last fully-rendered workspace stays visible until the newest one is ready.
    for (content, staged) in &contents {
        if staged.is_some() {
            commands.entity(content).despawn();
        }
    }
    commands.entity(*root).with_children(|root| {
        spawn_editor_content(root, &menu, &localizer, Some(session.ui_revision));
    });
}

#[allow(clippy::type_complexity)]
fn update_editor_labels(
    session: Res<EditorSession>,
    mut labels: Query<(
        &mut Text,
        Option<&PropertiesTitle>,
        Option<&DocumentMenuLabel>,
    )>,
) {
    if !session.is_changed() {
        return;
    }
    let layer = session.selected_layer();
    for (mut text, title, document_menu) in &mut labels {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin};

    #[test]
    fn source_breadcrumb_ends_with_the_inspected_emitter() {
        let temporary = tempfile::tempdir().unwrap();
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let expected = session.selected_layer().name.clone();
        let timeline = TimelineState::framed(session.playback_duration());
        let navigation = SourceNavigationState::default();
        let catalog = ProjectEffectCatalog::scan(temporary.path());

        assert_eq!(
            source_navigation_breadcrumb(&session, &navigation, &timeline, &catalog),
            [session.effect.name.clone(), expected]
        );
    }

    #[test]
    fn new_scroll_area_keeps_memory_until_same_frame_restore() {
        let saved = Vec2::new(0.0, 184.0);
        let mut memory = ScrollMemoryState::default();
        memory.0.insert(ScrollMemoryKey::Properties, saved);
        let mut app = App::new();
        app.insert_resource(memory);
        app.add_systems(
            Update,
            (remember_scroll_positions, restore_scroll_positions).chain(),
        );
        let rebuilt = app
            .world_mut()
            .spawn((
                PersistedScroll(ScrollMemoryKey::Properties),
                ScrollPosition::default(),
            ))
            .id();

        app.update();

        assert_eq!(app.world().get::<ScrollPosition>(rebuilt).unwrap().0, saved);
        assert_eq!(
            app.world()
                .resource::<ScrollMemoryState>()
                .0
                .get(&ScrollMemoryKey::Properties),
            Some(&saved)
        );
    }

    #[test]
    fn editor_rebuild_keeps_rendered_content_until_hidden_replacement_is_ready() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.ui_revision = 1;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), ScenePlugin));
        app.insert_resource(session);
        app.insert_resource(MenuState::default());
        app.insert_resource(Localizer::new("en-US").unwrap());
        app.insert_resource(RenderedUiRevision::default());
        app.add_systems(First, activate_staged_editor_ui);
        app.add_systems(Update, stage_editor_ui_rebuild);
        app.world_mut().spawn(EditorRoot);
        let rendered_content = app.world_mut().spawn(EditorContent).id();

        app.update();

        assert!(app.world().get_entity(rendered_content).is_ok());
        let staged = app
            .world_mut()
            .query_filtered::<(Entity, &StagedEditorContent, &Visibility), With<EditorContent>>()
            .single(app.world())
            .unwrap();
        assert_eq!(staged.1.0, 1);
        assert_eq!(*staged.2, Visibility::Hidden);
        assert_eq!(app.world().resource::<RenderedUiRevision>().0, 0);

        app.update();

        assert!(app.world().get_entity(rendered_content).is_err());
        let contents = app
            .world_mut()
            .query_filtered::<(Option<&StagedEditorContent>, &Visibility), With<EditorContent>>()
            .single(app.world())
            .unwrap();
        assert!(contents.0.is_none());
        assert_eq!(*contents.1, Visibility::Inherited);
        assert_eq!(app.world().resource::<RenderedUiRevision>().0, 1);
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
