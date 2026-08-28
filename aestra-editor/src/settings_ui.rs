//! Dockable Settings workspace and live editor-preference controls.

use crate::localization::SUPPORTED_LOCALES;
use crate::*;
use bevy::{ecs::system::SystemParam, ui_widgets::Activate};

const MAX_PREVIEW_PARTICLE_LIMIT: usize = 384;

pub(crate) struct EditorSettingsUiPlugin;

impl Plugin for EditorSettingsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsPanelState>()
            .add_observer(queue_settings_action_activation)
            .add_observer(handle_settings_toggle_change)
            .add_observer(handle_settings_integer_change)
            .add_observer(handle_settings_scalar_change)
            .add_systems(
                Update,
                handle_settings_actions
                    .before(crate::handle_buttons)
                    .in_set(crate::EditorSet::PreViewport),
            )
            .add_systems(
                Update,
                sync_settings_number_inputs.in_set(crate::EditorSet::UiSync),
            );
    }
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
pub(crate) struct SettingsPanelState {
    category: SettingsCategory,
}

#[derive(Component, Clone, Copy)]
enum SettingsUiAction {
    SelectCategory(SettingsCategory),
    SetLocale(usize),
    Reset,
}

#[derive(Component)]
struct SettingsToggleControl(SettingsToggle);

#[derive(Component)]
struct SettingsNumberControl(SettingsNumber);

#[derive(SystemParam)]
struct SettingsActionResources<'w> {
    panel: ResMut<'w, SettingsPanelState>,
    settings: ResMut<'w, EditorSettings>,
    persistence: ResMut<'w, SettingsPersistence>,
    menu: ResMut<'w, MenuState>,
    ui_scale: ResMut<'w, UiScale>,
    localizer: ResMut<'w, Localizer>,
}

pub(crate) fn spawn_settings_workspace(
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
                        SettingsUiAction::Reset,
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
            SettingsUiAction::SelectCategory(category),
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
                action: SettingsUiAction::SetLocale(index),
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

fn queue_settings_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<SettingsUiAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_settings_actions(
    mut commands: Commands,
    mut actions: Query<
        (Entity, &Interaction, &SettingsUiAction),
        (Changed<Interaction>, With<PendingFeathersActivation>),
    >,
    mut session: ResMut<EditorSession>,
    mut resources: SettingsActionResources,
) {
    for (entity, interaction, action) in &mut actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        match *action {
            SettingsUiAction::SelectCategory(category) => {
                if resources.panel.category != category {
                    resources.panel.category = category;
                    session.ui_revision += 1;
                }
            }
            SettingsUiAction::SetLocale(index) => {
                if let Some(locale) = SUPPORTED_LOCALES.get(index)
                    && resources.localizer.set_locale(locale)
                {
                    resources.settings.language.locale = resources.localizer.locale().into();
                    session.ui_revision += 1;
                    persist_editor_settings(
                        &resources.settings,
                        &mut resources.persistence,
                        &mut session,
                    );
                }
            }
            SettingsUiAction::Reset => match resources.persistence.replace_with_defaults() {
                Ok(defaults) => {
                    *resources.settings = defaults;
                    resources.menu.show_grid = resources.settings.preview.show_grid;
                    resources.ui_scale.0 = resources.settings.appearance.ui_scale;
                    resources
                        .localizer
                        .set_locale(&resources.settings.language.locale);
                    session.ui_revision += 1;
                    session.status = "Editor settings reset".into();
                }
                Err(error) => {
                    session.status = format!("Settings reset failed: {error}");
                }
            },
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_apply_persisted_constraints() {
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
    fn settings_action_activation_uses_its_own_feathers_contract() {
        let mut app = App::new();
        app.add_observer(queue_settings_action_activation);
        let action = app
            .world_mut()
            .spawn((
                SettingsUiAction::SelectCategory(SettingsCategory::Preview),
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
