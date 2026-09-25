"""Regression checks for source provenance, attribution and archive contents."""

import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_release
import prepare_release
import third_party_notices


class ReleaseSourceTests(unittest.TestCase):
    SHA = "a" * 40

    def test_draft_and_tag_must_both_match_the_build(self):
        prepare_release.verify_source({"target_commitish": self.SHA}, self.SHA, self.SHA)
        prepare_release.verify_source({"target_commitish": self.SHA}, None, self.SHA)
        for target, tag in [("b" * 40, self.SHA), (self.SHA, "b" * 40), ("main", self.SHA)]:
            with self.subTest(target=target, tag=tag), self.assertRaises(RuntimeError):
                prepare_release.verify_source({"target_commitish": target}, tag, self.SHA)

    def test_api_errors_are_not_treated_as_missing_releases(self):
        for status in (401, 403, 429, 500):
            failure = subprocess.CompletedProcess([], 1, "", f"gh: failed (HTTP {status})")
            with patch("prepare_release.subprocess.run", return_value=failure):
                with self.assertRaises(RuntimeError):
                    prepare_release.gh_json("owner/repo", "releases/tags/v1", allow_missing=True)
        missing = subprocess.CompletedProcess([], 1, "", "gh: Not Found (HTTP 404)")
        with patch("prepare_release.subprocess.run", return_value=missing):
            self.assertIsNone(prepare_release.gh_json("owner/repo", "releases/tags/v1", True))

    def test_annotated_tags_are_peeled_to_the_commit(self):
        replies = [{"object": {"type": "tag", "sha": "b" * 40}},
                   {"object": {"type": "commit", "sha": self.SHA}}]
        with patch("prepare_release.gh_json", side_effect=replies):
            self.assertEqual(prepare_release.tag_commit("owner/repo", "v1"), self.SHA)


class LicenseTests(unittest.TestCase):
    def test_inventory_excludes_build_and_dev_only_dependencies(self):
        def edge(name, kind):
            return {"pkg": name, "dep_kinds": [{"kind": kind}]}

        metadata = {
            "packages": [{"id": n, "name": n, "version": "1"} for n in ("root", "normal", "nested", "build", "dev")],
            "resolve": {"root": "root", "nodes": [
                {"id": "root", "deps": [edge("normal", None), edge("build", "build"), edge("dev", "dev")]},
                {"id": "normal", "deps": [edge("nested", None)]},
                {"id": "nested", "deps": []}, {"id": "build", "deps": []}, {"id": "dev", "deps": []},
            ]},
        }
        self.assertEqual([p["name"] for p in third_party_notices.normal_dependencies(metadata)], ["nested", "normal"])

    def test_missing_license_fails_instead_of_guessing_from_spdx(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = {"name": "example", "version": "1", "license": "MIT", "manifest_path": str(Path(temporary) / "Cargo.toml")}
            with self.assertRaisesRegex(RuntimeError, "No complete license texts"):
                third_party_notices.license_texts(package, {})
            (Path(temporary) / "LICENSE").write_text("Actual upstream license and attribution")
            self.assertEqual(len(third_party_notices.license_texts(package, {})), 1)

    def test_font_notices_cannot_be_silently_omitted(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = {"name": "epaint_default_fonts", "version": "1", "manifest_path": str(Path(temporary) / "Cargo.toml")}
            (Path(temporary) / "LICENSE").write_text("Code license")
            with self.assertRaisesRegex(RuntimeError, "Missing bundled font notice"):
                third_party_notices.license_texts(package, {})

    def test_source_modules_and_flattened_symlinks_are_not_license_texts(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            package = {"name": "example", "version": "1", "manifest_path": str(source / "Cargo.toml")}
            (source / "copying.rs").write_text("pub fn copy() {}")
            (source / "LICENSE").write_text("../LICENSE\n")
            with self.assertRaisesRegex(RuntimeError, "No complete license texts"):
                third_party_notices.license_texts(package, {})

    def test_changed_license_invalidates_an_override(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = {"name": "example", "version": "1", "license": "MPL-2.0", "manifest_path": str(Path(temporary) / "Cargo.toml")}
            with self.assertRaisesRegex(RuntimeError, "License expression changed"):
                third_party_notices.license_texts(package, {"example@1": {"license": "MIT"}})


class ArchiveTests(unittest.TestCase):
    def test_both_archive_formats_include_documents_provenance_and_valid_checksum(self):
        for target in ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                for name in package_release.DOCUMENTS:
                    (root / name).write_text(f"Contents of {name}")
                binary = root / ("scrannotate.exe" if "windows" in target else "scrannotate")
                binary.write_bytes(b"test executable")
                binary.chmod(0o755)
                with patch.object(package_release, "ROOT", root), patch("package_release.subprocess.check_output", return_value="rustc fixture"):
                    archive = package_release.package(binary, target, "a" * 40, root / "THIRD_PARTY_NOTICES.txt", root / "dist")
                if "windows" in target:
                    with zipfile.ZipFile(archive) as opened:
                        names = opened.namelist()
                        info = json.loads(opened.read("BUILD_INFO.json"))
                else:
                    with tarfile.open(archive) as opened:
                        names = opened.getnames()
                        info = json.load(opened.extractfile("BUILD_INFO.json"))
                        self.assertTrue(opened.getmember(binary.name).mode & 0o111)
                self.assertEqual(set(names), {binary.name, "BUILD_INFO.json", *package_release.DOCUMENTS})
                self.assertEqual(info["source_sha"], "a" * 40)
                self.assertEqual(info["target"], target)
                checksum = Path(str(archive) + ".sha256").read_text().split()[0]
                self.assertEqual(checksum, hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_missing_document_prevents_packaging(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "scrannotate"
            binary.write_bytes(b"test")
            with patch.object(package_release, "ROOT", root), self.assertRaisesRegex(RuntimeError, "Missing distribution document"):
                package_release.package(binary, "x86_64-unknown-linux-gnu", "a" * 40, root / "missing.txt", root / "dist")


if __name__ == "__main__":
    unittest.main()
