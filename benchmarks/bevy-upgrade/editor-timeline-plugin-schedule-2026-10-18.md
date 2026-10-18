# B20-2 slice 55: TimelinePlugin schedule integration

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
installs the same production `TimelinePlugin` as shipping. Its nine action and drop
observers, three chained `Update` groups, and host-motion observer/system wiring
are shared directly from `timeline/plugin.rs` and `timeline/host_motion_install.rs`.
The existing timeline and host-motion modules retain their native APIs; the
candidate imports the production execution, region, and visual systems needed by
the installed schedule. No authored format, control geometry, or root engine
dependency changes are part of this slice.

Two shared tests exercise the installed plugin. One activates a real timeline
button, verifies its snap action is consumed exactly once, clears the pending and
pressed markers, and leaves document history clean. The other triggers a direct
action and initializes the full `Update` schedule, then checks the executable
order of input, action, visual, and host-motion systems by Bevy's registered
system keys. It also verifies both host-motion systems are installed. System keys
are used because release builds deliberately hide system names.

These headless tests qualify installation, action routing, and explicit schedule
dependencies. They do not execute a native window layout, pointer picking,
rendering, floating windows, or the full application workflow. Earlier pointer
ownership and stale-action hardening remain separate work.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/10472ab1524d4a81a973a57fd2021c77/`.
It executes **1,521 checks**, including **1,100 editor graph/session/history and
consumer checks**, with all **873 input hashes** unchanged. These counts include
executions across overlapping gates, not unique tests. The graph suite reports
one previously ignored timing probe. The runner confirms Bevy **0.20.0**, Rust
**1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`a3673387bd68484d2f013b4ce7daf9180f17d13338f77da93fe92f6c4853b100`.

The full Bevy 0.20 editor graph suite passed **1,100 tests** with one existing
ignore. The two new plugin tests also passed in isolation before the frozen run.
Shipping Bevy **0.19.1** passed **2,128 editor unit tests** with ten existing
ignores and **3 architecture tests**. All shipping editor and candidate targets
pass Clippy with warnings denied. Root and candidate
formatting checks and Git whitespace validation pass.

Next: qualify Curves action/panel/plugin integration, then remaining viewport and
application consumers and native workflow acceptance. B20-2 remains open; the
shipping engine stays on 0.19.1 until the complete compatibility gate passes.
