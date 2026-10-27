"""Check native smoke reports and describe image controls; never edit captures.

Usage: python benchmarks/fireworks/check-star-path-review.py OUTPUT_DIRECTORY [REPEAT_DIRECTORY]
Image measurements require Pillow and NumPy. This is not a cost/art approval gate.
"""

import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def totals(report):
    samples = list(report["star_path_admission"].values())
    return {
        "owners": len(samples),
        "source_overflow": sum(s["source_overflow"] for s in samples),
        **{
            key: sum(s["counts"][key] for s in samples)
            for key in ("accepted", "captured_demand", "expansion_omitted", "destination_rejected")
        },
    }


def capture(report, seconds):
    return min(report["captures"], key=lambda c: abs(c["nominal_seconds"] - seconds))


def roi(report, seconds):
    # Fixed close-camera upper-right old wake; exclude HUD and current bright heads.
    image = np.asarray(Image.open(capture(report, seconds)["path"]).convert("RGB"))
    pixels = image[135:230, 740:925].astype(float) / 255
    linear = np.where(pixels <= 0.04045, pixels / 12.92, ((pixels + 0.055) / 1.055) ** 2.4)
    return {
        "mean_display_rgb_255": (pixels.mean(axis=(0, 1)) * 255).tolist(),
        "mean_linear_luminance": float((linear @ np.array([0.2126, 0.7152, 0.0722])).mean()),
    }


def check(root, repeat_root=None):
    manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8-sig"))
    runs = {
        item["name"]: json.loads(Path(item["report"]).read_text(encoding="utf-8-sig"))
        for item in manifest["runs"]
    }
    exits = {item["name"]: item["exit_code"] for item in manifest["runs"]}
    smoke_failures, flash_failures, unaccounted_flashes, safety_failures = [], [], [], []
    summary = {}
    for name, report in runs.items():
        population = totals(report)
        caps = report["capacity"]
        ledger = report["encoding_ledger"]
        passed = (
            exits[name] == 0 and report["completed"] and report["native_gpu"] and report["hdr"]
            and report["history"] == "PlaybackOnly" and report["last_async_alive"] == 0
            and population["owners"] == 13 and population["accepted"] > 0
            and population["accepted"] == population["captured_demand"]
            and not any(population[k] for k in ("source_overflow", "expansion_omitted", "destination_rejected"))
            and caps["loss_free"] and caps["observations"] > 0
            and len(report["captures"]) == 8 and all(c["delivered"] for c in report["captures"])
            and not report["selected_lighting_enabled"]
            and ledger["blocked_owner_frames"] == ledger["missing_scheduled_owner_frames"] == 0
        )
        if not passed:
            smoke_failures.append(name)
        if report["flashes_accepted"] != 13 or any(report["flash_rejections"].values()):
            flash_failures.append(name)
        rejected = report["flash_rejections"]
        accounted = report["flashes_accepted"] + rejected["budget_dropped"]
        if accounted != 13 or any(rejected[k] for k in ("expired", "invalid", "disabled")):
            unaccounted_flashes.append(name)
        cap = {"high": 8, "medium": 4, "low": 2}[report["tier"]]
        if report["flashes_peak"] > cap:
            safety_failures.append(name)
        muted = not report["representative_flashes_visible"]
        peaks = [c["representative_proxy_peak_lumens"] for c in report["captures"]]
        if (muted and any(peaks)) or (not muted and max(peaks) <= 0):
            flash_failures.append(name + ": intensity control")
        summary[name] = {"admission": population, "smoke_gate": passed,
                         "flashes": report["flashes_accepted"], "rejections": report["flash_rejections"],
                         "pending_updates": report["pending_pipeline_updates"],
                         "sampled_encoding_lag_seconds": ledger["max_requested_minus_encoded_seconds"]}
    comparisons = {}
    for tier in ("high", "medium", "low"):
        names = [f"{tier}-{case}" for case in ("audience-lit", "close-lit", "close-muted")]
        if not all(n in runs for n in names):
            smoke_failures.append(tier + ": missing camera/control")
            continue
        if len({totals(runs[n])["accepted"] for n in names}) != 1:
            smoke_failures.append(tier + ": control changed births")
        on, off = runs[names[1]], runs[names[2]]
        comparisons[tier] = {
            str(seconds): {"lit": roi(on, seconds), "muted": roi(off, seconds),
                           "lit_request_seconds": capture(on, seconds)["request_seconds"],
                           "muted_request_seconds": capture(off, seconds)["request_seconds"]}
            for seconds in (9.3, 22)
        }
    result = {
        "smoke_gate_passed": not smoke_failures, "smoke_failures": smoke_failures,
        "all_flash_delivery_passed": not flash_failures, "flash_failures": flash_failures,
        "flash_accounting_passed": not unaccounted_flashes, "unaccounted_flashes": unaccounted_flashes,
        "light_budget_safety_passed": not safety_failures, "light_budget_safety_failures": safety_failures,
        "photographic_art_approved": False, "performance_accepted": False,
        "runs": summary, "close_roi_xyxy": [740, 135, 925, 230], "roi_controls": comparisons,
        "image_scope": "Forward asynchronous nominal-time captures, not exact pairs. Fixed ROI is descriptive, not an exact-pixel or isolated light-family acceptance threshold. Quiet 22s is the no-active-flash comparison. Selected lighting is absent.",
    }
    if repeat_root:
        repeat_manifest = json.loads((repeat_root / "manifest.json").read_text(encoding="utf-8-sig"))
        result["repetitions_do_not_erase_initial_failures"] = {}
        for item in repeat_manifest["runs"]:
            report = json.loads(Path(item["report"]).read_text(encoding="utf-8-sig"))
            result["repetitions_do_not_erase_initial_failures"][item["name"]] = {
                "exit_code": item["exit_code"], "completed": report["completed"],
                "admission": totals(report), "flashes": report["flashes_accepted"],
                "rejections": report["flash_rejections"], "capacity": report["capacity"],
                "last_async_alive": report["last_async_alive"],
                "sampled_encoding_lag_seconds": report["encoding_ledger"]["max_requested_minus_encoded_seconds"],
                "roi": {str(t): roi(report, t) for t in (9.3, 22)},
            }
    (root / "assessment.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: result[k] for k in ("smoke_gate_passed", "all_flash_delivery_passed", "flash_accounting_passed", "unaccounted_flashes")}, indent=2))
    return 0 if not smoke_failures and not unaccounted_flashes and not safety_failures else 1


if __name__ == "__main__":
    sys.exit(check(Path(sys.argv[1]), Path(sys.argv[2]) if len(sys.argv) > 2 else None))
