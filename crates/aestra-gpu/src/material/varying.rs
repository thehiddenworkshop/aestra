//! One deterministic interface used by both stages of a semantic sprite pipeline.

use aestra_compiler::{MaterialIrInstruction, MaterialIrProgram};
use aestra_core::material::{MaterialDomain, MaterialInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialVarying {
    Normal,
    WorldPosition,
    LocalPosition,
    ViewDirection,
    QuadPosition,
    Softness,
    Textured,
    Visible,
    Uv0,
    ParticleColor,
    ParticleOpacity,
    EffectTime,
    ParticleNormalizedAge,
}

impl MaterialVarying {
    // Name, WGSL type, interpolation attribute, shared vertex-data expression.
    fn fields(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Normal => ("normal", "vec3<f32>", "", "mesh.normal"),
            Self::WorldPosition => ("world_position", "vec3<f32>", "", "mesh.world_position"),
            Self::LocalPosition => ("local_position", "vec3<f32>", "", "mesh.local_position"),
            Self::ViewDirection => ("view_direction", "vec3<f32>", "", "mesh.view_direction"),
            Self::QuadPosition => ("quad_position", "vec2<f32>", "", "sprite.quad_position"),
            Self::Softness => ("softness", "f32", "", "sprite.softness"),
            Self::Textured => ("textured", "u32", "@interpolate(flat) ", "sprite.textured"),
            Self::Visible => ("visible", "u32", "@interpolate(flat) ", "sprite.visible"),
            Self::Uv0 => ("uv0", "vec2<f32>", "", "sprite.uv"),
            Self::ParticleColor => ("particle_color", "vec4<f32>", "", "sprite.color"),
            Self::ParticleOpacity => ("particle_opacity", "f32", "", "sprite.color.a"),
            Self::EffectTime => ("effect_time", "f32", "", "sprite.effect_time"),
            Self::ParticleNormalizedAge => (
                "particle_normalized_age",
                "f32",
                "",
                "sprite.particle_normalized_age",
            ),
        }
    }

    pub const fn components(self) -> u32 {
        match self {
            Self::QuadPosition | Self::Uv0 => 2,
            Self::ParticleColor => 4,
            Self::Normal | Self::WorldPosition | Self::LocalPosition | Self::ViewDirection => 3,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialVaryingSlot {
    pub location: u32,
    pub varying: MaterialVarying,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialVaryingLayout {
    pub domain: MaterialDomain,
    pub slots: Vec<MaterialVaryingSlot>,
}

impl MaterialVaryingLayout {
    pub(super) fn from_ir(ir: &MaterialIrProgram) -> Self {
        let reads = |input| {
            ir.values.iter().any(|value| {
            matches!(value.instruction, MaterialIrInstruction::Input(candidate) if candidate == input)
        })
        };
        // Coverage and visibility are required independently of authored material expressions.
        let mut varyings = vec![
            MaterialVarying::QuadPosition,
            MaterialVarying::Softness,
            MaterialVarying::Textured,
            MaterialVarying::Visible,
        ];
        if ir.domain == MaterialDomain::Mesh {
            varyings = vec![MaterialVarying::Visible];
            for (input, varying) in [
                (MaterialInput::Normal, MaterialVarying::Normal),
                (MaterialInput::WorldPosition, MaterialVarying::WorldPosition),
                (MaterialInput::LocalPosition, MaterialVarying::LocalPosition),
                (MaterialInput::ViewDirection, MaterialVarying::ViewDirection),
            ] {
                if reads(input) {
                    varyings.push(varying);
                }
            }
        }
        for (input, varying) in [
            (MaterialInput::Uv0, MaterialVarying::Uv0),
            (MaterialInput::ParticleColor, MaterialVarying::ParticleColor),
            (
                MaterialInput::ParticleOpacity,
                MaterialVarying::ParticleOpacity,
            ),
            (MaterialInput::EffectTime, MaterialVarying::EffectTime),
            (
                MaterialInput::ParticleNormalizedAge,
                MaterialVarying::ParticleNormalizedAge,
            ),
        ] {
            if reads(input)
                && !(input == MaterialInput::ParticleOpacity && reads(MaterialInput::ParticleColor))
            {
                varyings.push(varying);
            }
        }
        Self {
            domain: ir.domain,
            slots: varyings
                .into_iter()
                .enumerate()
                .map(|(location, varying)| MaterialVaryingSlot {
                    location: location as u32,
                    varying,
                })
                .collect(),
        }
    }

    pub fn component_count(&self) -> u32 {
        self.slots
            .iter()
            .map(|slot| slot.varying.components())
            .sum()
    }

    pub(super) fn has_color(&self) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.varying == MaterialVarying::ParticleColor)
    }

    pub(super) fn declarations(&self) -> String {
        self.slots
            .iter()
            .map(|slot| {
                let (name, ty, interpolation, _) = slot.varying.fields();
                format!(
                    "    @location({}) {interpolation}{name}: {ty},\n",
                    slot.location
                )
            })
            .collect()
    }

    pub(super) fn vertex_wesl(&self) -> String {
        let mut source = if self.domain == MaterialDomain::Mesh {
            crate::shader::mesh_vertex_wesl()
        } else {
            String::from(crate::shader::SPRITE_VERTEX_WESL)
        };
        source.push_str(
            "\nstruct MaterialVertexOutput {\n    @builtin(position) clip_position: vec4<f32>,\n",
        );
        source.push_str(&self.declarations());
        source.push_str("}\n\n@vertex\nfn vertex(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> MaterialVertexOutput {\n    let sprite = aestra_sprite_vertex(vertex_index, instance_index);\n    var output: MaterialVertexOutput;\n    output.clip_position = sprite.clip_position;\n");
        if self.domain == MaterialDomain::Mesh {
            source = source.replace(
                "@builtin(vertex_index) vertex_index: u32, @builtin(instance_index)",
                "vertex_input: MeshVertexInput, @builtin(instance_index)",
            );
            source = source.replace("let sprite = aestra_sprite_vertex(vertex_index, instance_index);", "let mesh = aestra_mesh_vertex(vertex_input, instance_index);\n    let sprite = mesh.particle;");
        }
        for slot in &self.slots {
            let (name, _, _, expression) = slot.varying.fields();
            source.push_str(&format!("    output.{name} = {expression};\n"));
        }
        source.push_str("    return output;\n}\n\n");
        source
    }
}
