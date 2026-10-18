#!/usr/bin/env python3
"""Build the public Windows ZIP from an explicit, user-data-free allowlist."""

import argparse
import re
import sqlite3
import struct
import sys
import tomllib
import zipfile
from pathlib import Path

TARGET = "x86_64-pc-windows-msvc"
EXE = "gersang-widget.exe"
VERSION_RE = re.compile(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\Z")


def version_from_manifest(root: Path, expected_tag: str = "") -> str:
    with (root / "widget/Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    if not VERSION_RE.fullmatch(version):
        raise ValueError("Windows releases require a stable major.minor.patch version")
    if expected_tag and expected_tag != f"v{version}":
        raise ValueError(f"Tag {expected_tag!r} must match Cargo package version v{version}")
    return version


def windows_imports(binary: Path) -> list[str]:
    """Read PE32+ imports; reject non-x64 binaries and dynamic MSVC runtimes."""
    data = binary.read_bytes()

    def unpack(fmt: str, offset: int):
        if offset < 0 or offset + struct.calcsize(fmt) > len(data):
            raise ValueError("Truncated Windows executable")
        return struct.unpack_from(fmt, data, offset)

    if data[:2] != b"MZ":
        raise ValueError("Expected a Windows executable (MZ header)")
    pe = unpack("<I", 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError("Missing Windows PE header")
    machine, section_count = unpack("<HH", pe + 4)
    optional_size = unpack("<H", pe + 20)[0]
    optional = pe + 24
    if machine != 0x8664 or unpack("<H", optional)[0] != 0x20B or optional_size < 128:
        raise ValueError("Release executable must target x86_64-pc-windows-msvc")
    sections = []
    for index in range(section_count):
        section = optional + optional_size + 40 * index
        virtual_size, rva, raw_size, raw = unpack("<IIII", section + 8)
        sections.append((rva, max(virtual_size, raw_size), raw, raw_size))

    def offset_of(rva: int) -> int:
        for start, size, raw, raw_size in sections:
            if start <= rva < start + size and rva - start < raw_size:
                return raw + rva - start
        raise ValueError("Invalid Windows executable import address")

    import_rva, import_size = unpack("<II", optional + 120)
    if not import_rva or import_size < 20:
        raise ValueError("Windows executable has no import table")
    imports = []
    descriptor = offset_of(import_rva)
    for index in range(import_size // 20):
        entry = unpack("<IIIII", descriptor + index * 20)
        if entry == (0, 0, 0, 0, 0):
            break
        name_start = offset_of(entry[3])
        name_end = data.find(b"\0", name_start, name_start + 256)
        if name_end == -1:
            raise ValueError("Invalid Windows executable DLL name")
        imports.append(data[name_start:name_end].decode("ascii"))
    else:
        raise ValueError("Unterminated Windows executable import table")
    dynamic_crt = [name for name in imports if name.lower().startswith(("vcruntime", "msvcp", "msvcr", "concrt"))]
    if dynamic_crt:
        raise ValueError(f"Release still requires the Visual C++ runtime: {', '.join(dynamic_crt)}")
    return imports


def package(root: Path, binary: Path, output: Path, expected_tag: str = "") -> Path:
    version = version_from_manifest(root, expected_tag)
    windows_imports(binary)
    source_files = [(binary, EXE)]
    for relative in ("data/gersang.db", "data/client-version.txt"):
        source_files.append((root / relative, relative))
    assets = root / "data/client"
    if not assets.is_dir() or assets.is_symlink():
        raise ValueError("Missing bundled data/client directory")
    for file in sorted(assets.rglob("*")):
        if file.is_symlink():
            raise ValueError(f"Do not ship symlinked assets: {file}")
        if file.is_file():
            source_files.append((file, file.relative_to(root).as_posix()))
    if len(source_files) == 3:
        raise ValueError("Bundled data/client assets are empty")
    for file, _ in source_files:
        if not file.is_file() or file.is_symlink() or file.stat().st_size == 0:
            raise ValueError(f"Missing, empty or symlinked release file: {file}")
    with sqlite3.connect((root / "data/gersang.db").resolve().as_uri() + "?mode=ro", uri=True) as db:
        if db.execute("PRAGMA quick_check").fetchone() != ("ok",):
            raise ValueError("Catalog failed SQLite integrity check")
        for table in ("items", "monsters"):
            if not db.execute(f"SELECT 1 FROM {table} LIMIT 1").fetchone():
                raise ValueError(f"Catalog has no {table}")

    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"gs-widget-v{version}-windows-x64.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for file, name in source_files:
            bundle.write(file, name)
    with zipfile.ZipFile(archive) as bundle:
        if bundle.testzip() is not None:
            raise ValueError("Release ZIP failed its CRC check")
        if set(bundle.namelist()) != {name for _, name in source_files}:
            raise ValueError("Release ZIP does not match the approved file list")
    return archive


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--expected-tag", default="")
    parser.add_argument("--version-only", action="store_true")
    args = parser.parse_args()
    if args.version_only:
        print(version_from_manifest(args.root, args.expected_tag))
        return
    binary = args.binary or args.root / f"widget/target/{TARGET}/release/{EXE}"
    print(package(args.root, binary, args.output or args.root / "dist", args.expected_tag))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, sqlite3.Error, KeyError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        sys.exit(1)
