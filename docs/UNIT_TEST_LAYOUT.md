# Rust unit-test layout

Keep external unit-test files together in a `tests/` folder inside their owning
subsystem. Do not create an implementation-named directory containing only a
`tests.rs` file.

Examples:

- `src/asset_browser/tests/bookmarks.rs`
- `src/asset_browser/tests/deletion_history.rs`
- `src/feathers/node_graph/tests/geometry.rs`
- `src/gpu/tests/extraction.rs`
- `src/tests/choreography.rs` for a crate-root module without a subsystem directory

Keep production subfolders when they contain implementation files. Use descriptive
test filenames; flatten nested names when useful (`deletion_history.rs` rather
than another test-only `deletion/history/` hierarchy). Already established nested
subsystems, such as asset-drop adapters, can keep their own `tests/` folders.

The test file remains a child module of its original implementation, even when
its physical location changes:

```rust
// In asset_browser/bookmarks.rs:
#[cfg(test)]
#[path = "tests/bookmarks.rs"]
mod tests;
```

This preserves private-helper access, module identity and Cargo test filters.
Small inline unit tests may stay next to their implementation. Shared test helpers
do not have to move unless their ownership becomes clearer by doing so.

Cargo integration tests stay in each crate's existing top-level `tests/` directory.
Do not move vendored sources, generated files or historical benchmark reports.
When relocating a file, update relative fixture includes and live qualification
harnesses/source lists, then run formatting, affected tests and strict Clippy.

The editor's `unit_test_layout` integration test checks this convention across
repository-owned `src/` trees without modifying Cargo integration-test placement.
