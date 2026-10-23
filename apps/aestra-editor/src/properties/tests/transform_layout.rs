//! Measure the production transform rows, including their label and three axis wrappers.
use super::*;
use crate::feathers::{
    icon::SvgFile,
    number_input::{
        EditorNumberInput, NumberScrubPlugin,
        arrows::{NumberArrowsPlugin, NumberArrowsSet},
    },
    text_input::tests::{app_with, focus},
};
use bevy::{
    picking::{backend::HitData, hover::HoverMap, pointer::PointerId},
    text::TextLayoutInfo,
    ui_widgets::Button,
};

#[derive(Resource, Default)]
struct TestHover(Option<Entity>);

fn geometry(app: &App, entities: &[Entity]) -> Vec<(Vec2, Vec2)> {
    entities
        .iter()
        .map(|&entity| {
            (
                app.world().get::<ComputedNode>(entity).unwrap().size(),
                app.world()
                    .get::<UiGlobalTransform>(entity)
                    .unwrap()
                    .translation,
            )
        })
        .collect()
}

#[test]
fn narrow_transform_rows_keep_values_inside_their_editable_cells() {
    for width in [220.0, 320.0, 440.0] {
        for effect_clip in [false, true] {
            let mut app = app_with(|app| {
                app.init_asset::<SvgFile>()
                    .insert_resource(Localizer::new("en-US").unwrap())
                    .init_resource::<TestHover>()
                    .add_plugins((NumberArrowsPlugin, NumberScrubPlugin))
                    .add_systems(
                        Update,
                        (|test: Res<TestHover>, mut hover: ResMut<HoverMap>| {
                            hover.clear();
                            if let Some(entity) = test.0 {
                                hover.entry(PointerId::Mouse).or_default().insert(
                                    entity,
                                    HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                                );
                            }
                        })
                        .before(NumberArrowsSet),
                    );
            });
            let camera = app.world_mut().spawn(Camera2d).id();
            let root = app
                .world_mut()
                .spawn((
                    Node {
                        width: Val::Px(width),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    UiTargetCamera(camera),
                ))
                .id();
            app.world_mut()
                .commands()
                .entity(root)
                .with_children(|parent| {
                    if effect_clip {
                        spawn_effect_clip_transform_row(
                            parent,
                            EffectClipId::new(),
                            "Position",
                            "Position in effect-local units.",
                            EmitterNumberControl::Translation,
                        );
                    } else {
                        spawn_emitter_transform_row(
                            parent,
                            "Position",
                            "Position in effect-local units.",
                            EmitterNumberControl::Translation,
                        );
                    }
                });
            app.world_mut().flush();
            for _ in 0..4 {
                app.update();
            }
            let owners: Vec<_> = app
                .world_mut()
                .query_filtered::<Entity, With<EditorNumberInput>>()
                .iter(app.world())
                .collect();
            assert_eq!(owners.len(), 3);
            for &owner in &owners {
                app.world_mut().trigger(UpdateNumberInput {
                    entity: owner,
                    value: NumberInputValue::F32(-12.5),
                });
            }
            for _ in 0..8 {
                app.update();
            }
            let mut measured_entities = vec![root];
            let mut inputs = Vec::new();
            for &owner in &owners {
                let native = app
                    .world()
                    .get::<Children>(owner)
                    .unwrap()
                    .iter()
                    .find(|&child| {
                        app.world()
                            .get::<bevy::feathers::controls::FeathersNumberInput>(child)
                            .is_some()
                    })
                    .unwrap();
                let input = app
                    .world()
                    .get::<Children>(native)
                    .unwrap()
                    .iter()
                    .find(|&child| app.world().get::<EditableText>(child).is_some())
                    .unwrap();
                let node = app.world().get::<ComputedNode>(input).unwrap();
                let text = app.world().get::<TextLayoutInfo>(input).unwrap();
                assert_eq!(text.glyphs.len(), 5, "all digits must be shaped");
                assert!(
                    node.content_box().width() >= text.size.x,
                    "clipped transform at panel width {width} (clip={effect_clip}): cell={:?}, text={:?}",
                    node.size(),
                    text.size,
                );
                let owner_size = app.world().get::<ComputedNode>(owner).unwrap().size();
                let owner_center = app
                    .world()
                    .get::<UiGlobalTransform>(owner)
                    .unwrap()
                    .translation;
                let root_center = app
                    .world()
                    .get::<UiGlobalTransform>(root)
                    .unwrap()
                    .translation;
                assert!(
                    owner_center.x - owner_size.x * 0.5 >= root_center.x - width * 0.5 - 0.5
                        && owner_center.x + owner_size.x * 0.5 <= root_center.x + width * 0.5 + 0.5,
                    "axis control must remain inside the inspector at width {width}",
                );
                inputs.push(input);
                measured_entities.push(owner);
                measured_entities.extend(app.world().get::<Children>(owner).unwrap().iter());
            }
            let idle_geometry = geometry(&app, &measured_entities);
            for (&owner, &input) in owners.iter().zip(&inputs) {
                app.world_mut().resource_mut::<TestHover>().0 = Some(owner);
                for _ in 0..3 {
                    app.update();
                }
                assert_eq!(geometry(&app, &measured_entities), idle_geometry);
                let arrows: Vec<_> = app
                    .world()
                    .get::<Children>(owner)
                    .unwrap()
                    .iter()
                    .filter(|&child| app.world().get::<Button>(child).is_some())
                    .collect();
                assert_eq!(arrows.len(), 2);
                for &arrow in &arrows {
                    assert_eq!(
                        app.world().get::<Visibility>(arrow),
                        Some(&Visibility::Visible)
                    );
                }
                focus(&mut app, input);
                assert_eq!(geometry(&app, &measured_entities), idle_geometry);
                for &arrow in &arrows {
                    assert_eq!(
                        app.world().get::<Visibility>(arrow),
                        Some(&Visibility::Hidden)
                    );
                }
                app.world_mut()
                    .resource_mut::<bevy::input_focus::InputFocus>()
                    .clear();
                for _ in 0..3 {
                    app.update();
                }
            }
        }
    }
}
