import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_release_ref import check_release_ref


class ReleaseRefTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "--quiet")
        self.tree = self.git("hash-object", "-t", "tree", "-w", "--stdin", stdin="")
        self.base = self.commit("base")
        self.feature = self.commit("feature", self.base)
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        self.git("update-ref", "HEAD", self.feature)
        self.git("tag", "v1.0.0", self.feature)

    def git(self, *arguments, stdin=None):
        return subprocess.run(
            ["git", "-c", "user.name=Release test", "-c", "user.email=release-test@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", *arguments],
            cwd=self.root, input=stdin, text=True, check=True, capture_output=True,
        ).stdout.strip()

    def commit(self, message, *parents):
        arguments = ["commit-tree", self.tree, "-m", message]
        for parent in parents:
            arguments.extend(["-p", parent])
        return self.git(*arguments)

    def test_unmerged_feature_tag_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "not on origin/main"):
            check_release_ref(self.root, "v1.0.0")

    def test_merged_ancestor_and_annotated_tag_are_accepted(self):
        other_main = self.commit("other main work", self.base)
        merged = self.commit("merge feature", other_main, self.feature)
        self.git("update-ref", "refs/remotes/origin/main", merged)
        self.git("tag", "--delete", "v1.0.0")
        self.git("tag", "--annotate", "v1.0.0", "--message", "release", self.feature)
        self.assertEqual(check_release_ref(self.root, "v1.0.0", self.feature), self.feature)

    def test_tag_at_main_tip_is_accepted(self):
        self.git("update-ref", "refs/remotes/origin/main", self.feature)
        self.assertEqual(check_release_ref(self.root, "v1.0.0"), self.feature)

    def test_checkout_and_built_artifact_must_match_tag(self):
        self.git("update-ref", "refs/remotes/origin/main", self.feature)
        with self.assertRaisesRegex(ValueError, "produced the ZIP"):
            check_release_ref(self.root, "v1.0.0", self.base)
        self.git("update-ref", "HEAD", self.base)
        with self.assertRaisesRegex(ValueError, "checked-out commit"):
            check_release_ref(self.root, "v1.0.0")


if __name__ == "__main__":
    unittest.main()
