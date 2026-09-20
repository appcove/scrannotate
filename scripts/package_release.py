#!/usr/bin/env python3
"""Package an already-built executable with notices and source provenance."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent
DOCUMENTS = ("LICENSE", "NOTICE", "PRIVACY.md", "THIRD_PARTY_NOTICES.txt")


def package(binary, target, source_sha, notices, output):
    if not binary.is_file():
        raise RuntimeError(f"Missing built executable: {binary}")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"scrannotate-{target}{'.zip' if 'windows' in target else '.tar.gz'}"
    with tempfile.TemporaryDirectory() as temporary:
        staging = Path(temporary)
        shutil.copy2(binary, staging / binary.name)
        for name in DOCUMENTS:
            source = notices if name == "THIRD_PARTY_NOTICES.txt" else ROOT / name
            if not source.is_file() or source.stat().st_size == 0:
                raise RuntimeError(f"Missing distribution document: {source}")
            shutil.copy2(source, staging / name)
        info = {
            "source_sha": source_sha,
            "target": target,
            "rustc": subprocess.check_output(["rustc", "--version", "--verbose"], text=True).strip(),
        }
        (staging / "BUILD_INFO.json").write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
        if "windows" in target:
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zipped:
                for path in sorted(staging.iterdir()):
                    zipped.write(path, path.name)
        else:
            with tarfile.open(archive, "w:gz") as tar:
                for path in sorted(staging.iterdir()):
                    tar.add(path, arcname=path.name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    Path(str(archive) + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(f"Packaged {archive}")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--notices", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=Path("dist"))
    args = parser.parse_args()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != args.source_sha:
        raise RuntimeError(f"Checked out {head}, expected {args.source_sha}")
    package(args.binary, args.target, args.source_sha, args.notices, args.output)


if __name__ == "__main__":
    main()
