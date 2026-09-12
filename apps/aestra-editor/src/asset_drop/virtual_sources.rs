//! Snapshot-only virtual catalog, shared by the browser and compatible field pickers.
use super::VirtualAsset;
use crate::*;
use aestra_compiler::MaterialCompiler;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub asset: VirtualAsset,
    pub name: String,
    pub kind: &'static str,
    pub description: String,
}

pub(crate) fn entries(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    built_in: bool,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    if built_in {
        for preset in MaterialCompiler.material_preset_catalog().iter() {
            entries.push(Entry {
                asset: VirtualAsset::BuiltInPreset(preset.id),
                name: preset.display_name.clone(),
                kind: "Built-in preset",
                description: format!(
                    "{}\n{}\n{}",
                    preset.category.display_name(),
                    preset.description,
                    preset.tags.join(", ")
                ),
            });
        }
    } else {
        for material in &session.effect.materials {
            entries.push(Entry {
                asset: VirtualAsset::Material(material.id), name: material.name.clone(),
                kind: "Local material", description: format!("Sprite · {:?}\nMaterial stored in this effect. Drop onto a compatible renderer or Material field.", material.blend),
            });
        }
        let programs = catalog
            .material_programs_for_effect(&session.effect)
            .unwrap_or_default();
        for (index, instance) in session.effect.material_instances.iter().enumerate() {
            let name = programs
                .iter()
                .find(|p| p.id == instance.program.id())
                .map_or("Material", |p| p.name.as_str());
            entries.push(Entry {
                asset: VirtualAsset::Material(instance.id), name: format!("{name} · Local instance {}", index + 1),
                kind: "Local instance", description: "Material instance stored in this effect. Drop onto a compatible Material field.".into(),
            });
        }
        for asset in &session.effect.assets {
            let (value, kind) = match asset.kind {
                AssetKind::Texture => (VirtualAsset::Texture(asset.id), "Local texture"),
                AssetKind::Mesh => (VirtualAsset::Mesh(asset.id), "Local mesh"),
                AssetKind::Flipbook => (
                    VirtualAsset::FlipbookDeclaration(asset.id),
                    "Legacy flipbook declaration",
                ),
            };
            entries.push(Entry {
                asset: value,
                name: asset.name.clone(),
                kind,
                description: if matches!(value, VirtualAsset::FlipbookDeclaration(_)) {
                    format!("{}\nDeclaration only: no atlas metadata. Create a flipbook from a registered texture before assigning it.", asset.path)
                } else {
                    asset.path.clone()
                },
            });
        }
        for flipbook in &session.effect.flipbooks {
            entries.push(Entry {
                asset: VirtualAsset::Flipbook(flipbook.id), name: flipbook.name.clone(), kind: "Local flipbook",
                description: format!("{} frames · {} fps\nAtlas metadata stored in this effect. Drop onto a Flipbook renderer. Creating an atlas does not change the texture file.", flipbook.frames.len(), flipbook.frame_rate),
            });
        }
    }
    entries.sort_by_key(|entry| (entry.name.to_lowercase(), entry.kind));
    entries
}
