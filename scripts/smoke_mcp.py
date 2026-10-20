#!/usr/bin/env python3
"""Exercise the real GUI-subsystem binary as a stdio MCP child on Windows/macOS."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading


class Client:
    def __init__(self, binary, cwd, env, version="2025-11-25"):
        self.process = subprocess.Popen(
            [str(binary), "--mcp"], cwd=cwd, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", bufsize=1,
        )
        self.output = queue.Queue()
        self.errors = []
        self.sequence = 0
        self.version = version
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._errors, daemon=True).start()
        # New lifecycle carries the protocol and capabilities on every request.
        if version == "2026-07-28":
            return
        try:
            response = self.request("initialize", {
                "protocolVersion": version, "capabilities": {},
                "clientInfo": {"name": "gersang-smoke", "version": "1"},
            })
            assert response["protocolVersion"] == version, response
            assert response["capabilities"]["tools"] == {}, response
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.process.kill()
            self.process.wait(timeout=10)
            raise

    def _read(self):
        for line in self.process.stdout:
            self.output.put(line)
        self.output.put(None)

    def _errors(self):
        self.errors.extend(self.process.stderr)

    def send(self, message):
        self.process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.process.stdin.flush()

    def request(self, method, params=None, error=False):
        self.sequence += 1
        request_id = f"request-{self.sequence}"
        message = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if self.version == "2026-07-28":
            params = dict(params or {})
            params["_meta"] = {
                "io.modelcontextprotocol/protocolVersion": self.version,
                "io.modelcontextprotocol/clientCapabilities": {},
            }
        if params is not None:
            message["params"] = params
        self.send(message)
        try:
            line = self.output.get(timeout=30)
        except queue.Empty as exc:
            raise AssertionError(f"Timeout in {method}; stderr={''.join(self.errors)}") from exc
        assert line is not None, f"Server exited during {method}: {''.join(self.errors)}"
        response = json.loads(line)  # stdout must contain only MCP JSON.
        assert response["jsonrpc"] == "2.0" and response["id"] == request_id, response
        assert ("error" in response) == error, response
        return response["error" if error else "result"]

    def call(self, name, arguments=None, error=False):
        result = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        assert result.get("isError", False) == error, result
        value = result["structuredContent"]
        assert json.loads(result["content"][0]["text"]) == value
        if not error:
            assert value.get("catalog_version"), value
        else:
            assert value.get("error_code") and value.get("error"), value
        return value

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
                raise AssertionError("MCP child did not exit after stdin EOF")
        assert self.process.returncode == 0, "".join(self.errors)

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        if kind is not None:
            self.process.kill()
            self.process.wait(timeout=10)
        else:
            self.close()


def digest_tree(root):
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in root.rglob("*") if path.is_file()}


def smoke_scoped_queries(client, info, monster):
    stats = client.call("get_entity", {"key": "monster:M24", "language": "en", "sections": ["stats"]})
    assert stats["stats"] == monster["stats"]
    assert "text" not in stats and "related" not in stats and "facts" not in stats
    hero = client.call("get_entity", {"key": "hero:T49", "sections": ["stats", "skills"]})
    assert hero["stats"]["hp"] > 0 and isinstance(hero["skills"], list)
    assert "text" not in hero and "related" not in hero
    focused = client.call("query_catalog", {
        "keys": ["monster:M24"], "sections": ["stats"], "language": "en",
    })
    assert focused["action"] == "overview" and focused["results"][0]["stats"] == monster["stats"]
    assert "text" not in focused["results"][0] and "related" not in focused["results"][0]
    effects = client.call("get_entity", {"key": "set:66", "sections": ["effects"]})
    assert any(isinstance(row["required_pieces"], int) and isinstance(row["value"], int) for row in effects["effects"])
    production = client.call("get_entity", {"key": "item:597", "sections": ["production"]})
    assert any(row["key"] == "item:63" and row["quantity"] == 200
               for row in production["production"]["formula"]["requirements"])

    inferred = client.call("query_catalog", {"query": "SSA邊度打？", "category": "item", "limit": 1})
    assert inferred["status"] == "resolved" and inferred["action"] == "acquisition"
    assert inferred["results"]
    for row in inferred["results"]:
        acquisition = row["acquisition"]
        assert acquisition["requested_sources"] == ["monster_drops"]
        assert "monster_drops" in acquisition
        assert not {"recipes", "shops", "production", "quest_rewards"} & acquisition.keys()
    explicit = client.call("query_catalog", {
        "keys": ["item:6213"], "action": "acquisition", "sources": ["monster_drops"], "limit": 1,
    })["results"][0]["acquisition"]["monster_drops"]
    expected = client.call("find_item_drops", {"item_code": 6213, "limit": 1})
    assert explicit["total"] == expected["total"] and explicit["results"] == expected["results"]
    assert explicit["total"] > 1 and explicit["next_call"]
    call = explicit["next_call"]
    page = client.call(call["tool"], call["arguments"])
    assert page["offset"] == explicit["next_offset"] and page["total"] == explicit["total"]

    options = info["filters"]
    japan = next(row for row in options["regions"]["town"] if row["value"] == "japan")
    towns = client.call("search_catalog", {"category": "town", "filters": {"region": "japan"}, "limit": 2})
    assert towns["total"] == japan["count"] and towns["total"] > 2
    for row in towns["results"]:
        town = client.call("get_entity", {"key": row["key"], "sections": ["overview"]})
        assert town["region"] == "japan"
    first_item_category = next(row for row in options["item_categories"] if row["count"] > 0)
    items = client.call("search_catalog", {"category": "item", "filters": {
        "item_category": first_item_category["value"]}, "limit": 1})
    assert items["total"] == first_item_category["count"]
    monsters = client.call("search_catalog", {"category": "monster", "filters": {
        "region": "china", "element": 1, "is_boss": False}, "limit": 3})
    assert monsters["total"] > 0
    for row in monsters["results"]:
        stats = client.call("get_entity", {"key": row["key"], "sections": ["stats"]})["stats"]
        assert stats["element"] == 1 and not stats["is_boss"]
    for category, prefix in [("facility", "bay:"), ("building", "yard:")]:
        row = client.call("search_catalog", {"category": category, "limit": 1})["results"][0]
        assert row["key"].startswith(prefix)
        assert client.call("get_entity", {"key": row["key"], "sections": ["overview"]})["key"] == row["key"]
    client.call("search_catalog", {"category": "item", "filters": {"min_level": 100}}, error=True)
    invalid = client.call("search_catalog", {"category": "town", "filters": {"region": "not-a-region"}}, error=True)
    assert invalid["error_code"] == "INVALID_ARGUMENTS"
    unsupported = client.call("get_entity", {"key": "item:20", "sections": ["skills"]}, error=True)
    assert unsupported["error_code"] == "UNSUPPORTED_QUERY"
    client.call("query_catalog", {"keys": ["item:143"], "action": "overview", "sources": ["monster_drops"]}, error=True)


def smoke_relations(client):
    # These lists exceed the GUI's old 24-row limit; follow every executable
    # continuation and ensure the declared count is reachable without duplicates.
    page = client.call("get_relations", {"key": "item:20", "relation": "shops", "language": "ko", "limit": 7})
    total, keys = page["total"], set()
    assert total > 24
    while True:
        assert page["total"] == total and page["key"] == "item:20" and page["relation"] == "shops"
        for row in page["results"]:
            assert row["key"].startswith("town:") and row["key"] not in keys
            keys.add(row["key"])
        if page["next_call"] is None:
            assert page["next_offset"] is None
            break
        call = page["next_call"]
        assert call["tool"] == "get_relations" and call["arguments"]["language"] == "ko"
        assert call["arguments"]["offset"] == page["next_offset"] == len(keys)
        page = client.call(call["tool"], call["arguments"])
    assert len(keys) == total
    deep = client.call("get_relations", {"key": "item:5507", "relation": "uses", "offset": 200, "limit": 3})
    assert deep["total"] > 1000 and deep["offset"] == 200 and len(deep["results"]) == 3
    assert all(row["usage_kind"] == "recipe_material" for row in deep["results"])
    summary = client.call("get_entity", {"key": "item:5507", "sections": ["related"]})
    assert summary["related_total"] > 1000 and summary["related_truncated"]
    assert len(summary["related"]) == 50
    assert all(not row["key"].startswith("section:") for row in summary["related"])
    for key, relation in [("town:KY000001", "goods"), ("bay:WEA", "products"), ("yard:FAC", "facilities")]:
        page = client.call("get_relations", {"key": key, "relation": relation, "limit": 1})
        assert page["total"] > 0
        target = page["results"][0]["key"]
        assert client.call("get_entity", {"key": target, "sections": ["overview"]})["key"] == target
    client.call("get_relations", {"key": "item:20", "relation": "goods"}, error=True)
    client.call("get_relations", {"key": "item:20", "relation": "shops", "limit": 0}, error=True)


def smoke_candidate_pages(client):
    page = client.call("query_catalog", {
        "query": "副本傳送符(幽冥界3層)", "category": "item", "limit": 2,
    })
    total, keys = page["matched_count"], set()
    assert total > 2
    while True:
        assert page["status"] == "ambiguous" and not page["results"]
        assert page["matched_count"] == total
        assert all(call["requires_choice"] for call in page["next_calls"])
        for row in page["candidates"]:
            assert row["key"] not in keys
            keys.add(row["key"])
        if page["next_call"] is None:
            assert not page["truncated"]
            break
        call = page["next_call"]
        assert call["arguments"]["candidate_offset"] == len(keys)
        assert call["arguments"]["limit"] == 2
        page = client.call(call["tool"], call["arguments"])
    assert len(keys) == total

    prices = [{"item_code": 4157, "enhance": 0, "unit_price": 10}]
    group = client.call("query_catalog", {
        "query": "SSA", "action": "calculate", "quantity": 10, "prices": prices, "limit": 4,
    })
    assert group["status"] == "ambiguous" and not group["results"]
    call = group["next_call"]
    assert call["arguments"]["quantity"] == 10 and call["arguments"]["prices"] == prices
    last = client.call(call["tool"], call["arguments"])
    # Reaching the last candidate is still pagination, not a recipe choice.
    assert last["status"] == "ambiguous" and not last["results"]
    assert len(last["candidates"]) == 1 and last["next_call"] is None
    assert all(call["requires_choice"] for call in last["next_calls"])


def smoke_external_terminology(binary, root, data, env):
    source = {"source": "test", "reference": "local smoke fixture", "note": "A test-only reviewed mapping."}
    term = {"id": "fixture-hero", "aliases": ["Sample Alias"], "keys": ["hero:T49"],
            "group": False, "canonical_names": {"zh": "持國天王", "en": "Dhritarashtra", "ko": "지국천왕"},
            "evidence": [source]}
    book = {"schema_version": 1, "description": "Complete external smoke fixture",
            "acquisition_note": "Recorded sources only", "entities": [term],
            "intents": [{"id": "introduce", "intent": "overview", "topic": "details",
                         "aliases": ["介紹"], "evidence": [source]}]}
    path = data / "terminology.json"
    def save(destination=path):
        destination.write_text(json.dumps(book, ensure_ascii=False), encoding="utf-8")

    save()
    before = digest_tree(root)
    with Client(binary, root, env) as client:
        assert client.call("catalog_info")["terminology"]["source"] == "external"
        value = client.call("query_catalog", {"query": "Sample   Alias 介紹"})
        assert value["action"] == "overview" and value["results"][0]["key"] == "hero:T49"
        assert digest_tree(root) == before, "Loading external terminology must be read-only"
        term["aliases"] = ["Replacement Alias"]
        save()
        # Running clients retain a validated snapshot until restart.
        assert client.call("query_catalog", {"query": "Sample Alias"})["status"] == "resolved"
        assert client.call("query_catalog", {"query": "Replacement Alias"})["status"] == "not_found"
        with Client(binary, root, env) as restarted:
            assert restarted.call("query_catalog", {"query": "Replacement Alias"})["status"] == "resolved"
            assert restarted.call("query_catalog", {"query": "Sample Alias"})["status"] == "not_found"

    # An explicit path wins over the adjacent file, without polluting other clients.
    explicit = root / "explicit vocabulary 巨商.json"
    term["aliases"] = ["Explicit Alias"]
    save(explicit)
    override_env = dict(env, GERSANG_TERMINOLOGY=str(explicit))
    with Client(binary, root, override_env) as client:
        assert client.call("query_catalog", {"query": "Explicit Alias"})["status"] == "resolved"
        assert client.call("query_catalog", {"query": "Replacement Alias"})["status"] == "not_found"

    term["aliases"] = ["持國天王"]
    other = dict(term, id="fixture-second-hero", keys=["hero:T51"],
                 canonical_names={"zh": "增長天王", "en": "Virudhaka", "ko": "증장천왕"})
    book["entities"].append(other)
    save()
    with Client(binary, root, env) as client:
        value = client.call("query_catalog", {"query": "持國天王", "category": "hero"})
        assert value["status"] == "ambiguous" and not value["results"]
        assert {row["key"] for row in value["candidates"]} == {"hero:T49", "hero:T51"}

    def startup_error(environment, message):
        failed = subprocess.run([str(binary), "--mcp"], cwd=root, env=environment, input="",
                                capture_output=True, text=True, encoding="utf-8", timeout=15)
        assert failed.returncode != 0 and not failed.stdout and message in failed.stderr, failed

    term["keys"] = ["hero:not-in-the-catalog"]
    save()
    startup_error(env, "target does not exist")
    path.write_text("{broken", encoding="utf-8")
    startup_error(env, "terminology")
    startup_error(dict(env, GERSANG_TERMINOLOGY=str(root / "missing-terms.json")), "terminology")
    path.unlink()
    with Client(binary, root, env) as client:
        assert client.call("catalog_info")["terminology"]["source"] == "embedded"


def smoke(binary, database, packaged):
    with tempfile.TemporaryDirectory(prefix="gersang-mcp-") as temporary:
        root = Path(temporary)
        data = root / "catalog with spaces 巨商"
        data.mkdir()
        shutil.copyfile(database, data / "gersang.db")
        # Files that an MCP query must never migrate, overwrite or create alongside.
        (data / "calculator-library.json").write_text('{"sentinel":true}', encoding="utf-8")
        profile = root / "profile"
        profile.mkdir()
        env = os.environ.copy()
        env["LOCALAPPDATA"] = str(profile)
        env["GERSANG_DB"] = str(data / "gersang.db")
        env.pop("GERSANG_CLIENT", None)
        env.pop("GERSANG_TERMINOLOGY", None)
        before = digest_tree(root)
        with Client(binary, root, env) as client:
            tools = client.request("tools/list")["tools"]
            assert {tool["name"] for tool in tools} == {
                "catalog_info", "search_catalog", "get_entity", "find_item_drops",
                "get_quest_dependencies", "get_recipes", "calculate_recipe", "query_catalog", "get_relations",
            }
            assert all(tool["annotations"]["readOnlyHint"] for tool in tools)
            assert all(not tool["annotations"]["openWorldHint"] for tool in tools)
            assert all(tool["outputSchema"]["type"] == "object" for tool in tools)
            info = client.call("catalog_info")
            assert info["read_only"] and info["counts"]["items"] > 100
            assert info["terminology"]["source"] == "embedded"
            acquisition = client.call("query_catalog", {"query": "雀天要點合？"})
            assert acquisition["status"] == "resolved" and acquisition["action"] == "acquisition"
            assert acquisition["results"][0]["key"] == "hero:T49"
            assert any(route.get("materials") for route in acquisition["results"][0]["acquisition"]["routes"])
            comparison = client.call("query_catalog", {
                "keys": ["monster:M24", "monster:M25"], "action": "compare", "language": "ko",
            })
            assert comparison["status"] == "resolved" and len(comparison["results"]) == 2
            assert all(row["stats"]["hp"] > 0 for row in comparison["results"])
            hp = next(row for row in comparison["comparison"]["stats"] if row["field"] == "hp")
            assert hp["values"] == [row["stats"]["hp"] for row in comparison["results"]]
            aliases = client.call("search_catalog", {"category": "item", "query": "SSA"})
            assert any(row["key"] == "item:6213" for row in aliases["results"])
            first = client.call("search_catalog", {"category": "item", "limit": 2})
            later = client.call("search_catalog", {"category": "item", "offset": 80, "limit": 2})
            assert first["total"] == later["total"]
            assert first["results"][0]["key"] != later["results"][0]["key"]
            monster = client.call("get_entity", {"key": "monster:M24", "language": "en"})
            assert monster["stats"]["hp"] > 0
            smoke_scoped_queries(client, info, monster)
            smoke_relations(client)
            smoke_candidate_pages(client)
            # Character data must remain available without starting the GUI or
            # initializing a personal profile to discover portraits/animations.
            for key in ["hero:D115", "hero:mount:3", "hero:V03"]:
                character = client.call("get_entity", {"key": key, "language": "en"})
                assert character["key"] == key and character["kind"] == "hero"
                assert character["name"] and character["facts"]
            drop_key = next(row["key"] for row in monster["related"] if row["key"].startswith("item:"))
            drops = client.call("find_item_drops", {"item_code": int(drop_key.split(":")[1]), "limit": 50})
            assert any(row["key"] == "monster:M24" for row in drops["results"])
            quests = client.call("search_catalog", {"category": "quest", "limit": 1})
            quest_id = int(quests["results"][0]["key"].split(":")[1])
            client.call("get_quest_dependencies", {"quest_id": quest_id, "direction": "chain"})
            recipes = client.call("search_catalog", {"category": "recipe", "limit": 20})
            calculation = None
            for row in recipes["results"]:
                recipe_id = int(row["key"].split(":")[1])
                raw = client.request("tools/call", {"name": "calculate_recipe", "arguments": {"recipe_id": recipe_id, "quantity": 10}})
                if not raw.get("isError"):
                    calculation = raw["structuredContent"]
                    break
            assert calculation is not None, "No calculable recipe found"
            assert calculation["total"] is None and not calculation["complete"]
            prices = [{"item_code": row["item_code"], "enhance": row["enhance"], "unit_price": 10} for row in calculation["materials"]]
            priced = client.call("calculate_recipe", {"recipe_id": recipe_id, "quantity": 10, "prices": prices})
            expected = sum(row["quantity"] * 10 for row in calculation["materials"]) + calculation["crafting_fee"]
            assert priced["complete"] and priced["total"] == expected
            direct = client.call("query_catalog", {
                "keys": [f"item:{calculation['item_code']}"], "action": "calculate",
                "recipe_id": recipe_id, "quantity": 10, "prices": prices,
            })["results"][0]["calculation"]
            assert direct["status"] == "calculated" and direct["total"] == priced["total"]
            assert direct["materials"] == priced["materials"] and direct["requested_quantity"] == 10
            automatic = client.call("query_catalog", {"query": "強化的高級白水晶", "quantity": 3, "recipe_id": 1040})
            assert automatic["action"] == "calculate" and automatic["status"] == "resolved"
            direct = automatic["results"][0]["calculation"]
            assert direct["status"] == "calculated" and direct["crafting_fee"] == 182_040_000
            assert direct["total"] is None and not direct["complete"]
            choices = client.call("query_catalog", {"keys": ["item:1082"], "action": "calculate", "quantity": 10})["results"][0]["calculation"]
            assert choices["status"] == "choose_recipe" and len(choices["candidates"]) > 1
            assert all(row["next_call"]["requires_choice"] and row["next_call"]["arguments"]["quantity"] == 10
                       for row in choices["candidates"])
            client.call("get_recipes", {"item_code": calculation["item_code"]})
            missing = client.call("get_entity", {"key": "item:999999999"}, error=True)
            assert missing["error_code"] == "NOT_FOUND"
            client.call("query_catalog", {"query": "雀天", "keys": ["hero:T49"]}, error=True)
            client.call("query_catalog", {"keys": ["monster:M24"], "action": "compare"}, error=True)
            client.call("query_catalog", {"query": "雀天", "limit": 11}, error=True)
            client.call("search_catalog", {"category": "item", "limit": 0}, error=True)
            client.call("calculate_recipe", {"recipe_id": recipe_id, "prices": [prices[0], prices[0]]}, error=True)
            client.request("tools/call", {"name": "execute_sql", "arguments": {}}, error=True)
            client.request("ping")
            # A second GUI client may connect while the first one stays open.
            with Client(binary, root, env, "2024-11-05") as other:
                assert other.call("catalog_info")["catalog_version"] == info["catalog_version"]
        assert digest_tree(root) == before, "MCP changed catalog or personal data files"
        for version in ["2025-06-18", "2026-07-28"]:
            with Client(binary, root, env, version) as client:
                assert len(client.request("tools/list")["tools"]) == 9
                assert client.call("catalog_info")["read_only"]
        # Test the command path itself, not just the database path.
        spaced_directory = root / "application with spaces 巨商"
        spaced_directory.mkdir()
        spaced_binary = spaced_directory / binary.name
        shutil.copy2(binary, spaced_binary)
        with Client(spaced_binary, root, env) as client:
            assert client.call("catalog_info")["catalog_version"] == info["catalog_version"]
        smoke_external_terminology(binary, root, data, env)
        if packaged:
            env.pop("GERSANG_DB")
            with Client(binary, root, env) as client:
                assert client.call("catalog_info")["catalog_version"] == info["catalog_version"]
        env["GERSANG_DB"] = str(root / "missing.db")
        failed = subprocess.run([str(binary), "--mcp"], cwd=root, env=env, input="",
                                capture_output=True, text=True, encoding="utf-8", timeout=15)
        assert failed.returncode != 0 and not failed.stdout and "GERSANG_DB" in failed.stderr
    print("MCP smoke passed: 9 tools, scoped acquisition, numeric comparison, detail sections, structured filters, complete relation paging, Korean output, 4 protocol versions, concurrent clients, Unicode/spaced paths, EOF, read-only data, external terminology restart/precedence/ambiguity and startup errors.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--packaged", action="store_true", help="Also verify database discovery beside the packaged executable")
    args = parser.parse_args()
    smoke(args.exe.resolve(strict=True), args.database.resolve(strict=True), args.packaged)
