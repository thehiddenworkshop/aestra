use super::*;

#[test]
fn atlas_crop_flips_size_and_anchor_match_bevy_sprite_coordinates() {
    let mut images = Assets::default();
    let mut image = Image::default();
    image.resize(bevy::render::render_resource::Extent3d {
        width: 16,
        height: 8,
        depth_or_array_layers: 1,
    });
    let image = images.add(image);
    let mut atlases = Assets::default();
    let layout = atlases.add(TextureAtlasLayout::from_grid(
        UVec2::splat(8),
        2,
        1,
        None,
        None,
    ));
    let mut sprite = Sprite::from_atlas_image(image.clone(), TextureAtlas { layout, index: 1 });
    sprite.rect = Some(Rect::new(2.0, 1.0, 6.0, 3.0));
    sprite.custom_size = Some(Vec2::new(40.0, 24.0));
    sprite.flip_x = true;
    sprite.flip_y = true;

    let anchor = Anchor::BOTTOM_LEFT;
    let input = SpriteTextureInput::resolve(&sprite, anchor, &images, &atlases).unwrap();
    assert_eq!(input.image, image);
    assert_eq!(input.source_size, Vec2::new(4.0, 2.0));
    assert_eq!(input.size, Vec2::new(40.0, 24.0));
    assert_eq!(input.local_center, Vec2::new(20.0, 12.0));
    assert_eq!(input.uv_origin, Vec2::new(14.0 / 16.0, 3.0 / 8.0));
    assert_eq!(input.uv_scale, Vec2::new(-4.0 / 16.0, -2.0 / 8.0));

    let sample_uv = Vec2::new(0.25, 0.75);
    let local_point =
        input.local_center + (sample_uv - Vec2::splat(0.5)) * input.size * Vec2::new(1.0, -1.0);
    let bevy_pixel = sprite
        .compute_pixel_space_point(local_point, anchor, &images, &atlases)
        .unwrap();
    assert_eq!(
        (input.uv_origin + sample_uv * input.uv_scale) * Vec2::new(16.0, 8.0),
        bevy_pixel
    );

    sprite.texture_atlas.as_mut().unwrap().index = 2;
    assert_eq!(
        SpriteTextureInput::resolve(&sprite, anchor, &images, &atlases).unwrap_err(),
        SpriteTextureInputError::AtlasFrameUnavailable
    );
}
