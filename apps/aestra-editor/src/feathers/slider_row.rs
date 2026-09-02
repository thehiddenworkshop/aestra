//! A compact bounded slider paired with a precise Feathers number input.
//!
//! The widget owns layout and numeric metadata. The panel that spawns it remains responsible for
//! previewing and committing domain values.

use crate::{feathers::number_input, ui_shell};
use bevy::{
    prelude::*,
    ui_widgets::{SliderPrecision, SliderStep},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SliderRowProps {
    pub(crate) value: f32,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) step: f32,
    pub(crate) precision: i32,
}

impl SliderRowProps {
    pub(crate) fn new(value: f32, min: f32, max: f32, step: f32) -> Option<Self> {
        if !value.is_finite()
            || !min.is_finite()
            || !max.is_finite()
            || !step.is_finite()
            || min >= max
            || step <= 0.0
        {
            return None;
        }
        Some(Self {
            value: value.clamp(min, max),
            min,
            max,
            step,
            precision: number_input::decimal_places(step) as i32,
        })
    }
}

/// Links the slider to its precise text input so domain observers can keep both views in sync.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SliderNumberInputPair {
    pub(crate) input: Entity,
}

pub(crate) fn spawn_slider_input_pair(
    parent: &mut ChildSpawnerCommands,
    props: SliderRowProps,
    slider_bundle: impl Bundle,
    input_bundle: impl Bundle,
) {
    let mut slider = Entity::PLACEHOLDER;
    let mut input = Entity::PLACEHOLDER;

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|pair| {
            slider = pair
                .spawn_empty()
                .apply_scene(ui_shell::feathers_slider(props.value, props.min, props.max))
                .insert((
                    SliderStep(props.step),
                    SliderPrecision(props.precision),
                    slider_bundle,
                ))
                .id();

            pair.spawn(Node {
                width: Val::Px(72.0),
                min_width: Val::Px(58.0),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|wrapper| {
                input = wrapper
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_scalar_input())
                    .insert(input_bundle)
                    .id();
            });
        });

    parent
        .commands()
        .entity(slider)
        .insert(SliderNumberInputPair { input });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_slider_metadata_is_validated_and_clamped() {
        let props = SliderRowProps::new(12.0, 0.0, 10.0, 0.1).unwrap();
        assert_eq!(props.value, 10.0);
        assert_eq!(props.precision, 1);
        assert!(SliderRowProps::new(0.0, 1.0, 1.0, 0.1).is_none());
        assert!(SliderRowProps::new(0.0, 0.0, 1.0, 0.0).is_none());
        assert!(SliderRowProps::new(f32::NAN, 0.0, 1.0, 0.1).is_none());
    }
}
