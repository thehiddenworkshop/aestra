//! Texture/atlas integration oracle, not a smoke-art asset or public OIT default.
use super::hdr::{camera_hdr, center, healthy, image_difference, replace_blue};
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_configured;
use aestra_bevy::{
    AssetDefinition, BlendMode, ExperimentalOitDiagnostics, FlipbookDefinition, FlipbookTimeSource,
    MaterialDefinition, MaterialParameterId, MaterialProperties, RendererInstance,
    RendererProperties, material::MaterialValue,
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    render::render_resource::{Extent3d, TextureDimension},
};

const TEX: &str = "a3574a00-0000-4000-8000-000000f81810";
const ALTERNATE: &str = "a3574a00-0000-4000-8000-000000f81811";
const TEXTURE_PARAMETER: &str = "a3574a00-0000-4000-8000-000000f81801";
const GAIN_PARAMETER: &str = "a3574a00-0000-4000-8000-000000f81802";

#[derive(Clone, Copy, Debug)]
enum PathKind {
    SemanticSprite,
    SemanticFlipbook,
    LegacyFlipbook,
}

impl PathKind {
    fn semantic(self) -> bool {
        !matches!(self, Self::LegacyFlipbook)
    }
    fn frames(self) -> u32 {
        if matches!(self, Self::SemanticSprite) {
            1
        } else {
            3
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::SemanticSprite => "semantic-sprite",
            Self::SemanticFlipbook => "semantic-flipbook",
            Self::LegacyFlipbook => "legacy-flipbook",
        }
    }
}

