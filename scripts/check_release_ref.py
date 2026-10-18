#!/usr/bin/env python3
"""Require a release tag to point at the checkout and already belong to origin/main."""

import argparse
import re
import subprocess
import sys
from pathlib import Path


def git(root: Path, *arguments: str) -> str:
    result = subprocess.run(["git", *arguments], cwd=root, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(result.stderr.strip() or f"Git command failed: {' '.join(arguments)}")
    return result.stdout.strip()


def check_release_ref(root: Path, tag: str, expected_commit: str = "") -> str:
    if not re.fullmatch(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", tag):
        raise ValueError("Expected a stable vMAJOR.MINOR.PATCH release tag")
    # Peel annotated tags before comparing commits. Branch names alone do not
    # establish whether a tagged commit is part of the main branch's history.
    tagged_commit = git(root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    head = git(root, "rev-parse", "--verify", "HEAD^{commit}")
    main = git(root, "rev-parse", "--verify", "refs/remotes/origin/main^{commit}")
    if tagged_commit != head:
        raise ValueError("The checked-out commit does not match the requested release tag")
    if expected_commit and tagged_commit != expected_commit:
        raise ValueError("The release tag no longer matches the commit that produced the ZIP")
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", tagged_commit, main],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if result.returncode == 1:
        raise ValueError(f"{tag} is not on origin/main. Merge and push the release commit to main before tagging it.")
    if result.returncode:
        raise ValueError(result.stderr.strip() or "Cannot verify origin/main ancestry; fetch complete history")
    return tagged_commit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--expected-commit", default="")
    args = parser.parse_args()
    commit = check_release_ref(Path.cwd(), args.tag, args.expected_commit)
    print(f"Release tag {args.tag} points to {commit}, which is already on origin/main")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"Release eligibility check failed: {error}", file=sys.stderr)
        sys.exit(1)
