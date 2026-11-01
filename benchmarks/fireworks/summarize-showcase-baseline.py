"""Validate and summarize a completed run-showcase-baseline.ps1 directory (stdlib only)."""
import argparse
import hashlib
import json
from pathlib import Path

PHASES = ("launch-and-overlap", "active-fireworks", "smoke-drain", "idle")
GPU = "render/fireworks_render_graph/elapsed_gpu"
SCOPES = ("fireworks_render_graph/elapsed_gpu", "aestra_oit_partitions_with_health/elapsed_gpu",
          "aestra::gpu::simulate/elapsed_gpu", "aestra::gpu::particle_lights/elapsed_gpu",
          "aestra::gpu::particle_light_inject/elapsed_gpu", "aestra::gpu::trail_compaction/elapsed_gpu",
          "bloom/elapsed_gpu", "tonemapping/elapsed_gpu")
REPO = Path(__file__).resolve().parents[2]


def proof(path):
    return {"path": str(path.resolve().relative_to(REPO)).replace("\\", "/"),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def summarize(root):
    manifest = load(root / "manifest.json")
    expected = ["images", "warmup"] + [f"{lane}-{n}" for n in range(1, manifest["repetitions"] + 1)
                                        for lane in ("ordinary", "profile")]
    require(sorted(r["name"] for r in manifest["runs"]) == sorted(expected), "Missing/extra lanes")
    runs = []
    for entry in manifest["runs"]:
        path = root / f'{entry["name"]}.json'
        r = load(path)
        require(entry["exit_code"] == 0 and r["completed"], f"Failed lane: {path}")
        require(r["showcase_preset"] == "fireworks-showcase-v1" and r["native_gpu"]
                and r["history"] == "PlaybackOnly" and not r["audio_enabled"], "Candidate changed")
        require(r["build"]["profile"] == "release" and r["build"]["opt_level"] == "3"
                and not r["build"]["debug_assertions"], "Expected optimized release")
        require(r["review_camera"] == "audience" and r["camera_physical_size"] == [1280, 720], "View changed")
        require(r["fragment_capacity_passed"] is True and r["capacity"]["loss_free"], "Missing/failed health")
        require(r["node_head_allocated_bytes"] == 47_923_200, "Storage changed")
        h = r["host_state"]
        require(h["launches"] == h["detonations"] == r["flashes_accepted"] == 12, "Workload changed")
        require(h["owner_limit"] is None and h["current_vfx_owners"] == h["current_gameplay_rockets"]
                == r["last_async_alive"] == 0, "Admission capped or cleanup incomplete")
        require(r["alpha_sort"]["peak_view_draw_pairs"] == 0, "Redundant sort returned")
        row = {"name":entry["name"], "report":proof(path), "capacity":r["capacity"],
               "host_state":h, "build":r["build"], "host_phases":r["host_phase_intervals_including_vsync"]}
        for phase in PHASES:
            require(row["host_phases"][phase]["samples"] >= 20, f"Insufficient phase samples: {phase}")
        if entry["profile"]:
            p = r["profile"]
            row["profile_phases"] = {phase:{**q, "gpu_scopes_ms":{
                k:v for k,v in q["gpu_scopes_ms"].items() if k.endswith(SCOPES)}}
                for phase,q in p["observed_phases"].items()}
            for phase in PHASES:
                q = p["observed_phases"][phase]
                require(q["gpu_scopes_ms"][GPU]["samples"] >= 20 and q["cpu_render_graph_ms"]["samples"] >= 20,
                        f"Missing render-graph timestamps: {phase}")
            row["render_graph_scope"] = p["render_graph_scope"]
        if entry["images"]:
            routes = {}
            for link in r["event_link_admission"].values():
                c = link["counts"]
                require(c["captured_demand"] == c["accepted"] and c["destination_rejected"]
                        == c["expansion_omitted"] == link["source_overflow"] == 0, "Lost births")
                routes[link["target"]] = routes.get(link["target"], 0) + c["accepted"]
            require(routes == {"Gold tail embers":374514, "Star path smoke":115283}, "Birth totals changed")
            row["routes"] = routes
            row["captures"] = [proof(Path(c["path"])) for c in r["captures"]]
        runs.append(row)
    image_report = load(root / "images.json")
    return {"preset":manifest["preset"], "manifest":proof(root / "manifest.json"),
            "adapter":image_report["adapter"],
            "presentation":{k:image_report[k] for k in ("camera_physical_size", "review_camera", "hdr", "tonemapping", "bloom_intensity", "msaa")},
            "binary":proof(Path(manifest["binary"])), "analyzer":proof(Path(__file__)),
            "runs":runs, "scope":"One hardware/view release baseline, not final art/F9 approval. Warm-up and images excluded from timing tables. Phase classification and diagnostic sampling are asynchronous."}


def interval(values, key):
    numbers = [v[key] for v in values]
    return f"{min(numbers):.3f}–{max(numbers):.3f}"


def markdown(data):
    ordinary = [r for r in data["runs"] if r["name"].startswith("ordinary-")]
    profiles = [r for r in data["runs"] if r["name"].startswith("profile-")]
    lines = ["# Showcase v1 release baseline", "", data["scope"], "",
             "Ranges below are independent per-run p95/p99 values; they are not pooled or added.", "",
             "| Phase | Ordinary host p95 / p99 ms (vsync included) | Instrumented render-graph GPU p95 / p99 ms |",
             "|---|---:|---:|"]
    for phase in PHASES:
        host = [r["host_phases"][phase] for r in ordinary]
        gpu = [r["profile_phases"][phase]["gpu_scopes_ms"][GPU] for r in profiles]
        lines.append(f"| {phase} | {interval(host,'p95_ms')} / {interval(host,'p99_ms')} | {interval(gpu,'p95_ms')} / {interval(gpu,'p99_ms')} |")
    lines += ["", "| Instrumented CPU wall interval | Phase | p95 ms | p99 ms |", "|---|---|---:|---:|"]
    for phase in PHASES[:2]:
        for key in ("cpu_render_graph_ms", "cpu_playback_ms", "cpu_encoding_ms", "cpu_stateful_preparation_ms"):
            values = [r["profile_phases"][phase][key] for r in profiles]
            require(all(values), f"Missing CPU measurements: {key}")
            lines.append(f"| {key} | {phase} | {interval(values,'p95_ms')} | {interval(values,'p99_ms')} |")
    lines += ["", profiles[0]["render_graph_scope"], "",
              "Partition health is included in every lane. Image runs separately verify births and capacity. Node/head storage is 47,923,200 bytes, not total VRAM. Audio is disabled; owners are uncapped. A quiet full-show percentile is not an active-load budget.", ""]
    return "\n".join(lines)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output-prefix", type=Path, required=True)
    args = parser.parse_args()
    paths = [args.output_prefix.with_suffix(suffix) for suffix in (".json", ".md")]
    require(not any(p.exists() for p in paths), "Use a fresh output prefix")
    data = summarize(args.directory)
    report = markdown(data)
    paths[0].write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    paths[1].write_text(report, encoding="utf-8")
    print(report)
