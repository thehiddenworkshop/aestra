//! Actual signature controls; semantic handlers remain native-owned.
use super::*;
use crate::feathers::{number_input::ScrubbableNumber, text_input::tests::app_with};
use aestra_core::{
    MaterialFunctionInputId,
    material::{MaterialFunctionInput, MaterialTextureDescriptor},
};

fn graph() -> MaterialFunction {
    MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap()
}
fn mount(function: &MaterialFunction) -> App {
    let mut app = app_with(|app| {
        app.add_plugins(crate::feathers::number_input::NumberScrubPlugin);
    });
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| spawn(parent, function));
    app.world_mut().flush();
    for _ in 0..5 {
        app.update();
    }
    app
}
fn values() -> Vec<(MaterialValueType, Option<MaterialValue>)> {
    vec![
        (MaterialValueType::Float, None),
        (MaterialValueType::Float, Some(MaterialValue::Float(0.25))),
        (MaterialValueType::Bool, Some(MaterialValue::Bool(true))),
        (
            MaterialValueType::Vec2,
            Some(MaterialValue::Vec2([1.0, 2.0])),
        ),
        (
            MaterialValueType::Vec3,
            Some(MaterialValue::Vec3([1.0, 2.0, 3.0])),
        ),
        (
            MaterialValueType::Vec4,
            Some(MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0])),
        ),
        (
            MaterialValueType::Color,
            Some(MaterialValue::ColorSrgb([0.2, 0.3, 0.4, 0.5])),
        ),
        (
            MaterialValueType::Texture2D(MaterialTextureDescriptor {
                color_space: aestra_core::material::MaterialTextureColorSpace::SrgbColor,
                sampler: default(),
            }),
            None,
        ),
    ]
}
#[test]
fn real_signature_fields_keep_owner_port_identity_and_required_default_policy() {
    let mut function = graph();
    function.inputs = values()
        .into_iter()
        .enumerate()
        .map(|(i, (ty, default))| MaterialFunctionInput {
            id: MaterialFunctionInputId::new(),
            name: format!("Port {i}"),
            value_type: ty,
            default,
        })
        .collect();
    let mut app = mount(&function);
    let world = app.world_mut();
    let fields = world.query::<&Field>().iter(world).collect::<Vec<_>>();
    assert_eq!(
        fields.len(),
        1 + function.inputs.len() + function.outputs.len() + 7
    );
    assert!(fields.iter().all(|f| f.owner == function.id));
    assert_eq!(
        fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Name))
            .count(),
        1
    );
    for port in &function.inputs {
        assert_eq!(
            fields
                .iter()
                .filter(|f| matches!(f.kind, FieldKind::PortName(Port::Input(id)) if id == port.id))
                .count(),
            1
        );
        assert_eq!(
            fields
                .iter()
                .filter(|f| matches!(f.kind, FieldKind::Default(id) if id == port.id))
                .count(),
            usize::from(!matches!(port.value_type, MaterialValueType::Texture2D(_)))
        );
    }
    for port in &function.outputs {
        assert_eq!(
            fields
                .iter()
                .filter(
                    |f| matches!(f.kind, FieldKind::PortName(Port::Output(id)) if id == port.id)
                )
                .count(),
            1
        );
    }
    let controls = world
        .query::<(&Field, &ScrubbableNumber)>()
        .iter(world)
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    assert!(matches!(controls[0].0.kind, FieldKind::Default(id) if id == function.inputs[1].id));
    assert_eq!(
        (
            controls[0].1.value,
            controls[0].1.min,
            controls[0].1.max,
            controls[0].1.step
        ),
        (0.25, -f32::MAX, f32::MAX, 0.01)
    );
}

#[test]
fn signature_add_remove_and_type_actions_retain_typed_payloads_for_every_port() {
    let function = graph();
    let mut app = mount(&function);
    let world = app.world_mut();
    let actions = world.query::<&Action>().iter(world).collect::<Vec<_>>();
    assert!(actions.iter().all(|action| action.owner == function.id));
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::AddInput))
            .count(),
        1
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::AddOutput))
            .count(),
        1
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Remove(_)))
            .count(),
        function.inputs.len() + function.outputs.len()
    );
    for port in function
        .inputs
        .iter()
        .map(|p| Port::Input(p.id))
        .chain(function.outputs.iter().map(|p| Port::Output(p.id)))
    {
        let same = |candidate| match (candidate, port) {
            (Port::Input(a), Port::Input(b)) => a == b,
            (Port::Output(a), Port::Output(b)) => a == b,
            _ => false,
        };
        for ty in [
            MaterialValueType::Float,
            MaterialValueType::Vec2,
            MaterialValueType::Vec3,
            MaterialValueType::Vec4,
            MaterialValueType::Color,
            MaterialValueType::Bool,
        ] {
            assert_eq!(
                actions
                    .iter()
                    .filter(|a| matches!(a.kind, ActionKind::Type(p, t) if same(p) && t == ty))
                    .count(),
                1
            );
        }
    }
    assert_eq!(world.query::<&Field>().iter(world).filter(|f|
        matches!(f.kind, FieldKind::PortName(Port::Output(id)) if id == function.outputs[0].id)).count(), 1);
}

#[test]
fn required_and_typed_defaults_display_the_saved_values_and_never_add_texture_defaults() {
    let cases = [
        (None, ""),
        (Some(MaterialValue::Float(2.5)), "2.5"),
        (Some(MaterialValue::Bool(false)), "false"),
        (Some(MaterialValue::Vec2([1.0, 2.0])), "1, 2"),
        (Some(MaterialValue::Vec3([1.0, 2.0, 3.0])), "1, 2, 3"),
        (
            Some(MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0])),
            "1, 2, 3, 4",
        ),
        (
            Some(MaterialValue::ColorSrgb([0.2, 0.3, 0.4, 0.5])),
            "0.2, 0.3, 0.4, 0.5",
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(default_text(&value), expected);
    }
    let mut function = graph();
    function.inputs[0].default = Some(MaterialValue::Float(3.5));
    let mut app = mount(&function);
    let world = app.world_mut();
    let (entity, _) = world
        .query::<(Entity, &Field)>()
        .iter(world)
        .find(|(_, f)| matches!(f.kind, FieldKind::Default(id) if id == function.inputs[0].id))
        .unwrap();
    let native = world
        .get::<Children>(entity)
        .unwrap()
        .iter()
        .find(|&e| {
            world
                .get::<bevy::feathers::controls::FeathersNumberInput>(e)
                .is_some()
        })
        .unwrap();
    let children = world.get::<Children>(native).unwrap();
    assert!(children.iter().any(|e| {
        world
            .get::<bevy::text::EditableText>(e)
            .is_some_and(|t| t.value() == "3.5")
    }));
}

#[test]
fn custom_wesl_signature_is_read_only_and_creates_no_edit_fields_or_actions() {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    let mut app = mount(&function);
    assert_eq!(
        app.world_mut().query::<&Field>().iter(app.world()).count(),
        0
    );
    assert_eq!(
        app.world_mut().query::<&Action>().iter(app.world()).count(),
        0
    );
}
