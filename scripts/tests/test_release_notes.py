import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from release_notes import load_notes, render_english, validate_notes


def notes_fixture():
    return {"schema_version": 1, "releases": [
        {"version": "1.0.10", "date": "2026-10-18", "sections": [
            {"kind": "fixed", "en": ["Keep saved settings."], "zh-TW": ["保留已儲存的設定。"]},
        ]},
        {"version": "1.0.9", "date": "2026-10-17", "sections": [
            {"kind": "added", "en": ["Portable Windows downloads."], "zh-TW": ["Windows 免安裝下載。"]},
        ]},
    ]}


class ReleaseNotesTests(unittest.TestCase):
    def test_valid_numeric_order_and_only_requested_english_version_rendered(self):
        notes = validate_notes(notes_fixture())
        self.assertEqual(render_english(notes, "1.0.9"), "## Added\n\n- Portable Windows downloads.\n")
        self.assertEqual(render_english(notes, "1.0.10"), "## Fixes\n\n- Keep saved settings.\n")

    def test_missing_version_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Missing bilingual changelog entry"):
            render_english(validate_notes(notes_fixture()), "1.0.11")

    def test_schema_is_explicit_and_not_boolean(self):
        for schema in (None, True, 1.0, "1", 2):
            with self.subTest(schema=schema):
                notes = notes_fixture()
                notes["schema_version"] = schema
                with self.assertRaisesRegex(ValueError, "schema_version"):
                    validate_notes(notes)

    def test_invalid_stable_versions_are_rejected(self):
        for version in ("v1.0.10", "01.0.10", "1.0.10-beta.1", "1.0", "1.0.10\n", "１.0.10", f"{2**64}.0.0", None):
            with self.subTest(version=version):
                notes = notes_fixture()
                notes["releases"][0]["version"] = version
                with self.assertRaisesRegex(ValueError, "version"):
                    validate_notes(notes)

    def test_duplicate_or_ascending_versions_are_rejected(self):
        for version in ("1.0.10", "1.0.11"):
            with self.subTest(version=version):
                notes = notes_fixture()
                notes["releases"][1]["version"] = version
                with self.assertRaisesRegex(ValueError, "descending"):
                    validate_notes(notes)

    def test_dates_are_real_iso_calendar_dates(self):
        for date in ("2026-02-29", "2026-13-01", "2026-1-01", "20261018", "0000-01-01", None):
            with self.subTest(date=date):
                notes = notes_fixture()
                notes["releases"][0]["date"] = date
                with self.assertRaisesRegex(ValueError, "date"):
                    validate_notes(notes)

    def test_translations_are_required_and_have_matching_counts(self):
        for language in ("en", "zh-TW"):
            for value in ([], [""], [" padded "], ["two\nlines"], ["tab\tinside"], [False], "text", None, ["one", "two"]):
                with self.subTest(language=language, value=value):
                    notes = notes_fixture()
                    notes["releases"][0]["sections"][0][language] = value
                    with self.assertRaises(ValueError):
                        validate_notes(notes)
            notes = notes_fixture()
            del notes["releases"][0]["sections"][0][language]
            with self.assertRaisesRegex(ValueError, language):
                validate_notes(notes)

    def test_sections_have_known_unique_kinds(self):
        for kind in ("removed", "Added", None, {}):
            with self.subTest(kind=kind):
                notes = notes_fixture()
                notes["releases"][0]["sections"][0]["kind"] = kind
                with self.assertRaisesRegex(ValueError, "section kind"):
                    validate_notes(notes)
        notes = notes_fixture()
        notes["releases"][0]["sections"] *= 2
        with self.assertRaisesRegex(ValueError, "section kind"):
            validate_notes(notes)

    def test_empty_release_or_section_lists_are_rejected(self):
        notes = notes_fixture()
        notes["releases"][0]["sections"] = []
        with self.assertRaisesRegex(ValueError, "nonempty sections"):
            validate_notes(notes)
        with self.assertRaisesRegex(ValueError, "nonempty list"):
            validate_notes({"schema_version": 1, "releases": []})

    def test_unknown_fields_fail_instead_of_hiding_a_translation_typo(self):
        for location in ((), ("releases", 0), ("releases", 0, "sections", 0)):
            notes = notes_fixture()
            target = notes
            for key in location:
                target = target[key]
            target["unexpected"] = "value"
            with self.assertRaisesRegex(ValueError, "exactly"):
                validate_notes(notes)

    def test_file_load_rejects_duplicate_keys(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "release-notes.json"
            path.write_text('{"schema_version": 1, "schema_version": 1, "releases": []}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Duplicate changelog key"):
                load_notes(path)

    def test_file_load_keeps_unicode_translation_and_does_not_mutate(self):
        notes = notes_fixture()
        original = copy.deepcopy(notes)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "release-notes.json"
            path.write_text(json.dumps(notes, ensure_ascii=False), encoding="utf-8")
            self.assertEqual(load_notes(path), notes)
        self.assertEqual(validate_notes(notes), original)


if __name__ == "__main__":
    unittest.main()
