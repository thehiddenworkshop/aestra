//! Cursor ownership follows the shared payload, regardless of browser source or drop target.
use super::AssetPayload;
use crate::*;
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CustomCursor, CustomCursorImage},
};

pub(crate) struct AssetDragCursorPlugin;

#[derive(Resource, Default)]
struct DragCursor {
    owner: Option<Entity>,
    previous: Option<EntityCursor>,
    applied: Option<EntityCursor>,
}

#[derive(Resource, Default)]
pub(crate) struct ClosedHandCursor(pub(crate) Option<EntityCursor>);

// An embedded cursor avoids an asynchronous asset load on the first drag.
// Windows maps the system Grabbing cursor to move arrows, not a closed hand.
fn closed_hand_image() -> Image {
    let rows = [
        "",
        "",
        "",
        "",
        "",
        "        ## ## ##",
        "       #WW#WW#WW##",
        "       #WW#WW#WW#W#",
        "       #WW#WW#WW#W#",
        "       #WWWWWWWWWW#",
        "    ###WWWWWWWWWWW#",
        "   #WWW#WWWWWWWWWW#",
        "   #WWWWWWWWWWWWWW#",
        "    #WWWWWWWWWWWWW#",
        "     #WWWWWWWWWWWW#",
        "      #WWWWWWWWWW#",
        "       #WWWWWWWWW#",
        "       #WWWWWWWWW#",
        "        #WWWWWWW#",
        "        #########",
    ];
    let mut data = vec![0; 24 * 24 * 4];
    for (y, row) in rows.iter().enumerate() {
        for (x, pixel) in row.bytes().enumerate() {
            let rgba = match pixel {
                b'W' => [255, 255, 255, 255],
                b'#' => [24, 24, 24, 255],
                _ => [0, 0, 0, 0],
            };
            let offset = (y * 24 + x) * 4;
            data[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
    Image::new(
        Extent3d {
            width: 24,
            height: 24,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
}

impl Plugin for AssetDragCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverrideCursor>()
            .init_resource::<DragCursor>()
            .init_resource::<ClosedHandCursor>()
            .add_observer(begin)
            .add_observer(remove)
            .add_observer(end)
            .add_observer(cancel);
    }
}

fn begin(
    event: On<Add, AssetPayload>,
    mut drag: ResMut<DragCursor>,
    mut cursor: ResMut<OverrideCursor>,
    mut hand: ResMut<ClosedHandCursor>,
    mut images: ResMut<Assets<Image>>,
) {
    if drag.owner.is_none() {
        drag.previous = cursor.0.clone();
    }
    drag.owner = Some(event.entity);
    let hand = hand.0.get_or_insert_with(|| {
        EntityCursor::Custom(CustomCursor::Image(CustomCursorImage {
            handle: images.add(closed_hand_image()),
            hotspot: (11, 11),
            ..default()
        }))
    });
    drag.applied = Some(hand.clone());
    cursor.0 = drag.applied.clone();
}

fn restore(drag: &mut DragCursor, cursor: &mut OverrideCursor, origin: Entity) {
    if drag.owner != Some(origin) {
        return;
    }
    // A newer interaction may have taken over the override; do not clear its cursor.
    if cursor.0 == drag.applied {
        cursor.0 = drag.previous.take();
    }
    *drag = default();
}

fn remove(
    event: On<Remove, AssetPayload>,
    mut drag: ResMut<DragCursor>,
    mut cursor: ResMut<OverrideCursor>,
) {
    restore(&mut drag, &mut cursor, event.entity);
}

fn end(
    event: On<Pointer<DragEnd>>,
    mut drag: ResMut<DragCursor>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if event.button == PointerButton::Primary {
        restore(&mut drag, &mut cursor, event.original_event_target());
    }
}

fn cancel(
    event: On<Pointer<bevy::picking::events::Cancel>>,
    mut drag: ResMut<DragCursor>,
    mut cursor: ResMut<OverrideCursor>,
) {
    restore(&mut drag, &mut cursor, event.original_event_target());
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
    };

    #[test]
    fn payload_cursor_releases_on_end_cancel_removal_and_despawn() {
        for finish in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let catalog = ProjectEffectCatalog::scan(root.path());
            let payload = AssetPayload::capture(&catalog, catalog.content().source_tree().root());
            let mut app = App::new();
            app.init_resource::<Assets<Image>>();
            app.add_plugins(AssetDragCursorPlugin);
            let previous = Some(EntityCursor::System(SystemCursorIcon::Crosshair));
            app.world_mut().resource_mut::<OverrideCursor>().0 = previous.clone();
            let origin = app.world_mut().spawn(payload).id();
            assert_eq!(
                app.world().resource::<OverrideCursor>().0,
                app.world().resource::<ClosedHandCursor>().0
            );
            let location = Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            };
            match finish {
                0 => app.world_mut().trigger(Pointer::new(
                    PointerId::Mouse,
                    location,
                    DragEnd {
                        button: PointerButton::Primary,
                        distance: Vec2::ONE,
                    },
                    origin,
                )),
                1 => app.world_mut().trigger(Pointer::new(
                    PointerId::Mouse,
                    location,
                    bevy::picking::events::Cancel {
                        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                    },
                    origin,
                )),
                2 => {
                    app.world_mut().entity_mut(origin).remove::<AssetPayload>();
                }
                _ => {
                    app.world_mut().despawn(origin);
                }
            }
            assert_eq!(app.world().resource::<OverrideCursor>().0, previous);
        }
    }

    #[test]
    fn clearing_an_old_payload_or_another_cursor_owner_does_not_reset_the_new_cursor() {
        let root = tempfile::tempdir().unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = AssetPayload::capture(&catalog, catalog.content().source_tree().root());
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        app.add_plugins(AssetDragCursorPlugin);
        let old = app.world_mut().spawn(payload.clone()).id();
        let current = app.world_mut().spawn(payload).id();
        app.world_mut().despawn(old);
        assert_eq!(
            app.world().resource::<OverrideCursor>().0,
            app.world().resource::<ClosedHandCursor>().0
        );
        app.world_mut().resource_mut::<OverrideCursor>().0 =
            Some(EntityCursor::System(SystemCursorIcon::EwResize));
        app.world_mut().despawn(current);
        assert_eq!(
            app.world().resource::<OverrideCursor>().0,
            Some(EntityCursor::System(SystemCursorIcon::EwResize))
        );
    }

    #[test]
    fn closed_hand_is_a_cpu_readable_rgba_cursor_with_transparent_background() {
        let image = closed_hand_image();
        assert_eq!(image.width(), 24);
        assert_eq!(image.height(), 24);
        assert_eq!(image.asset_usage, RenderAssetUsages::MAIN_WORLD);
        let data = image.data.unwrap();
        assert_eq!(data.len(), 24 * 24 * 4);
        assert_eq!(&data[..4], &[0, 0, 0, 0]);
        assert_eq!(
            &data[(11 * 24 + 11) * 4..(11 * 24 + 11) * 4 + 4],
            &[255, 255, 255, 255]
        );
        assert!(data.chunks_exact(4).any(|pixel| pixel == [24, 24, 24, 255]));
    }
}
