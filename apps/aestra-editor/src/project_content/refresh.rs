use super::*;
use bevy::tasks::{IoTaskPool, Task, futures_lite::future};

#[cfg(test)]
mod tests;

const POLL_SECONDS: f32 = 0.25;

#[derive(Resource)]
pub(crate) struct ProjectEffectWatchState {
    tracker: ProjectContentRefresh,
    poll: Timer,
    task: Option<Task<RefreshResult>>,
    force: bool,
}

impl FromWorld for ProjectEffectWatchState {
    fn from_world(world: &mut World) -> Self {
        let catalog = world.resource::<EditorProjectContent>();
        Self {
            tracker: ProjectContentRefresh::new(catalog.version, catalog.snapshot.stamp.clone()),
            poll: Timer::from_seconds(POLL_SECONDS, TimerMode::Repeating),
            task: None,
            force: false,
        }
    }
}

impl ProjectEffectWatchState {
    #[cfg(test)]
    pub(crate) fn version(&self) -> ProjectContentVersion {
        self.tracker.version()
    }

    pub(crate) fn accept_current(&mut self, catalog: &EditorProjectContent) {
        self.tracker
            .reset(catalog.version, catalog.snapshot.stamp.clone());
        self.task = None;
        self.force = false;
        self.poll.reset();
    }

    pub(crate) fn request_refresh(&mut self) {
        self.task = None;
        self.tracker.unsettled();
        self.force = true;
    }
}

#[derive(Clone, PartialEq)]
struct RefreshInput {
    document_revision: u64,
    effect: EffectAsset,
    source_path: Option<PathBuf>,
    dirty: bool,
    drafts: crate::material_drafts::MaterialDrafts,
}

impl RefreshInput {
    fn capture(catalog: &EditorProjectContent, session: &EditorSession) -> Self {
        Self {
            document_revision: session.document_revision(),
            effect: session.effect.clone(),
            source_path: session.source_path.clone(),
            dirty: session.dirty,
            drafts: catalog.material_drafts.clone(),
        }
    }
    fn matches(&self, catalog: &EditorProjectContent, session: &EditorSession) -> bool {
        self.document_revision == session.document_revision()
            && self.effect == session.effect
            && self.source_path == session.source_path
            && self.dirty == session.dirty
            && self.drafts == catalog.material_drafts
    }
}

struct ReloadedSource {
    path: PathBuf,
    effect: EffectAsset,
    bytes: Vec<u8>,
    compiled: CompiledEffectProject,
}

struct RefreshResult {
    version: ProjectContentVersion,
    input: RefreshInput,
    snapshot: Option<ProjectContentSnapshot>,
    prepared: Option<PreparedProject>,
    reload: Option<Result<ReloadedSource, String>>,
    resolved_path: Option<PathBuf>,
    draft_conflict: Option<String>,
}

fn prepare_refresh(
    mut catalog: EditorProjectContent,
    input: RefreshInput,
    force: bool,
) -> RefreshResult {
    let mut result = RefreshResult {
        version: catalog.version,
        input,
        snapshot: catalog.snapshot.poll(force),
        prepared: None,
        reload: None,
        resolved_path: None,
        draft_conflict: None,
    };
    let Some(snapshot) = &result.snapshot else {
        return result;
    };
    let changes = snapshot.stamp.changes_from(&catalog.snapshot.stamp);
    if changes.is_empty() || !changes.semantic_changed {
        return result;
    }
    let source_changed = result
        .input
        .source_path
        .as_deref()
        .is_some_and(|path| catalog.snapshot.stamp.file(path) != snapshot.stamp.file(path));
    catalog.snapshot = snapshot.clone();
    catalog.prepared = None;
    result.resolved_path = catalog
        .openable_path(result.input.effect.id.into())
        .map(Path::to_owned);
    result.draft_conflict = catalog.material_drafts.preflight().err();
    result.prepared = Some(PreparedProject {
        effect: result.input.effect.clone(),
        drafts: catalog.material_drafts.clone(),
        compiled: catalog.compile_project(&result.input.effect),
    });
    if source_changed && !result.input.dirty && result.input.drafts.is_empty() {
        result.reload = Some((|| {
            let entry = catalog
                .index()
                .resolve(result.input.effect.id.into())
                .map_err(|e| e.to_string())?;
            let path = entry.path.clone();
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            let effect = EffectAsset::from_ron(text).map_err(|e| e.to_string())?;
            if effect.id != result.input.effect.id {
                return Err("Source identity changed during refresh".into());
            }
            let compiled = catalog.compile_project(&effect)?;
            Ok(ReloadedSource {
                path,
                effect,
                bytes,
                compiled,
            })
        })());
    }
    // Compilation/load may touch dependencies after discovery. Reject a moving filesystem target.
    if ProjectTreeStamp::scan(catalog.root()) != snapshot.stamp {
        result.snapshot = None;
    }
    result
}

