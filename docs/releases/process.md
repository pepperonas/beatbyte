# Release Process

BeatByte follows [Semantic Versioning](https://semver.org/), with one
rule specific to this project:

**The patch number rises with every user-visible change**, in the same
commit that makes it, so the version a build reports identifies that
build rather than the last release. A `vX.Y.Z` **tag** is a separate
act — it is what triggers this pipeline — and happens at milestones,
covering however many patch versions have accumulated since the last
one.

That means a release is rarely a version *bump*; it is a version being
*published*. The manifest is usually already correct, and the work
below is about proving the thing you are about to publish actually
runs.

`apps/beatbyte/tests/docs_stay_true.rs` enforces the parts of this a
person would otherwise have to remember: the manifest version must
have a CHANGELOG section and be the newest one, and the internal
dependency pins must move with it.

## Checklist

1. Ensure `main` is green (CI passing).
2. Update `workspace.package.version` in the root `Cargo.toml` **and** the
   internal `beatbyte-*` path-dependency versions in
   `[workspace.dependencies]`.
3. Confirm the newest `CHANGELOG.md` section carries the version being
   released and a date. Entries were written as the work happened, so
   this is a review rather than a transcription — read it as a
   stranger would and cut anything that only makes sense from inside
   the commit that produced it.
4. Run the full quality gate locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace
   ```
5. Commit as `chore: release vX.Y.Z`.
6. Tag: `git tag vX.Y.Z && git push origin main vX.Y.Z`.
7. The `release.yml` workflow builds macOS (arm64 + x86_64), Windows and
   Linux binaries and attaches them to a **draft** GitHub release.
8. Verify the artifacts actually launch before publishing the release.

Never tag a broken build. Never publish artifacts that were not produced
by the release workflow.
