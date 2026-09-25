//! The built-in module catalog: Aestra's own modules, registered through the same descriptors any
//! extension uses (Decision I â€” built-ins do not bypass the registry).

use crate::*;
use aestra_core::*;
use aestra_runtime::ParticleAttribute;
fn input(
    name: &'static str,
    display_name: &'static str,
    description: &'static str,
    default_value: aestra_core::Value,
    control: InputControl,
) -> InputMetadata {
    let sources = match control {
        InputControl::Range { .. } => {
            vec![InputSourceKind::Constant, InputSourceKind::RandomRange]
        }
        InputControl::Curve { .. } => vec![
            InputSourceKind::Constant,
            InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
        ],
        InputControl::Gradient => vec![
            InputSourceKind::Constant,
            InputSourceKind::Gradient(InputEvaluationDomain::ParticleLife),
        ],
        _ => vec![InputSourceKind::Constant],
    };
    InputMetadata {
        name,
        display_name,
        description,
        value_type: default_value.value_type(),
        default_value,
        unit: None,
        control,
        sources,
    }
}

impl InputMetadata {
    /// An input with the default authoring sources for its control (constant, plus random range /
    /// curve / gradient where the control supports them) — the builder plugins use (M10).
    pub fn new(
        name: &'static str,
        display_name: &'static str,
        description: &'static str,
        default_value: aestra_core::Value,
        control: InputControl,
    ) -> Self {
        input(name, display_name, description, default_value, control)
    }

    /// Clones the catalog default while assigning fresh IDs to nested authored data.
    pub fn instantiate_default(&self) -> aestra_core::Value {
        let mut value = self.default_value.clone();
        match &mut value {
            aestra_core::Value::Curve(curve) => curve.id = CurveId::new(),
            aestra_core::Value::Vec3Curve(curve) => {
                for axis in &mut curve.curves {
                    axis.id = CurveId::new();
                }
            }
            aestra_core::Value::Gradient(gradient) => gradient.id = GradientId::new(),
            _ => {}
        }
        value
    }

    pub fn with_unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }

    fn with_sources(mut self, sources: Vec<InputSourceKind>) -> Self {
        self.sources = sources;
        self
    }
}

fn metadata(
    type_id: &'static str,
    display_name: &'static str,
    description: &'static str,
    category: &'static str,
    stage: StageKind,
) -> ModuleMetadata {
    ModuleMetadata {
        type_id: ModuleTypeId::new(type_id),
        display_name,
        description,
        category,
        stages: vec![stage],
        inputs: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        tags: Vec::new(),
        capabilities: vec![
            CapabilityId::new(CAPABILITY_CPU_REFERENCE),
            CapabilityId::new(CAPABILITY_PARTICLE_SIMULATION),
        ],
        simulation: SimulationRequirements::ANALYTIC,
        multiplicity: ModuleMultiplicity::Multiple,
        approximate_cost: 0,
        requires: None,
        schema_version: BUILTIN_PROPERTY_SCHEMA_VERSION,
    }
}

impl ModuleMetadata {
    /// A plugin module descriptor (extensible-stages M10): no built-in capabilities, hosted wherever
    /// `requires` is satisfied. Chain the `with_*` builders to describe inputs, flow and cost.
    pub fn extension(
        type_id: ModuleTypeId,
        display_name: &'static str,
        description: &'static str,
        category: &'static str,
        requires: CapabilityExpression,
    ) -> Self {
        Self {
            type_id,
            display_name,
            description,
            category,
            stages: Vec::new(),
            inputs: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            tags: Vec::new(),
            capabilities: Vec::new(),
            simulation: SimulationRequirements::ANALYTIC,
            multiplicity: ModuleMultiplicity::Multiple,
            approximate_cost: 0,
            requires: Some(requires),
            schema_version: BUILTIN_PROPERTY_SCHEMA_VERSION,
        }
    }

    pub fn with_multiplicity(mut self, multiplicity: ModuleMultiplicity) -> Self {
        self.multiplicity = multiplicity;
        self
    }

    pub fn with_inputs(mut self, inputs: Vec<InputMetadata>) -> Self {
        self.inputs = inputs;
        self
    }

    pub fn with_flow(
        mut self,
        reads: Vec<ParticleAttribute>,
        writes: Vec<ParticleAttribute>,
    ) -> Self {
        self.reads = reads;
        self.writes = writes;
        self
    }

