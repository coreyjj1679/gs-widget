#!/usr/bin/env python3
"""Summarize the fixed four-page x five-visit native GUI trace protocol.

Usage:
  python3 parse_habitat_trace.py --before reports/habitat-before.jsonl --out reports/habitat-comparison.json
  python3 parse_habitat_trace.py --before reports/habitat-before.jsonl --after reports/habitat-after.jsonl --out reports/habitat-comparison.json

Navigation ordinal 0 is an item warm-up. Only ordinals 1..20 are included;
21+ belong to later verification/screenshots. Timings are not nested-span sums.
"""
import argparse
import collections
import hashlib
import json
import statistics
from pathlib import Path

SCENARIOS = [
    ("monster_directory", "怪物目錄", "navigation.monster", "list.publish"),
    ("black_merchant_camp", "黑色商團營地", "navigation.monster", "list.reload_board"),
    ("item_entry", "道具入口", "navigation.item", "list.publish"),
    ("item_materials", "道具材料", "navigation.item", "list.reload_board"),
]
METRICS = [
    "preparation_ms", "update_to_first_render_ms", "nav_to_first_render_ms",
    "first_frame_render_span_ms", "queued_icon_jobs", "icon_paint_batches",
    "icon_paint_targets", "first_icon_paint_after_nav_ms", "last_icon_paint_after_nav_ms",
    "max_icon_queue_wait_ms", "max_icon_paint_batch_ms", "max_pending_ui_batches",
    "first_render_after_last_icon_paint_ms", "max_event_loop_delay_first_500ms",
]


def distribution(values):
    present = [value for value in values if value is not None]
    if not present:
        return {"samples": 0, "median": None, "min": None, "max": None}
    return {"samples": len(present), "median": statistics.median(present), "min": min(present), "max": max(present)}


