//! Serialized explicit project I/O. Tasks are drained, never dropped after a disk write.
use super::*;
use bevy::{
    ecs::world::CommandQueue,
    tasks::{IoTaskPool, Task, futures_lite::future},
};

#[derive(Resource, Default)]
pub(crate) struct ProjectIoTasks {
    task: Option<Task<CommandQueue>>,
    busy: bool,
}

pub(crate) fn idle(tasks: Option<Res<ProjectIoTasks>>) -> bool {
    tasks.is_none_or(|tasks| !tasks.busy)
}

#[derive(Clone)]
pub(crate) struct IoGuard {
    material_target: crate::material_document::MaterialEditingTarget,
    version: ProjectContentVersion,
    generation: u64,
    revision: u64,
    effect: EffectAsset,
    pending: Option<EffectAsset>,
    locks: aestra_authoring::LockState,
    path: Option<PathBuf>,
    drafts: crate::material_drafts::MaterialDrafts,
}

impl IoGuard {
    pub(crate) fn same_project(&self, catalog: &EditorProjectContent) -> bool {
        self.version.generation == catalog.version.generation
    }
    pub(crate) fn capture(catalog: &EditorProjectContent, session: &EditorSession) -> Self {
        Self {
            material_target: session.material_target.clone(),
            version: catalog.version,
            generation: session.history_generation(),
            revision: session.document_revision(),
            effect: session.effect.clone(),
            pending: session
                .pending_change
                .as_ref()
                .map(|change| change.preview.candidate().clone()),
            locks: session.locks.clone(),
            path: session.source_path.clone(),
            drafts: catalog.material_drafts.clone(),
        }
    }
    pub(crate) fn same_document(
        &self,
        catalog: &EditorProjectContent,
        session: &EditorSession,
    ) -> bool {
        self.version.generation == catalog.version.generation
            && self.generation == session.history_generation()
            && self.effect.id == session.effect.id
            && self.path == session.source_path
    }
    pub(crate) fn matches(&self, catalog: &EditorProjectContent, session: &EditorSession) -> bool {
        self.revision == session.document_revision()
            && self.matches_material_reload(catalog, session)
    }

    /// Material reload may overlap preview recompilation after Save publishes its catalog.
    /// That invalidates simulation checkpoints, but does not change authored content.
    /// Still reject changes to the document, target, catalog, drafts, locks or proposal.
    pub(crate) fn matches_material_reload(
        &self,
        catalog: &EditorProjectContent,
        session: &EditorSession,
    ) -> bool {
        self.same_document(catalog, session)
            && self.material_target == session.material_target
            && self.version == catalog.version
            && self.effect == session.effect
            && self.locks == session.locks
            && self.pending.as_ref()
                == session
                    .pending_change
                    .as_ref()
                    .map(|change| change.preview.candidate())
            && self.drafts == catalog.material_drafts
    }
}

pub(crate) fn enqueue(
    commands: &mut Commands,
    guard: IoGuard,
    work: impl FnOnce() -> CommandQueue + Send + 'static,
) {
    commands.queue(move |world: &mut World| {
        world.init_resource::<ProjectIoTasks>();
        if world.resource::<ProjectIoTasks>().busy {
            return;
        }
        if !guard.matches(
            world.resource::<EditorProjectContent>(),
            world.resource::<EditorSession>(),
        ) {
            set_status(world, "project-operation-queued-cancelled");
            return;
        }
        set_status(world, "project-operation-running");
        let mut tasks = world.resource_mut::<ProjectIoTasks>();
        tasks.busy = true;
        tasks.task = Some(
            IoTaskPool::get_or_init(|| {
                bevy::tasks::TaskPoolBuilder::new()
                    .num_threads(1)
                    .thread_name("project-io".into())
                    .build()
            })
            .spawn(async move { work() }),
        );
    });
}

pub(crate) fn set_status(world: &mut World, id: &str) {
    let text = world.resource::<crate::localization::Localizer>().text(id);
    let mut session = world.resource_mut::<EditorSession>();
    session.status = text;
    session.ui_revision += 1;
}

/// Test boundary: wait for preparation without applying it, so races are deterministic.
#[cfg(test)]
pub(crate) fn prepared_completion(world: &mut World) -> CommandQueue {
    world.flush();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let mut tasks = world.resource_mut::<ProjectIoTasks>();
        assert!(tasks.busy);
        let task = tasks.task.as_mut().expect("pending worker");
        if let Some(queue) = future::block_on(future::poll_once(task)) {
            tasks.task = None;
            return queue;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "project I/O timed out"
        );
        std::thread::yield_now();
    }
}

#[cfg(test)]
pub(crate) fn drain(world: &mut World) {
    world.flush();
    while world
        .get_resource::<ProjectIoTasks>()
        .is_some_and(|tasks| tasks.busy)
    {
        prepared_completion(world).apply(world);
        world.flush();
    }
}

pub(crate) fn completion(apply: impl FnOnce(&mut World) + Send + 'static) -> CommandQueue {
    let mut queue = CommandQueue::default();
    queue.push(move |world: &mut World| {
        // Keep busy until application, not merely until the worker completes.
        world.resource_mut::<ProjectIoTasks>().busy = false;
        apply(world);
    });
    queue
}

pub(crate) fn poll(mut tasks: ResMut<ProjectIoTasks>, mut commands: Commands) {
    if let Some(task) = &mut tasks.task
        && let Some(mut queue) = future::block_on(future::poll_once(task))
    {
        tasks.task = None;
        commands.append(&mut queue);
    }
}

pub(crate) fn publish_catalog(world: &mut World, mut prepared: EditorProjectContent) {
    // Source operations do not own edits made to shared material drafts while they run.
    prepared.material_drafts = world
        .resource::<EditorProjectContent>()
        .material_drafts
        .clone();
    world.insert_resource(prepared);
    if world.contains_resource::<ProjectEffectWatchState>() {
        world.resource_scope(|world, mut watch: Mut<ProjectEffectWatchState>| {
            watch.accept_current(world.resource::<EditorProjectContent>());
        });
    }
}