    pub fn with_tags(mut self, tags: Vec<&'static str>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_capabilities(mut self, capabilities: Vec<CapabilityId>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_simulation(mut self, simulation: SimulationRequirements) -> Self {
        self.simulation = simulation;
        self
    }

    pub fn with_schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    pub fn with_cost(mut self, approximate_cost: u32) -> Self {
        self.approximate_cost = approximate_cost;
        self
    }
}

pub(crate) fn builtin_modules() -> Vec<ModuleMetadata> {
    use ParticleAttribute as A;
    vec![
        metadata(
            MODULE_EMISSION,
            "Emission",
            "Controls continuous spawning and the initial particle burst.",
            "Emitter",
            StageKind::EmitterUpdate,
        )
        .with_inputs(vec![
            input(
                "spawn_rate",
                "Spawn Rate",
                "Particles emitted per second.",
                aestra_core::Value::Scalar(24.0),
                InputControl::Number {
                    step: 5.0,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::EmitterTime),
            ])
            .with_unit("particles/s"),
            input(
                "burst_count",
                "Burst Count",
                "Particles emitted when the emitter starts.",
                aestra_core::Value::U32(0),
                InputControl::Number {
                    step: 4.0,
                    min: Some(0.0),
                    max: None,
                },
            ),
        ])
        .with_flow(vec![], vec![])
        .with_tags(vec!["spawn", "rate", "burst"])
        .with_cost(1),
        metadata(
            MODULE_SHAPE,
            "Shape",
            "Defines where newly spawned particles are placed.",
            "Spawn",
            StageKind::ParticleSpawn,
        )
        .with_inputs(vec![input(
            "shape",
            "Shape",
            "Volume used to place newly spawned particles.",
            aestra_core::Value::Shape(EmitterShape::Point),
            InputControl::Choice,
        )])
        .with_flow(vec![], vec![A::Position])
        .with_tags(vec!["spawn", "position"])
        .with_cost(2),
        metadata(
            MODULE_INITIALIZE,
            "Initialize Particle",
            "Sets lifetime, velocity, direction, and rotation for new particles.",
            "Spawn",
            StageKind::ParticleSpawn,
        )
        .with_inputs(vec![
            input(
                "lifetime",
                "Lifetime",
                "Minimum and maximum particle lifetime.",
                aestra_core::Value::Range(ScalarRange::new(0.8, 1.4)),
                InputControl::Range {
                    step: 0.1,
                    min: Some(0.05),
                    max: None,
                },
            )
            .with_unit("s"),
            input(
                "speed",
                "Speed",
                "Minimum and maximum initial particle speed.",
                aestra_core::Value::Range(ScalarRange::new(35.0, 70.0)),
                InputControl::Range {
                    step: 5.0,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_unit("units/s"),
            input(
                "direction",
                "Direction",
                "Central 3D launch direction.",
                aestra_core::Value::Vec3([0.0, 1.0, 0.0]),
                InputControl::Vector {
                    step: 0.1,
                    min: None,
                    max: None,
                },
            ),
            input(
                "spread_degrees",
                "Spread",
                "Angular launch cone around the central direction.",
                aestra_core::Value::Scalar(30.0),
                InputControl::Number {
                    step: 5.0,
                    min: Some(0.0),
                    max: Some(360.0),
                },
            )
            .with_unit("°"),
            input(
                "angular_velocity",
                "Angular Velocity",
                "Minimum and maximum particle spin.",
                aestra_core::Value::Range(ScalarRange::new(-1.0, 1.0)),
                InputControl::Range {
                    step: 0.1,
                    min: None,
                    max: None,
                },
            )
            .with_unit("rad/s"),
        ])
        .with_flow(
            vec![],
            vec![A::Velocity, A::Lifetime, A::Rotation, A::AngularVelocity],
        )
        .with_tags(vec!["spawn", "velocity", "lifetime"])
        .with_cost(4),
        metadata(
            MODULE_MOTION,
            "Motion",
            "Updates particle movement using gravity, drag, and procedural turbulence.",
            "Forces",
            StageKind::ParticleUpdate,
        )
        .with_inputs(vec![
            input(
                "gravity",
                "Gravity",
                "Constant acceleration applied to particle velocity.",
                aestra_core::Value::Vec3([0.0, -18.0, 0.0]),
                InputControl::Vector {
                    step: 5.0,
                    min: None,
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ])
            .with_unit("units/s²"),
            input(
                "drag",
                "Drag",
                "Velocity damping applied over time.",
                aestra_core::Value::Scalar(0.6),
                InputControl::Number {
                    step: 0.1,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ]),
            input(
                "turbulence",
                "Turbulence",
                "Strength of deterministic procedural motion.",
                aestra_core::Value::Scalar(4.0),
                InputControl::Number {
                    step: 0.5,
                    min: None,
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ]),
        ])
        .with_flow(
            vec![A::Position, A::Velocity, A::Age],
            vec![A::Position, A::Velocity],
        )
        .with_tags(vec!["update", "force", "motion"])
        .with_cost(6),
        metadata(
            MODULE_APPEARANCE,
            "Appearance",
            "Controls particle size, opacity, and color.",
            "Appearance",
            StageKind::ParticleUpdate,
        )
        .with_inputs(vec![
            input(
                "size",
                "Size",
                "Particle size. The selected source controls how it varies.",
                aestra_core::Value::Curve(Curve {
                    id: CurveId::from_u128(0),
                    interpolation: Default::default(),
                    keys: vec![
                        CurveKey::new(0.0, 4.0),
                        CurveKey::new(0.35, 10.0),
                        CurveKey::new(1.0, 1.0),
                    ],
                    output_range: None,
                }),
                InputControl::Curve {
                    step: 0.5,
                    min: 0.0,
                    max: 32.0,
                },
            ),
            input(
                "opacity",
                "Opacity",
                "Particle opacity. The selected source controls how it varies.",
                aestra_core::Value::Curve(Curve {
                    id: CurveId::from_u128(0),
                    interpolation: Default::default(),
                    keys: vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.12, 1.0),
                        CurveKey::new(1.0, 0.0),
                    ],
                    output_range: None,
                }),
                InputControl::Curve {
                    step: 0.05,
                    min: 0.0,
                    max: 1.0,
                },
            ),
            input(
                "color",
                "Color",
                "Particle color and alpha. The selected source controls how they vary.",
                aestra_core::Value::Gradient(Gradient {
                    id: GradientId::from_u128(0),
                    keys: vec![
                        ColorKey::new(0.0, [0.35, 0.75, 1.0, 1.0]),
                        ColorKey::new(0.5, [0.62, 0.3, 1.0, 1.0]),
                        ColorKey::new(1.0, [0.15, 0.05, 0.4, 0.0]),
                    ],
                }),
                InputControl::Gradient,
            ),
        ])
        .with_flow(vec![A::NormalizedAge], vec![A::Size, A::Color])
        .with_tags(vec!["update", "color", "size"])
        .with_cost(5),
        metadata(
            MODULE_PERSISTENT,
            "Persistent State",
            "Simulates this emitter with persistent per-particle state that advances incrementally \
             across fixed ticks, enabling history-dependent behavior.",
            "Simulation",
            StageKind::ParticleUpdate,
        )
        // Reading and writing previous-tick state is what promotes the emitter to a stateful class:
        // the compiler derives SimulationClass::Stateful from this temporal requirement (S1-D2).
        .with_flow(
            vec![A::Position, A::Velocity, A::Age],
            vec![A::Position, A::Velocity, A::Age],
        )
        .with_simulation(SimulationRequirements {
            temporal: TemporalRequirement::PreviousState,
            ..SimulationRequirements::ANALYTIC
        })
        // One persistent solver per emitter — a second would be an ambiguous double state advance.
        .with_multiplicity(ModuleMultiplicity::Single)
        .with_tags(vec!["simulation", "stateful", "persistent"])
        .with_cost(6),
        metadata(
            MODULE_COLLISION,
            "Collision",
            "Collides this emitter's particles with authored planes, spheres, and boxes, bouncing \
             (restitution + friction) or killing them on contact.",
            "Simulation",
            StageKind::ParticleUpdate,
        )
        // Resolving collisions reads and writes previous-tick position/velocity, so — like the
        // persistent solver — this temporal requirement promotes the emitter to a stateful class
        // (hybrid roadmap M10): no manual stateful toggle is needed.
        .with_flow(
            vec![A::Position, A::Velocity, A::Age],
            vec![A::Position, A::Velocity, A::Age],
        )
        .with_simulation(SimulationRequirements {
            temporal: TemporalRequirement::PreviousState,
            ..SimulationRequirements::ANALYTIC
        })
        .with_tags(vec!["simulation", "stateful", "collision"])
        .with_cost(7),
    ]
}
