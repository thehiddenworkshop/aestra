//! Real production worker orchestration, with pending tasks only where cancellation must be exact.
use super::*;
use crate::material_document::MaterialEditingTarget;
use bevy::ecs::system::SystemState;

struct Fixture {
    root: tempfile::TempDir,
    program: MaterialProgram,
    request: MaterialGraphPreviewRaster,
    app: App,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Worker qualification").normalized();
        program
            .save_ron(root.path().join("preview.aestra.material.ron"))
            .unwrap();
        let request = MaterialGraphPreviewRaster {
            program: program.id,
            editing_target: MaterialEditingTarget::Program {
                root: root.path().to_owned(),
                id: program.id,
            },
            instance: None,
            target: MaterialGraphPreviewTarget::Output,
            value_type: None,
        };
        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<Assets<Image>>();
        Self {
            root,
            program,
            request,
            app,
        }
    }

    fn key(&self) -> Key {
        Key {
            program: self.program.id,
            target: self.request.target,
            root: self.root.path().to_owned(),
            instance: self.request.instance,
        }
    }

    fn input(&self) -> Input {
        Input::new(self.program.clone(), None, vec![], &self.request)
    }

    fn spawn(&mut self) -> Entity {
        self.app.world_mut().spawn(self.request.clone()).id()
    }
}

type WorkerParams<'w, 's> = (
    Commands<'w, 's>,
    Query<
        'w,
        's,
        (
            Entity,
            &'static MaterialGraphPreviewRaster,
            Option<&'static ImageNode>,
        ),
    >,
    Res<'w, EditorSession>,
    Res<'w, ProjectEffectCatalog>,
    ResMut<'w, MaterialGraphPreviewState>,
    ResMut<'w, Assets<Image>>,
    Local<'s, Jobs>,
);

// System parameters are validated fallibly before the tuple can be used.
trait RequiredParams<'w, 's> {
    fn required(self) -> WorkerParams<'w, 's>;
}
impl<'w, 's> RequiredParams<'w, 's> for WorkerParams<'w, 's> {
    fn required(self) -> WorkerParams<'w, 's> {
        self
    }
}
impl<'w, 's, E: std::fmt::Debug> RequiredParams<'w, 's> for Result<WorkerParams<'w, 's>, E> {
    fn required(self) -> WorkerParams<'w, 's> {
        self.expect("required preview resources")
    }
}

// Calls the unmodified production system, retaining its actual Local<Jobs> between invocations.
struct Frames(SystemState<WorkerParams<'static, 'static>>);
impl Frames {
    fn new(world: &mut World) -> Self {
        Self(SystemState::new(world))
    }
    fn step(&mut self, world: &mut World) {
        let (commands, requests, session, catalog, previews, images, jobs) =
            self.0.get_mut(world).required();
        update(commands, requests, session, catalog, previews, images, jobs);
        self.0.apply(world);
    }
    fn jobs<R>(&mut self, world: &mut World, inspect: impl FnOnce(&mut Jobs) -> R) -> R {
        let (_, _, _, _, _, _, mut jobs) = self.0.get_mut(world).required();
        inspect(&mut jobs)
    }
    fn wait(
        &mut self,
        world: &mut World,
        entity: Entity,
        previous: Option<&Handle<Image>>,
    ) -> Handle<Image> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.step(world);
            if let Some(image) = world.get::<ImageNode>(entity)
                && previous != Some(&image.image)
            {
                return image.image.clone();
            }
            assert!(
                Instant::now() < deadline,
                "production preview worker did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn pool() -> &'static bevy::tasks::TaskPool {
    AsyncComputeTaskPool::get_or_init(|| {
        bevy::tasks::TaskPoolBuilder::new()
            .num_threads(WORKERS)
            .build()
    })
}

fn pending(input: Input) -> (Job, Arc<AtomicBool>) {
    let cancelled = Arc::new(AtomicBool::new(false));
    (
        Job {
            input,
            cancelled: cancelled.clone(),
            task: pool().spawn(future::pending()),
        },
        cancelled,
    )
}

fn wait_finished(job: &Job) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !job.task.is_finished() {
        assert!(Instant::now() < deadline, "test task did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn instance_and_expression_requests_have_independent_cache_entries() {
    use aestra_core::{MaterialId, material::MaterialProgramRef};
    let mut f = Fixture::new();
    register(&mut f.app);
    let mut entities = Vec::new();
    for _ in 0..2 {
        let instance = MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(f.program.id),
            values: BTreeMap::new(),
            render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
        };
        f.request.instance = Some(instance.id);
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .material_instances
            .push(instance);
        entities.push(f.spawn());
    }
    f.request.target = MaterialGraphPreviewTarget::Expression(f.program.outputs.color);
    entities.push(f.spawn());
    let mut images = Vec::new();
    for entity in entities {
        images.push(wait_for_image(&mut f.app, entity, None));
    }
    assert_ne!(images[0], images[1]);
    assert_ne!(images[1], images[2]);
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .cache
            .len(),
        3
    );
}

