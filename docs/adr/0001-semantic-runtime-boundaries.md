# ADR 0001: Semantic and runtime boundaries

- Status: accepted
- Date: 2026-08-26

## Context

The first Aestra prototype intentionally colocates its version-1 asset model,
persistence, deterministic evaluator, and Bevy playback in `aestra-bevy`. The
next architecture requires authored data, compiled data, runtime state, editor
state, and engine integration to evolve independently.

## Decision

Aestra uses three distinct lifecycle types:

- `EffectAsset` is the versioned, human-editable semantic source document.
- `CompiledEffect` is an immutable, target-specific compiler artifact.
- `EffectInstance` is the mutable state of one playing effect.

The repository keeps its three products at the workspace root:

- `aestra-editor`
- `aestra-bevy`
- `aestra-viewer`

Reusable implementation crates are added under `crates/` as their boundaries
become executable. The first extractions are `aestra-core`,
`aestra-authoring`, `aestra-compiler`, and `aestra-runtime`.

Dependency rules:

- semantic crates do not depend on Bevy UI, an AI provider, or editor layout;
- `aestra-bevy` adapts compilation/runtime contracts to Bevy ECS, assets, and
  rendering;
- editor and viewer preview through the same compile/runtime contracts used by
  games;
- visual graph positions are editor metadata keyed by semantic IDs, never the
  canonical effect program;
- AI and scripts submit the same commands and transactions as the editor.

## Compatibility strategy

The current CPU evaluator and format-v1 example are frozen as a reference
contract. Format v2 will be introduced with an explicit v1-to-v2 migration.
During the transition, loading may accept both versions, but saving produces the
current version. Existing format meaning must not change silently.

## Consequences

This introduces additional crates and a migration boundary before GPU work. It
also gives compiler, runtime, editor, viewer, automation, and future AI clients a
single semantic contract that can be tested without UI input.

