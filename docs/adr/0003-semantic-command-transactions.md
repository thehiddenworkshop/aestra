# ADR 0003: Semantic command transactions

- Status: accepted
- Date: 2026-08-26

## Context

The prototype editor mutated the effect through arbitrary closures and implemented
undo/redo with complete document snapshots. That path could not be invoked safely
by scripts, reviewed as a semantic change, locked at object boundaries, or reused
by a future AI client.

## Decision

All authored effect mutations use UI-independent `EffectCommand` values grouped
into named `EffectTransaction` values.

- A transaction applies to a temporary document and replaces the working asset
  only after complete semantic validation succeeds.
- Each applied command produces explicit inverse commands.
- Undo and redo store forward and inverse transactions, not document snapshots.
- Commands address objects with typed semantic IDs and fail when targets are
  missing or locked.
- Selection and locks use semantic IDs rather than collection indices.
- Every successful transaction produces an `EffectDiff`.
- Bevy UI controls submit the same operations available to tests, scripts, and
  future tool clients.

## Consequences

Editor actions remain deterministic, atomic, inspectable, and undoable without
simulating UI input. Transaction execution currently clones the semantic document
to guarantee rollback; this can be optimized later without changing the public
command contract.

