# Optional physics adapters: temporarily suspended

Avian (`aestra-bevy-avian`, Avian 0.7) and Rapier (`aestra-bevy-rapier`,
Rapier 0.36) currently require Bevy 0.19. They are explicitly excluded from the
shipping workspace while Aestra prepares its Bevy 0.20 migration. Their source,
examples and tests are preserved, but ordinary workspace CI no longer validates
these two integrations. This is a temporary availability reduction, not a claim
that they support Bevy 0.20.

Both adapters set `publish = false` while suspended to prevent accidentally
releasing an unqualified integration.

Both manifests are standalone workspaces with independent dependency resolution.
They still reference the local `aestra-bevy` implementation: after that implementation
migrates to 0.20, these adapters must not be advertised or released until ported.
Their old Bevy pin does **not** make them compatible with a newer local Aestra.
The duplicated 0.19 patch declarations are only for testing the present baseline;
review/remove them alongside the adapter port, never relabel the vendored code.

On the current Bevy 0.19 baseline, opt-in checks from the repository root are:

```powershell
cargo test --manifest-path bevy/aestra-bevy-avian/Cargo.toml --lib
cargo test --manifest-path bevy/aestra-bevy-rapier/Cargo.toml --lib
```

These commands generate separate adapter lockfiles/target directories. Do not run
them against a migrated core expecting working integrations. To restore an adapter:

1. Select an upstream release supporting Aestra's Bevy version (or explicitly port it).
2. Re-align its Bevy dependency and patches; verify a single Bevy ECS/render graph.
3. Run its tests/examples, including moving-collider particle collisions.
4. Remove its standalone workspace/patch declarations, restore inherited metadata
   and dependencies, and add it back to root workspace validation. Re-enable
   publishing only after the integration has been qualified for release.

Core particle collision support is **not** disabled. `PhysicsScene`, `PhysicsProxy`,
`AestraPhysicsColliders` and the `custom_physics` example remain available without
either physics engine. Hosts can continue supplying their own colliders.