def summarize(path):
    events = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    process_ids = {event["process_id"] for event in events}
    if len(process_ids) != 1:
        raise ValueError(f"{path}: expected one process, found {sorted(process_ids)}")
    navigations = sorted((event for event in events if event["stage"].startswith("navigation.")), key=lambda event: event["since_start_ms"])
    if len(navigations) < 21:
        raise ValueError(f"{path}: expected warm-up plus 20 target navigations, found {len(navigations)}")
    if navigations[0]["stage"] != "navigation.item":
        raise ValueError(f"{path}: ordinal 0 is not the expected item warm-up")
    visits = []
    for ordinal in range(1, 21):
        key, label, expected_stage, parent_span = SCENARIOS[(ordinal - 1) % 4]
        nav = navigations[ordinal]
        start = nav["since_start_ms"]
        end = navigations[ordinal + 1]["since_start_ms"] if ordinal + 1 < len(navigations) else float("inf")
        if nav["stage"] != expected_stage:
            raise ValueError(f"{path}: ordinal {ordinal}: expected {expected_stage}, got {nav['stage']}")
        # Opening a record has detail.open but no navigation.* marker. Later UI
        # verification can otherwise leak into ordinal20's long idle interval.
        end_reason = "next_navigation" if end != float("inf") else "trace_end"
        if ordinal == 20 and start + 1000 < end:
            end = start + 1000
            end_reason = "final_visit_1000ms_observation_window"
        details = [event["since_start_ms"] - event["elapsed_ms"] for event in events
                   if event["stage"] == "detail.open"
                   and start < event["since_start_ms"] - event["elapsed_ms"] < end]
        if details:
            end = min(details)
            end_reason = "subsequent_detail_open_start"
        segment = sorted((event for event in events if start <= event["since_start_ms"] < end), key=lambda event: event["since_start_ms"])
        stages = collections.defaultdict(list)
        for event in segment:
            stages[event["stage"]].append(event)
        parents = stages[parent_span]
        if len(parents) != 1:
            raise ValueError(f"{path}: ordinal {ordinal}: expected exactly one {parent_span}, found {len(parents)}")
        updates = stages["ui.update_to_render"]
        if not updates:
            raise ValueError(f"{path}: ordinal {ordinal} has no ui.update_to_render")
        update = updates[0]
        frame = next((event for event in reversed(stages["ui.render_span"]) if event["since_start_ms"] <= update["since_start_ms"]), None)
        paints = stages["icon.paint"]
        first_paint = paints[0]["since_start_ms"] - start if paints else None
        last_paint = paints[-1]["since_start_ms"] - start if paints else None
        next_frame = next((event["since_start_ms"] - start for event in stages["ui.render_span"] if paints and event["since_start_ms"] >= paints[-1]["since_start_ms"]), None)
        # icon.decode is recorded immediately before scheduling, queue_wait at
        # callback entry. This is an observed proxy, not a scheduler assertion.
        pending = maximum_pending = 0
        for event in segment:
            if event["stage"] == "icon.decode":
                pending += 1
                maximum_pending = max(maximum_pending, pending)
            elif event["stage"] == "icon.queue_wait":
                pending -= 1
        visits.append({
            "ordinal": ordinal, "visit": (ordinal - 1) // 4, "scenario": key, "label": label,
            "navigation_stage": nav["stage"], "navigation_ms": start,
            "segment_end_ms": end if end != float("inf") else None, "segment_end_reason": end_reason,
            "parent_span": parent_span, "preparation_ms": parents[0]["elapsed_ms"],
            "update_to_first_render_ms": update["elapsed_ms"],
            "nav_to_first_render_ms": update["since_start_ms"] - start,
            "first_frame_render_span_ms": frame["elapsed_ms"] if frame else None,
            "published_row_counts": [event["count"] for event in stages["list.rows"]],
            "queued_icon_jobs": sum(event["count"] for event in stages["icons.queued"]),
            "icon_paint_batches": len(paints),
            "icon_paint_targets": sum(event["count"] for event in paints),
            "first_icon_paint_after_nav_ms": first_paint,
            "last_icon_paint_after_nav_ms": last_paint,
            "first_render_after_last_icon_paint_ms": next_frame,
            "max_icon_queue_wait_ms": max((event["elapsed_ms"] for event in stages["icon.queue_wait"]), default=None),
            "max_icon_paint_batch_ms": max((event["elapsed_ms"] for event in paints), default=None),
            "max_pending_ui_batches": maximum_pending,
            "pending_batches_at_next_navigation": pending,
            "max_event_loop_delay_first_500ms": max((event["elapsed_ms"] for event in stages["ui.event_loop_delay"] if event["since_start_ms"] - start <= 500), default=None),
        })
    scenarios = []
    for key, label, _, _ in SCENARIOS:
        selected = [visit for visit in visits if visit["scenario"] == key]
        scenarios.append({
            "scenario": key, "label": label, "first": selected[0],
            "warm_samples": 4,
            "warm": {metric: distribution([visit[metric] for visit in selected[1:]]) for metric in METRICS},
            "visits": selected,
        })
    return {"source": str(path), "trace_sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "process_id": next(iter(process_ids)), "included_navigation_ordinals": [1, 20], "excluded_warmup_navigation_ordinal": 0, "excluded_later_navigation_count": max(0, len(navigations) - 21), "scenarios": scenarios}


def fmt(value):
    return "n/a" if value is None else f"{value:.3f}"


def print_summary(label, report):
    print(f"\n{label}: ordinals 1..20; first measured visit + 4 warm revisits")
    for scenario in report["scenarios"]:
        first, warm = scenario["first"], scenario["warm"]
        def warm_range(metric):
            item = warm[metric]
            return f"{fmt(item['median'])} [{fmt(item['min'])}, {fmt(item['max'])}]"
        print(f"{scenario['label']}: prep first {fmt(first['preparation_ms'])} ms; warm {warm_range('preparation_ms')} ms")
        print(f"  update→first-render first {fmt(first['update_to_first_render_ms'])} ms; warm {warm_range('update_to_first_render_ms')} ms")
        print(f"  queued jobs first {first['queued_icon_jobs']}; warm {warm_range('queued_icon_jobs')}; published rows {first['published_row_counts']}")
        print(f"  first/last icon.paint completion after nav {fmt(first['first_icon_paint_after_nav_ms'])}/{fmt(first['last_icon_paint_after_nav_ms'])} ms; batches {first['icon_paint_batches']}; max queued batches {first['max_pending_ui_batches']}")
        print(f"  max icon queue wait {fmt(first['max_icon_queue_wait_ms'])} ms; warm last icon.paint {warm_range('last_icon_paint_after_nav_ms')} ms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = {"environment": {
        "cpu": "Apple M5", "memory_gib": 24, "os": "macOS", "version": "1.2.1", "build": "release",
        "renderer": "native application default; exact renderer not independently identified",
        "logical_width": 1414, "logical_height": 907, "scale_factor": 2, "language": "zh-en",
        "language_confirmation": "On-screen settings showed primary Chinese and secondary English; unchanged languages.txt predated the baseline run.",
        "legacy": False, "battle": False, "gallery": False, "parallel_compilation": False,
        "windows_measurement": False, "os_cold_cache": False, "code_released": False,
        "provenance": "Operator-supplied context for this fixed 2026-10-25 measurement protocol."
    }, "methodology": {
        "protocol": "Item warm-up navigation ordinal0, then four scenarios in fixed order, five rounds; analyze ordinals1..20 only.",
        "first": "First measured visit within each shared process after item warm-up; not fresh OS/disk cache.",
        "warm": "Median/min/max of visits2..5 (four samples), computed independently for each metric.",
        "preparation": "One outer list.publish or list.reload_board span. Nested query/rows/cards/model spans are never added to it.",
        "first_render": "ui.update_to_render measures update start to rendering-notifier completion; not all images loaded or screen-present latency.",
        "icon_paint": "First/last observed icon.paint callback completion relative to navigation. Counts are attempted target updates, including failures; not proof all images are present.",
        "pending_batches": "Observed icon.decode minus icon.queue_wait starts; proxy for waiting UI callbacks, not guaranteed scheduler instrumentation.",
        "attribution_limit": "Events attributed by navigation timestamp until next navigation or subsequent detail.open span start, whichever is first; final ordinal20 is additionally bounded to1000ms after navigation. This excludes later record-opening regression checks that have no navigation marker. Assumes protocol waits for outstanding jobs; nonzero pending count flags possible spillover.",
        "event_loop_delay": "Only recorded delays in first500ms are summarized; timer reports delays >=20ms, so null means no recorded delay, not proven zero."
    }, "before": summarize(args.before)}
    print_summary("BEFORE", report["before"])
    if args.after:
        report["after"] = summarize(args.after)
        print_summary("AFTER", report["after"])
        comparisons = []
        for before, after in zip(report["before"]["scenarios"], report["after"]["scenarios"]):
            comparisons.append({"scenario": before["scenario"], "first_delta_after_minus_before": {metric: after["first"][metric] - before["first"][metric] if after["first"][metric] is not None and before["first"][metric] is not None else None for metric in METRICS}, "warm_median_delta_after_minus_before": {metric: after["warm"][metric]["median"] - before["warm"][metric]["median"] if after["warm"][metric]["median"] is not None and before["warm"][metric]["median"] is not None else None for metric in METRICS}})
        report["comparison"] = comparisons
    args.out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"\nJSON: {args.out}")


if __name__ == "__main__":
    main()
