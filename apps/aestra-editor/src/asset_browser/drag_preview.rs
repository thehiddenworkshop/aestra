//! A non-interactive copy follows the pointer; the actual browser item never
//! leaves its layout slot. Releasing/cancelling removes the copy, not the asset.
use super::{panel, state::Kind};
use crate::{feathers::context_menu::pointer_position_in_node, *};
use bevy::picking::pointer::PointerId;

#[derive(Component)]
pub(super) struct DragPreview {
    root: Entity,
    origin: Entity,
    pointer: PointerId,
}

const OFFSET: Vec2 = Vec2::new(16.0, 18.0);

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn(
    commands: &mut Commands,
    root: Entity,
    origin: Entity,
    pointer: PointerId,
    position: Vec2,
    entry: &aestra_project::ProjectSourceEntry,
    assets: &AssetServer,
    localizer: &Localizer,
) -> Entity {
    let kind = Kind::of(entry);
    let position = position + OFFSET;
    let mut preview = commands.spawn((
        DragPreview {
            root,
            origin,
            pointer,
        },
        ChildOf(root),
        Pickable::IGNORE,
        OverrideClip,
        GlobalZIndex(280),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(position.x),
            top: Val::Px(position.y),
            width: Val::Px(216.0),
            height: Val::Px(54.0),
            padding: UiRect::all(Val::Px(8.0)),
            column_gap: Val::Px(10.0),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL),
        BorderColor::all(theme::ACCENT),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.5),
            Val::Px(0.0),
            Val::Px(3.0),
            Val::Px(2.0),
            Val::Px(7.0),
        ),
    ));
    let entity = preview.id();
    preview.with_children(|preview| {
        let icon = panel::icon(preview, assets, kind.icon(), 30.0);
        preview
            .commands()
            .entity(icon)
            .insert(bevy_resvg::prelude::SvgColor(panel::kind_color(kind)));
        preview
            .spawn((
                Pickable::IGNORE,
                Node {
                    flex_direction: FlexDirection::Column,
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    row_gap: Val::Px(3.0),
                    ..default()
                },
            ))
            .with_children(|labels| {
                for (label, size, color) in [
                    (panel::display_name(entry), 12.0, theme::TEXT),
                    (localizer.text(kind.label()), 10.0, theme::TEXT_MUTED),
                ] {
                    labels.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: FontSize::Px(size),
                            ..default()
                        },
                        TextColor(color),
                        TextLayout::no_wrap(),
                        Pickable::IGNORE,
                    ));
                }
            });
    });
    entity
}

