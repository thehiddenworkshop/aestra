//! Trigger a flash (Space), outline (O), dissolve (D), palette swap (P), or wave (W).
//! Press R to restore the original sprite or cancel a pending replacement.
//! Run with `cargo run --locked -p aestra-bevy --example sprite_hit_flash`.

use std::{path::PathBuf, sync::Arc};

use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraRuntimeStatus, AestraSettings, AssetId,
    CompiledEffectProject, EffectAsset, EffectCompiler, EffectPipelinePreparation,
    EffectPipelinePreparationStatus, EffectPlayer, MaterialId, MaterialParameterId,
    PlaybackHistoryPolicy, PresentationMode, PresentedEffect, SpriteTextureInput,
};
use aestra_core::material::{MaterialParameterValue, MaterialValue};
use aestra_project::ProjectAssetIndex;
use bevy::{camera::visibility::RenderLayers, ecs::query::QueryData, prelude::*, sprite::Anchor};

// Match the size authored in the effect's Appearance module.
const AUTHORED_SIZE: f32 = 64.0;
// Leave enough room for any Pixel Outline width the material supports.
const OUTLINE_GUTTER: f32 = 4.0;

#[derive(Resource)]
struct SpriteOverlays {
    flash: SpriteOverlay,
    outline: SpriteOverlay,
    dissolve: SpriteOverlay,
    palette_swap: SpriteOverlay,
    distortion: SpriteOverlay,
}

impl SpriteOverlays {
    fn load() -> Self {
        let index = ProjectAssetIndex::scan(asset_root());
        Self {
            flash: SpriteOverlay::load(&index, "sprite_hit_flash"),
            outline: SpriteOverlay::load(&index, "sprite_outline"),
            dissolve: SpriteOverlay::load(&index, "sprite_dissolve"),
            palette_swap: SpriteOverlay::load(&index, "sprite_palette_swap"),
            distortion: SpriteOverlay::load(&index, "sprite_distortion"),
        }
    }

    fn replacement_pipelines(&self) -> EffectPipelinePreparation {
        EffectPipelinePreparation::new(
            [&self.dissolve, &self.palette_swap, &self.distortion]
                .map(|effect| effect.template.fresh_instance()),
        )
    }
}

struct SpriteOverlay {
    project: Arc<CompiledEffectProject>,
    template: PresentedEffect,
    material: MaterialId,
    texture: AssetId,
    uv_origin: MaterialParameterId,
    uv_scale: MaterialParameterId,
    frame_bounds: Option<MaterialParameterId>,
    source_size: Option<MaterialParameterId>,
    sprite_tint: Option<MaterialParameterId>,
}

impl SpriteOverlay {
    fn load(index: &ProjectAssetIndex, name: &str) -> Self {
        let source =
            std::fs::read_to_string(asset_root().join(format!("effects/{name}.aestra.ron")))
                .expect("read the sample project's sprite overlay effect");
        let effect = EffectAsset::from_ron(&source).expect("parse the sprite overlay effect");
        let resolved = index
            .resolve_effect_project(&effect)
            .expect("resolve the overlay's material and preview texture");
        let project = Arc::new(
            EffectCompiler::default()
                .compile_resolved_project(&resolved)
                .expect("compile the sprite overlay effect"),
        );
        let program = &project.root.material_programs[0];
        let instance = &project.root.material_instances[0];
        let parameter = |name| {
            program
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .map(|parameter| parameter.id)
        };
        let MaterialParameterValue::Constant(MaterialValue::Texture2D(texture)) =
            instance.values[&parameter("Sprite Texture").expect("authored Sprite Texture")]
        else {
            panic!("Sprite Texture must bind a project texture for the editor preview");
        };
        Self {
            template: PresentedEffect::new(project.root.clone()),
            material: instance.id,
            texture,
            uv_origin: parameter("UV Origin").expect("authored UV Origin"),
            uv_scale: parameter("UV Scale").expect("authored UV Scale"),
            frame_bounds: parameter("Frame Bounds"),
            source_size: parameter("Source Size"),
            sprite_tint: parameter("Sprite Tint"),
            project,
        }
    }

