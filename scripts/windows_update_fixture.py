#!/usr/bin/env python3
"""Disposable fixtures for smoke_windows_update.ps1; never edits real profiles."""

import argparse
import hashlib
import json
import shutil
import sqlite3
import stat
import zipfile
from contextlib import closing
from pathlib import Path, PurePosixPath

EXE = "gersang-widget.exe"
MANAGED = (EXE, "data/gersang.db", "data/client-version.txt", "data/client")
MARKER = "dummy-update-context.json"


def digest(path: Path) -> dict:
    with path.open("rb") as stream:
        sha256 = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"size": path.stat().st_size, "sha256": sha256}


def manifest(root: Path, paths: list[str] | tuple[str, ...]) -> dict:
    result = {}
    for name in paths:
        path = root / name
        files = sorted(path.rglob("*")) if path.is_dir() else [path]
        for file in files:
            if file.is_file():
                result[file.relative_to(root).as_posix()] = digest(file)
    return result


def save(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def extract(archive: Path, destination: Path) -> None:
    """Accept only the same application-owned areas as the production updater."""
    seen = set()
    with zipfile.ZipFile(archive) as bundle:
        for entry in bundle.infolist():
            name = entry.filename
            parts = PurePosixPath(name).parts
            if (
                "\\" in name or ":" in name or name.startswith("/")
                or any(part in (".", "..") for part in name.split("/"))
                or not parts or stat.S_ISLNK(entry.external_attr >> 16)
                or name.casefold() in seen
            ):
                raise ValueError(f"Unsafe fixture ZIP entry: {name}")
            seen.add(name.casefold())
            allowed = (
                name in MANAGED[:3]
                or name.startswith("data/client/")
                or (entry.is_dir() and name.rstrip("/") in ("data", "data/client"))
            )
            if not allowed:
                raise ValueError(f"Unexpected fixture ZIP entry: {name}")
        bundle.extractall(destination)
    for required in MANAGED:
        if not (destination / required).exists():
            raise ValueError(f"Fixture ZIP is missing {required}")


def prepare(args) -> None:
    root = args.root.resolve()
    # Refuse reuse so a typo cannot overwrite an installation or user profile.
    root.mkdir(parents=True, exist_ok=False)
    app = root / "app with spaces"
    candidate = root / "candidate package"
    profile = root / "isolated profile" / "gs-widget"
    profile.mkdir(parents=True)
    (root / "logs").mkdir()
    extract(args.baseline_zip.resolve(), app)
    extract(args.candidate_zip.resolve(), candidate)
    baseline_manifest = manifest(app, MANAGED)
    candidate_manifest = manifest(candidate, MANAGED)
    if baseline_manifest[EXE] == candidate_manifest[EXE]:
        raise ValueError("Dummy candidate must contain a genuinely different executable")
    with closing(sqlite3.connect((app / "data/gersang.db").as_uri() + "?mode=ro", uri=True)) as db:
        items = [str(row[0]) for row in db.execute("SELECT code FROM items ORDER BY code LIMIT 2")]
        monsters = [str(row[0]) for row in db.execute("SELECT code FROM monsters ORDER BY code LIMIT 2")]
    if len(items) != 2 or len(monsters) != 2:
        raise ValueError("Fixture catalogue needs at least two items and monsters")
    preferences = {
        "favourites.txt": "\n".join(monsters) + "\n",
        "aliases.txt": f"# Dummy upgrade: Unicode aliases must survive.\n升級測試 {items[0]}\nmy_dummy_alias {items[1]}\n",
        "ui-lang.txt": "en-zh",
        "view.txt": "pin=off\nbattle=0\ngallery=1\nanimate=0\nhotkeys=0\nlegacy=0\n",
        "dock.txt": "v=1\nshow=110111111\norder=8,7,6,5,4,3,2,1,0\nsize=84,580\n",
        "home.txt": "size=2\norder=8,7,6,5,4,3,2,1,0\n",
        "opacity.txt": "85",
        "set-extra.txt": "1",
        "window.txt": "size=560,740\npos=32,32\nmode=full\nfull-pos=32,32\nfull-size=960,640\n",
        "window-macos.txt": "preserve cross-platform personal settings\n",
        "client.txt": str(app / "data/client"),
    }
    unmanaged = []
    for name, body in preferences.items():
        (profile / name).write_text(body, encoding="utf-8")
        # Existing profile data must win over every legacy installation copy.
        relative = f"data/{name}"
        (app / relative).write_text(f"Legacy fallback must not win: {name}\n", encoding="utf-8")
        unmanaged.append(relative)
    (app / "personal notes.txt").write_text("Keep this unrelated root file.\n", encoding="utf-8")
    (app / "data/personal notes.txt").write_text("Keep this unrelated data file. 收藏\n", encoding="utf-8")
    unmanaged += ["personal notes.txt", "data/personal notes.txt"]
    save(root / MARKER, {
        "schema": 1,
        "candidate_version": args.candidate_version,
        "baseline_archive": digest(args.baseline_zip),
        "candidate_archive": digest(args.candidate_zip),
        "baseline_managed": baseline_manifest,
        "candidate_managed": candidate_manifest,
        "initial_profile": manifest(profile, list(preferences)),
        "unmanaged": manifest(app, unmanaged),
    })


def context(root: Path) -> dict:
    value = json.loads((root / MARKER).read_text(encoding="utf-8"))
    if value["schema"] != 1:
        raise ValueError("Unsupported dummy fixture context")
    return value


def assert_manifest(root: Path, expected: dict, label: str) -> None:
    for name, wanted in expected.items():
        path = root / name
        if not path.is_file() or digest(path) != wanted:
            raise AssertionError(f"{label} changed or disappeared: {name}")


def snapshot(args) -> None:
    value = context(args.root)
    profile = args.root / "isolated profile/gs-widget"
    # A normal close may normalize window geometry, but all other settings must
    # already match the seeds before we accept them as the upgrade baseline.
    unchanged = {k: v for k, v in value["initial_profile"].items() if k != "window.txt"}
    assert_manifest(profile, unchanged, "Initial personal data")
    window = (profile / "window.txt").read_text(encoding="utf-8")
    if "mode=full\n" not in window or "full-size=" not in window:
        raise AssertionError("Baseline GUI lost its saved full mode/window settings")
    value["profile_before_update"] = manifest(profile, list(value["initial_profile"]))
    save(args.root / MARKER, value)


def stage(args) -> None:
    value = context(args.root)
    app = args.root / "app with spaces"
    staging = app / f".gs-widget-update-smoke-{args.case}"
    staging.mkdir(exist_ok=False)
    shutil.copytree(args.root / "candidate package", staging / "payload")
    shutil.copy2(app / EXE, staging / "updater.exe")
    save(staging / "plan.json", {
        "schema": 1,
        "version": value["candidate_version"],
        "parent_pid": args.parent_pid,
        "files": [{"path": name, **entry} for name, entry in value["candidate_managed"].items()],
    })
    if args.case == "tampered":
        with (staging / "payload/data/client-version.txt").open("ab") as stream:
            stream.write(b"\nintentionally tampered after manifest creation\n")


def verify(args) -> None:
    value = context(args.root)
    app = args.root / "app with spaces"
    assert_manifest(args.root / "isolated profile/gs-widget", value["profile_before_update"], "Personal data")
    assert_manifest(app, value["unmanaged"], "Legacy/unrelated installation data")
    expected = value["candidate_managed" if args.phase == "updated" else "baseline_managed"]
    actual = manifest(app, MANAGED)
    if actual != expected:
        raise AssertionError(f"Installed application files do not match {args.phase} package")
    if args.phase == "rejected":
        staging = app / ".gs-widget-update-smoke-tampered"
        for forbidden in ("ready", "completed", "backup", "journal.json"):
            if (staging / forbidden).exists():
                raise AssertionError(f"Tampered update unexpectedly created {forbidden}")
        error = (staging / "error.txt").read_text(encoding="utf-8")
        if "Staged file changed size: data/client-version.txt" not in error:
            raise AssertionError(f"Unexpected tamper rejection: {error}")
    else:
        staging = app / ".gs-widget-update-smoke-success"
        if (staging / "completed").read_text(encoding="utf-8") != value["candidate_version"]:
            raise AssertionError("Helper did not confirm the expected version")
        if (staging / "error.txt").exists():
            raise AssertionError((staging / "error.txt").read_text(encoding="utf-8"))
        if manifest(staging / "backup", MANAGED) != value["baseline_managed"]:
            raise AssertionError("Recovery backup does not exactly match the original package")
        journal = json.loads((staging / "journal.json").read_text(encoding="utf-8"))
        if len(journal) != 4 or any(not entry["installed"] or not entry["backed_up"] for entry in journal):
            raise AssertionError(f"Incomplete update journal: {journal}")
    print(f"{args.phase}: {len(value['profile_before_update'])} personal files, "
          f"{len(value['unmanaged'])} unrelated files, and complete application manifest verified")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("prepare", "snapshot", "stage", "verify"):
        sub = subparsers.add_parser(command)
        sub.add_argument("--root", type=Path, required=True)
        if command == "prepare":
            sub.add_argument("--baseline-zip", type=Path, required=True)
            sub.add_argument("--candidate-zip", type=Path, required=True)
            sub.add_argument("--candidate-version", required=True)
        elif command == "stage":
            sub.add_argument("--parent-pid", type=int, required=True)
            sub.add_argument("--case", choices=("tampered", "success"), required=True)
        elif command == "verify":
            sub.add_argument("--phase", choices=("rejected", "updated"), required=True)
    args = parser.parse_args()
    globals()[args.command](args)


if __name__ == "__main__":
    main()
