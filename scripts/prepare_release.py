#!/usr/bin/env python3
"""Create/resume or publish a release only for its original immutable commit."""

import argparse
import json
import os
import re
import subprocess
import sys


def gh_json(repository, endpoint, allow_missing=False):
    result = subprocess.run(
        ["gh", "api", f"repos/{repository}/{endpoint}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        if allow_missing and "(HTTP 404)" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip() or "GitHub API request failed")
    return json.loads(result.stdout)


def verify_source(release, tag_sha, source_sha):
    # A branch name can move; resolving it today cannot prove the source that
    # was originally selected when the draft was created. Require a full SHA.
    target = release["target_commitish"]
    if not re.fullmatch(r"[0-9a-f]{40}", target) or target != source_sha:
        raise RuntimeError(
            f"Draft targets {target}, but this run targets {source_sha}. "
            "Rerun the original workflow commit or use a new package version."
        )
    if tag_sha is not None and tag_sha != source_sha:
        raise RuntimeError(f"Release tag points to {tag_sha}, expected {source_sha}")


def tag_commit(repository, tag):
    ref = gh_json(repository, f"git/ref/tags/{tag}", allow_missing=True)
    if ref is None:
        return None
    obj = ref["object"]
    for _ in range(10):
        if obj["type"] == "commit":
            return obj["sha"]
        if obj["type"] != "tag":
            break
        obj = gh_json(repository, f"git/tags/{obj['sha']}")["object"]
    raise RuntimeError(f"Cannot resolve {tag} to a commit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    if not args.repository or not re.fullmatch(r"[0-9a-f]{40}", args.source_sha):
        raise RuntimeError("A repository and full source commit SHA are required")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != args.source_sha:
        raise RuntimeError(f"Checked out {head}, expected {args.source_sha}")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        text=True,
    ))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "scrannotate")
    tag = f"v{version}"
    release = gh_json(args.repository, f"releases/tags/{tag}", allow_missing=True)
    if release is not None and not release["draft"]:
        if args.publish:
            raise RuntimeError(f"Release {tag} is already published")
        build = False
        print(f"Release {tag} is already published; nothing to do")
    else:
        tag_sha = tag_commit(args.repository, tag)
        if release is None:
            if args.publish:
                raise RuntimeError(f"Draft release {tag} does not exist")
            verify_source({"target_commitish": args.source_sha}, tag_sha, args.source_sha)
            subprocess.run([
                "gh", "release", "create", tag, "--repo", args.repository,
                "--draft", "--generate-notes", "--target", args.source_sha,
                "--title", f"scrannotate {version}",
            ], check=True)
        else:
            verify_source(release, tag_sha, args.source_sha)
        if args.publish:
            subprocess.run([
                "gh", "release", "edit", tag, "--repo", args.repository, "--draft=false",
            ], check=True)
        build = True
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(
                f"build={str(build).lower()}\ntag={tag}\nversion={version}\n"
                f"source_sha={args.source_sha}\n"
            )


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
