# Aestra candidate patch for bevy_pbr 0.20.0

This is the actual published 0.20.0 crate with original shaders, assets and licenses.
It is used only by `benchmarks/bevy-upgrade/patches-020`. The working 0.19 workspace
still selects `vendor/bevy_pbr`. `UPSTREAM_SHA256.json` records the registry archive
checksum and all original file hashes. Intentional changes are limited to
`src/cluster/gpu.rs` and `src/cluster/mod.rs`.

Ported contracts from the 0.19 Aestra patch:

- Retain per-view public and private cluster buffers at their high-water capacity,
  resetting logical counts, scratchpad lengths and current metadata every frame.
- Preserve asynchronous adaptive growth and shader overflow protection.
- Bound metadata readback to eight 48-byte staging buffers per view; remove the
  pending owner on success or failure. Saturation skips feedback, never GPU work.
- Retire buffers, bind groups and readback owners on inactive/removed views or a
  GPU-to-CPU clustering switch. GPU resumption restores storage bindings.
- Keep opt-in, doc-hidden read-only lifetime diagnostics with weak readback probes.
  No production GPU wait, host particle readback, Aestra light policy or shader edit.

0.20-specific reconciliation preserves WESL asset paths, render pipeline constants,
`ExtractResource<RenderApp, ...>` and `clusterable_decal_count()` (light textures
share decal storage but are not clusterable objects). wgpu 30's fallible
`get_mapped_range()` joins the existing decode-failure cleanup path: unmap and
discard the slot, rather than panic or leak a pending staging owner.

Qualification includes the four existing pool/reset/growth unit tests and a 0.20
port of both native cluster lifetime tests. Native tests remain explicit and serial;
they check actual buffer generations, overflow recovery, per-view independence,
mode switches and named allocator retirement. They are not a fireworks visual or
performance acceptance gate. Root patches do not propagate to downstream crates.
