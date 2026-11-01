//! Noise uses the existing typed reusable-function contract. No alternate
//! material graph language or renderer-specific editor nodes are introduced.
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialCustomWeslImplementation, MaterialExpressionDomain, MaterialFunction,
        MaterialFunctionInput, MaterialFunctionOutput, MaterialSchemaVersion, MaterialValue,
        MaterialValueType,
    },
    noise::{self, NoiseBasis, NoiseFunction, NoiseSettings},
};
use std::collections::BTreeMap;

pub(super) fn definition(kind: NoiseFunction) -> MaterialFunction {
    let vector = if kind.dimensions == 2 {
        MaterialValueType::Vec2
    } else {
        MaterialValueType::Vec3
    };
    let zero = if kind.dimensions == 2 {
        MaterialValue::Vec2([0.0; 2])
    } else {
        MaterialValue::Vec3([0.0; 3])
    };
    let mut inputs = vec![
        (0, "Position", vector, None),
        (
            1,
            "Scale",
            MaterialValueType::Float,
            Some(MaterialValue::Float(4.0)),
        ),
        (2, "Offset", vector, Some(zero)),
        (
            3,
            "Amplitude",
            MaterialValueType::Float,
            Some(MaterialValue::Float(1.0)),
        ),
        (
            4,
            "Seed",
            MaterialValueType::Float,
            Some(MaterialValue::Float(0.0)),
        ),
    ];
    if kind.fractal {
        inputs.extend([
            (
                5,
                "Octaves",
                MaterialValueType::Float,
                Some(MaterialValue::Float(4.0)),
            ),
            (
                6,
                "Lacunarity",
                MaterialValueType::Float,
                Some(MaterialValue::Float(2.0)),
            ),
            (
                7,
                "Gain",
                MaterialValueType::Float,
                Some(MaterialValue::Float(0.5)),
            ),
        ]);
    }
    inputs.push((
        8,
        "Footprint",
        MaterialValueType::Float,
        Some(MaterialValue::Float(0.0)),
    ));
    let source = source(kind, false, "field");
    MaterialFunction {
        id: kind.id(),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: kind.name(),
        inputs: inputs
            .into_iter()
            .map(|(i, name, value_type, default)| MaterialFunctionInput {
                id: noise::input(i),
                name: name.into(),
                value_type,
                default,
            })
            .collect(),
        outputs: vec![MaterialFunctionOutput {
            id: noise::OUTPUT,
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression: MaterialExpressionId::from_u128(0xa3574a0000004000800000000000f400),
        }],
        expressions: vec![],
        custom_wesl: Some(MaterialCustomWeslImplementation {
            evaluation_domain: MaterialExpressionDomain::Particle,
            source,
            entry_points: BTreeMap::from([(noise::OUTPUT, "field".into())]),
        }),
    }
}

/// Converts the canonical typed function inputs into the public CPU sampler's
/// settings. This is shared by previews, thumbnails and tools; callers still
/// validate the function reference against the registered built-in definition.
pub fn evaluate_builtin_noise(
    kind: NoiseFunction,
    values: &BTreeMap<aestra_core::MaterialFunctionInputId, MaterialValue>,
) -> Option<f32> {
    let scalar = |i| match values.get(&noise::input(i))? {
        MaterialValue::Float(v) if v.is_finite() => Some(*v),
        _ => None,
    };
    let vector = |i| match values.get(&noise::input(i))? {
        MaterialValue::Vec2(v) if kind.dimensions == 2 => Some([v[0], v[1], 0.0]),
        MaterialValue::Vec3(v) if kind.dimensions == 3 => Some(*v),
        _ => None,
    };
    let s = NoiseSettings {
        basis: kind.basis,
        dimensions: kind.dimensions,
        scale: scalar(1)?,
        offset: vector(2)?,
        amplitude: scalar(3)?,
        seed: scalar(4)?.clamp(0.0, noise::MAX_MATERIAL_NOISE_SEED as f32) as u32,
        octaves: if kind.fractal {
            scalar(5)?.clamp(1.0, 8.0) as u32
        } else {
            1
        },
        lacunarity: if kind.fractal { scalar(6)? } else { 2.0 },
        gain: if kind.fractal { scalar(7)? } else { 0.5 },
        footprint: scalar(8)?,
    };
    aestra_runtime::noise::sample(vector(0)?, &s)
}

fn source(kind: NoiseFunction, raw_seed: bool, entry: &str) -> String {
    include_str!("material_noise/field.wesl")
        .replace(
            "@DIMENSIONS@",
            if kind.dimensions == 2 { "2u" } else { "3u" },
        )
        .replace("@CORNERS@", if kind.dimensions == 2 { "4u" } else { "8u" })
        .replace(
            "@SAMPLE@",
            if kind.basis == NoiseBasis::Gradient {
                include_str!("material_noise/gradient.wesl")
            } else {
                "let corner_value = f32(h>>8u)/16777216.0;"
            },
        )
        .replace(
            "@RESULT@",
            if kind.basis == NoiseBasis::Gradient {
                "clamp(0.5+0.5*sum,0.0,1.0)"
            } else {
                "sum"
            },
        )
        .replace(
            "@VECTOR@",
            if kind.dimensions == 2 {
                "vec2<f32>"
            } else {
                "vec3<f32>"
            },
        )
        .replace(
            "@POSITION@",
            if kind.dimensions == 2 {
                "vec3<f32>((position+offset)*frequency,0.0)"
            } else {
                "(position+offset)*frequency"
            },
        )
        .replace(
            "@FRACTAL_PARAMETERS@",
            if kind.fractal {
                "octaves: f32, lacunarity: f32, gain: f32, "
            } else {
                ""
            },
        )
        .replace(
            "@OCTAVES@",
            if kind.fractal {
                "u32(clamp(octaves,1.0,8.0))"
            } else {
                "1u"
            },
        )
        .replace(
            "@LACUNARITY@",
            if kind.fractal {
                "clamp(lacunarity,1.0,4.0)"
            } else {
                "2.0"
            },
        )
        .replace(
            "@GAIN@",
            if kind.fractal {
                "clamp(gain,0.0,1.0)"
            } else {
                "0.5"
            },
        )
        // The gradient body is inserted after the first dimension replacement.
        .replace(
            "@DIMENSIONS@",
            if kind.dimensions == 2 { "2u" } else { "3u" },
        )
        .replace("@ENTRY@", entry)
        .replace("@SEED_TYPE@", if raw_seed { "u32" } else { "f32" })
        .replace(
            "@SEED_WORD@",
            if raw_seed {
                "seed"
            } else {
                "u32(clamp(seed,0.0,16777215.0))"
            },
        )
}

/// Canonical N1 value/fBm kernel with a full u32 seed, for spatial particle flow.
pub fn spatial_noise_wesl() -> String {
    source(
        NoiseFunction {
            basis: NoiseBasis::Value,
            dimensions: 3,
            fractal: true,
        },
        true,
        "aestra_flow_potential",
    )
}
