# Release builds and dependency notices

GitHub releases use the package version in `Cargo.toml`. A qualifying push to
`main` or a manual dispatch starts `.github/workflows/release.yml`. This workflow
publishes the existing portable downloads; store submission is a separate step.

## One commit per release

The preparation job checks out the triggering commit by its full SHA and reads
its package version. An already-published version is a no-op. Otherwise, it
creates a draft whose `target_commitish` is that immutable SHA.

An existing draft can be resumed only if its original full SHA and any existing
tag agree with the triggering commit. Annotated tags are resolved to their
underlying commit. A branch name is rejected as an existing draft target because
its current position cannot establish the draft's original source. API errors
other than a confirmed 404 stop preparation.

To recover a failed release, rerun the original Actions run. If a source fix is
needed, bump the package version and create a new release. Do not resume an old
version from a newer commit or move its tag to make validation pass.

The release calls the reusable CI workflow with the source SHA. Tests and Clippy
must pass on Linux, Windows, and macOS, for both x86-64 and ARM64, before any
release assets are built or uploaded. Linux also validates the build with capture
disabled and the release-script regression tests. Every target generates its
dependency notices as part of validation.
macOS also runs Clippy, tests, and notice generation with `mac-app-store` enabled.

Packaging checks out the same SHA, builds with `--locked`, and archives the
already-built executable. It does not delegate a second build to an upload
action. Publication depends on all validation and upload jobs, then rechecks the
draft and tag source immediately before publishing. No workflow change here
publishes or modifies a release until that workflow is actually run on GitHub.

## Pinned tools and recorded inputs

`rust-toolchain.toml` pins Rust 1.96.1 with Clippy and rustfmt. Checkout, cache, and
artifact-upload actions are pinned to commit SHAs obtained from their upstream
release tags. Runner image families are explicit; hosted runner images and native
SDK patch versions can still change. The macOS deployment target is 12.3 and
requires the checked-in capture-backend availability fix.

Portable archives contain `BUILD_INFO.json` with the source SHA, target triple,
and verbose compiler version. They also have SHA-256 sidecars. This records the
source and compiler; it does not claim bit-for-bit reproduction across changes
to hosted runner images. Full native signing, SDK pinning, symbol retention, and
store installation validation remain separate release work.

Formatting can be checked with `cargo fmt --all -- --check`. It is not currently
a release gate because the repository's pre-existing formatting baseline needs
to be normalized first.

## Distribution documents

Every portable ZIP/tar archive includes the executable and these documents:

- `LICENSE`: the application's Apache-2.0 license.
- `NOTICE`: the application's attribution notice.
- `PRIVACY.md`: the offline privacy policy.
- `THIRD_PARTY_NOTICES.txt`: dependency attribution for that target.

The macOS `.app` includes the same documents in `Contents/Resources` before
signing. `packaging/macos/bundle.sh` requires the generated notices as its fourth
argument and checks that every document exists and is nonempty. The About view
can therefore find documents after installation without network access.

Generate notices using the same target and features as the packaged executable:

```sh
python3 scripts/third_party_notices.py \
  --target aarch64-apple-darwin \
  --output target/THIRD_PARTY_NOTICES.txt

# Include this option when building with the Mac App Store feature:
python3 scripts/third_party_notices.py \
  --target aarch64-apple-darwin --features mac-app-store \
  --output target/store-THIRD_PARTY_NOTICES.txt
```

`--no-default-features` is also supported. For an existing metadata export, pass
`--metadata path/to/metadata.json`; that file must have been produced with
`cargo metadata --locked --format-version 1 --filter-platform TARGET` and the
same features as the build. The generator cannot reconstruct target filtering
from an unfiltered metadata file.

The inventory follows normal dependency edges in Cargo's target-filtered graph.
It includes procedural macro tooling conservatively and excludes dependencies
reachable only through build or development edges. It reports the exact SPDX
expressions declared by packages and retains their source license/notice files;
it does not select a license by a substring match. Bundled font notices include
the Ubuntu, Noto, Hack, and emoji-icon texts. Missing font notices fail generation.

Some published crates omit their license files. Version-specific entries in
`scripts/license_sources/manifest.json` supply checked-in copies from immutable
upstream revisions, including original attribution. The file URLs record where
each text came from. Generation is offline after Cargo has fetched dependencies.

Documented exceptions use canonical SPDX texts because the published crate
and its source revision supply a license declaration but omit the text:
`hexf-parse 0.2.1` declares CC0-1.0, and `dispatch 0.2.0` declares MIT. Their entries
record this explicitly. For dispatch, the generator retains author attribution
from the package manifest and removes the canonical SPDX copyright placeholder;
it does not invent a copyright owner or year. Recent objc2 releases similarly
link to canonical terms from their upstream license document; those terms are
included alongside the original document, as are the terms linked by siphasher's
copyright notice. These exceptions, and obligations
such as source availability for any MPL-licensed code actually shipped, still
need distribution review. A generated inventory is not a license-compliance
certification.

When updating dependencies, regenerate notices for all supported targets. A
missing license fails CI and packaging. Obtain the actual text from that crate's
published source or immutable upstream revision, add an exact `name@version`
entry with its declared license and provenance, and review the attribution.
Do not copy an unrelated project's copyright notice or silently substitute a
generic license. Changed SPDX expressions invalidate existing overrides.

## Local validation

These checks need no release credentials and never call publication APIs:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py' -v
shellcheck packaging/macos/bundle.sh
python3 -m py_compile scripts/*.py
cargo clippy --all-targets --no-default-features --locked -- -D warnings
cargo test --no-default-features --locked
```

The Python regressions check mismatched draft/tag commits, annotated tags, API
failure handling, missing notices, dependency-edge filtering, archive contents,
source metadata, executable permissions, and checksum correctness. To validate
the workflows themselves, run `actionlint` from the repository root. Full
capture-enabled CI requires the native platform dependencies; Linux needs
PipeWire 1.x development headers, Clang, and pkg-config. macOS bundle signing and
store-package installation must be tested on macOS and Windows respectively.
