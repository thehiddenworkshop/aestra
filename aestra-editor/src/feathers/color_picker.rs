//! Reusable color-picker composition built from Bevy Feathers color controls.

use super::{scenes, text_input};
use crate::{EditorNativeControl, feathers::number_input, theme, ui_shell};
use bevy::{
    color::{Hsla, Srgba},
    feathers::controls::{
        ButtonVariant, ColorChannel, ColorPlaneValue, NumberInputValue, SliderBaseColor,
        UpdateNumberInput,
    },
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, TextEdit},
    ui::Selected,
    ui_widgets::{Activate, SliderValue, ValueChange},
};

#[derive(Clone)]
pub(crate) struct ColorPickerLabels {
    pub(crate) accessible: String,
    pub(crate) hue_saturation: String,
    pub(crate) lightness: String,
    pub(crate) alpha: String,
    pub(crate) automatic: String,
    pub(crate) rgb: String,
    pub(crate) hsl: String,
    pub(crate) red: String,
    pub(crate) green: String,
    pub(crate) blue: String,
    pub(crate) hue: String,
    pub(crate) saturation: String,
    pub(crate) hex: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ColorPickerMode {
    #[default]
    Rgb,
    Hsl,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ColorPickerState {
    hsla: Hsla,
    automatic_color: [f32; 4],
    automatic: bool,
    mode: ColorPickerMode,
}

#[derive(Component)]
pub(crate) struct ColorPickerRoot;

impl ColorPickerState {
    fn from_srgba(color: [f32; 4]) -> Self {
        Self {
            hsla: Hsla::from(Srgba::new(color[0], color[1], color[2], color[3])),
            automatic_color: color,
            automatic: false,
            mode: ColorPickerMode::Rgb,
        }
    }

    fn new(authored: Option<[f32; 4]>, automatic_color: [f32; 4]) -> Self {
        let mut state = Self::from_srgba(authored.unwrap_or(automatic_color));
        state.automatic_color = automatic_color;
        state.automatic = authored.is_none();
        state
    }

    fn srgba(self) -> [f32; 4] {
        let color = Srgba::from(self.hsla);
        [color.red, color.green, color.blue, color.alpha]
    }
}

#[derive(Component)]
pub(crate) struct ColorPickerPlane;

#[derive(Component)]
pub(crate) struct ColorPickerLightness;

#[derive(Component)]
pub(crate) struct ColorPickerPreview;

#[derive(Component)]
pub(crate) struct ColorPickerAutomatic;

#[derive(Component)]
pub(crate) struct ColorPickerModeButton(ColorPickerMode);

#[derive(Component)]
pub(crate) struct ColorPickerChannelPanel(ColorPickerMode);

#[derive(Component, Clone, Copy)]
pub(crate) enum ColorPickerChannelInput {
    Red,
    Green,
    Blue,
    Hue,
    Saturation,
    Lightness,
    Alpha,
}

#[derive(Component)]
pub(crate) struct ColorPickerAlpha;

#[derive(Component)]
pub(crate) struct ColorPickerHexInput;

/// Spawns a complete hue/saturation/lightness picker.
///
/// The caller marker is attached to the root. The picker emits
/// `ValueChange<Option<[f32; 4]>>` from that root. `None` means automatic color.
pub(crate) fn spawn_color_picker<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    authored: Option<[f32; 4]>,
    automatic_color: [f32; 4],
    labels: ColorPickerLabels,
    marker: M,
) -> Entity {
    let initial = authored.unwrap_or(automatic_color);
    let state = ColorPickerState::new(authored, automatic_color);
    let mut root = parent.spawn((
        marker,
        ColorPickerRoot,
        state,
        AccessibleLabel(labels.accessible.clone()),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            ..default()
        },
    ));
    let root_entity = root.id();
    root.with_children(|picker| {
        picker
            .spawn(color_plane_frame_node())
            .with_children(|wrapper| {
                wrapper
                    .spawn_empty()
                    .apply_scene(scenes::feathers_hue_saturation_plane())
                    .insert((
                        ColorPickerPlane,
                        ColorPlaneValue(Vec3::new(
                            state.hsla.hue / 360.0,
                            state.hsla.saturation,
                            state.hsla.lightness,
                        )),
                        AccessibleLabel(labels.hue_saturation.clone()),
                    ))
                    .observe(finalize_color_plane);
            });
        spawn_color_slider_row(
            picker,
            labels.lightness.clone(),
            state.hsla.lightness,
            ColorChannel::HslLightness,
            (
                ColorPickerLightness,
                SliderBaseColor(Color::Hsla(state.hsla)),
            ),
        );
        spawn_color_slider_row(
            picker,
            labels.alpha.clone(),
            state.hsla.alpha,
            ColorChannel::Alpha,
            (ColorPickerAlpha, SliderBaseColor(Color::Hsla(state.hsla))),
        );
        spawn_color_mode_tabs(picker, &labels);
        spawn_color_channel_panel(picker, ColorPickerMode::Rgb, &labels);
        spawn_color_channel_panel(picker, ColorPickerMode::Hsl, &labels);
        picker
            .spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(7.0),
                ..default()
            })
            .with_children(|footer| {
                footer.spawn((
                    ColorPickerPreview,
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(20.0),
                        flex_shrink: 0.0,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(initial[0], initial[1], initial[2], initial[3])),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    Pickable::IGNORE,
                ));
                footer.spawn((
                    Text::new(labels.hex.clone()),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
                footer
                    .spawn(Node {
                        min_width: Val::Px(72.0),
                        flex_grow: 1.0,
                        ..default()
                    })
                    .with_children(|input| {
                        text_input::spawn_text_input(
                            input,
                            &format_hex(initial),
                            &labels.hex,
                            ColorPickerHexInput,
                        );
                    });
                let mut automatic = footer.spawn_empty();
                automatic
                    .apply_scene(scenes::feathers_plain_button())
                    .insert((
                        ColorPickerAutomatic,
                        EditorNativeControl,
                        AccessibleLabel(labels.automatic.clone()),
                        Node {
                            height: Val::Px(24.0),
                            padding: UiRect::horizontal(Val::Px(8.0)),
                            ..default()
                        },
                    ))
                    .observe(reset_to_automatic)
                    .with_child((
                        Text::new(labels.automatic),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                if authored.is_none() {
                    automatic.insert((Selected, ButtonVariant::Primary));
                }
            });
    });
    root_entity
}

fn spawn_color_slider_row<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    label: String,
    value: f32,
    channel: ColorChannel,
    marker: M,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label.clone()),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    width: Val::Px(45.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                min_width: Val::Px(0.0),
                height: Val::Px(16.0),
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|wrapper| {
                wrapper
                    .spawn_empty()
                    .apply_scene(scenes::feathers_color_slider(value, channel))
                    .insert((marker, AccessibleLabel(label)));
            });
        });
}

