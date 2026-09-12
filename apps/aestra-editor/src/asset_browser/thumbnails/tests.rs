use super::*;

fn epoch(revision: u64) -> Epoch {
    (
        PathBuf::from("project"),
        ProjectContentVersion {
            generation: 1,
            revision,
        },
    )
}
fn pixels() -> Vec<u8> {
    vec![255; (EDGE * EDGE * 4) as usize]
}
fn pixel(bytes: &[u8], x: u32, y: u32) -> &[u8] {
    let offset = ((y * EDGE + x) * 4) as usize;
    &bytes[offset..offset + 4]
}
fn texture(path: &Path) {
    image::RgbImage::from_pixel(64, 16, image::Rgb([255, 0, 0]))
        .save(path)
        .unwrap();
}

#[test]
fn supported_rasters_preserve_aspect_and_sources() {
    let root = tempfile::tempdir().unwrap();
    for extension in ["png", "jpg", "webp", "bmp", "tga"] {
        let name = format!("wide.{extension}");
        let path = root.path().join(&name);
        texture(&path);
        let original = fs::read(&path).unwrap();
        let decoded = decode(root.path(), Path::new(&name), &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.len(), (EDGE * EDGE * 4) as usize);
        assert!(pixel(&decoded, EDGE / 2, EDGE / 2)[0] > 240);
        assert_eq!(pixel(&decoded, 0, 0), [40, 40, 40, 255]);
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

#[test]
fn transparency_is_visible_over_checkerboard() {
    let root = tempfile::tempdir().unwrap();
    image::RgbaImage::from_pixel(128, 128, image::Rgba([255, 0, 0, 0]))
        .save(root.path().join("alpha.png"))
        .unwrap();
    let decoded = decode(root.path(), Path::new("alpha.png"), &AtomicBool::new(false)).unwrap();
    assert_eq!(pixel(&decoded, 0, 0), [40, 40, 40, 255]);
    assert_eq!(pixel(&decoded, 8, 0), [56, 56, 56, 255]);
}

#[test]
fn corrupt_missing_oversized_and_unsafe_sources_fail_without_allocating_thumbnails() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("broken.png"), b"not an image").unwrap();
    fs::File::create(root.path().join("huge.png"))
        .unwrap()
        .set_len(FILE_LIMIT + 1)
        .unwrap();
    image::RgbImage::new(4097, 1)
        .save(root.path().join("wide.png"))
        .unwrap();
    for name in [
        "broken.png",
        "missing.png",
        "huge.png",
        "wide.png",
        "../outside.png",
    ] {
        assert!(
            decode(root.path(), Path::new(name), &AtomicBool::new(false)).is_err(),
            "{name}"
        );
    }
    assert!(
        decode(
            root.path(),
            &root.path().join("broken.png"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        decode(
            root.path(),
            Path::new("missing.png"),
            &AtomicBool::new(true)
        )
        .unwrap_err(),
        "Cancelled"
    );
}

#[test]
fn cache_eviction_releases_images_and_preserves_the_current_page() {
    let mut cache = ThumbnailCache::default();
    let mut images = Assets::<Image>::default();
    let epoch = epoch(1);
    cache.reset(epoch.clone(), &mut images);
    for n in 0..CAPACITY {
        let id = ProjectSourceId::from_u64(n as u64);
        cache.entries.insert(
            id,
            Entry {
                preview: Preview::Loading,
                touched: n as u64,
            },
        );
        cache.accept(id, &epoch, Ok(pixels()), &mut images);
    }
    assert_eq!(images.len(), CAPACITY);
    let wanted = BTreeSet::from([ProjectSourceId::from_u64(0)]);
    assert!(cache.room(&wanted, &mut images));
    assert!(cache.entries.contains_key(&ProjectSourceId::from_u64(0)));
    assert!(!cache.entries.contains_key(&ProjectSourceId::from_u64(1)));
    assert_eq!(images.len(), CAPACITY - 1);
    let current = cache.entries.keys().copied().collect();
    cache.entries.insert(
        ProjectSourceId::from_u64(999),
        Entry {
            preview: Preview::Failed("bad".into()),
            touched: 999,
        },
    );
    let mut current: BTreeSet<_> = current;
    current.insert(ProjectSourceId::from_u64(999));
    assert!(!cache.room(&current, &mut images));
}

#[test]
fn content_refresh_and_root_switch_reject_stale_completions_and_release_images() {
    let mut cache = ThumbnailCache::default();
    let mut images = Assets::<Image>::default();
    let id = ProjectSourceId::from_u64(1);
    let old = epoch(1);
    cache.reset(old.clone(), &mut images);
    cache.entries.insert(
        id,
        Entry {
            preview: Preview::Loading,
            touched: 0,
        },
    );
    cache.accept(id, &old, Ok(pixels()), &mut images);
    let next = epoch(2);
    cache.reset(next.clone(), &mut images);
    assert!(images.is_empty());
    cache.entries.insert(
        id,
        Entry {
            preview: Preview::Loading,
            touched: 0,
        },
    );
    cache.accept(id, &old, Ok(pixels()), &mut images);
    assert_eq!(cache.entries[&id].preview, Preview::Loading);
    cache.accept(id, &next, Err("bad data".into()), &mut images);
    assert!(matches!(cache.entries[&id].preview, Preview::Failed(_)));
    cache.reset((PathBuf::from("other-project"), next.1), &mut images);
    cache.accept(id, &next, Ok(pixels()), &mut images);
    assert!(images.is_empty());
    assert!(cache.entries.is_empty());
}

fn finish_jobs(app: &mut App) {
    let jobs = std::mem::take(&mut app.world_mut().resource_mut::<ThumbnailCache>().jobs);
    for job in jobs {
        let result = future::block_on(job.task);
        app.world_mut()
            .resource_scope(|world, mut cache: Mut<ThumbnailCache>| {
                cache.accept(
                    job.source,
                    &job.epoch,
                    result,
                    &mut world.resource_mut::<Assets<Image>>(),
                );
            });
    }
    app.update();
}

#[test]
fn browser_texture_rows_publish_previews_without_editing_or_loading_hidden_rows() {
    let root = tempfile::tempdir().unwrap();
    texture(&root.path().join("a.png"));
    texture(&root.path().join("b.png"));
    fs::write(root.path().join("c.png"), b"bad").unwrap();
    let mut app = super::super::tests::browser_app(root.path());
    let before = app.world().resource::<EditorSession>().effect.clone();
    for _ in 0..4 {
        assert!(app.world().resource::<ThumbnailCache>().jobs.len() <= WORKERS);
        finish_jobs(&mut app);
    }
    let world = app.world_mut();
    let rows: Vec<_> = world
        .query::<&Thumbnail>()
        .iter(world)
        .map(|thumbnail| (thumbnail.rendered.clone(), thumbnail.image))
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .filter(|(preview, _)| matches!(preview, Some(Preview::Ready(_))))
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|(preview, _)| matches!(preview, Some(Preview::Failed(_))))
            .count(),
        1
    );
    for (preview, image) in rows {
        assert_eq!(
            world.get::<ImageNode>(image).is_some(),
            matches!(preview, Some(Preview::Ready(_)))
        );
    }
    assert_eq!(world.resource::<EditorSession>().effect, before);
    let entries = world.resource::<ThumbnailCache>().entries.len();
    app.update();
    assert!(
        app.world().resource::<ThumbnailCache>().jobs.is_empty(),
        "failed previews must not retry every frame"
    );
    assert_eq!(
        app.world().resource::<ThumbnailCache>().entries.len(),
        entries
    );
    app.world_mut().resource_mut::<AssetBrowserState>().scope = SourceScope::BuiltIns;
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    assert!(app.world().resource::<ThumbnailCache>().jobs.is_empty());
    assert!(app.world().resource::<ThumbnailCache>().entries.is_empty());
}

#[test]
fn cancelled_workers_still_count_toward_the_global_worker_limit() {
    let root = tempfile::tempdir().unwrap();
    let mut app = super::super::tests::browser_app(root.path());
    let epoch = app
        .world()
        .resource::<ThumbnailCache>()
        .epoch
        .clone()
        .unwrap();
    for n in 0..WORKERS {
        let flag = Arc::new(AtomicBool::new(false));
        app.world_mut()
            .resource_mut::<ThumbnailCache>()
            .jobs
            .push(Job {
                source: ProjectSourceId::from_u64(n as u64),
                epoch: epoch.clone(),
                cancelled: flag,
                task: IoTaskPool::get().spawn(future::pending()),
            });
    }
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    let cache = app.world().resource::<ThumbnailCache>();
    assert_eq!(cache.jobs.len(), WORKERS);
    assert!(
        cache
            .jobs
            .iter()
            .all(|job| job.cancelled.load(Ordering::Relaxed))
    );
}
