# Optional physics adapters — Bevy 0.20

Re-enabled on 2026-10-23 after upstream released compatible versions:

| Aestra integration | Released backend | Bevy requirement |
| --- | --- | --- |
| `aestra-bevy-avian` | `avian3d` 0.8.0 (2026-10-20) | ^0.20 |
| `aestra-bevy-rapier` | `bevy_rapier3d` 0.37.0 (2026-10-10) | ^0.20 |

Sources: [Avian released manifest](https://docs.rs/crate/avian3d/0.8.0/source/Cargo.toml),
[Avian announcement](https://joonaa.dev/blog/14/avian-0-8),
[Rapier plugin released manifest](https://docs.rs/crate/bevy_rapier3d/0.37.0/source/Cargo.toml).
The relevant Rapier version is the **Bevy plugin**, not the standalone `rapier3d` version.

Both adapters are ordinary root workspace members again, with inherited package metadata,
Bevy dependency and qualified patches, and one shared root lockfile. The previous
standalone workspace declarations and publishing suspension are removed. Obsolete local
adapter lockfiles from the suspended period are ignored and unused; Cargo resolves these
members through the root. No Git forks or Bevy 0.19 compatibility graph are needed.

Avian uses `3d` / `parry-f32`; 0.8 removed the separate `f32` feature. Rapier uses `dim3`
and its shape-query callback now receives both the entity and backend collider. Both
continue disabling upstream default features, so optional physics debug rendering,
scene loaders and picking integrations are not pulled in by the adapters.

Root formatting, all-target checks, strict Clippy and workspace tests include both
adapters on Windows and Linux CI. The redundant suspended-manifest formatting step is
removed, not the validation coverage. Local compatibility tests cover sphere, rotated
box, capsule, half-space and compound proxies, moving/scaled kinematic colliders,
out-of-range/removal cleanup and nearest-collider budgets after the effect moves.

Local restoration validation (Windows, Rust 1.98.1): all six adapter integration tests
and both documentation examples pass. Both sparks examples build, launch, save their
automatic four-second screenshots and exit successfully; the captures show particles,
the ramp, ground and simulated balls. These are launch/visual smoke checks, not a
deterministic GPU collision-conformance or performance benchmark. Captures are retained
under `target/physics-adapters-2026-10-23-{avian,rapier}.png`. Strict workspace all-target
Clippy with `aestra-bench/gpu`, workspace formatting and CI YAML parsing pass. Cargo's
active graph contains only Bevy/ECS/render/math 0.20; core `aestra-bevy` pulls neither
backend. Full workspace test execution and Linux/native runs remain CI follow-ups.

```powershell
cargo test -p aestra-bevy-avian -p aestra-bevy-rapier --all-targets --locked
cargo test -p aestra-bevy-avian -p aestra-bevy-rapier --doc --locked
cargo clippy -p aestra-bevy-avian -p aestra-bevy-rapier --all-targets --locked -- -D warnings
cargo run -p aestra-bevy-avian --example avian_sparks --locked
cargo run -p aestra-bevy-rapier --example rapier_sparks --locked
```

These integrations remain **optional**: `aestra-bevy` has no dependency on either engine,
and games choose an adapter explicitly. `PhysicsScene`, `PhysicsProxy`,
`AestraPhysicsColliders` and the `custom_physics` example work without them. Exact proxy
primitives and compound decomposition retain their existing behavior; unsupported
geometry still uses local bounding boxes, not exact mesh collisions. Physics inputs
remain forward-only host state, not a replayable physics-world snapshot.