    fn instantiate(
        &self,
        input: SpriteTextureInput,
        tint: Color,
        replaces_sprite: bool,
    ) -> (EffectPlayer, PresentedEffect, Transform) {
        let mut player = EffectPlayer::from_project(self.project.clone())
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        if replaces_sprite {
            // Start at the first simulation tick so the replacement has a particle.
            player.seek_frame(1);
        }
        let mut effect = self
            .template
            .fresh_instance()
            .with_texture_overrides([(self.texture, input.image)].into());
        let mut material = effect
            .material_binding(self.material)
            .expect("compiled sprite overlay material")
            .clone();
        let mut origin = input.uv_origin;
        let mut extent = input.uv_scale;
        let mut size = input.size;
        if let Some(parameter) = self.frame_bounds {
            let end = origin + extent;
            let min = origin.min(end);
            let max = origin.max(end);
            material
                .set_value(parameter, MaterialValue::Vec4([min.x, min.y, max.x, max.y]))
                .expect("Frame Bounds is a Vec4");
            // Cover the gutter in local units while retaining the source texel scale and flips.
            let padding = OUTLINE_GUTTER / input.source_size;
            origin -= extent * padding;
            extent *= Vec2::ONE + 2.0 * padding;
            size *= Vec2::ONE + 2.0 * padding;
        }
        material
            .set_value(self.uv_origin, MaterialValue::Vec2(origin.into()))
            .expect("UV Origin is a Vec2");
        material
            .set_value(self.uv_scale, MaterialValue::Vec2(extent.into()))
            .expect("UV Scale is a Vec2");
        if let Some(parameter) = self.source_size {
            material
                .set_value(parameter, MaterialValue::Vec2(input.source_size.into()))
                .expect("Source Size is a Vec2");
        }
        if let Some(parameter) = self.sprite_tint {
            material
                .set_value(
                    parameter,
                    MaterialValue::ColorSrgb(tint.to_srgba().to_f32_array()),
                )
                .expect("Sprite Tint is a Color");
        }
        effect.bind_material(self.material, material);

        let z = if replaces_sprite { 0.0 } else { 0.01 };
        let transform = Transform::from_translation(input.local_center.extend(z))
            .with_scale((size / AUTHORED_SIZE).extend(1.0));
        (player, effect, transform)
    }
}

#[derive(Component, Default)]
enum HostSprite {
    #[default]
    Original,
    ReplacementRequested(Replacement),
    Replaced(Option<RenderLayers>),
}

#[derive(Clone, Copy)]
enum Replacement {
    Dissolve,
    PaletteSwap,
    Distortion,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct HostSpriteSource {
    entity: Entity,
    sprite: &'static Sprite,
    anchor: &'static Anchor,
    layers: Option<&'static RenderLayers>,
    state: &'static mut HostSprite,
}

#[derive(Component)]
struct ActiveOverlay {
    restore_source_on_end: bool,
}

fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sample-project")
}

fn main() {
    App::new()
        .insert_resource(SpriteOverlays::load())
        .insert_resource(AestraSettings {
            presentation: PresentationMode::Gpu,
            ..default()
        })
        .insert_resource(ClearColor(Color::srgb_u8(14, 19, 29)))
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Aestra Sprite Effects | Space: Flash · O: Outline · D: Dissolve · P: Palette · W: Wave · R: Restore".into(),
                        ..default()
                    }),
                    ..default()
                }),
            AestraPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (retire_overlays, trigger_overlay).chain())
        .run();
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut atlases: ResMut<Assets<TextureAtlasLayout>>,
    overlays: Res<SpriteOverlays>,
) {
    commands.spawn((Camera2d, Msaa::Off, overlays.replacement_pipelines()));
    let layout = atlases.add(TextureAtlasLayout::from_grid(
        UVec2::splat(8),
        2,
        1,
        None,
        None,
    ));
    let mut sprite = Sprite::from_atlas_image(
        assets.load("textures/pixel_pulse.png"),
        TextureAtlas { layout, index: 1 },
    );
    sprite.custom_size = Some(Vec2::splat(144.0));
    sprite.flip_x = true;
    commands.spawn((
        HostSprite::default(),
        sprite,
        Anchor::BOTTOM_CENTER,
        Transform::from_xyz(0.0, -72.0, 0.0),
    ));
}

