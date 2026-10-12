//! Controlled numeric composition over Feathers text primitives.
//!
//! Aestra owns the value bridge, scrub policy and hover steppers. Do not compose
//! the upstream 0.20 number input as well: its private drag/chevron observers
//! would compete with these handlers. The document remains the source of truth.
use super::{pointer::PointerClick, scrub::NumberScrubState};
pub(crate) use bevy::feathers::controls::NumberInputValue;
use bevy::{
    app::PropagateOver,
    feathers::{
        constants::{fonts, size},
        controls::{FeathersTextInput, FeathersTextInputContainer},
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken},
        tokens,
    },
    input::keyboard::KeyboardInput,
    input_focus::{AcquireFocus, FocusCause, FocusLost, FocusedInput, InputFocus},
    prelude::*,
    scene::SceneScope,
    text::{EditableText, EditableTextFilter, FontSourceTemplate, TextEdit, TextEditChange},
    ui::InteractionDisabled,
    ui_widgets::{SelectAllOnFocus, ValueChange},
};

#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component, Default, Clone)]
pub(crate) enum NumberFormat {
    #[default]
    F32,
    F64,
    I32,
    I64,
}

/// App-owned update contract, independent of upstream widget synchronization.
#[derive(Clone, EntityEvent)]
pub(crate) struct UpdateNumberInput {
    pub(crate) entity: Entity,
    pub(crate) value: NumberInputValue,
}

#[derive(SceneComponent, Default, Clone, Reflect)]
#[scene(EditorNumberInputProps)]
#[reflect(Component, Default, Clone)]
pub(crate) struct EditorNumberInput;

/// TextEditChange also reports selection/cursor moves, not just value edits.
#[derive(Component, Default, Clone)]
struct NumericText(String);

/// Public, app-owned picking surface; no upstream private hierarchy is inspected.
#[derive(Component, Default, Clone)]
pub(super) struct NumericSurface;

pub(crate) struct EditorNumberInputProps {
    pub(crate) sigil_color: ThemeToken,
    pub(crate) label_text: Option<&'static str>,
    pub(crate) number_format: NumberFormat,
}

impl Default for EditorNumberInputProps {
    fn default() -> Self {
        Self {
            sigil_color: tokens::TEXT_INPUT_BG,
            label_text: None,
            number_format: NumberFormat::F32,
        }
    }
}

impl EditorNumberInput {
    // Bevy 0.19 requires template_value for a dynamic component value.
    #[allow(deprecated, reason = "Shared scene syntax for Bevy 0.19 and 0.20")]
    fn scene(props: EditorNumberInputProps) -> impl Scene {
        let surface = bsn! {
            Node {
                position_type: PositionType::Absolute,
                left: px(0), right: px(0), top: px(0), bottom: px(0),
            }
            NumericSurface
            Pickable::default()
            on(defer_focus)
            on(click_to_edit)
        };
        let input = bsn! {
            @FeathersTextInput { @max_characters: 20usize }
            NumericText::default()
            SelectAllOnFocus
            on(text_changed)
            on(enter)
            on(blur)
            EditableTextFilter::new(|c| {
                c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')
            })
            Children [{surface}]
        };
        let label = props.label_text.map(|text| {
            SceneScope(bsn! {
                Node {
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    align_self: AlignSelf::Stretch,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(px(6), px(0)),
                }
                ThemeBackgroundColor(tokens::TEXT_INPUT_LABEL_BG)
                Children [
                    Text(text)
                    TextFont {
                        font: FontSourceTemplate::Handle(fonts::REGULAR),
                        font_size: size::COMPACT_FONT,
                    }
                    PropagateOver<TextFont>
                    ThemeTextColor(tokens::TEXT_INPUT_TEXT)
                ]
            })
        });
        (
            bsn! {
                @FeathersTextInputContainer
                ThemeBorderColor({props.sigil_color})
                EditorNumberInput
                on(update_display)
                Children [{(label, SceneScope(input))}]
            },
            template_value(props.number_format),
        )
    }
}

fn defer_focus(mut request: On<AcquireFocus>) {
    // Do not focus/select text on press: a press may become a numeric drag.
    request.propagate(false);
}

fn click_to_edit(
    mut click: On<PointerClick>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<InteractionDisabled>>,
    state: Res<NumberScrubState>,
    mut focus: ResMut<InputFocus>,
) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let Ok(input) = parents.get(click.entity) else {
        return;
    };
    let Ok(owner) = parents.get(input.parent()) else {
        return;
    };
    click.propagate(false);
    // Picking dispatches Click before DragEnd on release. A drag must not enter editing.
    if !state.is_scrubbing(owner.parent()) && enabled(click.entity, &parents, &disabled) {
        focus.set(input.parent(), FocusCause::Navigated);
    }
}

pub(super) fn sync_numeric_surfaces(
    focus: Res<InputFocus>,
    mut surfaces: Query<(&ChildOf, &mut Pickable), With<NumericSurface>>,
) {
    for (parent, mut pickable) in &mut surfaces {
        let next = if focus.get() == Some(parent.parent()) {
            Pickable::IGNORE
        } else {
            Pickable::default()
        };
        if *pickable != next {
            *pickable = next;
        }
    }
}

