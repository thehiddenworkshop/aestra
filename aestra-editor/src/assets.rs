//! Assets workspace, project-effect catalog, and panel-local authoring actions.

use crate::*;
use bevy::ui_widgets::Activate;
use std::{fs, path::PathBuf};

pub(crate) struct EditorAssetsPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AssetsSet {
    Input,
    Actions,
    Sync,
}

impl Plugin for EditorAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(EffectCatalog::scan())
            .add_observer(queue_asset_action_activation)
            .add_observer(execute_assets_action)
            .add_systems(
                Update,
                (
                    assets_keyboard_input.in_set(AssetsSet::Input),
                    handle_asset_action_buttons.in_set(AssetsSet::Actions),
                ),
            )
            .add_systems(Update, update_layer_selection.in_set(AssetsSet::Sync));
    }
}

struct CatalogEntry {
    name: String,
    path: PathBuf,
}

#[derive(Resource, Default)]
pub(crate) struct EffectCatalog {
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
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Self { entries }
    }

    pub(crate) fn path(&self, index: usize) -> Option<&std::path::Path> {
        self.entries.get(index).map(|entry| entry.path.as_path())
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetsAction {
    AddSpriteMaterial,
    AddGridFlipbook,
    AddEmitter,
    DuplicateEmitter,
    DeleteEmitter,
    SelectLayer(usize),
}

#[derive(Component)]
struct LayerRow(usize);

#[derive(Component)]
struct AssetButtonLabel;

fn queue_asset_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<AssetsAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

pub(crate) fn spawn_asset_browser(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &EffectCatalog,
    localizer: &Localizer,
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
                &localizer.text("assets-current-effect"),
                &localizer.text(if session.dirty {
                    "assets-modified"
                } else {
                    "assets-saved"
                }),
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

            let mut args = FluentArgs::new();
            args.set("count", catalog.entries.len());
            panel_heading(
                panel,
                &localizer.text("assets-project-effects"),
                &localizer.text_with("assets-found", &args),
            );
            for (index, entry) in catalog.entries.iter().enumerate() {
                panel
                    .spawn((
                        Button,
                        EditorNativeControl,
                        DocumentAction::OpenCatalog(index),
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

            args.set("count", session.effect.assets.len());
            panel_heading(
                panel,
                &localizer.text("assets-render-assets"),
                &localizer.text_with("assets-registered", &args),
            );
            if session.effect.assets.is_empty() {
                panel.spawn((
                    Text::new(localizer.text("assets-no-render-assets")),
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

            args.set("count", session.effect.materials.len());
            panel_heading(
                panel,
                &localizer.text("assets-materials"),
                &localizer.text_with("assets-registered", &args),
            );
            asset_toolbar_button(
                panel,
                &localizer.text("assets-add-sprite-material"),
                AssetsAction::AddSpriteMaterial,
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
                            Text::new(format!(
                                "{}  ·  {:?}",
                                localizer.text("assets-sprite"),
                                material.blend
                            )),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            args.set("count", session.effect.flipbooks.len());
            panel_heading(
                panel,
                &localizer.text("assets-flipbooks"),
                &localizer.text_with("assets-registered", &args),
            );
            asset_toolbar_button(
                panel,
                &localizer.text("assets-add-grid-flipbook"),
                AssetsAction::AddGridFlipbook,
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
                        let mut args = FluentArgs::new();
                        args.set("frames", flipbook.frames.len());
                        args.set("fps", flipbook.frame_rate as f64);
                        row.spawn((
                            Text::new(localizer.text_with("assets-flipbook-summary", &args)),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            args.set("count", session.effect.emitters.len());
            panel_heading(
                panel,
                &localizer.text("assets-layers"),
                &localizer.text_with("assets-active", &args),
            );
            asset_toolbar_button(
                panel,
                &localizer.text("assets-add-emitter"),
                AssetsAction::AddEmitter,
            );
            for (index, layer) in session.effect.emitters.iter().enumerate() {
                let selected = index == session.selected_layer_index();
                panel
                    .spawn((
                        Button,
                        EditorNativeControl,
                        AssetsAction::SelectLayer(index),
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

fn asset_toolbar_button<A: Component>(parent: &mut ChildSpawnerCommands, label: &str, action: A) {
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
                AssetButtonLabel,
                Pickable::IGNORE,
            ));
        });
}

#[allow(clippy::type_complexity)]
fn handle_asset_action_buttons(
    mut commands: Commands,
    mut interactions: Query<
        (
            Entity,
            &Interaction,
            &AssetsAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut interactions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL_DARK,
            Interaction::Pressed => {
                if feathers.is_some() {
                    if pending.is_none() {
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
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn execute_assets_action(
    action: On<AssetsAction>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
) {
    match *action {
        AssetsAction::AddSpriteMaterial => session.add_sprite_material(),
        AssetsAction::AddGridFlipbook => session.add_grid_flipbook(),
        AssetsAction::AddEmitter => {
            session.add_layer();
            curves.clear();
        }
        AssetsAction::DuplicateEmitter => {
            session.duplicate_selected_layer();
            curves.clear();
        }
        AssetsAction::DeleteEmitter => {
            if preview_selected_emitter_deletion(&mut session, &localizer) {
                reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                curves.clear();
            }
        }
        AssetsAction::SelectLayer(index) => {
            session.select_layer(index);
            curves.clear();
        }
    }
}

fn assets_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::Enter) {
        commands.trigger(AssetsAction::AddEmitter);
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        commands.trigger(AssetsAction::DuplicateEmitter);
    }
    if keys.just_pressed(KeyCode::Delete) {
        commands.trigger(AssetsAction::DeleteEmitter);
    }
}

fn preview_selected_emitter_deletion(session: &mut EditorSession, localizer: &Localizer) -> bool {
    if session.effect.emitters.len() <= 1 {
        session.status = localizer.text("assets-status-minimum-emitter");
        return false;
    }
    let id = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        localizer.text("assets-change-delete-emitter"),
        EffectCommand::RemoveEmitter { id },
    ))
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

pub(crate) fn layer_color(index: usize) -> Color {
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

    fn app_with_session(session: EditorSession) -> App {
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").expect("test locale should load"))
            .add_plugins(EditorAssetsPlugin);
        app
    }

    #[test]
    fn assets_plugin_owns_catalog_and_panel_actions() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_materials = session.effect.materials.len();
        let mut app = app_with_session(session);
        let control = app
            .world_mut()
            .spawn((
                Button,
                FeathersActionButton,
                Interaction::None,
                AssetsAction::AddSpriteMaterial,
                BackgroundColor::default(),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: control });
        app.update();

        assert!(app.world().contains_resource::<EffectCatalog>());
        assert!(
            !app.world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .materials
                .len(),
            initial_materials + 1
        );
    }

    #[test]
    fn selecting_a_layer_clears_the_curve_workspace_selection() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.select_layer(0);
        let mut app = app_with_session(session);
        app.insert_resource({
            let mut state = CurvesState::default();
            state.select_for_test(ModuleId::new(), 0, 0);
            state
        });
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            AssetsAction::SelectLayer(1),
            BackgroundColor::default(),
        ));

        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer_index(),
            1
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
    }

    #[test]
    fn emitter_actions_add_and_duplicate_through_one_contract() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_emitters = session.effect.emitters.len();
        let mut app = app_with_session(session);

        app.world_mut().trigger(AssetsAction::AddEmitter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            initial_emitters + 1
        );

        app.world_mut().trigger(AssetsAction::DuplicateEmitter);
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), initial_emitters + 2);
        assert!(session.can_undo());
    }

    #[test]
    fn delete_emitter_action_opens_a_review_and_clears_curves() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut app = app_with_session(session);
        assert!(
            app.world_mut()
                .resource_mut::<WorkspaceLayout>()
                .show(DockPanel::Changes)
        );
        app.insert_resource({
            let mut state = CurvesState::default();
            state.select_for_test(ModuleId::new(), 0, 0);
            state
        });

        app.world_mut().trigger(AssetsAction::DeleteEmitter);
        app.update();

        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_some()
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
    }

    #[test]
    fn delete_emitter_action_protects_the_last_emitter() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect.emitters.truncate(1);
        let mut app = app_with_session(session);

        app.world_mut().trigger(AssetsAction::DeleteEmitter);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), 1);
        assert!(session.pending_change.is_none());
        assert_eq!(session.status, "An effect must keep at least one emitter");
    }

    #[test]
    fn emitter_shortcuts_route_through_the_assets_action_contract() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_emitters = session.effect.emitters.len();
        let mut app = app_with_session(session);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::Enter);
        }

        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            initial_emitters + 1
        );
    }
}
