import hashlib
import sqlite3
import struct
import sys
import tempfile
import unittest
import urllib.error
import zipfile
from contextlib import closing
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from package_windows import package, version_from_manifest, windows_imports
from publish_release import publish


def pe_fixture(dll=b"KERNEL32.dll"):
    """Small PE64 import fixture; no native executable runs in these tests."""
    data = bytearray(1024)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, 0x8664, 1)
    struct.pack_into("<H", data, 0x94, 240)
    struct.pack_into("<H", data, 0x98, 0x20B)
    struct.pack_into("<II", data, 0x98 + 120, 0x1000, 40)
    struct.pack_into("<IIII", data, 0x98 + 240 + 8, 512, 0x1000, 512, 512)
    struct.pack_into("<IIIII", data, 512, 0x1080, 0, 0, 0x1040, 0x1080)
    data[576 : 576 + len(dll)] = dll
    return data


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "widget").mkdir()
        (self.root / "widget/Cargo.toml").write_text('[package]\nversion = "1.0.0"\n')
        (self.root / "data/client/Assets").mkdir(parents=True)
        (self.root / "data/client/Assets/example.agf").write_bytes(b"asset")
        (self.root / "data/client-version.txt").write_text("V3.4220\n")
        with closing(sqlite3.connect(self.root / "data/gersang.db")) as db:
            with db:
                for name in ("items", "monsters"):
                    db.execute(f"CREATE TABLE {name}(id)")
                    db.execute(f"INSERT INTO {name} VALUES (1)")
        self.binary = self.root / "gersang-widget.exe"
        self.binary.write_bytes(pe_fixture())

    def test_package_includes_catalog_assets_but_excludes_all_user_data(self):
        for name in ("favourites.txt", "aliases.txt", "ui-lang.txt", "window.txt", "view.txt", "client.txt", "mt-cache.sqlite", "game-notices.txt"):
            (self.root / "data" / name).write_text("private")
        archive = package(self.root, self.binary, self.root / "dist", "v1.0.0")
        self.assertEqual(archive.name, "gs-widget-v1.0.0-windows-x64.zip")
        with zipfile.ZipFile(archive) as bundle:
            self.assertEqual(set(bundle.namelist()), {
                "gersang-widget.exe", "data/gersang.db", "data/client-version.txt", "data/client/Assets/example.agf"
            })
            self.assertIsNone(bundle.testzip())

    def test_tag_mismatch_blocks_packaging(self):
        with self.assertRaisesRegex(ValueError, "must match"):
            package(self.root, self.binary, self.root / "dist", "v1.1.0")
        self.assertFalse((self.root / "dist").exists())

    def test_missing_or_empty_assets_block_packaging(self):
        (self.root / "data/client/Assets/example.agf").unlink()
        with self.assertRaisesRegex(ValueError, "empty"):
            package(self.root, self.binary, self.root / "dist")

    def test_missing_catalog_records_block_packaging(self):
        with closing(sqlite3.connect(self.root / "data/gersang.db")) as db:
            with db:
                db.execute("DELETE FROM monsters")
        with self.assertRaisesRegex(ValueError, "no monsters"):
            package(self.root, self.binary, self.root / "dist")

    def test_stable_version_required(self):
        (self.root / "widget/Cargo.toml").write_text('[package]\nversion = "1.0.0-beta.1"\n')
        with self.assertRaisesRegex(ValueError, "stable"):
            version_from_manifest(self.root)

    def test_dynamic_runtime_and_wrong_architecture_are_rejected(self):
        self.binary.write_bytes(pe_fixture(b"VCRUNTIME140.dll"))
        with self.assertRaisesRegex(ValueError, "Visual C"):
            windows_imports(self.binary)
        data = pe_fixture()
        struct.pack_into("<H", data, 0x84, 0x14C)
        self.binary.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "x86_64"):
            windows_imports(self.binary)


