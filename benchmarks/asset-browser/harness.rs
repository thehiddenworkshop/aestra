//! Opt-in measurement of the actual private editor browser state and retained UI.
//! Included by asset_browser only for tests; not a second browser implementation.
use super::{
    actions::BrowserAction,
    state::*,
    tests::{browser_app, rows},
};
use crate::*;
use aestra_project::{ProjectContent, ProjectContentVersion};
use std::{hint::black_box, time::Instant};

fn report(label: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    println!(
        "{label}: median_ms={:.3} p95_ms={:.3} samples={}",
        samples[samples.len() / 2],
        samples[(samples.len() * 95 / 100).min(samples.len() - 1)],
        samples.len()
    );
}

#[test]
#[ignore = "opt-in 10,000-source Asset Browser baseline; see benchmarks/asset-browser/README.md"]
fn asset_browser_10k_baseline() {
    let root = tempfile::tempdir().unwrap();
    for folder in 0..100 {
        let path = root.path().join(format!("folder-{folder:03}"));
        std::fs::create_dir(&path).unwrap();
        for file in 0..100 {
            std::fs::write(path.join(format!("texture-{file:03}.png")), []).unwrap();
        }
    }
    let started = Instant::now();
    let content = ProjectContent::scan(root.path());
    println!(
        "snapshot_ms={:.3} sources={} (10,000 generic texture files + 100 folders + root)",
        started.elapsed().as_secs_f64() * 1000.0,
        content.source_tree().entries().count()
    );
    assert_eq!(content.source_tree().entries().count(), 10_101);
    let mut state = AssetBrowserState::default();
    state.reconcile(
        &content,
        ProjectContentVersion {
            generation: 1,
            revision: 1,
        },
    );
    state.recursive = true;
    let mut samples = Vec::new();
    for index in 0..72 {
        state.query = if index % 2 == 0 {
            "texture"
        } else {
            "texture-09"
        }
        .into();
        let started = Instant::now();
        let filtered = black_box(state.filtered(&content));
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(filtered.len(), if index % 2 == 0 { 10000 } else { 1000 });
        if index >= 8 {
            samples.push(elapsed);
        }
    }
    report("snapshot_filter_sort", samples);
    let started = Instant::now();
    let mut app = browser_app(root.path());
    let original_effect = app.world().resource::<EditorSession>().effect.clone();
    println!(
        "headless_browser_startup_ms={:.3} (includes second discovery)",
        started.elapsed().as_secs_f64() * 1000.0
    );
    app.world_mut().trigger(BrowserAction::Recursive);
    app.update();
    let mut peak_entities = 0;
    let mut selection = Vec::new();
    let mut paging = Vec::new();
    for index in 0..72 {
        let started = Instant::now();
        app.world_mut().trigger(BrowserAction::Page(true));
        app.update();
        if index >= 8 {
            paging.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let current = rows(&mut app);
        assert!(current.len() <= 192, "retained row cache exceeded budget");
        peak_entities = peak_entities.max(app.world().entities().len());
        let id = *current.first_key_value().unwrap().0;
        let started = Instant::now();
        app.world_mut().resource_mut::<AssetBrowserState>().selected = Some(id);
        app.update();
        if index >= 8 {
            selection.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        assert_eq!(
            rows(&mut app),
            current,
            "selection replaced retained entities"
        );
    }
    report("headless_page_update", paging);
    report("headless_selection_update", selection);
    let mut idle = Vec::new();
    for _ in 0..64 {
        let started = Instant::now();
        app.update();
        idle.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    report("headless_idle_update", idle);
    println!("peak_entities={peak_entities} retained_asset_rows_max=192 page_size={PAGE_SIZE}");
    assert!(
        peak_entities < 4000,
        "UI entity count scales with total source count"
    );
    assert_eq!(
        app.world().resource::<EditorSession>().effect,
        original_effect
    );
}
