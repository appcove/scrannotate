#!/usr/bin/env python3
"""Collect dependency license texts for one target, without guessing licenses.

Uses Cargo's target-filtered normal dependency graph (including proc macros,
but excluding build-only and dev-only dependencies). The result is a
conservative attribution inventory, not a determination of license compliance.
Missing texts fail packaging. Reviewed version-specific upstream source files
in license_sources supply texts omitted from published crates.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


LICENSE_NAME = re.compile(
    r"(^|[-_])(licen[cs]e|copying|copyright|notice|ofl|ufl|unlicense)([-_.]|$)", re.I
)
SOURCE_ROOT = Path(__file__).resolve().parent / "license_sources"


def is_notice(path):
    # A module such as objc2-foundation/src/copying.rs is source code, not a
    # license notice. Some projects put the terms in licenses/MIT instead.
    if path.suffix.lower() in {".rs", ".c", ".h", ".cpp", ".stderr", ".json", ".png", ".svg", ".ttf", ".otf", ".pdf"}:
        return False
    return bool(LICENSE_NAME.search(path.name)) or path.parent.name.lower() in {"license", "licenses", "licence", "licences"}


def normal_dependencies(metadata):
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    if root is None:
        raise RuntimeError("Expected metadata for a single root package")
    visited = set()
    pending = [root]
    while pending:
        package_id = pending.pop()
        if package_id in visited:
            continue
        visited.add(package_id)
        pending.extend(
            dep["pkg"] for dep in nodes[package_id]["deps"]
            if any(kind["kind"] is None for kind in dep["dep_kinds"])
        )
    return sorted(
        (p for p in metadata["packages"] if p["id"] in visited and p["id"] != root),
        key=lambda p: (p["name"], p["version"]),
    )


def license_texts(package, overrides):
    source = Path(package["manifest_path"]).parent
    files = {
        path for path in source.rglob("*")
        if path.is_file() and is_notice(path)
    }
    if package.get("license_file"):
        declared = source / package["license_file"]
        if not declared.is_file():
            raise RuntimeError(f"Missing declared license file: {declared}")
        files.add(declared)
    # Hack's font attribution file is not named LICENSE; preserve every
    # bundled font text as well as the Rust code's upstream license files.
    if package["name"] == "epaint_default_fonts":
        for name in ("OFL.txt", "UFL.txt", "Hack-Regular.txt", "emoji-icon-font-mit-license.txt"):
            if not (source / "fonts" / name).is_file():
                raise RuntimeError(f"Missing bundled font notice: {source / 'fonts' / name}")
        files.update((source / "fonts").glob("*.txt"))
    texts = []
    for path in sorted(files):
        contents = path.read_text(encoding="utf-8")
        # Cargo archives sometimes preserve a symlink as its plain relative
        # target (e.g. harfrust's ../LICENSE). That is not a license text.
        if not re.fullmatch(r"\s*\.\.?/\S+\s*", contents):
            texts.append((os.path.relpath(path, source), contents))
    key = f"{package['name']}@{package['version']}"
    if key in overrides:
        override = overrides[key]
        if override["license"] != package.get("license"):
            raise RuntimeError(f"License expression changed for {key}; review its override")
        if not override["files"]:
            raise RuntimeError(f"No upstream license files in override for {key}")
        if override.get("notes"):
            texts.append(("License source note", override["notes"]))
        for item in override["files"]:
            contents = (SOURCE_ROOT / item["file"]).read_text(encoding="utf-8")
            if override.get("omit_spdx_copyright_template"):
                # The upstream release provides author attribution but no
                # copyright notice. Do not invent an owner/year or ship SPDX
                # placeholders as if they were an actual upstream notice.
                contents = contents.replace("Copyright (c) <year> <copyright holders>\n\n", "")
            texts.append((item["url"], contents))
    if not texts or not all(text.strip() for _, text in texts):
        raise RuntimeError(f"No complete license texts for {key}; add reviewed upstream sources")
    return texts


def generate(metadata, target, overrides):
    lines = [
        "scrannotate — third-party notices",
        f"Target: {target}",
        "",
        "This inventory includes normal dependencies resolved for this target,",
        "including procedural macro tooling. Build-only and dev-only dependencies",
        "are excluded. SPDX expressions are reported as declared by each package;",
        "the accompanying upstream texts retain their own terms and notices.",
        "",
    ]
    for package in normal_dependencies(metadata):
        lines.extend([
            "=" * 78,
            f"{package['name']} {package['version']}",
            f"Declared license: {package.get('license') or 'See license file'}",
            f"Repository: {package.get('repository') or 'Not declared'}",
            f"Authors from package manifest: {', '.join(package.get('authors', [])) or 'Not declared'}",
            "",
        ])
        for label, contents in license_texts(package, overrides):
            lines.extend([f"--- {label} ---", contents.rstrip(), ""])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, help="Previously target-filtered cargo metadata JSON")
    parser.add_argument("--no-default-features", action="store_true")
    parser.add_argument("--features", help="Comma-separated Cargo features, matching the packaged build")
    args = parser.parse_args()
    if args.metadata and (args.features or args.no_default_features):
        parser.error("Feature options must be applied when creating the --metadata file")
    if args.metadata:
        metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
    else:
        command = ["cargo", "metadata", "--locked", "--format-version", "1",
                   "--filter-platform", args.target]
        if args.no_default_features:
            command.append("--no-default-features")
        if args.features:
            command.extend(["--features", args.features])
        metadata = json.loads(subprocess.check_output(command, text=True))
    overrides = json.loads((SOURCE_ROOT / "manifest.json").read_text(encoding="utf-8"))
    result = generate(metadata, args.target, overrides)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(result, encoding="utf-8")
    print(f"Wrote {args.output} for {len(normal_dependencies(metadata))} dependencies")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