class FakeGitHub:
    def __init__(self, existing=None, digest=None, pending=0, release_pages=None):
        self.existing = existing
        self.digest = digest
        self.pending = pending
        self.release_pages = release_pages or [[]]
        self.calls = []
        self.archive = b""

    def request(self, method, url, payload=None, *, binary=False):
        self.calls.append((method, url, payload))
        if "/tags/" in url:
            if self.existing is not None:
                return self.existing
            raise urllib.error.HTTPError(url, 404, "Not found", {}, None)
        if method == "GET" and "/releases?per_page=100&page=" in url:
            page = int(url.rsplit("=", 1)[1])
            return self.release_pages[page - 1] if page <= len(self.release_pages) else []
        if method == "POST" and url.endswith("/releases"):
            return {"id": 1, "upload_url": "https://uploads.github.com/repos/owner/repo/releases/1/assets{?name,label}"}
        if binary:
            self.archive = payload
            return {"id": 2}
        if method == "GET" and "/assets/" in url:
            if self.pending:
                self.pending -= 1
                return {"state": "uploaded", "digest": None}
            digest = self.digest if self.digest is not None else "sha256:" + hashlib.sha256(self.archive).hexdigest()
            return {"state": "uploaded", "digest": digest, "size": len(self.archive)}
        if method == "PATCH":
            return {"draft": False, "body": "", "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0"}
        raise AssertionError((method, url))


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.archive = Path(self.temp.name) / "gs-widget-v1.0.0-windows-x64.zip"
        self.archive.write_bytes(b"verified archive content")

    def run_publish(self, github):
        return publish(github, "owner/repo", "v1.0.0", "a" * 40, self.archive, sleep=lambda _: None)

    def test_publish_is_last_and_release_notes_remain_empty(self):
        github = FakeGitHub(pending=2)
        self.run_publish(github)
        creation = next(payload for method, url, payload in github.calls if method == "POST" and url.endswith("/releases"))
        self.assertTrue(creation["draft"])
        self.assertEqual(creation["body"], "")
        self.assertFalse(creation["generate_release_notes"])
        method, _, publication = github.calls[-1]
        self.assertEqual(method, "PATCH")
        self.assertFalse(publication["draft"])
        self.assertEqual(publication["body"], "")

    def test_digest_mismatch_never_publishes(self):
        github = FakeGitHub(digest="sha256:" + "0" * 64)
        with self.assertRaisesRegex(ValueError, "digest or size"):
            self.run_publish(github)
        self.assertFalse(any(method == "PATCH" for method, _, _ in github.calls))

    def test_unavailable_digest_never_publishes(self):
        github = FakeGitHub(pending=100)
        with self.assertRaisesRegex(ValueError, "did not provide"):
            self.run_publish(github)
        self.assertFalse(any(method == "PATCH" for method, _, _ in github.calls))

    def test_existing_release_or_draft_is_never_overwritten(self):
        for draft in (False, True):
            with self.subTest(draft=draft):
                github = FakeGitHub(existing={"draft": draft})
                with self.assertRaisesRegex(ValueError, "Refusing to overwrite"):
                    self.run_publish(github)
                self.assertEqual(len(github.calls), 1)

    def test_draft_hidden_from_tag_endpoint_prevents_duplicate_publication(self):
        github = FakeGitHub(release_pages=[[{"tag_name": "v1.0.0", "draft": True}]])
        with self.assertRaisesRegex(ValueError, "existing draft"):
            self.run_publish(github)
        self.assertTrue(all(method == "GET" for method, _, _ in github.calls))
        self.assertEqual(len(github.calls), 2)

    def test_draft_search_checks_later_pages(self):
        first_page = [{"tag_name": f"v2.0.{index}", "draft": False} for index in range(100)]
        github = FakeGitHub(release_pages=[first_page, [{"tag_name": "v1.0.0", "draft": True}]])
        with self.assertRaisesRegex(ValueError, "existing draft"):
            self.run_publish(github)
        self.assertTrue(github.calls[-1][1].endswith("page=2"))
        self.assertTrue(all(method == "GET" for method, _, _ in github.calls))

    def test_tag_filename_mismatch_does_not_contact_github(self):
        github = FakeGitHub()
        with self.assertRaisesRegex(ValueError, "filename"):
            publish(github, "owner/repo", "v1.1.0", "a" * 40, self.archive)
        self.assertEqual(github.calls, [])


if __name__ == "__main__":
    unittest.main()
