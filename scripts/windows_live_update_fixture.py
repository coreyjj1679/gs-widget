#!/usr/bin/env python3
"""Isolated data and evidence for an actual published-release GUI update.

This fixture never stages an update, invokes a helper, or changes the update
endpoint. Only the running app prepares and installs its downloaded release.
"""

import argparse
import json
import sqlite3
from contextlib import closing
from pathlib import Path

from windows_update_fixture import EXE, MANAGED, assert_manifest, digest, extract, manifest, save

MARKER = "live-update-context.json"
BASELINE_SHA256 = "5e036aad294b60cab04f4ca22271f28bedd5b47e281cced5a463561f88230098"


def context(root: Path) -> dict:
    value = json.loads((root / MARKER).read_text(encoding="utf-8"))
    if value.get("schema") != 1 or value.get("kind") != "published-gui-update":
        raise ValueError("Unsupported live update fixture context")
    return value


def prepare(args) -> None:
    root = args.root.resolve()
    archive = args.baseline_zip.resolve()
    baseline_digest = digest(archive)
    if baseline_digest["sha256"] != BASELINE_SHA256:
        raise ValueError("Live update fixture requires the exact published v1.0.0 ZIP")
    # Do not accept an existing installation or real user profile as a fixture.
    root.mkdir(parents=True, exist_ok=False)
    app = root / "app with spaces"
    profile = root / "isolated profile" / "gs-widget"
    profile.mkdir(parents=True)
    (root / "logs").mkdir()
    extract(archive, app)
    with closing(sqlite3.connect((app / "data/gersang.db").as_uri() + "?mode=ro", uri=True)) as db:
        items = [str(row[0]) for row in db.execute("SELECT code FROM items ORDER BY code LIMIT 2")]
        monsters = [str(row[0]) for row in db.execute("SELECT code FROM monsters ORDER BY code LIMIT 2")]
    if len(items) != 2 or len(monsters) != 2:
        raise ValueError("Live update fixture needs at least two valid items and monsters")
    preferences = {
        "favourites.txt": "\n".join(monsters) + "\n",
        "aliases.txt": f"# Live update: Unicode aliases must survive.\n升級測試 {items[0]}\nmy_live_alias {items[1]}\n",
        "ui-lang.txt": "en-zh",
        "view.txt": "pin=off\nbattle=0\ngallery=1\nanimate=0\nhotkeys=0\nlegacy=0\n",
        "dock.txt": "v=1\nshow=110111111\norder=8,7,6,5,4,3,2,1,0\nsize=84,580\n",
        "home.txt": "size=2\norder=8,7,6,5,4,3,2,1,0\n",
        # Keep the recorded GUI screenshots opaque; the earlier dummy update
        # already exercises preservation of a non-default 85% opacity setting.
        "opacity.txt": "100",
        "set-extra.txt": "1",
        "window.txt": "size=560,600\npos=32,32\nmode=widget\nfull-pos=48,48\nfull-size=1120,720\n",
        "window-macos.txt": "preserve cross-platform personal settings\n",
        "client.txt": str(app / "data/client"),
    }
    unmanaged = []
    for name, body in preferences.items():
        (profile / name).write_text(body, encoding="utf-8")
        relative = f"data/{name}"
        (app / relative).write_text(f"Legacy fallback must not win: {name}\n", encoding="utf-8")
        unmanaged.append(relative)
    (app / "personal notes.txt").write_text("Keep this unrelated root file.\n", encoding="utf-8")
    (app / "data/personal notes.txt").write_text("Keep this unrelated data file. 收藏\n", encoding="utf-8")
    unmanaged += ["personal notes.txt", "data/personal notes.txt"]
    save(root / MARKER, {
        "schema": 1,
        "kind": "published-gui-update",
        "baseline_version": "1.0.0",
        "baseline_archive_path": str(archive),
        "baseline_archive": baseline_digest,
        "installation_path": str(app),
        "profile_path": str(profile),
        "baseline_managed": manifest(app, MANAGED),
        "initial_profile": manifest(profile, list(preferences)),
        "initial_window_text": preferences["window.txt"],
        "unmanaged": manifest(app, unmanaged),
    })
    print(f"Prepared published v1.0.0 with {len(preferences)} personal files and {len(unmanaged)} unrelated files")


def snapshot(args) -> None:
    root = args.root.resolve()
    value = context(root)
    profile = root / "isolated profile/gs-widget"
    window = (profile / "window.txt").read_text(encoding="utf-8")
    value["window_before_update"] = window
    save(root / MARKER, value)
    unchanged = {name: entry for name, entry in value["initial_profile"].items() if name != "window.txt"}
    assert_manifest(profile, unchanged, "Initial personal data")
    fields = dict(line.split("=", 1) for line in window.splitlines() if "=" in line)
    expected = {"mode": "widget", "size": "560,600", "full-pos": "48,48", "full-size": "1120,720"}
    if any(fields.get(key) != wanted for key, wanted in expected.items()):
        raise AssertionError(f"Baseline GUI changed saved mode or dimensions before updating: {window!r}")
    assert_manifest(root / "app with spaces", value["baseline_managed"], "Baseline application")
    assert_manifest(root / "app with spaces", value["unmanaged"], "Legacy/unrelated installation data")
    value["profile_before_update"] = manifest(profile, list(value["initial_profile"]))
    save(root / MARKER, value)
    print(f"Recorded all {len(value['profile_before_update'])} personal files before the real GUI update")


def read_json(path: Path, limit: int) -> object:
    if path.stat().st_size > limit:
        raise ValueError(f"Evidence file exceeds its expected limit: {path.name}")
    return json.loads(path.read_text(encoding="utf-8"))


