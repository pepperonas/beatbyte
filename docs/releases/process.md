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
9. Publish it (`gh release edit vX.Y.Z --draft=false --prerelease`) and,
   within 15 minutes, run `tools/website-check.sh`. It must exit 0: the
   product page offers this build on every platform and shows the
   current CHANGELOG. Until it does, the release is not finished.

## Product page

**https://beatbyte.celox.io always offers the newest published build
and the current CHANGELOG.** Nobody deploys it on a release; it follows
by itself:

- `beatbyte-latest.timer` on the VPS runs
  `website/server/beatbyte-latest.py` every 15 minutes. It takes the
  newest **published** release whose tag matches `release_tag` (`^v[0-9]`
  — drafts are skipped, `models-v1` is not a game build, pre-releases
  count), finds one asset per download target (macOS Apple silicon,
  macOS Intel, Windows, Linux AppImage) by the regexes in
  `website/site.json`, copies each one to `/files/` after checking its
  size and SHA-256, and points `/download/<target>` at it.
- In the same run it copies `CHANGELOG.md` from `main` to
  `/changelog.md`, which the page's changelog dialog shows. A changelog
  entry is therefore on the page within 15 minutes of its push, tagged
  or not.
- When anything does not fit — GitHub unreachable, a target with no
  matching asset, a changelog that does not start with `# Changelog` or
  exceeds 2 MB — the run changes nothing and the page keeps its last
  good state. That is safe, and it is also how the page can go stale
  without anyone seeing it, so two guards exist:

| Guard | Catches | Runs |
|-------|---------|------|
| `tools/website/test_product_page.py` | a tag or asset name the patterns cannot match (built from `release.yml` and `packaging/*.sh`, for plain and pre-release versions); `site.json` and the deployed script drifting apart; the picking rule; the real CHANGELOG failing the script's check or nearing its size cap | CI, every push (`python3 -m unittest discover -s tools/website -v`) |
| `tools/website-check.sh` | the live page not showing the newest release, a download redirecting elsewhere or not answering 200, `changelog.md` differing from `main` | by hand, after every publish (step 9) |

Changing what the page downloads (a new platform, a renamed asset, a
new tag form): edit `website/site.json`, regenerate the page with the
kit (`website/README.md`), run the tests, then `./website/deploy.sh
server` so the VPS gets the new script. A pattern change that reaches
only `site.json` does nothing: the timer runs the copy on the VPS.

Never tag a broken build. Never publish artifacts that were not produced
by the release workflow.