pub(crate) fn poll_project_effect_catalog(
    time: Option<Res<Time>>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut catalog: ResMut<EditorProjectContent>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if watch.tracker.version() != catalog.content_revision() {
        watch.accept_current(&catalog);
        return;
    }
    if let Some(task) = &mut watch.task {
        if let Some(result) = future::block_on(future::poll_once(task)) {
            watch.task = None;
            finish_result(
                result,
                &mut watch,
                catalog.reborrow(),
                session.reborrow(),
                &localizer,
            );
        }
        return;
    }
    let Some(time) = time else {
        return;
    };
    if !watch.poll.tick(time.delta()).just_finished() {
        return;
    }
    let input = RefreshInput::capture(&catalog, &session);
    let candidate = catalog.clone();
    let force = watch.force;
    watch.task =
        Some(IoTaskPool::get().spawn(async move { prepare_refresh(candidate, input, force) }));
}

fn finish_result(
    result: RefreshResult,
    watch: &mut ProjectEffectWatchState,
    mut catalog: Mut<EditorProjectContent>,
    mut session: Mut<EditorSession>,
    localizer: &Localizer,
) {
    if result.version != catalog.version || !result.input.matches(&catalog, &session) {
        watch.tracker.unsettled();
        return;
    }
    let Some(snapshot) = &result.snapshot else {
        watch.tracker.unsettled();
        return;
    };
    let unchanged = snapshot.stamp == catalog.snapshot.stamp;
    if let Some(changes) = watch.tracker.observe(result.version, &snapshot.stamp) {
        // Browser consumers observe content_revision. Generic changes must not trigger
        // the legacy catalog's coarse Bevy change detection / whole-editor invalidation.
        if changes.semantic_changed {
            install_refresh(&mut catalog, &mut session, result, &changes, localizer);
        } else {
            install_refresh(
                catalog.bypass_change_detection(),
                session.bypass_change_detection(),
                result,
                &changes,
                localizer,
            );
        }
        watch.accept_current(&catalog);
    } else if unchanged {
        watch.force = false;
    }
}

fn install_refresh(
    catalog: &mut EditorProjectContent,
    session: &mut EditorSession,
    mut result: RefreshResult,
    changes: &ProjectContentChanges,
    localizer: &Localizer,
) {
    let Some(snapshot) = result.snapshot.take() else {
        return;
    };
    let source_changed = session
        .source_path
        .as_deref()
        .is_some_and(|path| catalog.snapshot.stamp.file(path) != snapshot.stamp.file(path));
    catalog.snapshot = snapshot;
    catalog.version.revision = catalog.version.revision.wrapping_add(1);
    if !changes.semantic_changed {
        return;
    }
    catalog.prepared = result.prepared;
    let revision = session.ui_revision;
    let mut status_set = false;
    if source_changed && let Some(source_path) = session.source_path.clone() {
        let path = result.resolved_path.as_deref().unwrap_or(&source_path);
        if session.dirty || !catalog.material_drafts.is_empty() {
            if result.resolved_path.is_some() && !same_project_source_location(path, &source_path) {
                session.source_path = Some(path.to_owned());
                let mut args = FluentArgs::new();
                args.set("path", path.display().to_string());
                session.status = localizer.text_with("library-status-source-moved-dirty", &args);
            } else if catalog.snapshot.stamp.file(path).is_some() {
                session.status = localizer.text("library-status-source-conflict");
            } else {
                session.status = localizer.text("library-status-open-source-missing");
            }
        } else if catalog.snapshot.stamp.file(path).is_none() {
            session.status = localizer.text("library-status-open-source-missing");
        } else if let Some(reload) = result.reload {
            match reload {
                Ok(reload) => {
                    if !same_project_source_location(&reload.path, &source_path)
                        || reload.effect != session.effect
                    {
                        session.open_refreshed_effect(
                            &reload.path,
                            reload.effect.clone(),
                            reload.compiled.root.clone(),
                            reload.bytes,
                        );
                        let mut args = FluentArgs::new();
                        args.set("path", reload.path.display().to_string());
                        session.status =
                            localizer.text_with("library-status-source-reloaded", &args);
                    }
                    catalog.prepared = Some(PreparedProject {
                        effect: reload.effect,
                        drafts: catalog.material_drafts.clone(),
                        compiled: Ok(reload.compiled),
                    });
                }
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("message", error);
                    session.status =
                        localizer.text_with("library-status-source-reload-failed", &args);
                }
            }
        }
        status_set = true;
    }
    if let Some(error) = result.draft_conflict {
        let mut args = FluentArgs::new();
        args.set("message", error);
        session.status = localizer.text_with("library-status-draft-conflict", &args);
        status_set = true;
    }
    if session.ui_revision == revision {
        session.ui_revision += 1;
    }
    if !status_set {
        let mut args = FluentArgs::new();
        args.set("count", catalog.entries().len());
        session.status = localizer.text_with("library-status-catalog-refreshed", &args);
    }
}

#[cfg(test)]
pub(crate) fn apply_project_effect_catalog_refresh(
    catalog: &mut EditorProjectContent,
    session: &mut EditorSession,
    previous: &ProjectTreeStamp,
    current: &ProjectTreeStamp,
    localizer: &Localizer,
) {
    // Legacy interaction tests apply a worker result synchronously, without clocks or threads.
    catalog.snapshot.stamp = previous.clone();
    let result = prepare_refresh(
        catalog.clone(),
        RefreshInput::capture(catalog, session),
        true,
    );
    let changes = current.changes_from(previous);
    install_refresh(catalog, session, result, &changes, localizer);
}
