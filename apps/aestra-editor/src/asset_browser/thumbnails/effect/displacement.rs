//! Conservative interval bounds for the supported saved mesh-displacement expressions.
use super::*;
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialInput, MaterialInstance, MaterialParameterValue, MaterialValue},
};

pub(super) fn radius(
    program: &MaterialProgram,
    instance: &MaterialInstance,
    geometry: f32,
) -> Result<f32, String> {
    let Some(output) = program.outputs.vertex_offset else {
        return Ok(geometry);
    };
    if !program.disabled_expressions.is_empty() {
        return Err("Disabled displacement expressions need resolved thumbnail bounds".into());
    }
    let mut memo = BTreeMap::new();
    let (lo, hi) = interval(output, program, instance, geometry, &mut memo, 0)
        .ok_or("Mesh displacement cannot be bounded for this thumbnail")?;
    let extent = lo.abs().max(hi.abs());
    let radius = geometry + extent.length();
    if !radius.is_finite() || radius > 1.0e6 {
        return Err("Mesh displacement exceeds thumbnail bounds".into());
    }
    Ok(radius * 1.0001)
}

fn value(value: &MaterialValue) -> Option<Vec3> {
    match value {
        MaterialValue::Float(v) => Some(Vec3::splat(*v)),
        MaterialValue::Vec3(v) => Some(Vec3::from_array(*v)),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn interval(
    id: MaterialExpressionId,
    program: &MaterialProgram,
    instance: &MaterialInstance,
    geometry: f32,
    memo: &mut BTreeMap<MaterialExpressionId, (Vec3, Vec3)>,
    depth: usize,
) -> Option<(Vec3, Vec3)> {
    if depth > 64 || memo.len() > 256 {
        return None;
    }
    if let Some(bounds) = memo.get(&id) {
        return Some(*bounds);
    }
    use MaterialExpressionKind as K;
    let expression = &program.expressions.iter().find(|e| e.id == id)?.kind;
    let bounds = match expression {
        K::Constant(v) => {
            let v = value(v)?;
            (v, v)
        }
        K::Parameter(id) => match instance.values.get(id) {
            Some(MaterialParameterValue::Constant(v)) => {
                let v = value(v)?;
                (v, v)
            }
            Some(MaterialParameterValue::RandomRange { min, max, .. }) => {
                let a = value(min)?;
                let b = value(max)?;
                (a.min(b), a.max(b))
            }
            Some(_) => return None,
            None => {
                let v = value(
                    program
                        .parameters
                        .iter()
                        .find(|p| p.id == *id)?
                        .default
                        .as_ref()?,
                )?;
                (v, v)
            }
        },
        K::Input(MaterialInput::LocalPosition) => (Vec3::splat(-geometry), Vec3::splat(geometry)),
        K::Input(MaterialInput::ParticleNormalizedAge) => (Vec3::ZERO, Vec3::ONE),
        K::Add(a, b) | K::Subtract(a, b) | K::Multiply(a, b) => {
            let (al, ah) = interval(*a, program, instance, geometry, memo, depth + 1)?;
            let (bl, bh) = interval(*b, program, instance, geometry, memo, depth + 1)?;
            match expression {
                K::Add(..) => (al + bl, ah + bh),
                K::Subtract(..) => (al - bh, ah - bl),
                _ => {
                    let products = [al * bl, al * bh, ah * bl, ah * bh];
                    (
                        products
                            .into_iter()
                            .fold(Vec3::splat(f32::INFINITY), Vec3::min),
                        products
                            .into_iter()
                            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max),
                    )
                }
            }
        }
        _ => return None,
    };
    if !bounds.0.is_finite() || !bounds.1.is_finite() {
        return None;
    }
    memo.insert(id, bounds);
    Some(bounds)
}
