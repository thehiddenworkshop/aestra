# ADR 0004: Compiled execution boundary

- Status: accepted
- Date: 2026-08-26

## Context

The first reference evaluator read `EffectAsset` directly from `aestra-bevy`.
That coupled authored data, execution behavior, and engine rendering, and meant
every preview consumer depended on the source schema.

## Decision

`aestra-compiler` owns module discovery, compiler diagnostics, particle-attribute
analysis, and lowering. It turns `EffectAsset` into an immutable `CompiledEffect`
defined by `aestra-runtime`. Lowered instructions retain source module IDs and a
source-to-instruction map.

`aestra-runtime` owns compiled execution plans, `EffectInstance`, parameter
overrides, deterministic seeds, playback time, and the CPU interpreter. It has no
Bevy dependency. `aestra-bevy` only compiles at its public authoring boundary and
adapts runtime particle samples to ECS rendering.

The editor recompiles after successful semantic transactions. The viewer and
game plugin compile once when creating a player. There is no direct authored
asset evaluator.

## Consequences

Authored assets, compiled artifacts, and live instances now have independent
lifecycles. Compiler errors can identify semantic paths before runtime, and the
same compiled path is exercised by editor, viewer, and game integration. The CPU
interpreter remains the golden conformance oracle while a GPU backend is added.
