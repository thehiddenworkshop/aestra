use std::path::Path;

/// Source-format hint, not a guarantee that a loader or semantic parser accepts the file.
/// Flipbooks are document-local metadata, not a format inferred from image filenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFileClassification {
    Effect,
    MaterialProgram,
    MaterialFunction,
    MaterialPreset,
    Texture,
    Mesh,
    Shader,
    Generic,
}

impl ProjectFileClassification {
    pub fn for_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // Most specific suffixes first. Unknown .ron files are not assumed to be effects.
        if name.ends_with(".aestra.material-preset.ron") {
            Self::MaterialPreset
        } else if name.ends_with(".aestra.material-function.ron") {
            Self::MaterialFunction
        } else if name.ends_with(".aestra.material.ron") {
            Self::MaterialProgram
        } else if name.ends_with(".aestra.ron") {
            Self::Effect
        } else {
            match path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tga" | "dds" | "ktx2" | "exr"
                | "hdr" => Self::Texture,
                "gltf" | "glb" | "obj" => Self::Mesh,
                "wesl" | "wgsl" | "glsl" | "hlsl" => Self::Shader,
                _ => Self::Generic,
            }
        }
    }
}