fn retire_overlays(
    mut commands: Commands,
    overlays: Query<(Entity, &EffectPlayer, &ActiveOverlay, &ChildOf)>,
    mut sources: Query<&mut HostSprite>,
) {
    for (entity, player, overlay, parent) in &overlays {
        if player.instance().time() >= player.effect().duration {
            if overlay.restore_source_on_end
                && let Ok(mut source) = sources.get_mut(parent.parent())
            {
                restore_source_sprite(&mut commands, parent.parent(), &mut source);
            }
            commands.entity(entity).despawn();
        }
    }
}

fn restore_source_sprite(commands: &mut Commands, entity: Entity, state: &mut HostSprite) {
    if let HostSprite::Replaced(layers) = std::mem::take(state) {
        let mut original = commands.entity(entity);
        if let Some(layers) = layers {
            original.insert(layers);
        } else {
            original.remove::<RenderLayers>();
        }
    }
}

fn trigger_overlay(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    source: Single<HostSpriteSource>,
    view: (
        Single<&EffectPipelinePreparation, With<Camera2d>>,
        Res<AestraRuntimeStatus>,
    ),
    textures: (Res<Assets<Image>>, Res<Assets<TextureAtlasLayout>>),
    overlays: Res<SpriteOverlays>,
    previous: Query<Entity, With<ActiveOverlay>>,
) {
    let mut source = source.into_inner();
    if keys.just_pressed(KeyCode::KeyR) {
        restore_source_sprite(&mut commands, source.entity, &mut source.state);
        for old in &previous {
            commands.entity(old).despawn();
        }
        return;
    }
    if matches!(*source.state, HostSprite::Replaced(_)) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyD) {
        *source.state = HostSprite::ReplacementRequested(Replacement::Dissolve);
    } else if keys.just_pressed(KeyCode::KeyP) {
        *source.state = HostSprite::ReplacementRequested(Replacement::PaletteSwap);
    } else if keys.just_pressed(KeyCode::KeyW) {
        *source.state = HostSprite::ReplacementRequested(Replacement::Distortion);
    }
    let replacement = match *source.state {
        HostSprite::ReplacementRequested(replacement) => Some(replacement),
        _ => None,
    };
    let overlay = if let Some(replacement) = replacement {
        let (preparation, runtime) = view;
        if runtime.active == ActiveBackend::Pending {
            return;
        }
        if runtime.active != ActiveBackend::Gpu {
            warn!("Sprite replacement requires native GPU presentation");
            *source.state = HostSprite::Original;
            return;
        }
        match preparation.status() {
            EffectPipelinePreparationStatus::Pending => return,
            EffectPipelinePreparationStatus::Failed(reason) => {
                warn!("Sprite replacement pipeline preparation failed: {reason}");
                *source.state = HostSprite::Original;
                return;
            }
            EffectPipelinePreparationStatus::Ready { .. } => match replacement {
                Replacement::Dissolve => &overlays.dissolve,
                Replacement::PaletteSwap => &overlays.palette_swap,
                Replacement::Distortion => &overlays.distortion,
            },
        }
    } else if keys.just_pressed(KeyCode::Space) {
        &overlays.flash
    } else if keys.just_pressed(KeyCode::KeyO) {
        &overlays.outline
    } else {
        return;
    };
    // The sprite image may still be loading when the host receives input.
    let (images, atlases) = textures;
    let Ok(input) = SpriteTextureInput::resolve(source.sprite, *source.anchor, &images, &atlases)
    else {
        return;
    };
    for old in &previous {
        commands.entity(old).despawn();
    }
    let (mut player, effect, transform) =
        overlay.instantiate(input, source.sprite.color, replacement.is_some());
    if matches!(replacement, Some(Replacement::PaletteSwap)) {
        // Hold the authored palette on the first populated frame until the host restores it.
        player.playing = false;
    }
    commands.spawn((
        (player, effect, transform),
        ActiveOverlay {
            restore_source_on_end: matches!(replacement, Some(Replacement::Distortion)),
        },
        ChildOf(source.entity),
        source.layers.cloned().unwrap_or_default(),
    ));
    if replacement.is_some() {
        // This entity owns only the source sprite draw. Keep its transform and inherited visibility
        // available to the effect child while suppressing the original draw on every camera.
        *source.state = HostSprite::Replaced(source.layers.cloned());
        commands.entity(source.entity).insert(RenderLayers::none());
    }
}

#[cfg(test)]
#[path = "tests/sprite_hit_flash.rs"]
mod tests;
