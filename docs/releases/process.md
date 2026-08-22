# Release Process

BeatByte follows [Semantic Versioning](https://semver.org/). During early
development versions are `0.0.x`; the first playable prototype family is
`0.1.x`.

## Checklist

1. Ensure `main` is green (CI passing).
2. Update `workspace.package.version` in the root `Cargo.toml` **and** the
   internal `beatbyte-*` path-dependency versions in
   `[workspace.dependencies]`.
3. Move `[Unreleased]` entries in `CHANGELOG.md` into a new dated section
   with Added/Changed/Fixed/Removed subsections; update the compare links.
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
