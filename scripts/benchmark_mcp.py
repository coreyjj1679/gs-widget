#!/usr/bin/env python3
"""Measure local stdio MCP startup, tool latency and response size (no model calls)."""
import argparse
import json
import math
import os
from pathlib import Path
import statistics
import tempfile
import time

from smoke_mcp import Client


CASES = [
    ("hero_alias", "search_catalog", {"category": "hero", "query": "雀天"}),
    ("hero_name", "search_catalog", {"category": "hero", "query": "持國天王"}),
    ("item_alias", "search_catalog", {"category": "item", "query": "SSA"}),
    ("browse_items", "search_catalog", {"category": "item", "limit": 1}),
    ("hero_details", "get_entity", {"key": "hero:T49"}),
    ("item_drops", "find_item_drops", {"item_code": 143, "limit": 1}),
]
QUERY_CASES = [
    ("hero_question", "query_catalog", {"query": "雀天要點合？"}),
    ("item_question", "query_catalog", {"query": "SSA", "action": "acquisition"}),
]


def extended_cases(tools):
    """Use advertised parameters so the original cases still run on old builds."""
    schemas = {tool["name"]: tool["inputSchema"].get("properties", {}) for tool in tools}
    cases = []
    if "sources" in schemas.get("query_catalog", {}):
        cases.append(("drop_question", "query_catalog", {"query": "SSA邊度打？", "category": "item", "limit": 1}))
    if "sections" in schemas.get("get_entity", {}):
        cases.append(("monster_stats", "get_entity", {"key": "monster:M24", "sections": ["stats"]}))
    if "filters" in schemas.get("search_catalog", {}):
        cases.extend([
            ("filtered_monsters", "search_catalog", {"category": "monster", "filters": {
                "region": "china", "element": 1, "is_boss": False}, "limit": 5}),
            ("filtered_towns", "search_catalog", {"category": "town", "filters": {"region": "japan"}, "limit": 5}),
        ])
    if "get_relations" in schemas:
        cases.append(("relations_deep_page", "get_relations", {"key": "item:5507", "relation": "uses", "offset": 200, "limit": 5}))
    return cases


def summary(samples):
    ordered = sorted(samples)
    return {"median_ms": round(statistics.median(samples), 3),
            "p95_ms": round(ordered[math.ceil(len(ordered) * .95) - 1], 3)}


def case_result(tool, arguments, samples, value):
    return {
        "tool": tool, "arguments": arguments, **summary(samples),
        "first_ms": round(samples[0], 3),
        "result_bytes": len(json.dumps(value, ensure_ascii=False).encode("utf-8")),
        "total": value.get("total"), "status": value.get("status"),
        "keys": [row.get("key") for row in value.get("results", [])],
    }


def benchmark(binary, database, repeats, profile):
    env = os.environ.copy()
    env["GERSANG_DB"] = str(database)
    env.pop("GERSANG_TERMINOLOGY", None)
    report = {"profile": profile, "repeats": repeats,
              "scope": "Local subprocess and stdio only; excludes model reasoning and client tool selection. Startup includes index loading; OS file caches are not flushed. result_bytes measures structured JSON only, excluding the JSON-RPC envelope and duplicated text content.",
              "cases": {}, "cold_cases": {}}
    starts = []
    with tempfile.TemporaryDirectory(prefix="gersang-mcp-benchmark-") as root:
        for _ in range(3):
            start = time.perf_counter()
            with Client(binary, root, env) as client:
                info = client.call("catalog_info")
                starts.append((time.perf_counter() - start) * 1000)
        report["startup"] = summary(starts)
        report["catalog_version"] = info["catalog_version"]
        report["app_version"] = info.get("app_version")
        with Client(binary, root, env) as client:
            tools = client.request("tools/list")["tools"]
            names = {tool["name"] for tool in tools}
            report["tool_count"] = len(tools)
            cases = CASES + (QUERY_CASES if "query_catalog" in names else []) + extended_cases(tools)
            quest = client.call("search_catalog", {"category": "quest", "limit": 1})["results"][0]
            quest_arguments = {"quest_id": int(quest["key"].split(":")[1]), "direction": "chain", "limit": 5}
            cases.extend([
                ("recipes", "get_recipes", {"item_code": 6893, "limit": 5}),
                ("quest_dependencies", "get_quest_dependencies", quest_arguments),
            ])
            for label, tool, arguments in cases:
                samples = []
                for _ in range(repeats):
                    start = time.perf_counter()
                    value = client.call(tool, arguments)
                    samples.append((time.perf_counter() - start) * 1000)
                report["cases"][label] = case_result(tool, arguments, samples, value)
        # Each sample gets a fresh process and its first tool call. OS disk caches
        # stay warm; initialization is measured separately from lazy tool setup.
        cold_cases = [
            ("recipes", "get_recipes", {"item_code": 6893, "limit": 5}),
            ("quest_dependencies", "get_quest_dependencies", quest_arguments),
        ]
        if "query_catalog" in names:
            cold_cases.insert(0, QUERY_CASES[0])
        for label, tool, arguments in cold_cases:
            samples, initializations = [], []
            for _ in range(3):
                start = time.perf_counter()
                with Client(binary, root, env) as client:
                    initializations.append((time.perf_counter() - start) * 1000)
                    start = time.perf_counter()
                    value = client.call(tool, arguments)
                    samples.append((time.perf_counter() - start) * 1000)
            report["cold_cases"][label] = {
                **case_result(tool, arguments, samples, value),
                "samples": len(samples), "initialization": summary(initializations),
                "process_to_response": summary([start + call for start, call in zip(initializations, samples)]),
            }
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--profile", required=True, help="Build profile, e.g. debug or release; compare the same profile")
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.repeats <= 100:
        parser.error("--repeats must be 1..100")
    report = benchmark(args.exe.resolve(strict=True), args.database.resolve(strict=True), args.repeats, args.profile)
    output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(output, encoding="utf-8")
    print(output)
