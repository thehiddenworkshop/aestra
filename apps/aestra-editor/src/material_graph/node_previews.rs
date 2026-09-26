//! Bounded, cancellable CPU rasterization. Never evaluate preview pixels on the UI thread.
use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures_lite::future};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const WORKERS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    pub program: MaterialProgramId,
    pub target: MaterialGraphPreviewTarget,
    root: PathBuf,
    instance: Option<MaterialId>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Input {
    program: MaterialProgram,
    instance: Option<MaterialInstance>,
    functions: Vec<aestra_core::material::MaterialFunction>,
    value_type: Option<MaterialValueType>,
}

impl Input {
    fn new(
        mut program: MaterialProgram,
        instance: Option<MaterialInstance>,
        functions: Vec<aestra_core::material::MaterialFunction>,
        request: &MaterialGraphPreviewRaster,
    ) -> Self {
        // Disconnected additions/deletions cannot change an existing node's pixels. Cache only its
        // dependency closure, not graph layout metadata or the effect's unrelated revision.
        let mut pending = match request.target {
            MaterialGraphPreviewTarget::Expression(id) => vec![id],
            MaterialGraphPreviewTarget::Output => {
                vec![program.outputs.color, program.outputs.alpha]
            }
        };
        let mut reachable = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if reachable.insert(id)
                && let Some(expression) = program.expressions.iter().find(|node| node.id == id)
            {
                pending.extend(expression.kind.dependencies());
            }
        }
        program
            .expressions
            .retain(|node| reachable.contains(&node.id));
        program
            .disabled_expressions
            .retain(|id| reachable.contains(id));
        program.node_constants.clear();
        program.name.clear();
        Self {
            program,
            instance,
            functions,
            value_type: request.value_type,
        }
    }

    fn render(
        &self,
        target: MaterialGraphPreviewTarget,
        cancelled: &AtomicBool,
    ) -> Result<Image, String> {
        let functions = aestra_compiler::MaterialFunctionLibrary::new(self.functions.clone());
        let pixels = render_material_preview_pixels(
            &self.program,
            self.instance.as_ref(),
            &functions,
            target,
            self.value_type,
            MATERIAL_PREVIEW_SIZE,
            false,
            || cancelled.load(Ordering::Relaxed),
        )?;
        let mut image = Image::new(
            Extent3d {
                width: MATERIAL_PREVIEW_SIZE,
                height: MATERIAL_PREVIEW_SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::linear();
        Ok(image)
    }
}

struct Job {
    input: Input,
    cancelled: Arc<AtomicBool>,
    task: Task<Result<Image, String>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub(super) struct Jobs {
    running: BTreeMap<Key, Job>,
}

/// Mount the last completed image in the same command batch as the node. A graph rebuild must not
/// briefly replace every preview with an empty surface while new pixels are being prepared.
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    previews: &MaterialGraphPreviewState,
    request: MaterialGraphPreviewRaster,
) {
    let cached = previews
        .cache
        .iter()
        .find(|(key, _)| {
            key.program == request.program
                && key.target == request.target
                && key.instance == request.instance
                && match &request.editing_target {
                    crate::material_document::MaterialEditingTarget::Program { root, .. } => {
                        &key.root == root
                    }
                    _ => true,
                }
        })
        .map(|(_, cached)| cached.image.clone());
    let entity = spawn_graph_node_preview(parent, request);
    if let Some(image) = cached {
        parent
            .commands()
            .entity(entity)
            .insert(ImageNode::new(image).with_mode(NodeImageMode::Stretch));
    }
}

pub(super) fn update(
    mut commands: Commands,
    requests: Query<(Entity, &MaterialGraphPreviewRaster, Option<&ImageNode>)>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut images: ResMut<Assets<Image>>,
    mut jobs: Local<Jobs>,
) {
    let mut desired = BTreeMap::<Key, (Input, Vec<Entity>)>::new();
    if !requests.is_empty() {
        let functions = catalog.material_functions().unwrap_or_default();
        for (entity, request, shown) in &requests {
            let Ok(programs) =
                session.graph_material_programs_for(&request.editing_target, &catalog)
            else {
                continue;
            };
            let Some(program) = programs
                .into_iter()
                .find(|program| program.id == request.program)
            else {
                continue;
            };
            let key = Key {
                program: request.program,
                target: request.target,
                root: catalog.root().to_owned(),
                instance: request.instance,
            };
            // Keep stale-but-valid pixels visible while this exact source snapshot updates.
            if let Some(cached) = previews.cache.get(&key)
                && shown.is_none_or(|shown| shown.image != cached.image)
            {
                commands
                    .entity(entity)
                    .insert(ImageNode::new(cached.image.clone()).with_mode(NodeImageMode::Stretch));
            }
            let instance = session
                .effect
                .material_instances
                .iter()
                .find(|instance| Some(instance.id) == request.instance)
                .cloned();
            desired
                .entry(key)
                .or_insert_with(|| {
                    (
                        Input::new(program, instance, functions.clone(), request),
                        Vec::new(),
                    )
                })
                .1
                .push(entity);
        }
    }
    // Removed nodes, closed views, project switches and newer edits cancel obsolete scanlines.
    jobs.running.retain(|key, job| {
        desired
            .get(key)
            .is_some_and(|(input, _)| *input == job.input)
    });
    let completed = jobs
        .running
        .iter_mut()
        .filter_map(|(key, job)| {
            future::block_on(future::poll_once(&mut job.task)).map(|result| (key.clone(), result))
        })
        .collect::<Vec<_>>();
    for (key, result) in completed {
        let job = jobs.running.remove(&key).unwrap();
        if let Ok(image) = result {
            let image = images.add(image);
            for entity in &desired[&key].1 {
                commands
                    .entity(*entity)
                    .insert(ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch));
            }
            previews.cache.insert(
                key,
                MaterialGraphPreviewCache {
                    input: job.input.clone(),
                    image,
                },
            );
        }
    }
    for (key, (input, _)) in desired {
        if jobs.running.len() >= WORKERS {
            break;
        }
        if jobs.running.contains_key(&key)
            || previews
                .cache
                .get(&key)
                .is_some_and(|cached| cached.input == input)
        {
            continue;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let worker_input = input.clone();
        let target = key.target;
        let task = AsyncComputeTaskPool::get_or_init(|| {
            bevy::tasks::TaskPoolBuilder::new()
                .num_threads(WORKERS)
                .thread_name("graph-previews".into())
                .build()
        })
        .spawn(async move { worker_input.render(target, &worker_cancelled) });
        jobs.running.insert(
            key,
            Job {
                input,
                cancelled,
                task,
            },
        );
    }
}

#[cfg(test)]
pub(super) fn wait_for_image(
    app: &mut App,
    entity: Entity,
    previous: Option<&Handle<Image>>,
) -> Handle<Image> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if let Some(image) = app.world().get::<ImageNode>(entity)
            && previous != Some(&image.image)
        {
            return image.image.clone();
        }
        assert!(
            Instant::now() < deadline,
            "background graph preview did not complete"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(program: &MaterialProgram) -> MaterialGraphPreviewRaster {
        MaterialGraphPreviewRaster {
            program: program.id,
            editing_target: crate::material_document::MaterialEditingTarget::Program {
                root: PathBuf::from("project"),
                id: program.id,
            },
            instance: None,
            target: MaterialGraphPreviewTarget::Output,
            value_type: None,
        }
    }

    #[test]
    fn disconnected_add_remove_does_not_invalidate_surviving_preview_pixels() {
        let program = MaterialProgram::additive_sprite("Preview").normalized();
        let request = request(&program);
        let before = Input::new(program.clone(), None, vec![], &request);
        let mut edited = program.clone();
        edited.expressions.push(MaterialExpression {
            id: MaterialExpressionId::from_u128(123),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(42.0)),
        });
        edited
            .node_constants
            .push(MaterialExpressionId::from_u128(123));
        assert_eq!(before, Input::new(edited, None, vec![], &request));
        let mut changed = program;
        changed
            .expressions
            .iter_mut()
            .find(|node| node.id == changed.outputs.alpha)
            .unwrap()
            .kind = MaterialExpressionKind::Constant(MaterialValue::Float(0.0));
        assert_ne!(before, Input::new(changed, None, vec![], &request));
    }

    #[test]
    fn cancelled_preview_stops_before_rasterizing() {
        let program = MaterialProgram::additive_sprite("Cancel").normalized();
        let request = request(&program);
        let input = Input::new(program, None, vec![], &request);
        assert!(
            input
                .render(request.target, &AtomicBool::new(true))
                .is_err()
        );
    }

    #[test]
    fn graph_rebuild_mounts_cached_pixels_without_waiting_for_a_worker() {
        use bevy::ecs::system::RunSystemOnce;
        let program = MaterialProgram::additive_sprite("Cached").normalized();
        let request = request(&program);
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let mut previews = MaterialGraphPreviewState::default();
        previews.cache.insert(
            Key {
                program: program.id,
                target: request.target,
                root: PathBuf::from("project"),
                instance: None,
            },
            MaterialGraphPreviewCache {
                input: Input::new(program, None, vec![], &request),
                image: image.clone(),
            },
        );
        app.insert_resource(previews);
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands, previews: Res<MaterialGraphPreviewState>| {
                    commands
                        .spawn(Node::default())
                        .with_children(|parent| spawn(parent, &previews, request.clone()));
                },
            )
            .unwrap();
        let mut query = app.world_mut().query::<&ImageNode>();
        assert_eq!(query.single(app.world()).unwrap().image, image);
    }
}
