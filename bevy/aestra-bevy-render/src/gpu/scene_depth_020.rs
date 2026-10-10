//! Bevy 0.20 explicitly exposes the depth-only view (including depth/stencil formats).
use bevy::{core_pipeline::prepass::ViewPrepassTextures, render::render_resource::TextureView};
pub(super) fn depth_view(prepass: &ViewPrepassTextures) -> Option<&TextureView> {
    prepass.depth_only_view()
}