#[test]
fn registrar_queues_real_pixels_once_for_duplicate_views_without_semantic_edits() {
    let mut f = Fixture::new();
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    register(&mut f.app);
    let first = f.spawn();
    let second = f.spawn();
    f.app.update();
    assert!(f.app.world().get::<ImageNode>(first).is_none());
    assert!(f.app.world().get::<ImageNode>(second).is_none());
    let image = wait_for_image(&mut f.app, first, None);
    assert_eq!(f.app.world().get::<ImageNode>(second).unwrap().image, image);
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .cache
            .len(),
        1
    );
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
    let expected = f
        .input()
        .render(f.request.target, &AtomicBool::new(false))
        .unwrap();
    let actual = f
        .app
        .world()
        .resource::<Assets<Image>>()
        .get(&image)
        .unwrap();
    assert_eq!(actual.data, expected.data);
    assert_eq!(
        actual.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb
    );
    assert_eq!(actual.texture_descriptor.size.width, MATERIAL_PREVIEW_SIZE);
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.effect, before);
    assert_eq!(session.operation_order.edit_serial(), 0);
    assert_eq!(
        session.material_target,
        MaterialEditingTarget::EffectInstance
    );
}

#[test]
fn stable_requests_reuse_assets_without_dirtying_the_preview_cache() {
    let mut f = Fixture::new();
    register(&mut f.app);
    let entity = f.spawn();
    let image = wait_for_image(&mut f.app, entity, None);
    f.app.update();
    let tick = f
        .app
        .world()
        .get_resource_ref::<MaterialGraphPreviewState>()
        .unwrap()
        .last_changed();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(
        f.app
            .world()
            .get_resource_ref::<MaterialGraphPreviewState>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert_eq!(f.app.world().get::<ImageNode>(entity).unwrap().image, image);
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
}

#[test]
fn two_worker_limit_deduplicates_requests_and_fills_a_vacancy_after_close() {
    let mut f = Fixture::new();
    let first = f.spawn();
    let duplicate = f.spawn();
    let mut expression = f.request.clone();
    expression.target = MaterialGraphPreviewTarget::Expression(f.program.outputs.color);
    f.app.world_mut().spawn(expression.clone());
    expression.target = MaterialGraphPreviewTarget::Expression(f.program.outputs.alpha);
    let alpha = f.app.world_mut().spawn(expression).id();
    let mut frames = Frames::new(f.app.world_mut());
    let mut cancelled = Vec::new();
    let key_base = f.key();
    let base_request = f.request.clone();
    let program = f.program.clone();
    frames.jobs(f.app.world_mut(), |jobs| {
        for target in [
            MaterialGraphPreviewTarget::Output,
            MaterialGraphPreviewTarget::Expression(program.outputs.color),
        ] {
            let mut key = key_base.clone();
            key.target = target;
            let mut request = base_request.clone();
            request.target = target;
            let (job, token) = pending(Input::new(program.clone(), None, vec![], &request));
            jobs.running.insert(key, job);
            cancelled.push(token);
        }
    });
    frames.step(f.app.world_mut());
    assert_eq!(
        frames.jobs(f.app.world_mut(), |jobs| jobs.running.len()),
        WORKERS
    );
    assert!(cancelled.iter().all(|token| !token.load(Ordering::Relaxed)));
    f.app.world_mut().despawn(first);
    frames.step(f.app.world_mut());
    assert!(
        !cancelled[0].load(Ordering::Relaxed),
        "the duplicate view still owns the request"
    );
    f.app.world_mut().despawn(duplicate);
    frames.step(f.app.world_mut());
    assert!(cancelled[0].load(Ordering::Relaxed));
    assert_eq!(
        frames.jobs(f.app.world_mut(), |jobs| jobs.running.len()),
        WORKERS
    );
    let image = frames.wait(f.app.world_mut(), alpha, None);
    assert!(f.app.world().resource::<Assets<Image>>().contains(&image));
}

#[test]
fn closing_all_views_cancels_obsolete_tasks_without_publishing() {
    let mut f = Fixture::new();
    let entity = f.spawn();
    let mut frames = Frames::new(f.app.world_mut());
    let (job, token) = pending(f.input());
    let key = f.key();
    frames.jobs(f.app.world_mut(), |jobs| {
        jobs.running.insert(key, job);
    });
    f.app.world_mut().despawn(entity);
    frames.step(f.app.world_mut());
    assert!(token.load(Ordering::Relaxed));
    assert_eq!(frames.jobs(f.app.world_mut(), |jobs| jobs.running.len()), 0);
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .cache
            .is_empty()
    );
    assert!(f.app.world().resource::<Assets<Image>>().is_empty());
}

