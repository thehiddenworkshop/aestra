"""Verify bounded cue/binding conservation in the F8 forward-output native runs.

Usage: python benchmarks/fireworks/check-fireworks-output-audit.py RUN_DIRECTORY [--wind-haze]
Uses only the standard library. Never changes historical reports or captures.
"""
import json
import sys
from collections import Counter
from pathlib import Path


def assess(root, wind_haze=False):
    summaries, failures = {}, []
    for manifest_path in sorted(root.rglob("manifest.json")):
        manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
        for entry in manifest["runs"]:
            report = json.loads(Path(entry["report"]).read_text(encoding="utf-8-sig"))
            key = f"{manifest_path.parent.name}/{entry['name']}"
            audit = report["particle_output_audit"]
            kinds = Counter()
            age = 0.0
            for row in audit["routes"].values():
                kinds[row["kind"]] += row["messages"]
                age = max(age, row["max_delivery_age_seconds"])
            binding = report["flash_binding"]
            rejection = report["flash_rejections"]
            classified = sum(binding[k] for k in (
                "scan_dropped", "non_particle", "missing_root", "epoch_mismatch", "unbound", "invalid", "bound"))
            realized = report["flashes_accepted"] + sum(rejection.values()) + binding["stale_intents"] + binding["duplicates"]
            links = list(report["star_path_admission"].values())
            births = sum(s["counts"]["accepted"] for s in links)
            expected_births = {"high": 37996, "medium": 18959, "low": 9313}[report["tier"]]
            expected_accepted = 11 if report["tier"] == "low" else 13
            expected_drops = 2 if report["tier"] == "low" else 0
            passed = (
                entry["exit_code"] == 0 and report["completed"] and report["native_gpu"]
                and report["history"] == "PlaybackOnly" and report["last_async_alive"] == 0
                and kinds["main_break"] == kinds["launch"] == binding["bound"] == 13
                and sum(kinds.values()) == binding["packets_seen"] - binding["non_particle"]
                and classified == binding["packets_seen"] and realized == binding["bound"]
                and not any(binding[k] for k in ("scan_dropped", "missing_root", "epoch_mismatch", "invalid", "stale_intents", "duplicates"))
                and report["flashes_accepted"] == expected_accepted
                and rejection == {"budget_dropped": expected_drops, "expired": 0, "invalid": 0, "disabled": 0}
                and audit["omitted_route_messages"] == 0 and len(audit["routes"]) <= 256
                and len(links) == 13 and births == expected_births
                and all(s["source_overflow"] == 0 and s["counts"]["expansion_omitted"] == 0
                        and s["counts"]["destination_rejected"] == 0 for s in links)
                and report["capacity"]["loss_free"] and report["capacity"]["observations"] > 0
                and len(report["captures"]) == 8 and all(c["delivered"] for c in report["captures"])
            )
            if wind_haze:
                passed = (passed and report["effect"].endswith("fireworks_show_wind_haze_smoke.aestra.ron")
                          and report["star_smoke_visible"] == (manifest_path.parent.name != "hidden")
                          and report["representative_flashes_visible"] == (not entry["name"].endswith("muted")))
            if not passed:
                failures.append(key)
            summaries[key] = {
                "passed": passed, "source_messages_by_kind": dict(kinds), "route_keys": len(audit["routes"]),
                "binding": binding, "flashes": report["flashes_accepted"], "rejections": rejection,
                "max_source_delivery_age_seconds": age, "births": births,
                "encoding_ledger": report["encoding_ledger"], "pending_updates": report["pending_pipeline_updates"],
            }
    expected = {f"medium-r{i}/medium-close-lit" for i in (1, 2, 3)} | {
        "high/high-close-lit", "low/low-close-lit"}
    if wind_haze:
        expected = {"high/high-audience-lit", "high/high-close-lit", "high/high-close-muted",
                    "medium/medium-close-lit", "low/low-close-lit", "hidden/high-close-lit"}
    if set(summaries) != expected:
        failures.append("missing or unexpected native control cells")
    result = {"passed": not failures, "failures": failures, "runs": summaries,
              "scope": "Finite forward image-lane runs, not universal lossless output transport, display latency or a cost gate. Low's two explicit light-budget rejections are accounted, not complete 13-flash realization."}
    (root / "assessment.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": result["passed"], "failures": failures, "runs": len(summaries)}, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(assess(Path(sys.argv[1]), len(sys.argv) > 2 and sys.argv[2] == "--wind-haze"))
