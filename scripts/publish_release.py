#!/usr/bin/env python3
"""Publish an already-verified ZIP only after GitHub reports the matching digest."""

import argparse
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from release_notes import DEFAULT_NOTES, load_notes, render_english

API = "https://api.github.com"


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        # Do not forward the publishing token to an unexpected redirect target.
        return None


class GitHub:
    def __init__(self, token: str):
        self.token = token
        self.opener = urllib.request.build_opener(NoRedirects)

    def request(self, method: str, url: str, payload=None, *, binary=False):
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != "https" or parsed.hostname not in ("api.github.com", "uploads.github.com"):
            raise ValueError("Unexpected GitHub API URL")
        headers = {
            "Authorization": f"Bearer {self.token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "gs-widget-release",
        }
        body = None
        if payload is not None:
            body = payload if binary else json.dumps(payload).encode()
            headers["Content-Type"] = "application/zip" if binary else "application/json"
        request = urllib.request.Request(url, data=body, headers=headers, method=method)
        with self.opener.open(request, timeout=120) as response:
            return json.load(response)


def publish(github: GitHub, repository: str, tag: str, commit: str, archive: Path, *, notes_file=DEFAULT_NOTES, sleep=time.sleep):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Invalid owner/repository")
    if not re.fullmatch(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", tag):
        raise ValueError("Expected a stable vMAJOR.MINOR.PATCH tag")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", commit):
        raise ValueError("Expected the checked-out commit SHA")
    if archive.name != f"gs-widget-{tag}-windows-x64.zip":
        raise ValueError("ZIP filename must match the release tag")
    if not archive.is_file() or not 0 < archive.stat().st_size < 2 * 1024**3:
        raise ValueError("Missing or invalid release ZIP")
    # Validate every translation and the selected version before any API call.
    # Both the app and GitHub use this file; no generated commit messages leak
    # into the user-facing notes, and a missing translation blocks publication.
    body = render_english(load_notes(notes_file), tag[1:])

    base = f"{API}/repos/{repository}/releases"
    try:
        existing = github.request("GET", f"{base}/tags/{tag}")
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
    else:
        state = "draft" if existing.get("draft") else "published release"
        raise ValueError(f"Refusing to overwrite existing {state} {tag}. Inspect it before rerunning.")

    # The release-by-tag endpoint does not reliably return drafts. Inspect the
    # authenticated list as well so an interrupted upload cannot create another
    # draft for the same tag when the workflow is rerun.
    page = 1
    while True:
        releases = github.request("GET", f"{base}?per_page=100&page={page}")
        if not isinstance(releases, list):
            raise ValueError("Unexpected GitHub release list response")
        for existing in releases:
            if existing.get("tag_name") == tag:
                state = "draft" if existing.get("draft") else "published release"
                raise ValueError(f"Refusing to overwrite existing {state} {tag}. Inspect it before rerunning.")
        if len(releases) < 100:
            break
        page += 1

    release = github.request("POST", base, {
        "tag_name": tag,
        "target_commitish": commit,
        "name": tag,
        "body": body,
        "draft": True,
        "prerelease": False,
        "generate_release_notes": False,
    })
    # Any later failure intentionally leaves a draft; users never see a partial update.
    if release.get("draft") is not True or release.get("body") != body:
        raise ValueError("Draft release returned unexpected notes or state; draft kept unpublished")
    release_url = f"{base}/{release['id']}"
    upload_url = release["upload_url"].split("{", 1)[0]
    archive_bytes = archive.read_bytes()
    expected_digest = "sha256:" + hashlib.sha256(archive_bytes).hexdigest()
    uploaded = github.request("POST", upload_url + "?" + urllib.parse.urlencode({"name": archive.name}), archive_bytes, binary=True)
    asset_url = f"{base}/assets/{uploaded['id']}"
    for attempt in range(12):
        asset = github.request("GET", asset_url)
        if asset.get("state") == "uploaded" and asset.get("digest"):
            if asset["digest"] != expected_digest or asset.get("size") != len(archive_bytes):
                raise ValueError("GitHub asset digest or size does not match the local ZIP; draft kept unpublished")
            break
        if attempt < 11:
            sleep(5)
    else:
        raise ValueError("GitHub did not provide a SHA-256 asset digest; draft kept unpublished")

    result = github.request("PATCH", release_url, {
        "name": tag,
        "body": body,
        "draft": False,
        "prerelease": False,
        "make_latest": "legacy",
    })
    if result.get("draft") is not False or result.get("body") != body:
        raise ValueError("Published release returned unexpected metadata; inspect it in GitHub")
    return result["html_url"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--notes-file", type=Path, default=DEFAULT_NOTES)
    args = parser.parse_args()
    token = os.environ.get("GH_TOKEN")
    if not token:
        raise ValueError("GH_TOKEN must be provided by GitHub Actions")
    print(publish(GitHub(token), args.repository, args.tag, args.commit, args.archive, notes_file=args.notes_file))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        sys.exit(1)