def verify(args) -> None:
    root = args.root.resolve()
    value = context(root)
    if "profile_before_update" not in value:
        raise ValueError("Record a personal-data snapshot before invoking the GUI update")
    app = root / "app with spaces"
    profile = root / "isolated profile/gs-widget"
    archive = args.candidate_zip.resolve()
    evidence = {
        "status": "running",
        "version": args.version,
        "public_candidate_archive_path": str(archive),
        "scope": "App-generated update plan, installed files and recovery backup compared with independently downloaded public release ZIP",
    }
    try:
        value["window_after_update"] = (profile / "window.txt").read_text(encoding="utf-8")
        save(root / MARKER, value)
        assert_manifest(profile, value["profile_before_update"], "Personal data")
        assert_manifest(app, value["unmanaged"], "Legacy/unrelated installation data")
        candidate = root / "independent candidate verification"
        candidate.mkdir(exist_ok=False)
        extract(archive, candidate)
        expected = manifest(candidate, MANAGED)
        public_archive_digest = digest(archive)
        if expected[EXE] == value["baseline_managed"][EXE]:
            raise AssertionError("Public candidate executable is identical to the baseline")
        if manifest(app, MANAGED) != expected:
            raise AssertionError("Installed files do not exactly match the public candidate ZIP")
        stages = [path for path in app.glob(".gs-widget-update-*") if path.is_dir()]
        completed = [path for path in stages if (path / "completed").is_file()]
        if len(completed) != 1:
            raise AssertionError(f"Expected one completed app-generated update, found {len(completed)}")
        stage = completed[0]
        evidence["completed_staging_path"] = str(stage)
        if (stage / "completed").read_text(encoding="utf-8") != args.version:
            raise AssertionError("Completed update version differs from the public candidate version")
        for marker in ("error.txt", "cancel"):
            if (stage / marker).exists():
                raise AssertionError(f"Completed update unexpectedly contains {marker}")
        if not (stage / "ready").is_file():
            raise AssertionError("App update helper did not leave its readiness acknowledgement")
        plan = read_json(stage / "plan.json", 32 * 1024 * 1024)
        if not isinstance(plan, dict) or plan.get("schema") != 1 or plan.get("version") != args.version:
            raise AssertionError("App-generated update plan has an unexpected schema or version")
        if type(plan.get("parent_pid")) is not int or plan["parent_pid"] <= 0:
            raise AssertionError("App-generated update plan has an invalid parent process ID")
        files = plan.get("files")
        if not isinstance(files, list) or len(files) != len(expected):
            raise AssertionError("App-generated update plan has an unexpected file count")
        planned = {}
        for entry in files:
            if not isinstance(entry, dict) or set(entry) != {"path", "size", "sha256"}:
                raise AssertionError("App-generated update plan contains an invalid file entry")
            name = entry["path"]
            if not isinstance(name, str) or name in planned:
                raise AssertionError("App-generated update plan contains duplicate or invalid paths")
            planned[name] = {"size": entry["size"], "sha256": entry["sha256"]}
        if planned != expected:
            raise AssertionError("App-generated download manifest differs from the public candidate ZIP")
        if digest(stage / "updater.exe") != value["baseline_managed"][EXE]:
            raise AssertionError("Executed updater helper does not match the published baseline executable")
        if manifest(stage / "backup", MANAGED) != value["baseline_managed"]:
            raise AssertionError("Recovery backup does not exactly match the published baseline package")
        journal = read_json(stage / "journal.json", 64 * 1024)
        if (
            not isinstance(journal, list)
            or len(journal) != len(MANAGED)
            or {entry.get("path") for entry in journal if isinstance(entry, dict)} != set(MANAGED)
            or any(not isinstance(entry, dict) or entry.get("installed") is not True or entry.get("backed_up") is not True for entry in journal)
        ):
            raise AssertionError("App-generated update journal does not confirm all four replacements")
        # Production deletes the verified archive before exposing READY. Do not
        # claim that its transient compressed bytes were independently observed;
        # compare its persisted manifest and installed files with the public ZIP.
        if (stage / "download.zip").exists():
            raise AssertionError("Production updater unexpectedly retained its transient ZIP")
        evidence.update({
            "status": "passed",
            "public_candidate_archive": public_archive_digest,
            "profile_file_count": len(value["profile_before_update"]),
            "unrelated_file_count": len(value["unmanaged"]),
            "managed_file_count": len(expected),
            "profile_before_update": value["profile_before_update"],
            "profile_after_update": manifest(profile, list(value["profile_before_update"])),
            "installed_manifest": expected,
            "app_generated_plan_manifest": planned,
            "app_generated_plan_path": str(stage / "plan.json"),
            "app_generated_plan_parent_pid": plan["parent_pid"],
            "baseline_backup_path": str(stage / "backup"),
            "helper_matches_published_baseline": True,
            "backup_matches_published_baseline": True,
            "download_zip_removed_after_verification": True,
            "download_zip_hash_observed": False,
            "journal": journal,
        })
        print(f"Real GUI update verified: {len(expected)} application files, "
              f"{len(value['profile_before_update'])} personal files and {len(value['unmanaged'])} unrelated files; "
              "app-generated plan and recovery backup match the public releases")
    except Exception as error:
        evidence.update({"status": "failed", "error": str(error)})
        raise
    finally:
        save(root / "live-update-verification.json", evidence)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("prepare", "snapshot", "verify"):
        sub = subparsers.add_parser(command)
        sub.add_argument("--root", type=Path, required=True)
        if command == "prepare":
            sub.add_argument("--baseline-zip", type=Path, required=True)
        elif command == "verify":
            sub.add_argument("--candidate-zip", type=Path, required=True)
            sub.add_argument("--version", required=True)
    args = parser.parse_args()
    globals()[args.command](args)


if __name__ == "__main__":
    main()
