use bevy::{
    prelude::*,
    sprite::{Anchor, SpriteImageMode},
};

/// Texture and local geometry of a Bevy sprite at the time it is sampled.
///
/// Remap a material's top-left-origin local UV with `uv_origin + uv * uv_scale`.
/// Resolve again when the host changes the frame, crop, or flips.
#[derive(Debug, Clone)]
pub struct SpriteTextureInput {
    pub image: Handle<Image>,
    pub uv_origin: Vec2,
    /// Signed UV extent, including the sprite's flips.
    pub uv_scale: Vec2,
    /// Width and height of the selected source rectangle in texture pixels.
    pub source_size: Vec2,
    /// Sprite dimensions in local world units, including any custom size.
    pub size: Vec2,
    /// Center of the quad relative to its entity origin, accounting for its anchor.
    pub local_center: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SpriteTextureInputError {
    #[error("the sprite image is unavailable")]
    ImageUnavailable,
    #[error("the sprite atlas layout or frame is unavailable")]
    AtlasFrameUnavailable,
    #[error("a sprite texture input requires SpriteImageMode::Auto")]
    UnsupportedImageMode,
    #[error("the sprite texture rectangle must be nonempty and inside the image")]
    InvalidTextureRect,
    #[error("the sprite needs finite, positive dimensions and a finite local center")]
    InvalidSpriteGeometry,
}

impl SpriteTextureInput {
    /// Resolve an ordinary image sprite. Call again after pending image or atlas assets load.
    pub fn resolve(
        sprite: &Sprite,
        anchor: Anchor,
        images: &Assets<Image>,
        atlases: &Assets<TextureAtlasLayout>,
    ) -> Result<Self, SpriteTextureInputError> {
        use SpriteTextureInputError::*;

        if !matches!(sprite.image_mode, SpriteImageMode::Auto) {
            return Err(UnsupportedImageMode);
        }
        let image_size = images
            .get(&sprite.image)
            .ok_or(ImageUnavailable)?
            .size()
            .as_vec2();
        let atlas_rect = sprite
            .texture_atlas
            .as_ref()
            .map(|atlas| {
                atlas
                    .texture_rect(atlases)
                    .map(|rect| rect.as_rect())
                    .ok_or(AtlasFrameUnavailable)
            })
            .transpose()?;
        let rect = match (atlas_rect, sprite.rect) {
            (Some(atlas), Some(crop)) => Rect {
                min: atlas.min + crop.min,
                max: atlas.min + crop.max,
            },
            (Some(rect), None) | (None, Some(rect)) => rect,
            (None, None) => Rect {
                min: Vec2::ZERO,
                max: image_size,
            },
        };
        let extent = rect.size();
        if !rect.min.is_finite()
            || !rect.max.is_finite()
            || !rect.min.cmpge(Vec2::ZERO).all()
            || !rect.max.cmple(image_size).all()
            || !extent.cmpgt(Vec2::ZERO).all()
        {
            return Err(InvalidTextureRect);
        }

        let size = sprite.custom_size.unwrap_or(extent);
        let local_center = -anchor.as_vec() * size;
        if !size.is_finite() || !size.cmpgt(Vec2::ZERO).all() || !local_center.is_finite() {
            return Err(InvalidSpriteGeometry);
        }

        let flips = BVec2::new(sprite.flip_x, sprite.flip_y);
        let start = Vec2::select(flips, rect.max, rect.min);
        let end = Vec2::select(flips, rect.min, rect.max);
        Ok(Self {
            image: sprite.image.clone(),
            uv_origin: start / image_size,
            uv_scale: (end - start) / image_size,
            source_size: extent,
            size,
            local_center,
        })
    }
}

#[cfg(test)]
#[path = "tests/sprite_texture.rs"]
mod tests;