fn texture_program() -> MaterialProgram {
    // Sample RGB (including values above SDR after gain), sampled alpha, and native scene
    // lighting share the normal material compiler. The white test light's red irradiance
    // channel deliberately supplies a scalar gain; this is not a scattering model.
    MaterialProgram::from_ron(&format!(r#"(
        id:"a3574a00-0000-4000-8000-000000f81800",schema_version:1,name:"Texture atlas oracle",domain:Sprite,
        render_state_policy:(default:(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None),
        allowed:[(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None)]),
        parameters:[
        (id:"{TEXTURE_PARAMETER}",name:"Atlas",value_type:Texture2D((color_space:SrgbColor,sampler:(filter:Linear,mip_filter:Linear,address_u:ClampToEdge,address_v:ClampToEdge))),evaluation_domain:Instance,default:Some(Texture2D("{TEX}"))),
        (id:"{GAIN_PARAMETER}",name:"Radiance gain",value_type:Float,evaluation_domain:Instance,default:Some(Float(4.0)))],
        expressions:[
        (id:"a3574a00-0000-4000-8000-000000f81820",kind:Parameter("{TEXTURE_PARAMETER}")),
        (id:"a3574a00-0000-4000-8000-000000f81821",kind:Input(Uv0)),
        (id:"a3574a00-0000-4000-8000-000000f81822",kind:SampleTexture(texture:"a3574a00-0000-4000-8000-000000f81820",uv:"a3574a00-0000-4000-8000-000000f81821")),
        (id:"a3574a00-0000-4000-8000-000000f81823",kind:ExtractComponent(value:"a3574a00-0000-4000-8000-000000f81822",component:W)),
        (id:"a3574a00-0000-4000-8000-000000f81824",kind:Input(ParticleOpacity)),
        (id:"a3574a00-0000-4000-8000-000000f81825",kind:Multiply("a3574a00-0000-4000-8000-000000f81823","a3574a00-0000-4000-8000-000000f81824")),
        (id:"a3574a00-0000-4000-8000-000000f81826",kind:Parameter("{GAIN_PARAMETER}")),
        (id:"a3574a00-0000-4000-8000-000000f81827",kind:Input(ScenePointIrradiance)),
        (id:"a3574a00-0000-4000-8000-000000f81828",kind:ExtractComponent(value:"a3574a00-0000-4000-8000-000000f81827",component:X)),
        (id:"a3574a00-0000-4000-8000-000000f81829",kind:Constant(Float(0.02))),
        (id:"a3574a00-0000-4000-8000-000000f81830",kind:Multiply("a3574a00-0000-4000-8000-000000f81828","a3574a00-0000-4000-8000-000000f81829")),
        (id:"a3574a00-0000-4000-8000-000000f81831",kind:Add("a3574a00-0000-4000-8000-000000f81826","a3574a00-0000-4000-8000-000000f81830")),
        (id:"a3574a00-0000-4000-8000-000000f81832",kind:Multiply("a3574a00-0000-4000-8000-000000f81822","a3574a00-0000-4000-8000-000000f81831"))],
        outputs:(color:"a3574a00-0000-4000-8000-000000f81832",alpha:"a3574a00-0000-4000-8000-000000f81825"))"#)).unwrap()
}

fn fixture(kind: PathKind, count: u32) -> Arc<CompiledEffectProject> {
    let shape = if count == 1 {
        EmitterShape::Point
    } else {
        EmitterShape::Box {
            half_extents: [0.001, 0.001, 4.0],
        }
    };
    sheet_configured(count, shape, 0.0, texture_program(), |effect| {
        effect.assets.clear();
        for (id, name) in [(TEX, "atlas"), (ALTERNATE, "alternate")] {
            let mut asset = AssetDefinition::texture(name, format!("{name}.png"));
            asset.id = id.parse().unwrap();
            effect.assets.push(asset);
        }
        if !kind.semantic() {
            let mut material = MaterialDefinition::sprite("Legacy atlas", BlendMode::Alpha, 0.5);
            material.id = effect.emitters[0].renderers[0].material;
            let MaterialProperties::Sprite { texture, .. } = &mut material.properties;
            *texture = Some(TEX.parse().unwrap());
            effect.materials = vec![material];
            effect.material_instances.clear();
        }
        if kind.frames() > 1 {
            let mut atlas = FlipbookDefinition::grid("oracle", TEX.parse().unwrap(), 3, 1, 2.0);
            atlas.id = "a3574a00-0000-4000-8000-000000f81812".parse().unwrap();
            let mut renderer =
                RendererInstance::flipbook(effect.emitters[0].renderers[0].material, atlas.id);
            if let RendererProperties::Flipbook { time_source, .. } = &mut renderer.properties {
                *time_source = FlipbookTimeSource::EffectTime;
            }
            effect.emitters[0].renderers = vec![renderer];
            effect.flipbooks = vec![atlas];
        }
    })
}

fn texel(frame: u32, variant: u32) -> [u8; 4] {
    let selected = (frame + variant) % 3;
    let mut p = [32, 32, 32, [64, 128, 192][selected as usize]];
    p[selected as usize] = 255;
    p
}

fn atlas(kind: PathKind, variant: u32, transparent: bool) -> Image {
    let width = 32 * kind.frames();
    let mut bytes = Vec::with_capacity((width * 32 * 4) as usize);
    for y in 0..32 {
        for x in 0..width {
            let mut p = texel(x / 32, variant);
            // A large transparent notch survives differing perspective scales. A nonzero
            // RGB under zero alpha catches incorrect RGB-only sampling/premultiplication.
            if transparent || ((10..22).contains(&(x % 32)) && y < 10) {
                p[3] = 0;
            }
            if y > 25 {
                p[3] /= 2;
            }
            bytes.extend_from_slice(&p);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width,
            height: 32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

fn bind_images(app: &mut App, entity: Entity, primary: &Handle<Image>, alternate: &Handle<Image>) {
    let presented = app
        .world()
        .get::<PresentedEffect>(entity)
        .cloned()
        .unwrap_or_else(|| {
            PresentedEffect::new(
                app.world()
                    .get::<EffectPlayer>(entity)
                    .unwrap()
                    .effect()
                    .clone(),
            )
        })
        .with_texture_overrides(BTreeMap::from([
            (TEX.parse().unwrap(), primary.clone()),
            (ALTERNATE.parse().unwrap(), alternate.clone()),
        ]));
    app.world_mut().entity_mut(entity).insert(presented);
}

fn override_material(app: &mut App, entity: Entity, alternate: bool, gain: f32) {
    let mut presented = app.world_mut().get_mut::<PresentedEffect>(entity).unwrap();
    let emitter = &presented.effect().emitters[0];
    let material = emitter.renderers[0].material;
    let mut binding = presented
        .material_binding_for_emitter(material, emitter.source)
        .unwrap()
        .clone();
    let compiled = binding.program().clone();
    binding
        .set_value(
            TEXTURE_PARAMETER.parse::<MaterialParameterId>().unwrap(),
            MaterialValue::Texture2D(if alternate { ALTERNATE } else { TEX }.parse().unwrap()),
        )
        .unwrap();
    binding
        .set_value(GAIN_PARAMETER.parse().unwrap(), MaterialValue::Float(gain))
        .unwrap();
    assert!(Arc::ptr_eq(&compiled, binding.program()));
    presented.bind_material(material, binding);
}

fn expected_center(
    depths: &[f32],
    opposite: bool,
    frame: u32,
    variants: [u32; 2],
    gain: f32,
    stops: f32,
) -> [f32; 3] {
    let mut layers: Vec<_> = depths.iter().map(|z| (*z, variants[0])).collect();
    layers.push((0.0, variants[1]));
    layers.sort_by(|a, b| {
        if opposite {
            b.0.total_cmp(&a.0)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    let mut result = [0.0; 3];
    for (_, variant) in layers {
        let p = texel(frame, variant);
        let a = f32::from(p[3]) / 255.0 * 0.5;
        for c in 0..3 {
            result[c] = linear(p[c]) * gain * a + result[c] * (1.0 - a);
        }
    }
    result.map(|c| {
        let c = c * 2.0_f32.powf(stops);
        c / (1.0 + c)
    })
}

#[test]
fn textured_oracle_compiles_distinct_semantic_and_legacy_atlas_paths() {
    for kind in [
        PathKind::SemanticSprite,
        PathKind::SemanticFlipbook,
        PathKind::LegacyFlipbook,
    ] {
        let project = fixture(kind, 2);
        assert_eq!(project.root.max_particles, 2);
        assert_eq!(project.root.assets.len(), 2);
        assert_eq!(project.root.material_instances.is_empty(), !kind.semantic());
        assert_eq!(project.root.flipbooks.len(), usize::from(kind.frames() > 1));
        let image = atlas(kind, 0, false);
        assert_eq!(image.width(), 32 * kind.frames());
        assert_eq!(image.data.as_ref().unwrap()[3], 64);
    }
    // A copied frame or ignored alpha/texture change must not pass the numeric oracle.
    let z = [-3.0, 1.0];
    let red = expected_center(&z, false, 0, [0, 1], 4.0, 0.0);
    for wrong in [
        expected_center(&z, false, 1, [0, 1], 4.0, 0.0),
        expected_center(&z, false, 0, [2, 2], 4.0, 0.0),
    ] {
        assert!(red.into_iter().zip(wrong).any(|(a, b)| (a - b).abs() > 0.1));
    }
}

#[test]
#[ignore = "native textured alpha / semantic and legacy flipbook OIT reference; run alone"]
fn fragment_list_textures_and_flipbooks_follow_frames_images_and_material_bindings() {
    let root = root("fragment-list-textures");
    let (_, seed, z) = interleaved();
    let mut cases = Vec::new();
    for kind in [
        PathKind::SemanticSprite,
        PathKind::SemanticFlipbook,
        PathKind::LegacyFlipbook,
    ] {
        let receiver = fixture(kind, 2);
        let single = fixture(kind, 1);
        let mut samples = Vec::new();
        receiver.evaluate(0.5, seed, &mut samples);
        let mut positions: Vec<_> = samples
            .iter()
            .map(|s| {
                Mat4::from_cols_array(&s.world_from_effect)
                    .transform_point3(Vec3::from_array(s.particle.position))
                    + Vec3::Z * INTERLEAVED_HOST_Z
            })
            .collect();
        positions.push(Vec3::ZERO);
        for reversed in [false, true] {
            for opposite in [false, true] {
                let (mut app, red, blue, target) = pair(
                    receiver.clone(),
                    seed,
                    reversed,
                    opposite,
                    false,
                    INTERLEAVED_HOST_Z,
                );
                replace_blue(&mut app, blue, single.clone(), seed);
                let camera = enable(&mut app);
                camera_hdr(&mut app, camera, 0.0);
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .set_enabled(true);
                let images: Vec<_> = [0, 1, 2]
                    .into_iter()
                    .map(|v| {
                        app.world_mut()
                            .resource_mut::<Assets<Image>>()
                            .add(atlas(kind, v, false))
                    })
                    .collect();
                bind_images(&mut app, red, &images[0], &images[2]);
                bind_images(&mut app, blue, &images[1], &images[2]);
                let references: Vec<_> = positions
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let entity = spawn(&mut app, single.clone(), seed, p.z);
                        app.world_mut()
                            .get_mut::<Transform>(entity)
                            .unwrap()
                            .translation = *p;
                        bind_images(&mut app, entity, &images[usize::from(i == 2)], &images[2]);
                        app.world_mut()
                            .entity_mut(entity)
                            .insert(Visibility::Hidden);
                        entity
                    })
                    .collect();
                let all: Vec<_> = [red, blue]
                    .into_iter()
                    .chain(references.iter().copied())
                    .collect();
                let light = app
                    .world_mut()
                    .spawn((
                        PointLight {
                            intensity: 0.0,
                            range: 16.0,
                            radius: 0.5,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_xyz(-2.0, 1.0, 3.0),
                    ))
                    .id();
                let mut original = None;
                let mut previous_frame_image = None;
                // Ordered, non-monotonic state changes with explicit independent center predictions.
                for (phase, frame, variants, alternate, gain, stops, lit, empty) in [
                    ("frame-0", 10, [0, 1], false, 4.0, 0.0, false, false),
                    ("frame-1", 40, [0, 1], false, 4.0, 0.0, false, false),
                    ("frame-2", 70, [0, 1], false, 4.0, 0.0, false, false),
                    ("loop", 100, [0, 1], false, 4.0, 0.0, false, false),
                    ("image-update", 100, [2, 0], false, 4.0, 0.0, false, false),
                    ("image-restore", 100, [0, 1], false, 4.0, 0.0, false, false),
                    (
                        "material-texture",
                        100,
                        [2, 2],
                        true,
                        4.0,
                        0.0,
                        false,
                        false,
                    ),
                    ("material-gain", 100, [0, 1], false, 2.0, 0.0, false, false),
                    ("exposure", 100, [0, 1], false, 4.0, 1.0, false, false),
                    ("lit", 100, [0, 1], false, 4.0, 0.0, true, false),
                    ("transparent", 100, [0, 1], false, 4.0, 0.0, false, true),
                    ("restored", 100, [0, 1], false, 4.0, 0.0, false, false),
                ] {
                    if !kind.semantic()
                        && matches!(phase, "material-texture" | "material-gain" | "lit")
                    {
                        continue;
                    }
                    forward(&mut app, red, blue, frame);
                    for entity in &references {
                        app.world_mut()
                            .get_mut::<EffectPlayer>(*entity)
                            .unwrap()
                            .seek_frame(frame);
                    }
                    for (i, handle) in images.iter().take(2).enumerate() {
                        *app.world_mut()
                            .resource_mut::<Assets<Image>>()
                            .get_mut(handle)
                            .unwrap() = atlas(kind, variants[i], empty);
                    }
                    for entity in &all {
                        if kind.semantic() {
                            if phase == "restored" {
                                let mut presented =
                                    app.world_mut().get_mut::<PresentedEffect>(*entity).unwrap();
                                let material = presented.effect().emitters[0].renderers[0].material;
                                assert!(presented.unbind_material(material).is_some());
                            } else {
                                override_material(&mut app, *entity, alternate, gain);
                            }
                        }
                    }
                    app.world_mut()
                        .get_mut::<PointLight>(light)
                        .unwrap()
                        .intensity = if lit { 24000.0 } else { 0.0 };
                    camera_hdr(&mut app, camera, stops);
                    let image = capture(&mut app, &target);
                    let sample = app
                        .world()
                        .resource::<ExperimentalOitDiagnostics>()
                        .snapshot()
                        .unwrap();
                    assert_eq!(sample.dropped_fragments, 0);
                    assert_eq!(sample.over_sort_pixels, 0);
                    assert_eq!(sample.invalid_links, 0);
                    let expected_frame = if kind.frames() == 1 {
                        0
                    } else {
                        ((frame / 30) % 3) as u32
                    };
                    let actual_gain = if kind.semantic() { gain } else { 1.0 };
                    let expected = if empty {
                        [0.0; 3]
                    } else {
                        expected_center(&z, opposite, expected_frame, variants, actual_gain, stops)
                    };
                    let analytic_error = center(&image)
                        .into_iter()
                        .zip(expected)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0, f32::max);
                    if !lit {
                        assert!(
                            analytic_error < 0.01,
                            "{} {phase} frame {expected_frame} center {:?} expected {expected:?}, error {analytic_error}",
                            kind.name(),
                            center(&image)
                        );
                    }
                    let light_contrast = if lit {
                        let contrast = center(&image)
                            .into_iter()
                            .zip(center(original.as_ref().unwrap()))
                            .map(|(lit, dark)| lit - dark)
                            .fold(0.0, f32::max);
                        assert!(contrast > 0.01, "native lighting must affect textured RGB");
                        Some(contrast)
                    } else {
                        None
                    };
                    if empty {
                        assert_eq!(
                            sample.attempted_fragments, 0,
                            "zero-alpha texels must not allocate nodes"
                        );
                        assert!(image.pixels().all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
                    } else {
                        healthy(&app);
                    }
                    if kind.frames() > 1 && phase.starts_with("frame-") {
                        if let Some(previous) = &previous_frame_image {
                            assert!(
                                image_difference(previous, &image) > 0.1,
                                "atlas frame did not change"
                            );
                        }
                        previous_frame_image = Some(image.clone());
                    }
                    if phase == "frame-0" {
                        original = Some(image.clone());
                    }
                    let restoration = if matches!(phase, "loop" | "image-restore" | "restored") {
                        let difference = delta(original.as_ref().unwrap(), &image).1;
                        assert!(
                            difference <= 1,
                            "{phase} stale texture/frame/material output"
                        );
                        Some(difference)
                    } else {
                        None
                    };
                    app.world_mut()
                        .entity_mut(camera)
                        .remove::<ExperimentalOitSettings>();
                    app.world_mut()
                        .resource_mut::<AestraSettings>()
                        .transparent_order = TransparentOrderMode::DepthBackToFront;
                    for entity in [red, blue] {
                        app.world_mut()
                            .entity_mut(entity)
                            .insert(Visibility::Hidden);
                    }
                    for entity in &references {
                        app.world_mut()
                            .entity_mut(*entity)
                            .insert(Visibility::Visible);
                    }
                    let split = capture(&mut app, &target);
                    let split_error = image_difference(&image, &split);
                    assert!(
                        split_error < 0.01,
                        "{} {phase} whole-image split error={split_error}",
                        kind.name()
                    );
                    for entity in &references {
                        app.world_mut()
                            .entity_mut(*entity)
                            .insert(Visibility::Hidden);
                    }
                    for entity in [red, blue] {
                        app.world_mut()
                            .entity_mut(entity)
                            .insert(Visibility::Visible);
                    }
                    app.world_mut()
                        .resource_mut::<AestraSettings>()
                        .transparent_order = TransparentOrderMode::Fast;
                    app.world_mut()
                        .entity_mut(camera)
                        .insert(ExperimentalOitSettings::default());
                    let restored = capture(&mut app, &target);
                    let restore_error = delta(&image, &restored).1;
                    assert!(restore_error <= 1);
                    let name = format!(
                        "{}-spawn-{}-view-{}-{phase}",
                        kind.name(),
                        u8::from(reversed),
                        u8::from(opposite)
                    );
                    for (suffix, img) in
                        [("oit", &image), ("split", &split), ("restored", &restored)]
                    {
                        img.save(root.join(format!("{name}-{suffix}.png"))).unwrap();
                    }
                    cases.push(json!({"name":name,"path":kind.name(),"phase":phase,"frame":frame,"atlas_frame":expected_frame,
                        "center_linear_rgb":center(&image),"analytic_center_max_error":if lit {None} else {Some(analytic_error)},
                        "light_center_contrast":light_contrast,
                        "whole_image_split_max_error":split_error,"oit_restore_max_byte_error":restore_error,
                        "state_restore_max_byte_error":restoration,"attempted_fragments":sample.attempted_fragments,
                        "dropped_fragments":sample.dropped_fragments,"over_sort_pixels":sample.over_sort_pixels,"invalid_links":sample.invalid_links}));
                    assert_eq!(app.world().get::<EffectPlayer>(red).unwrap().frame(), frame);
                    assert!(!app.world().get::<EffectPlayer>(red).unwrap().playing);
                    assert!(Arc::ptr_eq(
                        app.world()
                            .get::<EffectPlayer>(red)
                            .unwrap()
                            .project()
                            .unwrap(),
                        &receiver
                    ));
                    assert_native_transport(&app);
                }
            }
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"scope":"bounded textured alpha and flipbook integration, not smoke art or production adoption","cases":cases}),
    );
}
