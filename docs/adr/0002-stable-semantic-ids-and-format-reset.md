# ADR 0002: Stable semantic IDs and the format-v2 reset

- Status: superseded by ADR 0006 for the current asset version
- Date: 2026-08-26

## Context

Editor selection, graph edges, events, commands, locks, diffs, diagnostics, and
future automation must address semantic objects independently of display names
or vector positions. The prototype format did not give every addressable nested
object a stable identity.

## Decision

Every independently addressable semantic object uses a typed ID newtype backed
by 128 bits and serialized in canonical UUID text form. An `EmitterId` cannot be
used where a `ModuleId` is required. Names and labels are mutable metadata and
never serve as references.

- Newly authored objects receive random UUID-v4 values.
- Duplicating an object assigns new IDs to the duplicate and all owned nested
  objects.
- References and editor metadata store typed IDs, never collection indices.
- Runtime storage slots and transient particle handles are not semantic IDs.
- Deterministic collections and serialization must not depend on randomized hash
  iteration order.

Format v2 replaces the prototype format directly:

- only `CURRENT_FORMAT_VERSION` is accepted;
- format v1 has no compatibility parser or migration module;
- invalid and unsupported documents return structured diagnostics;
- all checked-in examples and tests use format v2.

Future compatibility policy will be decided before Aestra publishes a stable
asset-format guarantee. This decision removes only the prototype v1 maintenance
burden.

## Consequences

The semantic model adds a UUID dependency with serialization and v4 support.
Human-authored RON is slightly more verbose, but commands and diffs remain stable
across renames, reordering, sessions, and UI projections. Prototype v1 assets
must be recreated as v2 assets.
