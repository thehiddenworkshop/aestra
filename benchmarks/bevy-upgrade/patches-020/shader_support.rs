use bevy::{
    asset::{AssetId, Assets},
    shader::{Shader, ShaderCache, ShaderCacheError, ShaderCacheSource, ShaderDefVal},
};
use std::{path::Path, process::Command, sync::OnceLock};

pub(crate) struct Compiled {
    pub(crate) wgsl: String,
    pub(crate) module: naga::Module,
}

fn libraries() -> &'static Vec<(String, String)> {
    static LIBRARIES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    LIBRARIES.get_or_init(|| {
        // Use the locked graph's source paths, not a developer's Cargo-home layout.
        let metadata = Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--manifest-path",
                concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--filter-platform",
                "x86_64-pc-windows-msvc",
            ])
            .output()
            .expect("Cargo metadata");
        assert!(
            metadata.status.success(),
            "{}",
            String::from_utf8_lossy(&metadata.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
        let mut libraries = Vec::new();
        for package in metadata["packages"].as_array().unwrap() {
            let name = package["name"].as_str().unwrap();
            if !name.starts_with("bevy_") || name == "bevy_mikktspace" {
                continue;
            }
            assert_eq!(package["version"], "0.20.0", "mixed Bevy graph: {name}");
            let src = Path::new(package["manifest_path"].as_str().unwrap())
                .parent()
                .unwrap()
                .join("src");
            if src.is_dir() {
                collect(&src, &src, name, &mut libraries);
            }
        }
        assert!(
            libraries
                .iter()
                .any(|(path, _)| path == "embedded://bevy_ui_render/ui_vertex_output.wesl")
        );
        assert!(
            libraries
                .iter()
                .any(|(path, _)| path == "embedded://bevy_pbr/render/clustered_forward.wesl")
        );
        libraries.sort_by(|a, b| a.0.cmp(&b.0));
        libraries
    })
}

fn collect(root: &Path, directory: &Path, package: &str, output: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, package, output);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "wesl")
        {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            output.push((
                format!("embedded://{package}/{relative}"),
                std::fs::read_to_string(path).unwrap(),
            ));
        }
    }
}

fn validate(
    _: &(),
    source: ShaderCacheSource<'_>,
    _: &bevy::shader::ValidateShader,
) -> Result<Compiled, ShaderCacheError> {
    let ShaderCacheSource::Wgsl(wgsl) = source else {
        panic!("expected WGSL");
    };
    let module = naga::front::wgsl::parse_str(&wgsl)
        .map_err(|error| ShaderCacheError::CreateShaderModule(error.emit_to_string(&wgsl)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|error| ShaderCacheError::CreateShaderModule(error.emit_to_string(&wgsl)))?;
    Ok(Compiled { wgsl, module })
}

pub(crate) fn compile(source: String, defs: &[ShaderDefVal]) -> std::sync::Arc<Compiled> {
    let mut assets = Assets::<Shader>::default();
    let mut cache = ShaderCache::new((), validate);
    for (path, source) in libraries() {
        let mut shader = Shader::from_wesl(source.clone(), path.clone());
        // Same library shader defs installed by MeshRenderPlugin, using real constants.
        if path == "embedded://bevy_pbr/render/mesh_view_types.wesl" {
            shader.shader_defs = vec![
                ShaderDefVal::UInt(
                    "MAX_DIRECTIONAL_LIGHTS".into(),
                    bevy::pbr::MAX_DIRECTIONAL_LIGHTS as u32,
                ),
                ShaderDefVal::UInt(
                    "MAX_CASCADES_PER_LIGHT".into(),
                    bevy::pbr::MAX_CASCADES_PER_LIGHT as u32,
                ),
                ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), bevy::pbr::MAX_RECT_LIGHTS as u32),
            ];
        }
        let id = assets.add(shader.clone()).id();
        cache.set_shader(id, shader);
    }
    let shader = Shader::from_wesl(source, "qualification/root.wesl");
    let id: AssetId<Shader> = assets.add(shader.clone()).id();
    cache.set_shader(id, shader);
    cache
        .get(0, id, defs)
        .unwrap_or_else(|error| panic!("{error}"))
}
