//! Standalone WESL source documents (Milestone 7).
//!
//! WESL modules are plain `.wesl` text files, distinct from the semantic material assets. They are
//! identified by their project-relative path (hashed into a stable, serializable [`WeslSourceId`] so
//! it can key a [`crate::document::DocumentKey`]) and their text lives in an in-memory buffer here.
//!
//! Milestone 7-1 opens them read-only; editing, dirty tracking, save, and diagnostics are layered on
//! in later slices.
#![allow(dead_code)] // Editing/save/diagnostics consume the rest from Milestone 7-2+.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Stable identity of a WESL source document: a deterministic hash of its project-relative path.
/// Serializable so it can persist in the editor-document manifest, and `Copy` so [`DocumentKey`]
/// stays `Copy`. Like the source-tree ids, it changes if the file is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct WeslSourceId {
    // Two u64 halves rather than a u128 so the id round-trips cleanly through RON in the manifest.
    high: u64,
    low: u64,
}

impl WeslSourceId {
    pub(crate) fn for_relative_path(relative: &Path) -> Self {
        // Normalize separators so the same file yields the same id on every platform.
        let normalized = relative.to_string_lossy().replace('\\', "/");
        let hash = |salt: u64| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            salt.hash(&mut hasher);
            normalized.hash(&mut hasher);
            hasher.finish()
        };
        Self {
            high: hash(0),
            low: hash(0xa5a5_5a5a),
        }
    }
}

impl std::fmt::Display for WeslSourceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:016x}{:016x}", self.high, self.low)
    }
}

/// One open WESL source buffer. `disk` is the last text read from (or written to) the file, so a
/// future editing slice can compare it against `text` for dirty state.
#[derive(Debug, Clone)]
struct WeslBuffer {
    relative_path: PathBuf,
    text: String,
    disk: String,
    revision: u64,
}

/// In-memory registry of open WESL source buffers, keyed by [`WeslSourceId`].
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslDocuments {
    buffers: BTreeMap<WeslSourceId, WeslBuffer>,
}

impl WeslDocuments {
    /// Opens (or refreshes) a WESL buffer for `relative_path`, returning its id. Opening the same
    /// file again refreshes the on-disk baseline without discarding an existing buffer identity.
    pub(crate) fn open(&mut self, relative_path: PathBuf, text: String) -> WeslSourceId {
        let id = WeslSourceId::for_relative_path(&relative_path);
        self.buffers
            .entry(id)
            .and_modify(|buffer| buffer.disk = text.clone())
            .or_insert_with(|| WeslBuffer {
                relative_path,
                text: text.clone(),
                disk: text,
                revision: 0,
            });
        id
    }

    pub(crate) fn text(&self, id: WeslSourceId) -> Option<&str> {
        self.buffers.get(&id).map(|buffer| buffer.text.as_str())
    }

    pub(crate) fn relative_path(&self, id: WeslSourceId) -> Option<&Path> {
        self.buffers
            .get(&id)
            .map(|buffer| buffer.relative_path.as_path())
    }

    pub(crate) fn is_open(&self, id: WeslSourceId) -> bool {
        self.buffers.contains_key(&id)
    }

    /// Replaces the buffer text, advancing its revision when the text actually changed. Returns
    /// whether it changed, so callers can avoid redundant work.
    pub(crate) fn set_text(&mut self, id: WeslSourceId, text: String) -> bool {
        let Some(buffer) = self.buffers.get_mut(&id) else {
            return false;
        };
        if buffer.text == text {
            return false;
        }
        buffer.text = text;
        buffer.revision += 1;
        true
    }

    /// Records that the current buffer text was written to disk, clearing the dirty state.
    pub(crate) fn mark_saved(&mut self, id: WeslSourceId) {
        if let Some(buffer) = self.buffers.get_mut(&id) {
            buffer.disk = buffer.text.clone();
        }
    }

    /// Reverts the buffer to the last on-disk text (the discard action).
    pub(crate) fn revert(&mut self, id: WeslSourceId) {
        if let Some(buffer) = self.buffers.get_mut(&id) {
            buffer.text = buffer.disk.clone();
        }
    }

    pub(crate) fn is_dirty(&self, id: WeslSourceId) -> bool {
        self.buffers
            .get(&id)
            .is_some_and(|buffer| buffer.text != buffer.disk)
    }

