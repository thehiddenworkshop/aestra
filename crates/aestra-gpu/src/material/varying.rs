//! One deterministic interface used by both stages of a semantic sprite pipeline.

use aestra_compiler::{MaterialIrInstruction, MaterialIrProgram};
use aestra_core::material::{MaterialDomain, MaterialInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialVarying {
    Normal,
    Tangent,
    Uv1,
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
    Bitangent,
    RibbonUv,
    RibbonDirection,
}

impl MaterialVarying {
    // Name, WGSL type, interpolation attribute, shared vertex-data expression.
    fn fields(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::RibbonUv => ("ribbon_uv", "vec2<f32>", "", "sprite.uv"),
            Self::RibbonDirection => (
                "ribbon_direction",
                "vec3<f32>",
                "",
                "sprite.ribbon_direction",
            ),
            Self::Uv1 => ("uv1", "vec2<f32>", "", "mesh.uv1"),
            Self::Tangent => ("tangent", "vec3<f32>", "", "mesh.tangent"),
            Self::Bitangent => ("bitangent", "vec3<f32>", "", "mesh.bitangent"),
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
            Self::QuadPosition | Self::Uv0 | Self::Uv1 | Self::RibbonUv => 2,
            Self::ParticleColor => 4,
            Self::Tangent
            | Self::Bitangent
            | Self::Normal
            | Self::WorldPosition
            | Self::LocalPosition
            | Self::ViewDirection
            | Self::RibbonDirection => 3,
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
                (MaterialInput::Tangent, MaterialVarying::Tangent),
                (MaterialInput::Bitangent, MaterialVarying::Bitangent),
                (MaterialInput::Uv1, MaterialVarying::Uv1),
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
            (MaterialInput::RibbonUv, MaterialVarying::RibbonUv),
            (
                MaterialInput::RibbonDirection,
                MaterialVarying::RibbonDirection,
            ),
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

    pub(super) fn vertex_wesl(&self, vertex_offset: bool) -> String {
        let mut source = if self.domain == MaterialDomain::Mesh {
            crate::shader::mesh_vertex_wesl_with_inputs(
                self.slots
                    .iter()
                    .any(|slot| slot.varying == MaterialVarying::Uv1),
                self.slots
                    .iter()
                    .any(|slot| slot.varying == MaterialVarying::Tangent),
                self.slots
                    .iter()
                    .any(|slot| slot.varying == MaterialVarying::Bitangent),
            )
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
        if vertex_offset {
            let mut context = String::from(
                "let initial_mesh = aestra_mesh_vertex(vertex_input, instance_index);\n    let initial_sprite = initial_mesh.particle;\n    var context: MaterialVertexOutput;\n    context.clip_position = initial_sprite.clip_position;\n",
            );
            for slot in &self.slots {
                let (name, _, _, expression) = slot.varying.fields();
                let expression = expression
                    .replace("mesh.", "initial_mesh.")
                    .replace("sprite.", "initial_sprite.");
                context.push_str(&format!("    context.{name} = {expression};\n"));
            }
            context.push_str("    var displaced = vertex_input;\n    displaced.position += aestra_vertex_offset(context);\n    let mesh = aestra_mesh_vertex(displaced, instance_index);");
            source = source.replace(
                "let mesh = aestra_mesh_vertex(vertex_input, instance_index);",
                &context,
            );
            source = source.replace("@vertex\nfn vertex(vertex_input: MeshVertexInput, @builtin(instance_index) instance_index: u32)",
                "fn aestra_material_vertex(vertex_input: MeshVertexInput, instance_index: u32)");
            for entry in ["vertex", "vertex_mesh_wireframe"] {
                source.push_str(&format!("@vertex\nfn {entry}(vertex_input: MeshVertexInput, @builtin(instance_index) instance_index: u32) -> MaterialVertexOutput {{\n    return aestra_material_vertex(vertex_input, instance_index);\n}}\n"));
            }
        }
        source
    }
}
