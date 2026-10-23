//! Editor policy wrapper around the released Feathers number input.
//! The native widget is the sole owner of text editing and drag arithmetic.
pub(crate) use bevy::feathers::controls::NumberInputValue;
use bevy::{
    app::PropagateOver,
    feathers::{
        constants::{fonts, size},
        controls::{FeathersNumberInput, NumberInputPrecision, NumberInputStep},
        theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken},
        tokens,
    },
    prelude::*,
    scene::SceneScope,
    text::FontSourceTemplate,
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

impl NumberFormat {
    pub(super) fn zero(self) -> NumberInputValue {
        match self {
            Self::F32 => NumberInputValue::F32(0.0),
            Self::F64 => NumberInputValue::F64(0.0),
            Self::I32 => NumberInputValue::I32(0),
            Self::I64 => NumberInputValue::I64(0),
        }
    }

    pub(super) fn matches(self, value: NumberInputValue) -> bool {
        std::mem::discriminant(&self.zero()) == std::mem::discriminant(&value)
    }
}

/// Model synchronization, never a user edit. Kept at the semantic owner boundary.
#[derive(Clone, EntityEvent)]
pub(crate) struct UpdateNumberInput {
    pub(crate) entity: Entity,
    pub(crate) value: NumberInputValue,
}

#[derive(SceneComponent, Default, Clone, Reflect)]
#[scene(EditorNumberInputProps)]
#[reflect(Component, Default, Clone)]
#[require(super::native::Model)]
pub(crate) struct EditorNumberInput;

/// App-owned marker on the native child; no private upstream hierarchy is used.
#[derive(Component, Default, Clone)]
pub(super) struct NativeNumber;

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
    #[allow(deprecated, reason = "Dynamic typed component values in a scene tuple")]
    fn scene(props: EditorNumberInputProps) -> impl Scene {
        let label = props.label_text.map(|text| SceneScope(bsn! {
            Node {
                grid_column: GridPlacement::start(1),
                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(6)),
            }
            ThemeBackgroundColor(tokens::TEXT_INPUT_LABEL_BG)
            Children [
                Text(text)
                TextFont { font: FontSourceTemplate::Handle(fonts::REGULAR), font_size: size::COMPACT_FONT }
                PropagateOver<TextFont>
                ThemeTextColor(tokens::TEXT_INPUT_TEXT)
            ]
        }));
        let native = (
            bsn! {
                @FeathersNumberInput
                Node { grid_column: GridPlacement::start(2), width: percent(100), min_width: px(0) }
                NativeNumber
                NumberInputStep({if matches!(props.number_format, NumberFormat::I32 | NumberFormat::I64) { 100.0 } else { 0.125 }})
                NumberInputPrecision({if matches!(props.number_format, NumberFormat::I32 | NumberFormat::I64) { 0 } else { 6 }})
            },
            template_value(props.number_format.zero()),
        );
        (
            bsn! {
                Node {
                    display: Display::Grid, position_type: PositionType::Relative,
                    width: percent(100), height: size::ROW_HEIGHT, min_width: px(0),
                    border: UiRect::left(px(3)),
                    grid_template_columns: {vec![
                        if props.label_text.is_some() { GridTrack::auto() } else { GridTrack::px(0.0) },
                        GridTrack::flex(1.0),
                    ]},
                }
                ThemeBorderColor({props.sigil_color})
                EditorNumberInput
                Children [
                    {label}
                    --
                    {SceneScope(native)}
                ]
            },
            template_value(props.number_format),
        )
    }
}

pub(crate) fn feathers_integer_input() -> impl Scene {
    bsn! { @EditorNumberInput { @number_format: NumberFormat::I32 } }
}

pub(crate) fn feathers_scalar_input() -> impl Scene {
    bsn! { @EditorNumberInput }
}

/// Number fields formerly composed as text (function defaults/constants) share
/// the same native interaction and semantic source identity as inspector fields.
pub(crate) fn spawn_scalar_input(
    parent: &mut ChildSpawnerCommands,
    value: f32,
    label: &str,
    control: impl Bundle,
) -> Entity {
    let entity = parent
        .spawn_empty()
        .apply_scene(feathers_scalar_input())
        .insert((
            control,
            AccessibleLabel(label.to_owned()),
            super::ScrubbableNumber::new(value, -f32::MAX, f32::MAX, 0.01),
        ))
        .id();
    parent.commands().trigger(UpdateNumberInput {
        entity,
        value: NumberInputValue::F32(value),
    });
    entity
}

pub(crate) fn feathers_labeled_scalar_input(
    label_text: &'static str,
    sigil_color: ThemeToken,
) -> impl Scene {
    bsn! { @EditorNumberInput { @label_text: label_text, @sigil_color: sigil_color } }
}

#[cfg(test)]
#[path = "tests/widget.rs"]
mod tests;