    pub(crate) fn revision(&self, id: WeslSourceId) -> Option<u64> {
        self.buffers.get(&id).map(|buffer| buffer.revision)
    }

    /// The open WESL documents that currently have unsaved edits, as (id, relative path).
    pub(crate) fn dirty_documents(&self) -> impl Iterator<Item = (WeslSourceId, &Path)> + '_ {
        self.buffers
            .iter()
            .filter(|(_, buffer)| buffer.text != buffer.disk)
            .map(|(id, buffer)| (*id, buffer.relative_path.as_path()))
    }

    pub(crate) fn close(&mut self, id: WeslSourceId) {
        self.buffers.remove(&id);
    }

    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Open buffers as (id, relative path, revision, text) — used to recompile changed modules.
    fn iter(&self) -> impl Iterator<Item = (WeslSourceId, &Path, u64, &str)> + '_ {
        self.buffers.iter().map(|(id, buffer)| {
            (
                *id,
                buffer.relative_path.as_path(),
                buffer.revision,
                buffer.text.as_str(),
            )
        })
    }
}

/// The compile state of a WESL module: clean, or the compiler's error message plus the 1-based
/// source line it points at (best-effort, parsed from the message) so the editor can mark it inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WeslCompileState {
    /// Compiled cleanly; carries the composed WGSL for the Compiler Inspector.
    Ok { wgsl: String },
    Error {
        message: String,
        line: Option<usize>,
        /// The failing character range in the source (start..end, char indices), parsed from the
        /// compiler's `chars A..B`, so the editor can underline the exact tokens and the diagnostics
        /// panel can jump the caret to them. Best-effort; `None` if the message has no range.
        span: Option<(usize, usize)>,
    },
}

/// Best-effort extraction of a 1-based line number from a WESL/Naga compiler message. Naga formats
/// locations as `wgsl:LINE:COL`; generated WGSL lines align with WESL for modules without imports.
fn error_line(message: &str) -> Option<usize> {
    for token in message.split(|c: char| c.is_whitespace() || c == '│' || c == '┌' || c == '─')
    {
        let digits = token
            .strip_prefix("wgsl:")
            .and_then(|rest| rest.split(':').next());
        if let Some(line) = digits.and_then(|line| line.parse::<usize>().ok()) {
            return Some(line);
        }
    }
    None
}

/// Per-document WESL compile diagnostics, refreshed whenever a buffer's revision changes. Paired
/// with the module's compiled revision so a stale buffer is not reported.
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslDiagnostics {
    entries: BTreeMap<WeslSourceId, (u64, WeslCompileState)>,
}

impl WeslDiagnostics {
    pub(crate) fn state(&self, id: WeslSourceId) -> Option<&WeslCompileState> {
        self.entries.get(&id).map(|(_, state)| state)
    }

    /// The composed WGSL of a cleanly-compiled module, for the Compiler Inspector.
    pub(crate) fn wgsl(&self, id: WeslSourceId) -> Option<&str> {
        match self.state(id)? {
            WeslCompileState::Ok { wgsl } => Some(wgsl),
            WeslCompileState::Error { .. } => None,
        }
    }

    /// The current compile errors across open WESL buffers, as (source id, message, one-based line,
    /// failing char span), for the unified diagnostics panel.
    #[allow(clippy::type_complexity)]
    pub(crate) fn errors(
        &self,
    ) -> impl Iterator<Item = (WeslSourceId, &str, Option<usize>, Option<(usize, usize)>)> {
        self.entries
            .iter()
            .filter_map(|(id, (_, state))| match state {
                WeslCompileState::Error {
                    message,
                    line,
                    span,
                } => Some((*id, message.as_str(), *line, *span)),
                WeslCompileState::Ok { .. } => None,
            })
    }
}

/// Sanitizes one path segment into a valid WESL/WGSL identifier (a module-path segment).
fn sanitize_wesl_ident(segment: &str) -> String {
    let mut name: String = segment
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        name.insert(0, '_');
    }
    name
}

/// The WESL module name for a project-relative path, built from its folder segments and file stem
/// joined by `::` (each sanitized to an identifier). Encoding the whole path — not just the stem —
/// keeps same-stem files in different folders distinct (`shaders/noise.wesl` → `shaders::noise`,
/// `lib/noise.wesl` → `lib::noise`), so they never collide in import resolution or the dependency
/// graph. A root-level file keeps its bare stem (`noise.wesl` → `noise`).
pub(crate) fn module_name_for(path: &Path) -> String {
    let mut segments: Vec<String> = Vec::new();
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(text) = component
                && let Some(text) = text.to_str()
            {
                segments.push(sanitize_wesl_ident(text));
            }
        }
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("module");
    segments.push(sanitize_wesl_ident(stem));
    segments.join("::")
}

