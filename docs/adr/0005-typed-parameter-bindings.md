# ADR 0005: Typed parameter bindings and runtime slots

- Status: accepted
- Date: 2026-08-26

## Context

Built-in module inputs were authored only as literal values. `EffectInstance`
could store overrides, but compiled instructions could not read them, so changing
an exposed gameplay value required recompiling the effect.

## Decision

Each `ModuleInstance` keeps its authored input values as inspectable fallbacks and
may bind a named input to a typed `EffectParameter`. Empty binding maps are
omitted from serialization, keeping existing format-v2 assets stable.

The compiler validates binding names and types against module metadata. Exposed,
referenced parameters receive deterministic indexed runtime slots. Unbound values
and bindings to non-exposed parameters become constant expressions. Unused
parameters do not enter the runtime artifact.

Curves and gradients are lowered into renderer-independent interpolation
segments. Particle-attribute liveness is computed backwards from renderer and
runtime requirements; derived values such as normalized age remain transient
rather than occupying persistent particle storage.

`EffectInstance` owns runtime-ready slot values. Overrides are type checked and
compiled on ingress, including curve and gradient overrides, then evaluated
without rebuilding `CompiledEffect`.

## Consequences

Games, the viewer, and editor previews can use one parameter API without runtime
access to authored assets. Compiler optimization choices stay outside the saved
effect, semantic commands can bind and unbind inputs atomically, and the current
unbound sample retains byte-stable serialization and golden visual behavior.
