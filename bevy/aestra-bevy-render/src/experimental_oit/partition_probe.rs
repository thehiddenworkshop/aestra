//! Isolated native feasibility oracle. No public/runtime partition mode is added.
#[path = "partition_gpu.rs"]
mod gpu;
use serde_json::json;
use std::{fs, path::PathBuf};

const VIEW: &str =
    "struct View { viewport: vec4<f32>, capacity: u32, pad0: u32, pad1: u32, pad2: u32 }";
const TYPES: &str = "struct OitFragmentNode { color: u32, depth_alpha: u32, next: u32 } struct OrderIndependentTransparencySettings { sorted_fragment_max_count: u32 }";
const CODEC: &str = include_str!("partition_codec.wgsl");
const FULLSCREEN: &str = r#"
@vertex fn vertex(@builtin(vertex_index) id: u32) -> FullscreenVertexOutput {
    let p = array(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.))[id];
    return FullscreenVertexOutput(vec4(p,0.,1.),p);
}"#;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn regions(
    size: [u32; 2],
    grid: [u32; 2],
    unsupported_view: bool,
    foreign: bool,
) -> Result<Vec<Rect>, &'static str> {
    if unsupported_view || foreign {
        return Err("probe requires full-target fixture-owned draws");
    }
    if size.contains(&0) || grid.contains(&0) || grid[0] > size[0] || grid[1] > size[1] {
        return Err("invalid partition extent");
    }
    if u64::from(size[0]) * u64::from(size[1]) > u64::from(u32::MAX) {
        return Err("pixel address overflow");
    }
    let mut result = Vec::new();
    for y in 0..grid[1] {
        for x in 0..grid[0] {
            let lo_x = u64::from(x) * u64::from(size[0]) / u64::from(grid[0]);
            let hi_x = u64::from(x + 1) * u64::from(size[0]) / u64::from(grid[0]);
            let lo_y = u64::from(y) * u64::from(size[1]) / u64::from(grid[1]);
            let hi_y = u64::from(y + 1) * u64::from(size[1]) / u64::from(grid[1]);
            result.push(Rect {
                x: lo_x as u32,
                y: lo_y as u32,
                w: (hi_x - lo_x) as u32,
                h: (hi_y - lo_y) as u32,
            });
        }
    }
    Ok(result)
}

fn shaders() -> [String; 3] {
    // This isolated transport supplies fixture bindings/types, not Bevy's full
    // view bind groups. Keep the production algorithm bodies and guard every
    // interface substitution; native phase/import integration is a later gate.
    let without_imports = |source: &str, expected| {
        assert_eq!(
            source.lines().filter(|l| l.starts_with("import ")).count(),
            expected
        );
        source
            .lines()
            .filter(|l| !l.starts_with("import "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let source = include_str!("sort.wesl").replace("\r\n", "\n");
    let entry = "fn main(@builtin(global_invocation_id) id: vec3<u32>) {";
    assert_eq!(source.matches(entry).count(), 1);
    assert_eq!(source.matches("@group(0) @binding(0)").count(), 1);
    let source = source.replace(entry, "fn main(@builtin(global_invocation_id) raw_id: vec3<u32>) {\n if any(raw_id.xy >= region.zw) { return; }\n let id = vec3(raw_id.xy + region.xy, 0u);");
    let source = source.replace(
        "@group(0) @binding(0)",
        "@group(0) @binding(4) var<uniform> region: vec4<u32>;\n@group(0) @binding(0)",
    );
    let sort = format!("{VIEW}\n{TYPES}\n{}", without_imports(&source, 2));
    let source = include_str!("resolve.wesl").replace("\r\n", "\n");
    assert_eq!(source.matches("@if(!DEPTH_PREPASS)").count(), 3);
    assert_eq!(
        source
            .matches("    @else\n    let opaque_depth = 0.0;")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("constants::SORTED_FRAGMENT_MAX_COUNT")
            .count(),
        1
    );
    let source = source
        .replace("@if(!DEPTH_PREPASS)\n", "")
        .replace("    @else\n    let opaque_depth = 0.0;", "")
        .replace("constants::SORTED_FRAGMENT_MAX_COUNT", "8u");
    let resolve = format!(
        "{VIEW}\n{TYPES}\n{CODEC}\n{}\n{FULLSCREEN}",
        without_imports(&source, 4)
    );
    [
        format!("{CODEC}\n{}", include_str!("partition_capture.wgsl")),
        sort,
        resolve,
    ]
}

#[test]
fn partitions_cover_odd_targets_once_and_preserve_production_shader_bodies() {
    for (size, grid) in [([257, 193], [4, 4]), ([31, 17], [3, 5]), ([7, 3], [7, 3])] {
        let mut counts = vec![0; (size[0] * size[1]) as usize];
        for r in regions(size, grid, false, false).unwrap() {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    counts[(y * size[0] + x) as usize] += 1;
                }
            }
        }
        assert!(counts.into_iter().all(|n| n == 1));
    }
    for (size, grid, view, foreign) in [
        ([0, 7], [1, 1], false, false),
        ([7, 7], [8, 1], false, false),
        ([7, 7], [0, 1], false, false),
        ([u32::MAX, 2], [1, 1], false, false),
        ([7, 7], [1, 1], true, false),
        ([7, 7], [1, 1], false, true),
    ] {
        assert!(regions(size, grid, view, foreign).is_err());
    }
    for source in shaders() {
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
#[ignore = "native partitioned HDR OIT feasibility, timing and buffer-reuse oracle; run alone"]
fn partitioned_capture_reuses_nodes_and_matches_full_list_without_seams() {
    let root = PathBuf::from(
        std::env::var_os("AESTRA_PARTITION_PROBE").expect("fresh absolute output directory"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("report.json"), "{\"passed\":false}").unwrap();
    let report = gpu::run(&root);
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!(report)).unwrap(),
    )
    .unwrap();
}