pub(super) fn follow_pointer(
    event: On<Pointer<Drag>>,
    roots: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut previews: Query<(&DragPreview, &mut Node)>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    for (preview, mut node) in &mut previews {
        if preview.origin != event.entity || preview.pointer != event.pointer_id {
            continue;
        }
        let Ok((root, transform)) = roots.get(preview.root) else {
            continue;
        };
        // Pointer coordinates and ComputedNode are physical; Node offsets are
        // logical. Convert once against this window's root (including UI scale).
        let position = pointer_position_in_node(event.pointer_location.position, root, transform)
            * root.inverse_scale_factor()
            + OFFSET;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{backend::HitData, pointer::Location},
    };

    fn location(position: Vec2) -> Location {
        Location {
            target: NormalizedRenderTarget::None {
                width: 1000,
                height: 900,
            },
            position,
        }
    }

    #[test]
    fn drag_copy_tracks_pointer_at_display_scale_and_cleans_up_without_moving_source() {
        for scale in [1.0, 1.5, 2.0] {
            for view in [
                super::super::state::ViewMode::List,
                super::super::state::ViewMode::Grid,
            ] {
                let root = tempfile::tempdir().unwrap();
                let original = root.path().join("drag_me.aestra.ron");
                EffectAsset::new("Effect", 1.0).save_ron(&original).unwrap();
                let bytes = std::fs::read(&original).unwrap();
                let mut app = super::super::tests::browser_layout_app(
                    root.path(),
                    UVec2::new(1000, 900),
                    scale,
                );
                app.world_mut()
                    .resource_mut::<super::super::AssetBrowserState>()
                    .view = view;
                for _ in 0..4 {
                    app.update();
                }
                let (_, row) = super::super::tests::rows(&mut app)
                    .into_iter()
                    .next()
                    .unwrap();
                let start = app
                    .world()
                    .get::<UiGlobalTransform>(row)
                    .unwrap()
                    .translation
                    .trunc();
                let size = app.world().get::<ComputedNode>(row).unwrap().size();
                for escape in [false, true] {
                    app.world_mut().trigger(Pointer::new(
                        PointerId::Mouse,
                        location(start),
                        DragStart {
                            button: PointerButton::Primary,
                            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                        },
                        row,
                    ));
                    app.update();
                    let preview = {
                        let world = app.world_mut();
                        world
                            .query_filtered::<Entity, With<DragPreview>>()
                            .single(world)
                            .unwrap()
                    };
                    let preview_root = app.world().get::<ChildOf>(preview).unwrap().parent();
                    assert_ne!(preview_root, row);
                    assert!(
                        app.world().get::<ChildOf>(preview_root).is_none(),
                        "Preview must escape scrolling/panel clipping"
                    );
                    assert!(app.world().get::<OverrideClip>(preview).is_some());
                    let before = app.world().get::<Node>(preview).unwrap().clone();
                    let delta = Vec2::new(280.0, 125.0) * scale;
                    app.world_mut().trigger(Pointer::new(
                        PointerId::Mouse,
                        location(start + delta),
                        Drag {
                            button: PointerButton::Primary,
                            distance: delta,
                            delta,
                        },
                        row,
                    ));
                    // The new position is written immediately, with no interpolation/lag.
                    let after = app.world().get::<Node>(preview).unwrap();
                    let (Val::Px(x0), Val::Px(y0), Val::Px(x1), Val::Px(y1)) =
                        (before.left, before.top, after.left, after.top)
                    else {
                        panic!("logical pixel offsets required")
                    };
                    assert!((x1 - x0 - 280.0).abs() < 0.01);
                    assert!((y1 - y0 - 125.0).abs() < 0.01);
                    app.update();
                    assert_eq!(app.world().get::<ComputedNode>(row).unwrap().size(), size);
                    assert_eq!(
                        app.world()
                            .get::<UiGlobalTransform>(row)
                            .unwrap()
                            .translation
                            .trunc(),
                        start
                    );
                    let mut descendants = vec![preview];
                    while let Some(entity) = descendants.pop() {
                        if let Some(children) = app.world().get::<Children>(entity) {
                            descendants.extend(children.iter());
                        }
                        let picking = app.world().get::<Pickable>(entity).unwrap();
                        assert!(
                            !picking.is_hoverable && !picking.should_block_lower,
                            "Preview must not consume drops"
                        );
                    }
                    if escape {
                        app.init_resource::<ButtonInput<KeyCode>>();
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::Escape);
                    } else {
                        // Releasing over empty/invalid space must also restore the visual.
                        app.world_mut().trigger(Pointer::new(
                            PointerId::Mouse,
                            location(start + delta),
                            DragEnd {
                                button: PointerButton::Primary,
                                distance: delta,
                            },
                            row,
                        ));
                    }
                    app.update();
                    assert!(app.world().get_entity(preview).is_err());
                    assert!(
                        app.world()
                            .get::<super::super::payload::AssetPayload>(row)
                            .is_none()
                    );
                    assert_eq!(
                        app.world()
                            .get::<UiGlobalTransform>(row)
                            .unwrap()
                            .translation
                            .trunc(),
                        start
                    );
                    assert_eq!(std::fs::read(&original).unwrap(), bytes);
                }
            }
        }
    }
}
