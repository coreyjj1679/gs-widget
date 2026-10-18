#!/usr/bin/env python3
"""Validate the shared bilingual changelog and render English GitHub notes."""

import argparse
import datetime
import json
import re
import sys
from pathlib import Path

DEFAULT_NOTES = Path(__file__).resolve().parents[1] / "release-notes.json"
VERSION_RE = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
SECTION_TITLES = {"added": "Added", "improved": "Improvements", "fixed": "Fixes"}


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate changelog key: {key}")
        result[key] = value
    return result


def _keys(value, expected, context):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError(f"{context} must contain exactly: {', '.join(expected)}")


def _version(value):
    if not isinstance(value, str) or not VERSION_RE.fullmatch(value):
        raise ValueError("Changelog versions must be stable MAJOR.MINOR.PATCH strings")
    numbers = tuple(int(part) for part in value.split("."))
    if any(part > 2**64 - 1 for part in numbers):
        raise ValueError("Changelog version components must fit in unsigned 64-bit integers")
    return numbers


def validate_notes(document):
    """Return the parsed document after validating every version and translation."""
    _keys(document, ("schema_version", "releases"), "Changelog")
    if type(document["schema_version"]) is not int or document["schema_version"] != 1:
        raise ValueError("Unsupported changelog schema_version; expected 1")
    releases = document["releases"]
    if not isinstance(releases, list) or not releases:
        raise ValueError("Changelog releases must be a nonempty list")
    previous_version = None
    for release in releases:
        _keys(release, ("version", "date", "sections"), "Changelog release")
        version = _version(release["version"])
        if previous_version is not None and version >= previous_version:
            raise ValueError("Changelog versions must be unique and in descending order")
        previous_version = version
        date = release["date"]
        if not isinstance(date, str) or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", date):
            raise ValueError("Changelog dates must use YYYY-MM-DD")
        try:
            datetime.date.fromisoformat(date)
        except ValueError as error:
            raise ValueError(f"Invalid changelog calendar date: {date}") from error
        sections = release["sections"]
        if not isinstance(sections, list) or not sections:
            raise ValueError("Each changelog release must have nonempty sections")
        seen_kinds = set()
        for section in sections:
            _keys(section, ("kind", "en", "zh-TW"), "Changelog section")
            kind = section["kind"]
            if not isinstance(kind, str) or kind not in SECTION_TITLES or kind in seen_kinds:
                raise ValueError("Changelog section kind must be unique and one of added, improved, fixed")
            seen_kinds.add(kind)
            for language in ("en", "zh-TW"):
                entries = section[language]
                if not isinstance(entries, list) or not entries:
                    raise ValueError(f"Changelog {language} translation must be a nonempty list")
                for entry in entries:
                    if (not isinstance(entry, str) or not entry or entry != entry.strip()
                            or any(ord(character) < 32 or ord(character) == 127 for character in entry)):
                        raise ValueError(f"Changelog {language} entries must be trimmed, nonempty single-line strings")
            if len(section["en"]) != len(section["zh-TW"]):
                raise ValueError("Changelog English and Traditional Chinese translation counts must match")
    return document


def load_notes(path=DEFAULT_NOTES):
    return validate_notes(json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=_unique_object))


def notes_for_version(document, version):
    _version(version)
    for release in document["releases"]:
        if release["version"] == version:
            return release
    raise ValueError(f"Missing bilingual changelog entry for version {version}")


def render_english(document, version):
    release = notes_for_version(document, version)
    return "\n\n".join(
        f"## {SECTION_TITLES[section['kind']]}\n\n" + "\n".join(f"- {entry}" for entry in section["en"])
        for section in release["sections"]
    ) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--notes-file", type=Path, default=DEFAULT_NOTES)
    parser.add_argument("--version", required=True, help="Cargo package version, without the v prefix")
    parser.add_argument("--render", action="store_true", help="Print the English GitHub Release body")
    args = parser.parse_args()
    document = load_notes(args.notes_file)
    body = render_english(document, args.version)
    if args.render:
        print(body, end="")
    else:
        print(f"Bilingual release notes validated for v{args.version}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"Release notes failed: {error}", file=sys.stderr)
        sys.exit(1)
