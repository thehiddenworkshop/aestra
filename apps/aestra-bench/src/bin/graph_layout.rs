//! Representative native `elkrs` qualification benchmark for editor graph arrangement.

use serde_json::json;
use std::{hint::black_box, time::Instant};

fn main() {
    let iterations = std::env::args()
        .skip_while(|arg| arg != "--iterations")
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
        .max(1);
    println!("elkrs layered layout · {iterations} measured iterations");
    for size in [16usize, 64, 256] {
        let request = representative(size).to_string();
        let elk = elkrs::create_elk();
        for _ in 0..5 {
            black_box(elk.layout_json(&request).expect("warm-up layout failed"));
        }
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            black_box(elk.layout_json(&request).expect("layout failed"));
            samples.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{size:>3} nodes / {:>3} edges · median {:>7.3} ms · p95 {:>7.3} ms · max {:>7.3} ms",
            edge_count(size),
            percentile(&samples, 0.50),
            percentile(&samples, 0.95),
            samples[samples.len() - 1],
        );
    }
}

fn representative(size: usize) -> serde_json::Value {
    let children = (0..size)
        .map(|index| {
            json!({
                "id": format!("n{index}"),
                "width": 140 + (index % 4) * 24,
                "height": 54 + (index % 5) * 18,
                "layoutOptions": { "org.eclipse.elk.nodeSize.constraints": "[]" }
            })
        })
        .collect::<Vec<_>>();
    let mut edges = Vec::with_capacity(edge_count(size));
    for target in 1..size {
        let source = target.saturating_sub(1 + target % 4);
        edges.push(json!({
            "id": format!("e{}", edges.len()),
            "sources": [format!("n{source}")],
            "targets": [format!("n{target}")]
        }));
        if target >= 7 && target % 3 == 0 {
            edges.push(json!({
                "id": format!("e{}", edges.len()),
                "sources": [format!("n{}", target - 7)],
                "targets": [format!("n{target}")]
            }));
        }
    }
    json!({
        "id": "root",
        "layoutOptions": {
            "org.eclipse.elk.algorithm": "org.eclipse.elk.layered",
            "org.eclipse.elk.direction": "RIGHT",
            "org.eclipse.elk.edgeRouting": "SPLINES",
            "org.eclipse.elk.spacing.nodeNode": 32,
            "org.eclipse.elk.layered.spacing.nodeNodeBetweenLayers": 72,
            "org.eclipse.elk.randomSeed": 1
        },
        "children": children,
        "edges": edges
    })
}

fn edge_count(size: usize) -> usize {
    size.saturating_sub(1)
        + (1..size)
            .filter(|target| *target >= 7 && target % 3 == 0)
            .count()
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
    samples[((samples.len() - 1) as f64 * percentile).round() as usize]
}
