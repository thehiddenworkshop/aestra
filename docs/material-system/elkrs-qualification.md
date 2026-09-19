# `elkrs` 0.1.1 qualification — M9

Qualified 2026-09-19 for Aestra's explicit whole-graph Arrange command.

## Exact dependency

- Crate: `elkrs = "=0.1.1"`
- crates.io archive SHA-256: `a0aa6d17007599c4bb42b342b55148832289bc8c7e41d83f01b19af1ef363de4`
- Packaged source VCS revision: `344c18cc1992ef39a622d1ca9f97cf89e159cb36`
- Packaged repository metadata: `https://github.com/depetrol/elkrs`
- License: Apache-2.0. Preserve the packaged license and third-party attribution when
  distributing binaries. There is no additional packaged NOTICE file.
- Direct dependencies: `indexmap 2`, `serde 1`, `serde_json 1`.

The package does not declare `rust-version`. Aestra therefore treats compatibility as
empirical: compilation and tests pass with the repository-pinned
`1.98.1-x86_64-pc-windows-msvc` toolchain on Windows.

## Capability result

Accepted behind `feathers::graph_layout`, not as editor state ownership.

- Native Rust `create_elk().layout_json(...)` needs no Node/JVM process.
- Layered RIGHT/DOWN directions, measured width/height, spline routing options and
  deterministic canonical input all work for the M9 subset.
- Output node positions remain Aestra coordinates. Aestra validates exact node identity,
  finite coordinates, bounds and fixed-node constraints before mapping results back.
- Existing cubic graph wires remain unchanged; ELK edge sections are intentionally ignored.
- Partial regions and pinned nodes fail closed until M10/M12 define boundary-anchor policy.
- ELK errors become structured adapter errors and never mutate graph presentation.
- Tests cover unequal measured sizes, linear/diamond/fan topology, disconnected nodes,
  cycles, deterministic repeats, no overlap, dependency direction and avoidable crossings.

## Execution boundary

Arrange owns at most one compute-pool task. The task receives an immutable graph snapshot and
cannot read ECS state. Completion is rejected if the project/document lifetime, topology,
mounted view, geometry revision, or graph placement revision changed. Starting another Arrange
while one is active is rejected instead of creating an unbounded queue.

Applying a valid result is one presentation-only transaction. One Undo restores the exact
previous base placements, including absence of previously unsaved bootstrap positions. It does
not modify material semantics, trigger compilation, or replace Aestra's cubic wires.

## Representative Windows performance

Command:

```text
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --bin graph_layout -- --iterations 20
```

Development profile after five warm-ups per case:

| Graph | Edges | Median | p95 | Max |
|---:|---:|---:|---:|---:|
| 16 nodes | 18 | 0.912 ms | 1.002 ms | 1.057 ms |
| 64 nodes | 82 | 3.551 ms | 3.730 ms | 3.774 ms |
| 256 nodes | 338 | 13.277 ms | 13.738 ms | 13.850 ms |

These are qualification samples, not release performance promises. The benchmark lives in
`apps/aestra-bench`, accepts `--iterations`, and can be rerun on target hardware.

## Decision

Qualifies for the explicit M9 full-graph prototype. Keep the dependency exact-pinned and the
Aestra abstraction/fallback. Requalify before using ports, compound graphs, hard pins, partial
regions, or ELK-provided wire routes.
