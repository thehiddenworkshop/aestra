//! Portable shader sources, WESL composition, and WGSL validation.

use naga::valid::{Capabilities, ValidationFlags, Validator};
use thiserror::Error;
use wesl::{ModulePath, VirtualResolver, Wesl};

use crate::GpuEffectArtifact;

pub const SIMULATION_MODULE: &str = "package::aestra_simulation";
pub const SPRITE_RENDER_MODULE: &str = "package::aestra_sprite_render";
pub const SIMULATION_WESL: &str = include_str!("shaders/aestra_simulation.wesl");
pub const SPRITE_VERTEX_WESL: &str = include_str!("shaders/aestra_sprite_vertex.wesl");
pub const SPRITE_RENDER_WESL: &str = concat!(
    include_str!("shaders/aestra_sprite_vertex.wesl"),
    "\n",
    include_str!("shaders/aestra_sprite_render.wesl"),
);
/// Shared geometry/transform ABI for semantic Mesh materials and diagnostic wireframes.
pub(crate) fn mesh_vertex_wesl() -> String {
    let mut source = SPRITE_VERTEX_WESL.replace("\r\n", "\n").replace(
        "alive_offset: u32,\n    _padding: vec2<u32>,",
        "alive_offset: u32,\n    _padding: vec2<u32>,\n    mesh_from_local: mat4x4<f32>,",
    );
    source.push_str(include_str!("shaders/aestra_mesh_vertex.wesl"));
    source
}

pub fn mesh_wireframe_wesl() -> String {
    let mut source = mesh_vertex_wesl();
    source.push_str(include_str!("shaders/aestra_mesh_wireframe.wesl"));
    source
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuShaderKind {
    Simulation,
    SpriteRender,
}

impl GpuShaderKind {
    pub const fn module_name(self) -> &'static str {
        match self {
            Self::Simulation => SIMULATION_MODULE,
            Self::SpriteRender => SPRITE_RENDER_MODULE,
        }
    }

    pub const fn wesl(self) -> &'static str {
        match self {
            Self::Simulation => SIMULATION_WESL,
            Self::SpriteRender => SPRITE_RENDER_WESL,
        }
    }

    const fn required_entry_points(self) -> &'static [&'static str] {
        match self {
            Self::Simulation => &["reset", "simulate"],
            Self::SpriteRender => &[
                "vertex",
                "fragment_alpha",
                "fragment_additive",
                "fragment_multiply",
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGpuShader {
    pub kind: GpuShaderKind,
    pub module_name: &'static str,
    pub wesl: &'static str,
    pub wgsl: String,
}

/// One engine-neutral WESL module after composition and Naga validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWesl {
    pub module_name: String,
    pub wesl: String,
    pub wgsl: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuShaderArtifactLayout {
    pub emitter_count: u32,
    pub renderer_count: u32,
    pub total_particle_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuShaderPackage {
    pub layout: GpuShaderArtifactLayout,
    pub simulation: CompiledGpuShader,
    pub sprite_render: CompiledGpuShader,
}

impl GpuShaderPackage {
    /// Compose and validate the reference shaders for one lowered artifact.
    ///
    /// The current shaders consume the packed artifact through runtime-sized
    /// buffers, so their WGSL is shared across effect instances. The layout
    /// summary keeps the compiled output tied to the artifact it was produced
    /// for and provides a stable seam for future specialization.
    pub fn for_artifact(artifact: &GpuEffectArtifact) -> Result<Self, GpuShaderError> {
        Ok(Self {
            layout: GpuShaderArtifactLayout {
                emitter_count: artifact.emitters.len() as u32,
                renderer_count: artifact.renderers.len() as u32,
                total_particle_slots: artifact.total_slots,
            },
            simulation: compile(GpuShaderKind::Simulation)?,
            sprite_render: compile(GpuShaderKind::SpriteRender)?,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GpuShaderError {
    #[error("failed to parse WESL module path '{module}': {message}")]
    ModulePath { module: String, message: String },
    #[error("failed to compose WESL module '{module}': {message}")]
    Wesl {
        module: String,
        message: String,
        wesl: String,
    },
    #[error("generated WGSL for '{module}' could not be parsed: {message}")]
    Wgsl {
        module: String,
        message: String,
        wesl: String,
        wgsl: String,
    },
    #[error("generated WGSL for '{module}' failed Naga validation: {message}")]
    Validation {
        module: String,
        message: String,
        wesl: String,
        wgsl: String,
    },
    #[error("generated WGSL for '{module}' is missing entry point '{entry_point}'")]
    MissingEntryPoint { module: String, entry_point: String },
}

pub fn compile(kind: GpuShaderKind) -> Result<CompiledGpuShader, GpuShaderError> {
    let module_name = kind.module_name();
    let compiled = compile_wesl(module_name, kind.wesl(), kind.required_entry_points())?;
    Ok(CompiledGpuShader {
        kind,
        module_name,
        wesl: kind.wesl(),
        wgsl: compiled.wgsl,
    })
}

/// Composes one WESL module and validates the generated WGSL without an engine backend.
pub fn compile_wesl(
    module_name: &str,
    wesl: &str,
    required_entry_points: &[&str],
) -> Result<CompiledWesl, GpuShaderError> {
    let module: ModulePath = module_name
        .parse()
        .map_err(|error| GpuShaderError::ModulePath {
            module: module_name.to_owned(),
            message: format!("{error:?}"),
        })?;
    let mut resolver = VirtualResolver::new();
    resolver.add_module(module.clone(), wesl.into());
    let wgsl = Wesl::new("")
        .set_custom_resolver(resolver)
        .compile(&module)
        .map_err(|error| GpuShaderError::Wesl {
            module: module_name.to_owned(),
            message: error.to_string(),
            wesl: wesl.to_owned(),
        })?
        .to_string();

    let naga_module = match naga::front::wgsl::parse_str(&wgsl) {
        Ok(module) => module,
        Err(error) => {
            return Err(GpuShaderError::Wgsl {
                module: module_name.to_owned(),
                message: error.emit_to_string(&wgsl),
                wesl: wesl.to_owned(),
                wgsl,
            });
        }
    };
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&naga_module)
        .map_err(|error| GpuShaderError::Validation {
            module: module_name.to_owned(),
            message: error.to_string(),
            wesl: wesl.to_owned(),
            wgsl: wgsl.clone(),
        })?;

    for &entry_point in required_entry_points {
        if !naga_module
            .entry_points
            .iter()
            .any(|entry| entry.name == entry_point)
        {
            return Err(GpuShaderError::MissingEntryPoint {
                module: module_name.to_owned(),
                entry_point: entry_point.to_owned(),
            });
        }
    }

    Ok(CompiledWesl {
        module_name: module_name.to_owned(),
        wesl: wesl.to_owned(),
        wgsl,
    })
}
