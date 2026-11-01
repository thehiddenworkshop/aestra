//! Opt-in water receiver shared by editor previews and host-owned environments.
//! Uses normal PBR clustered lighting; never samples or advances a VFX instance.
use bevy::{
    asset::embedded_asset,
    material::OpaqueRendererMethod,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use serde::{Deserialize, Serialize};

/// Surface state controls wave slopes and microfacet roughness, not VFX quality.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeaState {
    Calm,
    #[default]
    Rough,
}

impl SeaState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Rough => "rough",
        }
    }
}

/// Eight independently moving wave bands and a dim host-supplied sky radiance.
/// Normal variations are shaded in the fragment stage; there is no displaced
/// silhouette or independent water simulation. Optional planar capture is owned
/// by `preview::planar`, independently of this surface's playback clock.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExtension {
    /// Effect seconds, wave slope scale, reflection enabled, reserved.
    #[uniform(100)]
    parameters: Vec4,
    /// Linear radiance of an untextured sky. Set to zero when supplying an
    /// environment map to avoid adding a second environment contribution.
    #[uniform(100)]
    pub sky_radiance: Vec4,
    #[uniform(100)]
    reflection_clip: Mat4,
    #[uniform(100)]
    reflection_texel: Vec4,
    #[texture(101)]
    #[sampler(102)]
    reflection_image: Option<Handle<Image>>,
}

impl WaterExtension {
    pub(super) fn reflection_matches(
        &self,
        image: &Handle<Image>,
        clip: Mat4,
        size: UVec2,
    ) -> bool {
        self.parameters.z == 1.0
            && self.reflection_image.as_ref() == Some(image)
            && self.reflection_clip == clip
            && self.reflection_texel.xy() == size.as_vec2().recip()
    }
    pub(super) fn set_reflection(&mut self, image: Handle<Image>, clip: Mat4, size: UVec2) {
        self.reflection_image = Some(image);
        self.reflection_clip = clip;
        self.reflection_texel = size
            .as_vec2()
            .recip()
            .extend(clip.row(0).truncate().length())
            .extend(clip.row(1).truncate().length());
        self.parameters.z = 1.0;
    }
    pub(super) fn clear_reflection(&mut self) {
        self.parameters.z = 0.0;
        self.reflection_image = None;
    }
    pub fn time(&self) -> f32 {
        self.parameters.x
    }

    /// The host owns time: pause/restart/seek are reflected exactly. Non-finite
    /// input is ignored; identical values avoid changing the material asset.
    pub fn set_time(&mut self, seconds: f32) -> bool {
        if !seconds.is_finite() || seconds < 0.0 || self.parameters.x == seconds {
            return false;
        }
        self.parameters.x = seconds;
        true
    }
}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://aestra_bevy_render/preview/water.wesl".into()
    }

    fn enable_shadows() -> bool {
        false
    }
}

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

pub fn material(state: SeaState) -> WaterMaterial {
    let (slope, roughness) = match state {
        SeaState::Calm => (1.0, 0.055),
        SeaState::Rough => (4.0, 0.13),
    };
    // StandardMaterial uses F0 = 0.16 * reflectance^2. Water/air at IOR
    // 1.333 gives F0 ~= 0.02037 rather than the default plastic's 0.04.
    let f0 = ((1.333_f32 - 1.0) / (1.333 + 1.0)).powi(2);
    ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgb(0.008, 0.018, 0.028),
            reflectance: (f0 / 0.16).sqrt(),
            perceptual_roughness: roughness,
            metallic: 0.0,
            opaque_render_method: OpaqueRendererMethod::Forward,
            ..default()
        },
        extension: WaterExtension {
            parameters: Vec4::new(0.0, slope, 0.0, 0.0),
            sky_radiance: Vec4::new(0.00035, 0.0009, 0.0018, 1.0),
            reflection_clip: Mat4::IDENTITY,
            reflection_texel: Vec4::ZERO,
            reflection_image: None,
        },
    }
}

pub struct WaterMaterialPlugin;

impl Plugin for WaterMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
        app.add_plugins(super::planar::PlanarReflectionPlugin);
    }
}

#[cfg(test)]
#[path = "tests/water.rs"]
mod tests;
