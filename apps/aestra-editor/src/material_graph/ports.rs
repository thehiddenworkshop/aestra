//! Typed labels, descriptions and socket colors used by material graph composition.
use crate::theme;
use aestra_core::material::{MaterialExpressionDomain, MaterialValueType};
use bevy::prelude::*;
pub(crate) struct MaterialInputPortPresentation {
    pub(crate) label: String,
    pub(crate) description: String,
}

pub(crate) fn input_port_presentation(name: &str) -> MaterialInputPortPresentation {
    let (label, description) = match name {
        "left" => ("A", "First value used by this operation."),
        "right" => ("B", "Second value used by this operation."),
        "start" => ("Start", "Value returned when the blend factor is zero."),
        "end" => ("End", "Value returned when the blend factor is one."),
        "factor" => (
            "Factor",
            "Amount used to blend between the start and end values.",
        ),
        "value" => ("Value", "Value evaluated or transformed by this node."),
        "min" => ("Minimum", "Lowest value allowed by this node."),
        "max" => ("Maximum", "Highest value allowed by this node."),
        "input_min" => ("Source minimum", "Lower bound of the source range."),
        "input_max" => ("Source maximum", "Upper bound of the source range."),
        "output_min" => ("Target minimum", "Value mapped from the source minimum."),
        "output_max" => ("Target maximum", "Value mapped from the source maximum."),
        "edge_min" => ("Lower edge", "Value where the smooth transition begins."),
        "edge_max" => ("Upper edge", "Value where the smooth transition ends."),
        "normal" => ("Normal", "Surface normal used by the calculation."),
        "sample" => (
            "Sample",
            "Sampled linear RGB normal data; alpha is ignored. Set the normal texture color space to Linear, not sRGB.",
        ),
        "strength" => (
            "Strength",
            "Scales tangent-space X and Y. Zero returns the geometric normal.",
        ),
        "flip_y" => (
            "Flip Y",
            "Invert the sampled green axis to switch normal-map convention; does not change mesh handedness.",
        ),
        "tangent" => (
            "Tangent",
            "World-space tangent direction for the normal-map basis.",
        ),
        "bitangent" => (
            "Bitangent",
            "World-space bitangent including UV handedness and mirrored transforms.",
        ),
        "view" => (
            "View direction",
            "Direction from the surface toward the camera.",
        ),
        "power" => ("Power", "Exponent controlling the response curve."),
        "radius" => ("Radius", "Radius of the generated mask."),
        "softness" => ("Softness", "Width of the mask's feathered edge."),
        "threshold" => ("Threshold", "Cutoff used to evaluate the source value."),
        "edge_width" => (
            "Edge width",
            "Width of the transition around the threshold.",
        ),
        "scene_depth" => (
            "Scene depth",
            "Depth sampled from previously rendered geometry.",
        ),
        "pixel_depth" => ("Particle depth", "Depth of the current particle fragment."),
        "fade_distance" => (
            "Fade distance",
            "Distance over which the depth fade occurs.",
        ),
        "invert" => ("Invert", "Reverses the generated mask when enabled."),
        "speed" => ("Speed", "Rate of change applied over time."),
        "time" => ("Time", "Time value used to animate this operation."),
        "center" => ("Center", "Center point of the transformation or mask."),
        "angle" => ("Angle", "Rotation angle applied to the coordinates."),
        "scale" => ("Scale", "Scale applied to the coordinates."),
        "texture" => ("Texture", "Texture sampled by this node."),
        "uv" => (
            "UV coordinates",
            "Texture coordinates evaluated by this node.",
        ),
        "level" => ("Mip level", "Explicit texture mip level used for sampling."),
        "ddx" => (
            "X gradient",
            "Screen-space X derivative used to select and filter the texture mip.",
        ),
        "ddy" => (
            "Y gradient",
            "Screen-space Y derivative used to select and filter the texture mip.",
        ),
        "source" => ("Source", "Source value evaluated by this node."),
        "alpha" => ("Alpha", "Opacity used to combine the values."),
        _ => (name, "Value consumed by this node."),
    };
    MaterialInputPortPresentation {
        label: label.to_owned(),
        description: description.to_owned(),
    }
}

pub(super) fn material_slot_title(label: &str, value_type: Option<MaterialValueType>) -> String {
    format!("{label} [{}]", material_value_type_name(value_type))
}

pub(super) fn material_slot_description(
    description: &str,
    domain: Option<MaterialExpressionDomain>,
) -> String {
    format!(
        "{description}\nEvaluation: {}",
        material_domain_name(domain)
    )
}

pub(super) fn material_value_type_name(value_type: Option<MaterialValueType>) -> &'static str {
    match value_type {
        Some(MaterialValueType::Float) => "Float",
        Some(MaterialValueType::Vec2) => "Vector 2",
        Some(MaterialValueType::Vec3) => "Vector 3",
        Some(MaterialValueType::Vec4) => "Vector 4",
        Some(MaterialValueType::Color) => "Color",
        Some(MaterialValueType::Texture2D(_)) => "Texture 2D",
        Some(MaterialValueType::Bool) => "Boolean",
        None => "Unresolved",
    }
}

pub(super) fn material_domain_name(domain: Option<MaterialExpressionDomain>) -> &'static str {
    match domain {
        Some(MaterialExpressionDomain::ShaderStatic) => "Shader static",
        Some(MaterialExpressionDomain::Instance) => "Material instance",
        Some(MaterialExpressionDomain::Effect) => "Effect",
        Some(MaterialExpressionDomain::Emitter) => "Emitter",
        Some(MaterialExpressionDomain::Particle) => "Particle",
        Some(MaterialExpressionDomain::Vertex) => "Vertex",
        Some(MaterialExpressionDomain::Fragment) => "Fragment",
        None => "Unresolved",
    }
}

pub(super) fn socket_color(value_type: Option<MaterialValueType>) -> Color {
    match value_type {
        Some(MaterialValueType::Float) => Color::srgb(0.45, 0.78, 1.0),
        Some(MaterialValueType::Vec2) => Color::srgb(0.30, 0.86, 0.72),
        Some(MaterialValueType::Vec3) => Color::srgb(0.48, 0.86, 0.38),
        Some(MaterialValueType::Vec4) => Color::srgb(0.81, 0.55, 1.0),
        Some(MaterialValueType::Color) => Color::srgb(1.0, 0.60, 0.34),
        Some(MaterialValueType::Texture2D(_)) => Color::srgb(0.92, 0.42, 0.77),
        Some(MaterialValueType::Bool) => Color::srgb(0.96, 0.38, 0.42),
        None => theme::TEXT_FAINT,
    }
}

pub(super) fn wire_color(value_type: Option<MaterialValueType>) -> Vec4 {
    match value_type {
        Some(MaterialValueType::Float) => Vec4::new(0.45, 0.78, 1.0, 0.82),
        Some(MaterialValueType::Vec2) => Vec4::new(0.30, 0.86, 0.72, 0.82),
        Some(MaterialValueType::Vec3) => Vec4::new(0.48, 0.86, 0.38, 0.82),
        Some(MaterialValueType::Vec4) => Vec4::new(0.81, 0.55, 1.0, 0.82),
        Some(MaterialValueType::Color) => Vec4::new(1.0, 0.60, 0.34, 0.82),
        Some(MaterialValueType::Texture2D(_)) => Vec4::new(0.92, 0.42, 0.77, 0.82),
        Some(MaterialValueType::Bool) => Vec4::new(0.96, 0.38, 0.42, 0.82),
        None => Vec4::new(0.59, 0.62, 0.70, 0.68),
    }
}
