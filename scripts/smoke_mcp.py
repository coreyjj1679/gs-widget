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
        before = digest_tree(root)
        with Client(binary, root, env) as client:
            tools = client.request("tools/list")["tools"]
            assert len(tools) == 7 and len({tool["name"] for tool in tools}) == 7
            assert all(tool["annotations"]["readOnlyHint"] for tool in tools)
            assert all(not tool["annotations"]["openWorldHint"] for tool in tools)
            info = client.call("catalog_info")
            assert info["read_only"] and info["counts"]["items"] > 100
            aliases = client.call("search_catalog", {"category": "item", "query": "SSA"})
            assert any(row["key"] == "item:6213" for row in aliases["results"])
            first = client.call("search_catalog", {"category": "item", "limit": 2})
            later = client.call("search_catalog", {"category": "item", "offset": 80, "limit": 2})
            assert first["total"] == later["total"]
            assert first["results"][0]["key"] != later["results"][0]["key"]
            monster = client.call("get_entity", {"key": "monster:M24", "language": "en"})
            assert monster["stats"]["hp"] > 0
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
            client.call("get_recipes", {"item_code": calculation["item_code"]})
            client.call("get_entity", {"key": "item:999999999"}, error=True)
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
                assert len(client.request("tools/list")["tools"]) == 7
                assert client.call("catalog_info")["read_only"]
        # Test the command path itself, not just the database path.
        spaced_directory = root / "application with spaces 巨商"
        spaced_directory.mkdir()
        spaced_binary = spaced_directory / binary.name
        shutil.copy2(binary, spaced_binary)
        with Client(spaced_binary, root, env) as client:
            assert client.call("catalog_info")["catalog_version"] == info["catalog_version"]
        if packaged:
            env.pop("GERSANG_DB")
            with Client(binary, root, env) as client:
                assert client.call("catalog_info")["catalog_version"] == info["catalog_version"]
        env["GERSANG_DB"] = str(root / "missing.db")
        failed = subprocess.run([str(binary), "--mcp"], cwd=root, env=env, input="",
                                capture_output=True, text=True, encoding="utf-8", timeout=15)
        assert failed.returncode != 0 and not failed.stdout and "GERSANG_DB" in failed.stderr
    print("MCP smoke passed: 7 tools, 4 protocol versions, concurrent clients, Unicode/spaced paths, EOF, read-only data and startup errors.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--packaged", action="store_true", help="Also verify database discovery beside the packaged executable")
    args = parser.parse_args()
    smoke(args.exe.resolve(strict=True), args.database.resolve(strict=True), args.packaged)
