# ADR 0002: Stable semantic IDs and format migration

- Status: accepted
- Date: 2026-08-26

## Context

Editor selection, graph edges, events, commands, locks, diffs, diagnostics, and
future automation must address semantic objects independently of display names
or vector positions. Format v1 has string IDs for effects and layers, but nested
objects such as curves and future module instances do not have stable identity.

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

Format migration is explicit:

- each historical source schema remains readable through a dedicated migration
  type or module;
- v1-to-v2 migration derives UUID-v5 IDs from an Aestra migration namespace,
  the legacy effect ID, object kind, and legacy semantic path;
- loading the same v1 document repeatedly therefore produces identical v2 IDs;
- migration returns structured diagnostics and never silently drops unknown or
  invalid data;
- saves emit only the current format, while migration fixtures verify every
  supported historical version.

## Consequences

The semantic model will add a UUID dependency with serialization and v4/v5
support. Human-authored RON becomes slightly more verbose, but commands and diffs
remain stable across renames, reordering, sessions, and UI projections. Legacy
objects that had no identity receive repeatable IDs during migration.

