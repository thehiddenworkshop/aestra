# Optional physics adapters: temporarily suspended

Avian (`aestra-bevy-avian`, Avian 0.7) and Rapier (`aestra-bevy-rapier`,
Rapier 0.36) currently require Bevy 0.19. They are explicitly excluded from the
shipping Bevy 0.20 workspace. Their source,
examples and tests are preserved, but ordinary workspace CI no longer validates
these two integrations. This is a temporary availability reduction, not a claim
that they support Bevy 0.20.

Both adapters set `publish = false` while suspended to prevent accidentally
releasing an unqualified integration.

Both manifests are standalone workspaces with independent dependency resolution.
They still reference the local `aestra-bevy` implementation, which now uses 0.20.
These adapters must not be advertised or released until ported. Their frozen upstream
0.19 dependency pins record the unported integrations, **not** supported Aestra 0.19
compatibility. The obsolete 0.19 vendor patches and adapter patch declarations have
been removed. Do not run these manifests expecting working integrations.

To restore an adapter:

1. Select an upstream release supporting Aestra's Bevy version (or explicitly port it).
2. Re-align its Bevy dependency and patches; verify a single Bevy ECS/render graph.
3. Run its tests/examples, including moving-collider particle collisions.
4. Remove its standalone workspace/patch declarations, restore inherited metadata
   and dependencies, and add it back to root workspace validation. Re-enable
   publishing only after the integration has been qualified for release.

Core particle collision support is **not** disabled. `PhysicsScene`, `PhysicsProxy`,
`AestraPhysicsColliders` and the `custom_physics` example remain available without
either physics engine. Hosts can continue supplying their own colliders.