#[test]
fn new_source_cancels_old_work_but_keeps_cached_pixels_until_replacement_is_ready() {
    let mut f = Fixture::new();
    let entity = f.spawn();
    let old_input = f.input();
    let old_image = f.app.world_mut().resource_mut::<Assets<Image>>().add(
        old_input
            .render(f.request.target, &AtomicBool::new(false))
            .unwrap(),
    );
    let key = f.key();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .cache
        .insert(
            key.clone(),
            MaterialGraphPreviewCache {
                input: old_input.clone(),
                image: old_image.clone(),
            },
        );
    let mut frames = Frames::new(f.app.world_mut());
    let token = Arc::new(AtomicBool::new(false));
    let worker_input = old_input.clone();
    let job = Job {
        input: old_input,
        cancelled: token.clone(),
        task: pool().spawn(async move {
            worker_input.render(MaterialGraphPreviewTarget::Output, &AtomicBool::new(false))
        }),
    };
    wait_finished(&job);
    frames.jobs(f.app.world_mut(), |jobs| {
        jobs.running.insert(key.clone(), job);
    });
    let mut edited = f.program.clone();
    edited
        .expressions
        .iter_mut()
        .find(|node| node.id == edited.outputs.alpha)
        .unwrap()
        .kind = MaterialExpressionKind::Constant(MaterialValue::Float(0.0));
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&f.program, &edited)
        .unwrap();
    let revision = f
        .app
        .world()
        .resource::<EditorSession>()
        .document_revision();
    frames.step(f.app.world_mut());
    assert!(token.load(Ordering::Relaxed));
    assert_eq!(
        f.app.world().get::<ImageNode>(entity).unwrap().image,
        old_image
    );
    let new_image = frames.wait(f.app.world_mut(), entity, Some(&old_image));
    let images = f.app.world().resource::<Assets<Image>>();
    assert_eq!(
        images.len(),
        2,
        "obsolete completed pixels must never become an asset"
    );
    assert_ne!(
        images.get(&old_image).unwrap().data,
        images.get(&new_image).unwrap().data
    );
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .document_revision(),
        revision
    );
}

#[test]
fn project_switch_cancels_old_root_even_when_program_identity_is_the_same() {
    let mut f = Fixture::new();
    let entity = f.spawn();
    let mut frames = Frames::new(f.app.world_mut());
    let (job, token) = pending(f.input());
    let key = f.key();
    frames.jobs(f.app.world_mut(), |jobs| {
        jobs.running.insert(key.clone(), job);
    });
    let other = tempfile::tempdir().unwrap();
    f.program
        .save_ron(other.path().join("preview.aestra.material.ron"))
        .unwrap();
    f.app
        .insert_resource(ProjectEffectCatalog::scan(other.path()));
    frames.step(f.app.world_mut());
    assert!(token.load(Ordering::Relaxed));
    assert!(f.app.world().get::<ImageNode>(entity).is_none());
    assert_eq!(frames.jobs(f.app.world_mut(), |jobs| jobs.running.len()), 0);
    f.app
        .world_mut()
        .get_mut::<MaterialGraphPreviewRaster>(entity)
        .unwrap()
        .editing_target = MaterialEditingTarget::Program {
        root: other.path().to_owned(),
        id: f.program.id,
    };
    frames.wait(f.app.world_mut(), entity, None);
    let previews = f.app.world().resource::<MaterialGraphPreviewState>();
    assert!(!previews.cache.contains_key(&key));
    assert!(previews.cache.keys().all(|key| key.root == other.path()));
}

#[test]
fn failed_job_is_not_published_and_the_unchanged_request_can_retry() {
    let mut f = Fixture::new();
    let entity = f.spawn();
    let mut frames = Frames::new(f.app.world_mut());
    let job = Job {
        input: f.input(),
        cancelled: Arc::new(AtomicBool::new(false)),
        task: pool().spawn(async { Err("test raster failure".into()) }),
    };
    wait_finished(&job);
    let key = f.key();
    frames.jobs(f.app.world_mut(), |jobs| {
        jobs.running.insert(key, job);
    });
    frames.step(f.app.world_mut());
    assert!(f.app.world().get::<ImageNode>(entity).is_none());
    assert!(f.app.world().resource::<Assets<Image>>().is_empty());
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .cache
            .is_empty()
    );
    assert_eq!(frames.jobs(f.app.world_mut(), |jobs| jobs.running.len()), 1);
    frames.wait(f.app.world_mut(), entity, None);
}

#[test]
fn dropping_system_local_state_cancels_all_outstanding_jobs() {
    let mut f = Fixture::new();
    let mut frames = Frames::new(f.app.world_mut());
    let (job, token) = pending(f.input());
    let key = f.key();
    frames.jobs(f.app.world_mut(), |jobs| {
        jobs.running.insert(key, job);
    });
    drop(frames);
    assert!(token.load(Ordering::Relaxed));
}
