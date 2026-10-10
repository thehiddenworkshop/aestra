//! Shipping prepass accessor; the candidate API is isolated in scene_depth_020.rs.
use bevy::{core_pipeline::prepass::ViewPrepassTextures, render::render_resource::TextureView};
pub(super) fn depth_view(prepass: &ViewPrepassTextures) -> Option<&TextureView> {
    prepass.depth_view()
}
