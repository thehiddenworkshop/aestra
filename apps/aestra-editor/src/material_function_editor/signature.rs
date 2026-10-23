//! Production function signature composition; custom-WESL signatures remain read-only.
#[cfg(test)]
#[path = "tests/signature.rs"]
mod tests;
use crate::feathers::{
    button::spawn_action_button,
    combo_box::{ComboOption, spawn_combo_control},
    text_input::spawn_text_input,
};
use crate::material_function_editor::signature_model::*;
use aestra_core::{
    MaterialFunctionId,
    material::{MaterialFunction, MaterialValue, MaterialValueType},
};
use bevy::prelude::*;

pub(super) fn default_text(value: &Option<MaterialValue>) -> String {
    match value {
        None => String::new(),
        Some(MaterialValue::Float(v)) => v.to_string(),
        Some(MaterialValue::Bool(v)) => v.to_string(),
        Some(MaterialValue::Vec2(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Vec3(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Vec4(v) | MaterialValue::ColorSrgb(v)) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        Some(MaterialValue::Texture2D(id)) => id.to_string(),
    }
}

fn label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: 13.0.into(),
            ..default()
        },
        TextColor(crate::theme::TEXT_MUTED),
    ));
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands, function: &MaterialFunction) {
    if function.custom_wesl.is_some() {
        return;
    }
    label(
        parent,
        "Function signature · unsaved drafts · edit the body in Material Graph",
    );
    spawn_text_input(
        parent,
        &function.name,
        "Function name",
        Field {
            owner: function.id,
            kind: FieldKind::Name,
        },
    );
    label(
        parent,
        "Inputs · empty default means required · vectors use comma-separated components",
    );
    for port in &function.inputs {
        port_controls(
            parent,
            function.id,
            Port::Input(port.id),
            &port.name,
            port.value_type,
        );
        if !matches!(port.value_type, MaterialValueType::Texture2D(_)) {
            let field = Field {
                owner: function.id,
                kind: FieldKind::Default(port.id),
            };
            if let Some(MaterialValue::Float(value)) = port.default {
                let entity = crate::feathers::number_input::spawn_scalar_input(
                    parent,
                    value,
                    "Input default",
                    field,
                );
                parent
                    .commands()
                    .entity(entity)
                    .insert(crate::feathers::number_input::OptionalNumberInput);
            } else {
                spawn_text_input(
                    parent,
                    &default_text(&port.default),
                    "Input default (empty = required)",
                    field,
                );
            }
        }
    }
    spawn_action_button(
        parent,
        "+ Input",
        Action {
            owner: function.id,
            kind: ActionKind::AddInput,
        },
        false,
    );
    label(
        parent,
        "Outputs · new outputs initially return zero; connect them in Material Graph",
    );
    for port in &function.outputs {
        port_controls(
            parent,
            function.id,
            Port::Output(port.id),
            &port.name,
            port.value_type,
        );
    }
    spawn_action_button(
        parent,
        "+ Output",
        Action {
            owner: function.id,
            kind: ActionKind::AddOutput,
        },
        false,
    );
}

fn port_controls(
    parent: &mut ChildSpawnerCommands,
    owner: MaterialFunctionId,
    port: Port,
    name: &str,
    ty: MaterialValueType,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            spawn_text_input(
                row,
                name,
                "Port name",
                Field {
                    owner,
                    kind: FieldKind::PortName(port),
                },
            );
            let options = [
                MaterialValueType::Float,
                MaterialValueType::Vec2,
                MaterialValueType::Vec3,
                MaterialValueType::Vec4,
                MaterialValueType::Color,
                MaterialValueType::Bool,
            ]
            .map(|value| ComboOption {
                label: format!("{value:?}"),
                selected: value == ty,
                action: Action {
                    owner,
                    kind: ActionKind::Type(port, value),
                },
            });
            spawn_combo_control(row, &format!("{ty:?}"), "Port type", &options, 128.0);
            spawn_action_button(
                row,
                "Remove",
                Action {
                    owner,
                    kind: ActionKind::Remove(port),
                },
                false,
            );
        });
}