/// The WESL compiler module path for a module of the given (sanitized) name. Modules live under the
/// `package::` root so `import package::<name>::<item>;` resolves against the other open modules —
/// bare names would resolve relative to the importing module and fail.
pub(crate) fn wesl_module_path(module_name: &str) -> String {
    format!("package::{module_name}")
}

/// Compiles a WESL module (via the WESL compiler and Naga validation) to a clean/error state.
/// `imports` are the other available modules as `(sanitized name, source)`, registered so this
/// module's `import package::<name>::…;` statements resolve.
pub(crate) fn compile_wesl_source(
    module_name: &str,
    source: &str,
    imports: &[(&str, &str)],
) -> WeslCompileState {
    let module_path = wesl_module_path(module_name);
    let import_paths: Vec<(String, &str)> = imports
        .iter()
        .map(|(name, src)| (wesl_module_path(name), *src))
        .collect();
    let import_refs: Vec<(&str, &str)> = import_paths
        .iter()
        .map(|(path, src)| (path.as_str(), *src))
        .collect();
    match aestra_gpu::shader::compile_wesl_with_imports(&module_path, source, &[], &import_refs) {
        Ok(compiled) => WeslCompileState::Ok {
            wgsl: compiled.wgsl,
        },
        Err(error) => {
            // The WESL compiler formats errors with ANSI colour codes for a terminal; strip them so
            // the diagnostics panel shows plain text.
            let message = strip_ansi(&error.to_string());
            // A validation error's line is a line of the *composed* WGSL. For a module with no
            // imports that composed WGSL matches the source 1:1, so the line is the source line; but
            // once imports prepend code the line shifts and no longer maps to the source, so it is
            // suppressed rather than point at the wrong line.
            let line = if crate::wesl_syntax::imported_modules(source).is_empty() {
                error_line(&message)
            } else {
                None
            };
            // Parse errors carry a `chars A..B` range; semantic errors (e.g. duplicate declaration)
            // do not, so fall back to the offending identifier's location.
            let span =
                error_char_span(&message, source).or_else(|| identifier_span(&message, source));
            WeslCompileState::Error {
                message,
                line,
                span,
            }
        }
    }
}

/// Best-effort extraction of the failing character range from a WESL compiler message, which formats
/// locations as `chars A..B` (byte offsets into the source). Converts them to char indices so the
/// editor can underline exactly those characters. Returns `None` when no range is present.
fn error_char_span(message: &str, source: &str) -> Option<(usize, usize)> {
    let rest = message.split("chars ").nth(1)?;
    let digits = rest
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|part| part.contains(".."))?;
    let (start, end) = digits.split_once("..")?;
    let start: usize = start.parse().ok()?;
    let end: usize = end.parse().ok()?;
    let byte_to_char = |byte: usize| source.char_indices().take_while(|(i, _)| *i < byte).count();
    let (start, end) = (byte_to_char(start), byte_to_char(end));
    (start <= end).then_some((start, end))
}

/// Fallback location for errors with no character range: the offending identifier named in the
/// message (e.g. ``duplicate declaration of `name` ``), underlining its last occurrence in the
/// source — for a duplicate, that is the redeclaration.
fn identifier_span(message: &str, source: &str) -> Option<(usize, usize)> {
    let name = message
        .split("declaration of `")
        .nth(1)?
        .split('`')
        .next()?;
    if name.is_empty() {
        return None;
    }
    let byte = source.rfind(name)?;
    let start = source[..byte].chars().count();
    Some((start, start + name.chars().count()))
}

/// Removes ANSI escape sequences (e.g. colour codes) from `input`.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // A CSI sequence is ESC '[' … final-byte (0x40..=0x7e); drop it whole. A lone ESC is dropped.
        if chars.clone().next() == Some('[') {
            chars.next();
            for byte in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&byte) {
                    break;
                }
            }
        }
    }
    out
}