pub(crate) fn feathers_integer_input() -> impl Scene {
    bsn! { @EditorNumberInput { @number_format: NumberFormat::I32 } }
}

pub(crate) fn feathers_scalar_input() -> impl Scene {
    bsn! { @EditorNumberInput }
}

pub(crate) fn feathers_labeled_scalar_input(
    label_text: &'static str,
    sigil_color: ThemeToken,
) -> impl Scene {
    bsn! { @EditorNumberInput { @label_text: label_text, @sigil_color: sigil_color } }
}

fn update_display(
    update: On<UpdateNumberInput>,
    inputs: Query<(&Children, &NumberFormat), With<EditorNumberInput>>,
    mut texts: Query<&mut EditableText>,
    focus: Res<InputFocus>,
) {
    let Ok((children, format)) = inputs.get(update.entity) else {
        return;
    };
    let value_format = match update.value {
        NumberInputValue::F32(_) => NumberFormat::F32,
        NumberInputValue::F64(_) => NumberFormat::F64,
        NumberInputValue::I32(_) => NumberFormat::I32,
        NumberInputValue::I64(_) => NumberFormat::I64,
    };
    if *format != value_format {
        return;
    }
    for child in children {
        if let Ok(mut text) = texts.get_mut(*child) {
            // Preserve an in-progress edit when panels refresh their model.
            if focus.get() != Some(*child) {
                let digits = update.value.to_string();
                if text.value().to_string() != digits {
                    text.queue_edit(TextEdit::SelectAll);
                    text.queue_edit(TextEdit::Insert(digits.into()));
                    text.queue_edit(TextEdit::CollapseSelection);
                }
            }
            break;
        }
    }
}

fn enabled(
    entity: Entity,
    parents: &Query<&ChildOf>,
    disabled: &Query<(), With<InteractionDisabled>>,
) -> bool {
    !disabled.contains(entity) && !parents.iter_ancestors(entity).any(|e| disabled.contains(e))
}

fn text_changed(
    change: On<TextEditChange>,
    parents: Query<&ChildOf>,
    inputs: Query<&NumberFormat, With<EditorNumberInput>>,
    mut texts: Query<(&EditableText, &mut NumericText)>,
    focus: Res<InputFocus>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    let entity = change.event_target();
    let Ok((text, mut previous)) = texts.get_mut(entity) else {
        return;
    };
    let value = text.value().to_string();
    if previous.0 == value {
        return;
    }
    previous.0.clone_from(&value);
    // Display refreshes, scrub writes and arrow writes are not user typing.
    if focus.get() != Some(entity) || !enabled(entity, &parents, &disabled) {
        return;
    }
    let Ok(parent) = parents.get(entity) else {
        return;
    };
    if let Ok(format) = inputs.get(parent.parent()) {
        emit(&value, *format, parent.parent(), false, &mut commands);
    }
}

fn enter(
    key: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    inputs: Query<&NumberFormat, With<EditorNumberInput>>,
    texts: Query<&EditableText>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    if key.input.key_code != KeyCode::Enter || !key.input.state.is_pressed() {
        return;
    }
    submit(
        key.event_target(),
        &parents,
        &inputs,
        &texts,
        &disabled,
        &mut commands,
    );
}

fn blur(
    lost: On<FocusLost>,
    parents: Query<&ChildOf>,
    inputs: Query<&NumberFormat, With<EditorNumberInput>>,
    texts: Query<&EditableText>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    submit(
        lost.event_target(),
        &parents,
        &inputs,
        &texts,
        &disabled,
        &mut commands,
    );
}

fn submit(
    entity: Entity,
    parents: &Query<&ChildOf>,
    inputs: &Query<&NumberFormat, With<EditorNumberInput>>,
    texts: &Query<&EditableText>,
    disabled: &Query<(), With<InteractionDisabled>>,
    commands: &mut Commands,
) {
    if !enabled(entity, parents, disabled) {
        return;
    }
    let Ok(parent) = parents.get(entity) else {
        return;
    };
    if let (Ok(format), Ok(text)) = (inputs.get(parent.parent()), texts.get(entity)) {
        emit(
            &text.value().to_string(),
            *format,
            parent.parent(),
            true,
            commands,
        );
    }
}

fn emit(text: &str, format: NumberFormat, source: Entity, is_final: bool, commands: &mut Commands) {
    let text = text.trim();
    macro_rules! parsed {
        ($ty:ty, $valid:expr) => {
            if let Ok(value) = text.parse::<$ty>()
                && $valid(value)
            {
                commands.trigger(ValueChange {
                    source,
                    value,
                    is_final,
                });
            }
        };
    }
    match format {
        NumberFormat::F32 => parsed!(f32, f32::is_finite),
        NumberFormat::F64 => parsed!(f64, f64::is_finite),
        NumberFormat::I32 => parsed!(i32, |_| true),
        NumberFormat::I64 => parsed!(i64, |_| true),
    }
}

#[cfg(test)]
#[path = "widget_tests.rs"]
mod tests;