fn spawn_color_mode_tabs(parent: &mut ChildSpawnerCommands, labels: &ColorPickerLabels) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            column_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|tabs| {
            for (mode, label) in [
                (ColorPickerMode::Rgb, labels.rgb.clone()),
                (ColorPickerMode::Hsl, labels.hsl.clone()),
            ] {
                let mut button = tabs.spawn_empty();
                button
                    .apply_scene(scenes::feathers_plain_button())
                    .insert((
                        ColorPickerModeButton(mode),
                        EditorNativeControl,
                        AccessibleLabel(label.clone()),
                        Node {
                            height: Val::Px(23.0),
                            flex_grow: 1.0,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ))
                    .observe(select_color_picker_mode)
                    .with_child((
                        Text::new(label),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                if mode == ColorPickerMode::Rgb {
                    button.insert((Selected, ButtonVariant::Primary));
                }
            }
        });
}

fn spawn_color_channel_panel(
    parent: &mut ChildSpawnerCommands,
    mode: ColorPickerMode,
    labels: &ColorPickerLabels,
) {
    let channels = match mode {
        ColorPickerMode::Rgb => [
            (ColorPickerChannelInput::Red, labels.red.clone()),
            (ColorPickerChannelInput::Green, labels.green.clone()),
            (ColorPickerChannelInput::Blue, labels.blue.clone()),
            (ColorPickerChannelInput::Alpha, labels.alpha.clone()),
        ],
        ColorPickerMode::Hsl => [
            (ColorPickerChannelInput::Hue, labels.hue.clone()),
            (
                ColorPickerChannelInput::Saturation,
                labels.saturation.clone(),
            ),
            (ColorPickerChannelInput::Lightness, labels.lightness.clone()),
            (ColorPickerChannelInput::Alpha, labels.alpha.clone()),
        ],
    };
    parent
        .spawn((
            ColorPickerChannelPanel(mode),
            Node {
                display: if mode == ColorPickerMode::Rgb {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            },
        ))
        .with_children(|panel| {
            for (channel, label) in channels {
                spawn_color_channel_input(panel, channel, label);
            }
        });
}

fn spawn_color_channel_input(
    parent: &mut ChildSpawnerCommands,
    channel: ColorPickerChannelInput,
    label: String,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(22.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label.clone()),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    width: Val::Px(64.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn(Node {
                min_width: Val::Px(0.0),
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|input| {
                input
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert((
                        channel,
                        number_input::ScrubbableNumber::new(
                            0.0,
                            channel.minimum(),
                            channel.maximum(),
                            channel.scrub_step(),
                        ),
                        AccessibleLabel(label),
                    ));
            });
        });
}

impl ColorPickerChannelInput {
    fn minimum(self) -> f32 {
        0.0
    }

    fn maximum(self) -> f32 {
        match self {
            Self::Hue => 360.0,
            _ => 1.0,
        }
    }

    fn scrub_step(self) -> f32 {
        match self {
            Self::Hue => 1.0,
            _ => 0.01,
        }
    }
}

fn color_plane_frame_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(112.0),
        min_height: Val::Px(112.0),
        // The native plane uses `align_self: Stretch`. Its parent must therefore
        // lay out children vertically so the cross axis is horizontal.
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

pub(crate) fn handle_color_plane_change(
    change: On<ValueChange<Vec2>>,
    planes: Query<(), With<ColorPickerPlane>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    mut commands: Commands,
) {
    if !planes.contains(change.source) {
        return;
    }
    let Some(root) = find_picker_root(change.source, &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    picker.hsla.hue = change.value.x.clamp(0.0, 1.0) * 360.0;
    picker.hsla.saturation = change.value.y.clamp(0.0, 1.0);
    picker.automatic = false;
    emit_picker_change(&mut commands, root, *picker, change.is_final);
}

pub(crate) fn handle_color_lightness_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<ColorPickerLightness>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    mut commands: Commands,
) {
    if !sliders.contains(change.source) {
        return;
    }
    let Some(root) = find_picker_root(change.source, &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    picker.hsla.lightness = change.value.clamp(0.0, 1.0);
    picker.automatic = false;
    emit_picker_change(&mut commands, root, *picker, change.is_final);
}

pub(crate) fn handle_color_alpha_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<ColorPickerAlpha>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    mut commands: Commands,
) {
    if !sliders.contains(change.source) {
        return;
    }
    let Some(root) = find_picker_root(change.source, &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    picker.hsla.alpha = change.value.clamp(0.0, 1.0);
    picker.automatic = false;
    emit_picker_change(&mut commands, root, *picker, change.is_final);
}

pub(crate) fn handle_color_channel_change(
    change: On<ValueChange<f32>>,
    controls: Query<&ColorPickerChannelInput>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    mut commands: Commands,
) {
    let source = change.event_target();
    let Ok(channel) = controls.get(source) else {
        return;
    };
    let Some(root) = find_picker_root(source, &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    set_picker_channel(&mut picker, *channel, change.value);
    picker.automatic = false;
    emit_picker_change(&mut commands, root, *picker, change.is_final);
}

fn set_picker_channel(picker: &mut ColorPickerState, channel: ColorPickerChannelInput, value: f32) {
    match channel {
        ColorPickerChannelInput::Red
        | ColorPickerChannelInput::Green
        | ColorPickerChannelInput::Blue => {
            let mut color = Srgba::from(picker.hsla);
            match channel {
                ColorPickerChannelInput::Red => color.red = value.clamp(0.0, 1.0),
                ColorPickerChannelInput::Green => color.green = value.clamp(0.0, 1.0),
                ColorPickerChannelInput::Blue => color.blue = value.clamp(0.0, 1.0),
                _ => unreachable!(),
            }
            picker.hsla = Hsla::from(color);
        }
        ColorPickerChannelInput::Hue => picker.hsla.hue = value.rem_euclid(360.0),
        ColorPickerChannelInput::Saturation => {
            picker.hsla.saturation = value.clamp(0.0, 1.0);
        }
        ColorPickerChannelInput::Lightness => {
            picker.hsla.lightness = value.clamp(0.0, 1.0);
        }
        ColorPickerChannelInput::Alpha => picker.hsla.alpha = value.clamp(0.0, 1.0),
    }
}

pub(crate) fn handle_color_hex_change(
    change: On<ValueChange<String>>,
    controls: Query<(), With<ColorPickerHexInput>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    children: Query<&Children>,
    mut texts: Query<&mut EditableText>,
    mut commands: Commands,
) {
    let source = change.event_target();
    if !controls.contains(source) {
        return;
    }
    let Some(root) = find_picker_root(source, &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    let Some(color) = parse_hex(&change.value) else {
        if change.is_final {
            replace_editable_text(source, format_hex(picker.srgba()), &children, &mut texts);
        }
        return;
    };
    picker.hsla = Hsla::from(Srgba::new(color[0], color[1], color[2], color[3]));
    picker.automatic = false;
    emit_picker_change(&mut commands, root, *picker, change.is_final);
}

fn select_color_picker_mode(
    activate: On<Activate>,
    buttons: Query<&ColorPickerModeButton>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
) {
    let Ok(mode) = buttons.get(activate.event_target()) else {
        return;
    };
    let Some(root) = find_picker_root(activate.event_target(), &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    picker.mode = mode.0;
}

fn emit_picker_change(
    commands: &mut Commands,
    root: Entity,
    picker: ColorPickerState,
    is_final: bool,
) {
    commands.trigger(ValueChange {
        source: root,
        value: Some(picker.srgba()),
        is_final,
    });
}

fn finalize_color_plane(
    release: On<Pointer<Release>>,
    planes: Query<(), With<ColorPickerPlane>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    pickers: Query<&ColorPickerState>,
    mut commands: Commands,
) {
    if !planes.contains(release.event_target()) {
        return;
    }
    let Some(root) = find_picker_root(release.event_target(), &parents, &roots) else {
        return;
    };
    let Ok(picker) = pickers.get(root) else {
        return;
    };
    commands.trigger(ValueChange {
        source: root,
        value: Some(picker.srgba()),
        is_final: true,
    });
}

fn reset_to_automatic(
    activate: On<Activate>,
    automatic: Query<&ChildOf, With<ColorPickerAutomatic>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ColorPickerRoot>>,
    mut pickers: Query<&mut ColorPickerState>,
    mut commands: Commands,
) {
    if automatic.get(activate.event_target()).is_err() {
        return;
    }
    let Some(root) = find_picker_root(activate.event_target(), &parents, &roots) else {
        return;
    };
    let Ok(mut picker) = pickers.get_mut(root) else {
        return;
    };
    picker.hsla = Hsla::from(Srgba::new(
        picker.automatic_color[0],
        picker.automatic_color[1],
        picker.automatic_color[2],
        picker.automatic_color[3],
    ));
    picker.automatic = true;
    commands.trigger(ValueChange::<Option<[f32; 4]>> {
        source: root,
        value: None,
        is_final: true,
    });
}

pub(crate) fn sync_color_picker_visuals(
    mut commands: Commands,
    pickers: Query<(Entity, &ColorPickerState), Changed<ColorPickerState>>,
    children: Query<&Children>,
    mut planes: Query<&mut ColorPlaneValue, With<ColorPickerPlane>>,
    mut sliders: Query<(
        Entity,
        &mut SliderBaseColor,
        Has<ColorPickerLightness>,
        Has<ColorPickerAlpha>,
    )>,
    mut previews: Query<&mut BackgroundColor, With<ColorPickerPreview>>,
    channels: Query<(Entity, &ColorPickerChannelInput)>,
    mut scrubbable_numbers: Query<&mut number_input::ScrubbableNumber>,
    hex_inputs: Query<Entity, With<ColorPickerHexInput>>,
    mut texts: Query<&mut EditableText>,
    focus: Res<InputFocus>,
    mut panels: Query<(&ColorPickerChannelPanel, &mut Node)>,
    mode_buttons: Query<(Entity, &ColorPickerModeButton, Has<Selected>)>,
    automatic_buttons: Query<(Entity, Has<Selected>), With<ColorPickerAutomatic>>,
) {
    for (root, picker) in &pickers {
        let srgba = picker.srgba();
        for descendant in children.iter_descendants(root) {
            if let Ok(mut plane) = planes.get_mut(descendant) {
                plane.0 = Vec3::new(
                    picker.hsla.hue / 360.0,
                    picker.hsla.saturation,
                    picker.hsla.lightness,
                );
            }
            if let Ok((slider, mut base, lightness, alpha)) = sliders.get_mut(descendant) {
                base.0 = Color::Hsla(picker.hsla);
                let value = if lightness {
                    picker.hsla.lightness
                } else if alpha {
                    picker.hsla.alpha
                } else {
                    continue;
                };
                commands.entity(slider).insert(SliderValue(value));
            }
            if let Ok(mut preview) = previews.get_mut(descendant) {
                preview.0 = Color::srgba(srgba[0], srgba[1], srgba[2], srgba[3]);
            }
            if let Ok((input, channel)) = channels.get(descendant) {
                let value = channel_display_value(*channel, *picker);
                if let Ok(mut scrub) = scrubbable_numbers.get_mut(input) {
                    scrub.value = value;
                }
                commands.trigger(UpdateNumberInput {
                    entity: input,
                    value: NumberInputValue::F32(value),
                });
            }
            if hex_inputs.contains(descendant) {
                sync_hex_input(descendant, format_hex(srgba), &children, &mut texts, &focus);
            }
            if let Ok((panel, mut node)) = panels.get_mut(descendant) {
                node.display = if panel.0 == picker.mode {
                    Display::Flex
                } else {
                    Display::None
                };
            }
            if let Ok((button, mode, selected)) = mode_buttons.get(descendant) {
                sync_selected_button(&mut commands, button, mode.0 == picker.mode, selected);
            }
            if let Ok((button, selected)) = automatic_buttons.get(descendant) {
                sync_selected_button(&mut commands, button, picker.automatic, selected);
            }
        }
    }
}

fn channel_display_value(channel: ColorPickerChannelInput, picker: ColorPickerState) -> f32 {
    let srgba = picker.srgba();
    match channel {
        ColorPickerChannelInput::Red => number_input::rounded(srgba[0], 3),
        ColorPickerChannelInput::Green => number_input::rounded(srgba[1], 3),
        ColorPickerChannelInput::Blue => number_input::rounded(srgba[2], 3),
        ColorPickerChannelInput::Hue => number_input::rounded(picker.hsla.hue, 1),
        ColorPickerChannelInput::Saturation => number_input::rounded(picker.hsla.saturation, 3),
        ColorPickerChannelInput::Lightness => number_input::rounded(picker.hsla.lightness, 3),
        ColorPickerChannelInput::Alpha => number_input::rounded(picker.hsla.alpha, 3),
    }
}

fn sync_selected_button(
    commands: &mut Commands,
    entity: Entity,
    should_select: bool,
    selected: bool,
) {
    if should_select && !selected {
        commands
            .entity(entity)
            .insert((Selected, ButtonVariant::Primary));
    } else if !should_select && selected {
        commands
            .entity(entity)
            .remove::<Selected>()
            .insert(ButtonVariant::Plain);
    }
}

fn sync_hex_input(
    container: Entity,
    value: String,
    children: &Query<&Children>,
    texts: &mut Query<&mut EditableText>,
    focus: &InputFocus,
) {
    for descendant in children.iter_descendants(container) {
        if focus.get() == Some(descendant) {
            return;
        }
        let Ok(mut text) = texts.get_mut(descendant) else {
            continue;
        };
        if text.value() != value.as_str() {
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(value.into()));
            text.queue_edit(TextEdit::CollapseSelection);
        }
        return;
    }
}

fn replace_editable_text(
    container: Entity,
    value: String,
    children: &Query<&Children>,
    texts: &mut Query<&mut EditableText>,
) {
    for descendant in children.iter_descendants(container) {
        let Ok(mut text) = texts.get_mut(descendant) else {
            continue;
        };
        text.queue_edit(TextEdit::SelectAll);
        text.queue_edit(TextEdit::Insert(value.into()));
        text.queue_edit(TextEdit::CollapseSelection);
        return;
    }
}

fn find_picker_root(
    mut entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<ColorPickerRoot>>,
) -> Option<Entity> {
    loop {
        if roots.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

fn format_hex(color: [f32; 4]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        channel(color[0]),
        channel(color[1]),
        channel(color[2]),
        channel(color[3])
    )
}

fn parse_hex(value: &str) -> Option<[f32; 4]> {
    let digits = value.trim().strip_prefix('#').unwrap_or(value.trim());
    let expanded;
    let digits = match digits.len() {
        3 | 4 => {
            expanded = digits
                .chars()
                .flat_map(|digit| [digit, digit])
                .collect::<String>();
            expanded.as_str()
        }
        6 | 8 => digits,
        _ => return None,
    };
    let byte = |start: usize| u8::from_str_radix(&digits[start..start + 2], 16).ok();
    let red = byte(0)?;
    let green = byte(2)?;
    let blue = byte(4)?;
    let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
    let normalized = |channel: u8| f32::from(channel) / 255.0;
    Some([
        normalized(red),
        normalized(green),
        normalized(blue),
        normalized(alpha),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct CapturedColor(Option<(Option<[f32; 4]>, bool)>);

    fn capture_color_change(
        change: On<ValueChange<Option<[f32; 4]>>>,
        mut captured: ResMut<CapturedColor>,
    ) {
        captured.0 = Some((change.value, change.is_final));
    }

    #[test]
    fn color_picker_round_trips_srgba_and_formats_hex() {
        let color = [0.28, 0.78, 0.45, 1.0];
        let round_trip = ColorPickerState::from_srgba(color).srgba();
        for (actual, expected) in round_trip.into_iter().zip(color) {
            assert!((actual - expected).abs() < 0.000_1);
        }
        assert_eq!(format_hex(color), "#47C773FF");
        assert_eq!(parse_hex("#47C77380").unwrap()[3], 128.0 / 255.0);
        assert_eq!(
            parse_hex("abc"),
            Some([
                0xAA as f32 / 255.0,
                0xBB as f32 / 255.0,
                0xCC as f32 / 255.0,
                1.0
            ])
        );
    }

    #[test]
    fn color_plane_frame_stretches_the_native_plane_horizontally() {
        let frame = color_plane_frame_node();
        assert_eq!(frame.width, Val::Percent(100.0));
        assert_eq!(frame.height, Val::Px(112.0));
        assert_eq!(frame.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn plane_changes_are_composed_into_one_picker_value_event() {
        let mut app = App::new();
        app.init_resource::<CapturedColor>()
            .add_observer(handle_color_plane_change)
            .add_observer(capture_color_change);
        let root = app
            .world_mut()
            .spawn((
                ColorPickerRoot,
                ColorPickerState::from_srgba([1.0, 0.0, 0.0, 1.0]),
            ))
            .id();
        let plane = app
            .world_mut()
            .spawn((ColorPickerPlane, ChildOf(root)))
            .id();

        app.world_mut().trigger(ValueChange {
            source: plane,
            value: Vec2::new(0.5, 0.75),
            is_final: true,
        });
        app.update();

        let picker = app.world().get::<ColorPickerState>(root).unwrap();
        assert_eq!(picker.hsla.hue, 180.0);
        assert_eq!(picker.hsla.saturation, 0.75);
        let captured = app.world().resource::<CapturedColor>().0.unwrap();
        assert!(captured.0.is_some());
        assert!(captured.1);
    }

    #[test]
    fn rgb_channel_changes_update_the_shared_color_state() {
        let mut direct = ColorPickerState::from_srgba([1.0, 0.0, 0.0, 1.0]);
        set_picker_channel(&mut direct, ColorPickerChannelInput::Green, 0.5);
        assert!((direct.srgba()[1] - 0.5).abs() < 0.000_1);

        let mut app = App::new();
        app.init_resource::<CapturedColor>()
            .add_observer(handle_color_channel_change)
            .add_observer(capture_color_change);
        let root = app
            .world_mut()
            .spawn((
                ColorPickerRoot,
                ColorPickerState::from_srgba([1.0, 0.0, 0.0, 1.0]),
            ))
            .id();
        let input = app
            .world_mut()
            .spawn((ColorPickerChannelInput::Green, ChildOf(root)))
            .id();
        assert!(app.world().get::<ColorPickerChannelInput>(input).is_some());

        app.world_mut().trigger(ValueChange {
            source: input,
            value: 0.5_f32,
            is_final: true,
        });
        app.update();

        assert!(
            app.world().resource::<CapturedColor>().0.is_some(),
            "channel handler did not emit a picker change"
        );
        let actual = app.world().get::<ColorPickerState>(root).unwrap().srgba();
        assert!(
            (actual[1] - 0.5).abs() < 0.000_1,
            "actual color: {actual:?}"
        );
        assert!(app.world().resource::<CapturedColor>().0.unwrap().1);
    }

    #[test]
    fn rgba_hex_changes_update_all_channels() {
        let mut app = App::new();
        app.init_resource::<CapturedColor>()
            .add_observer(handle_color_hex_change)
            .add_observer(capture_color_change);
        let root = app
            .world_mut()
            .spawn((
                ColorPickerRoot,
                ColorPickerState::from_srgba([1.0, 0.0, 0.0, 1.0]),
            ))
            .id();
        let input = app
            .world_mut()
            .spawn((ColorPickerHexInput, ChildOf(root)))
            .id();

        app.world_mut().trigger(ValueChange {
            source: input,
            value: "#33669980".to_owned(),
            is_final: true,
        });
        app.update();

        let actual = app.world().get::<ColorPickerState>(root).unwrap().srgba();
        for (actual, expected) in actual.into_iter().zip([0x33_u8, 0x66_u8, 0x99_u8, 0x80_u8]) {
            assert!((actual - f32::from(expected) / 255.0).abs() < 0.000_1);
        }
        assert!(app.world().resource::<CapturedColor>().0.unwrap().1);
    }
}