/// A snapshot of one open WESL module for a recompile pass: its identity, sanitized module name,
/// buffer revision, the modules it imports, and its source.
struct WeslModuleSnapshot {
    id: WeslSourceId,
    name: String,
    revision: u64,
    imports: Vec<String>,
    source: String,
}

/// The set of module names to recompile: the `dirty` modules (own buffer changed) plus every module
/// that transitively imports one of them, so a dependency's change re-validates its dependents.
fn modules_needing_recompile<'a>(
    modules: &'a [WeslModuleSnapshot],
    dirty: &HashSet<&'a str>,
) -> HashSet<&'a str> {
    // Reverse dependency edges: a module name → the modules that import it.
    let mut importers: HashMap<&str, Vec<&str>> = HashMap::new();
    for module in modules {
        for dependency in &module.imports {
            importers
                .entry(dependency.as_str())
                .or_default()
                .push(module.name.as_str());
        }
    }
    let mut result: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = dirty.iter().copied().collect();
    while let Some(name) = stack.pop() {
        if !result.insert(name) {
            continue; // already visited (also breaks import cycles)
        }
        if let Some(dependents) = importers.get(name) {
            stack.extend(dependents.iter().copied());
        }
    }
    result
}

/// Recompiles WESL modules whose buffer changed since their last compile — and every module that
/// transitively imports a changed one — and drops entries for closed documents. Runs only when the
/// document store changes.
pub(crate) fn recompile_changed_wesl(
    documents: Res<WeslDocuments>,
    mut diagnostics: ResMut<WeslDiagnostics>,
) {
    if !documents.is_changed() {
        return;
    }
    // Snapshot every open module (name, revision, source, and its `import`ed module names) so any
    // module can import any other. Owned copies detach the borrow of `documents` before the
    // diagnostics are written.
    let modules: Vec<WeslModuleSnapshot> = documents
        .iter()
        .map(|(id, path, revision, text)| WeslModuleSnapshot {
            id,
            name: module_name_for(path),
            revision,
            imports: crate::wesl_syntax::imported_modules(text),
            source: text.to_owned(),
        })
        .collect();

    // A module recompiles when its own buffer changed, or when a module it (transitively) imports
    // changed — its composed WGSL and diagnostics depend on that dependency.
    let dirty: HashSet<&str> = modules
        .iter()
        .filter(|module| {
            diagnostics
                .entries
                .get(&module.id)
                .is_none_or(|(compiled, _)| *compiled != module.revision)
        })
        .map(|module| module.name.as_str())
        .collect();
    let to_recompile = modules_needing_recompile(&modules, &dirty);

    for module in &modules {
        if !to_recompile.contains(module.name.as_str()) {
            continue;
        }
        // Every other open module is available for `import package::<name>::…;`. An unimported
        // module is simply never composed, so listing them all is safe.
        let imports: Vec<(&str, &str)> = modules
            .iter()
            .filter(|other| other.id != module.id)
            .map(|other| (other.name.as_str(), other.source.as_str()))
            .collect();
        let state = compile_wesl_source(&module.name, &module.source, &imports);
        diagnostics.entries.insert(module.id, (module.revision, state));
    }
    diagnostics
        .entries
        .retain(|id, _| documents.buffers.contains_key(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_char_span_parses_the_failing_range_as_char_indices() {
        let source = "fn main() {}"; // ASCII: byte offset == char index
        assert_eq!(
            error_char_span("error: chars 3..7: unexpected token", source),
            Some((3, 7))
        );
        assert_eq!(error_char_span("no range in this message", source), None);
    }

    #[test]
    fn identifier_span_underlines_the_redeclaration() {
        let source = "fn foo() {}\nfn foo() {}";
        // The last occurrence (the duplicate) is underlined.
        let span = identifier_span("error: duplicate declaration of `foo`", source);
        let (start, end) = span.unwrap();
        assert_eq!(
            &source[..]
                .chars()
                .skip(start)
                .take(end - start)
                .collect::<String>(),
            "foo"
        );
        assert!(start > source.find("foo").unwrap()); // the second `foo`, not the first
    }

    #[test]
    fn strip_ansi_removes_colour_codes_but_keeps_text() {
        let coloured = "\u{1b}[1m\u{1b}[91merror\u{1b}[0m: duplicate declaration of `value_noise`";
        assert_eq!(
            strip_ansi(coloured),
            "error: duplicate declaration of `value_noise`"
        );
        assert_eq!(strip_ansi("plain message"), "plain message");
    }

    #[test]
    fn same_relative_path_yields_a_stable_id_across_separators() {
        let a = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let b = WeslSourceId::for_relative_path(Path::new("shaders\\noise.wesl"));
        assert_eq!(a, b);
        let other = WeslSourceId::for_relative_path(Path::new("shaders/other.wesl"));
        assert_ne!(a, other);
    }

    #[test]
    fn opening_registers_text_and_is_idempotent_by_path() {
        let mut documents = WeslDocuments::default();
        let id = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() {}".into());
        assert_eq!(documents.text(id), Some("fn main() {}"));
        assert_eq!(
            documents.relative_path(id),
            Some(Path::new("shaders/noise.wesl"))
        );
        assert!(!documents.is_dirty(id));
        // Reopening the same path returns the same id and refreshes the baseline.
        let again = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() { }".into());
        assert_eq!(again, id);
        assert_eq!(documents.len(), 1);
    }

    #[test]
    fn compile_reports_ok_for_valid_and_an_error_for_invalid_wesl() {
        // A module with a reachable entry point composes to WGSL that carries it; the Compiler
        // Inspector shows this text. (Pure library modules with no entry point tree-shake to empty
        // WGSL, which still compiles cleanly.)
        match compile_wesl_source(
            "noise",
            "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
            &[],
        ) {
            WeslCompileState::Ok { wgsl } => assert!(wgsl.contains("fn main")),
            WeslCompileState::Error { .. } => panic!("expected a clean compile for valid WESL"),
        }
        // Unused declarations are tree-shaken away, so a lone function composes to empty WGSL.
        match compile_wesl_source(
            "noise",
            "fn add(a: f32, b: f32) -> f32 { return a + b; }",
            &[],
        ) {
            WeslCompileState::Ok { wgsl } => assert!(wgsl.trim().is_empty()),
            WeslCompileState::Error { .. } => panic!("expected a clean compile for valid WESL"),
        }
        match compile_wesl_source("noise", "fn broken( {", &[]) {
            WeslCompileState::Error { message, .. } => assert!(!message.is_empty()),
            WeslCompileState::Ok { .. } => panic!("expected a compile error for malformed WESL"),
        }
    }

    #[test]
    fn module_name_encodes_folders_so_same_stem_files_do_not_collide() {
        assert_eq!(module_name_for(Path::new("noise.wesl")), "noise");
        assert_eq!(module_name_for(Path::new("shaders/noise.wesl")), "shaders::noise");
        assert_eq!(module_name_for(Path::new("lib/noise.wesl")), "lib::noise");
        // Same stem, different folders → distinct module names.
        assert_ne!(
            module_name_for(Path::new("shaders/noise.wesl")),
            module_name_for(Path::new("lib/noise.wesl"))
        );
        // Non-identifier segments are sanitized.
        assert_eq!(
            module_name_for(Path::new("fx-lib/2d/noise.wesl")),
            "fx_lib::_2d::noise"
        );
    }

    #[test]
    fn error_line_is_reported_without_imports_but_suppressed_with_them() {
        // A validation error (returning f32 where vec4<f32> is required) in a module with no imports
        // reports a source line.
        let bad = "@fragment fn main() -> @location(0) vec4<f32> { return 1.0; }";
        match compile_wesl_source("main", bad, &[]) {
            WeslCompileState::Error { line, .. } => {
                assert!(line.is_some(), "a no-import module should report its source line")
            }
            WeslCompileState::Ok { .. } => panic!("expected a validation error"),
        }
        // The same error in a module that imports: its composed-WGSL line no longer maps to the
        // source, so the line is suppressed (the message still shows).
        let helper = "fn unused() -> f32 { return 0.0; }";
        let importing = "import package::helpers::unused;\n\
                         @fragment fn main() -> @location(0) vec4<f32> { return 1.0; }";
        match compile_wesl_source("main", importing, &[("helpers", helper)]) {
            WeslCompileState::Error { line, .. } => assert!(
                line.is_none(),
                "an importing module's composed line must be suppressed, not shown wrong"
            ),
            WeslCompileState::Ok { .. } => panic!("expected a validation error"),
        }
    }

    #[test]
    fn a_module_in_a_folder_can_be_imported_by_its_path() {
        // A helper in a subfolder is imported by its folder-qualified path.
        let helper = "fn scale(x: f32) -> f32 { return x * 2.0; }";
        let main = "import package::shaders::noise::scale;\n\
                    @fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(scale(0.5)); }";
        match compile_wesl_source("main", main, &[("shaders::noise", helper)]) {
            WeslCompileState::Ok { wgsl } => assert!(wgsl.contains("fn main")),
            WeslCompileState::Error { message, .. } => {
                panic!("expected a folder-qualified import to resolve, got: {message}")
            }
        }
    }

    #[test]
    fn a_module_can_import_another_open_module() {
        // A library module of helpers, imported by an entry-point module. With the helper passed as
        // an available import, the entry point composes cleanly; without it, the import is unresolved.
        let helper = "fn scale(x: f32) -> f32 { return x * 2.0; }";
        let main = "import package::helpers::scale;\n\
                    @fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(scale(0.5)); }";
        match compile_wesl_source("main", main, &[("helpers", helper)]) {
            WeslCompileState::Ok { wgsl } => assert!(wgsl.contains("fn main")),
            WeslCompileState::Error { message, .. } => {
                panic!("expected the import to resolve, got: {message}")
            }
        }
        // Without the helper module available, the import cannot resolve.
        match compile_wesl_source("main", main, &[]) {
            WeslCompileState::Error { .. } => {}
            WeslCompileState::Ok { .. } => panic!("expected an unresolved-import error"),
        }
    }

    #[test]
    fn recompile_set_covers_transitive_dependents_and_terminates_on_cycles() {
        fn snap(name: &str, imports: &[&str]) -> WeslModuleSnapshot {
            WeslModuleSnapshot {
                id: WeslSourceId::for_relative_path(Path::new(name)),
                name: name.to_string(),
                revision: 0,
                imports: imports.iter().map(|import| import.to_string()).collect(),
                source: String::new(),
            }
        }
        // c imports b imports a; a change to `a` must recompile a, b and c. `x`/`y` import each
        // other (a cycle) and are unaffected.
        let modules = vec![
            snap("a", &[]),
            snap("b", &["a"]),
            snap("c", &["b"]),
            snap("x", &["y"]),
            snap("y", &["x"]),
        ];
        let dirty: HashSet<&str> = ["a"].into_iter().collect();
        let set = modules_needing_recompile(&modules, &dirty);
        assert!(["a", "b", "c"].iter().all(|name| set.contains(name)));
        assert!(!set.contains("x") && !set.contains("y"));

        // A dirty module inside a cycle terminates and includes the whole cycle.
        let dirty_cycle: HashSet<&str> = ["x"].into_iter().collect();
        let set = modules_needing_recompile(&modules, &dirty_cycle);
        assert!(set.contains("x") && set.contains("y"));
    }

    #[test]
    fn editing_a_dependency_recompiles_its_dependents() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        let mut documents = WeslDocuments::default();
        let helpers = documents.open(
            PathBuf::from("helpers.wesl"),
            "fn scale(x: f32) -> f32 { return x * 2.0; }".into(),
        );
        let main = documents.open(
            PathBuf::from("main.wesl"),
            "import package::helpers::scale;\n\
             @fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(scale(0.5)); }"
                .into(),
        );
        app.insert_resource(documents);
        app.init_resource::<WeslDiagnostics>();

        app.world_mut()
            .run_system_once(recompile_changed_wesl)
            .unwrap();
        assert!(
            matches!(
                app.world().resource::<WeslDiagnostics>().state(main),
                Some(WeslCompileState::Ok { .. })
            ),
            "main imports helpers::scale and should compile"
        );

        // Remove `scale` from the dependency. `main`'s own buffer is untouched, but its import can no
        // longer resolve, so recompiling only changed buffers would leave its diagnostic stale.
        app.world_mut()
            .resource_mut::<WeslDocuments>()
            .set_text(helpers, "fn other(x: f32) -> f32 { return x; }".into());
        app.world_mut()
            .run_system_once(recompile_changed_wesl)
            .unwrap();
        assert!(
            matches!(
                app.world().resource::<WeslDiagnostics>().state(main),
                Some(WeslCompileState::Error { .. })
            ),
            "editing the dependency must recompile the dependent and surface the broken import"
        );
    }

    #[test]
    fn round_trips_through_ron_for_the_manifest() {
        let id = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let encoded = ron::to_string(&id).unwrap();
        assert_eq!(ron::from_str::<WeslSourceId>(&encoded).unwrap(), id);
    }
}
