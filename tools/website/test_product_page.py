"""The product page at beatbyte.celox.io must always offer the newest build and the current changelog.

The page does not deploy itself on a release: a timer on the VPS runs
`website/server/beatbyte-latest.py` every 15 minutes, picks the newest release whose tag matches
`release_tag`, finds one asset per download target by regex, and copies CHANGELOG.md from `main`.
When any target matches no asset, the script keeps the previous state — correct for a broken
release, and exactly how the page sat on v0.18.18 for a day after v0.19.0-beta.1: the patterns
allowed `[0-9.]*` as the version and a pre-release suffix matched nothing.

These tests bind the patterns to the code that NAMES the assets (the release workflow and the two
packaging scripts) and run the real script against stubbed GitHub answers. Standard library only:

    python3 -m unittest discover -s tools/website -v
"""
import importlib.util
import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "website" / "server" / "beatbyte-latest.py"
SITE = json.loads((ROOT / "website" / "site.json").read_text())

# Tags the page must follow: plain releases and every pre-release form SemVer allows us to cut.
VERSIONS = ["v0.19.1", "v0.19.0-beta.1", "v1.0.0-rc.2", "v0.20.0-alpha", "v2.10.13"]


def load_script():
    spec = importlib.util.spec_from_file_location("beatbyte_latest", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


LATEST = load_script()
REAL_GET = LATEST.get


def stub_get(case, body):
    """Answer every GitHub call with `body` for the length of one test."""
    LATEST.get = lambda url, accept=None: body
    case.addCleanup(setattr, LATEST, "get", REAL_GET)


def release_targets():
    """The target triples the release workflow builds, read from its matrix."""
    text = (ROOT / ".github" / "workflows" / "release.yml").read_text()
    return re.findall(r"^\s+target:\s*(\S+)\s*$", text, re.M)


def asset_names(tag):
    """Every file the release workflow attaches for `tag`, built from the producers' own templates."""
    version = tag[1:]
    workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text()
    macos = (ROOT / "packaging" / "macos.sh").read_text()
    appimage = (ROOT / "packaging" / "appimage.sh").read_text()

    archive = re.findall(r"(beatbyte-\$\{\{ github\.ref_name \}\}-\$\{\{ matrix\.target \}\}\.(?:tar\.gz|zip))",
                         workflow)
    dmg = re.search(r'DMG="\$OUT/(BeatByte-\$\{VERSION\}-\$\{TARGET\}\.dmg)"', macos).group(1)
    app = re.search(r'APPIMAGE="\$OUT/(BeatByte-\$\{VERSION\}-x86_64\.AppImage)"', appimage).group(1)
    assert archive and dmg and app, "a producer changed its naming — update this test with it"

    names = []
    for target in release_targets():
        windows = "windows" in target
        for tpl in archive:
            if tpl.endswith(".zip") == windows:
                names.append(tpl.replace("${{ github.ref_name }}", tag).replace("${{ matrix.target }}", target))
        if "apple-darwin" in target:
            names.append(dmg.replace("${VERSION}", version).replace("${TARGET}", target))
        if "linux" in target:
            names.append(app.replace("${VERSION}", version))
    return names


def fake_release(tag, draft=False):
    return {
        "tag_name": tag,
        "draft": draft,
        "prerelease": "-" in tag,
        "published_at": "2026-10-18T00:00:00Z",
        "html_url": f"https://github.com/pepperonas/beatbyte/releases/tag/{tag}",
        "assets": [
            {
                "name": n,
                "size": 1000 + i,
                "digest": "sha256:" + f"{i:064x}",
                "browser_download_url": f"https://github.com/pepperonas/beatbyte/releases/download/{tag}/{n}",
            }
            for i, n in enumerate(asset_names(tag))
        ],
    }


class TheScriptIsTheSiteJson(unittest.TestCase):
    """The deployed script embeds its config; it must be what the generator made from site.json."""

    def test_targets_match(self):
        embedded = {t["id"]: t["asset"] for t in LATEST.CONFIG["targets"]}
        source = {t["id"]: t["asset"] for t in SITE["targets"]}
        self.assertEqual(embedded, source, "site.json changed without regenerating the server script")

    def test_release_tag_matches(self):
        self.assertEqual(LATEST.CONFIG["release_tag"], SITE["release_tag"])


class EveryReleaseFindsItsDownloads(unittest.TestCase):
    def test_the_workflow_builds_the_expected_targets(self):
        # If the matrix ever loses a platform, the page's target for it would fail every run.
        self.assertEqual(
            sorted(release_targets()),
            ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"],
        )

    def test_each_target_matches_exactly_one_asset(self):
        for tag in VERSIONS:
            names = asset_names(tag)
            for t in SITE["targets"]:
                hits = [n for n in names if re.search(t["asset"], n)]
                with self.subTest(tag=tag, target=t["id"]):
                    self.assertEqual(len(hits), 1, f"{t['asset']} against {names}: {hits}")

    def test_a_prerelease_is_found_end_to_end(self):
        rel = fake_release("v0.19.0-beta.1")
        stub_get(self, json.dumps([rel]).encode())
        d = LATEST.fetch_release()
        self.assertEqual(d["version"], "v0.19.0-beta.1")
        self.assertEqual([a["target"] for a in d["assets"]], [t["id"] for t in SITE["targets"]])
        self.assertEqual(d["assets"][0]["name"], "BeatByte-0.19.0-beta.1-aarch64-apple-darwin.dmg")
        self.assertTrue(all(len(a["sha256"]) == 64 for a in d["assets"]))


class TheNewestBuildIsPicked(unittest.TestCase):
    def test_newest_game_release_wins_over_models_and_drafts(self):
        # GitHub lists newest first. A draft is not published yet; models-v1 is not a game build.
        releases = [
            fake_release("v0.19.2", draft=True),
            {"tag_name": "models-v2", "draft": False, "assets": []},
            fake_release("v0.19.0-beta.1"),
            fake_release("v0.18.18"),
        ]
        self.assertEqual(LATEST.pick_release(releases, SITE["release_tag"])["tag_name"], "v0.19.0-beta.1")

    def test_models_alone_is_not_a_build(self):
        with self.assertRaises(ValueError):
            LATEST.pick_release([{"tag_name": "models-v1", "draft": False}], SITE["release_tag"])


class TheChangelogIsAccepted(unittest.TestCase):
    """The page shows CHANGELOG.md from main; the script refuses anything that does not look like one."""

    BODY = (ROOT / "CHANGELOG.md").read_bytes()

    def test_the_real_changelog_passes_the_check(self):
        stub_get(self, self.BODY)
        self.assertEqual(LATEST.fetch_changelog(), self.BODY.decode())

    def test_it_is_fetched_from_main(self):
        self.assertTrue(LATEST.CHANGELOG_URL.endswith("/pepperonas/beatbyte/main/CHANGELOG.md"))

    def test_it_leaves_room_below_the_size_cap(self):
        # Over the cap the dialog freezes on the last good copy. Warn at half, long before that.
        self.assertLess(len(self.BODY), LATEST.CHANGELOG_MAX // 2,
                        "CHANGELOG.md is approaching the page's size cap — raise it in the kit")


class DeployNeverDeletesWhatTheTimerWrites(unittest.TestCase):
    def test_server_written_files_are_excluded(self):
        deploy = (ROOT / "website" / "deploy.sh").read_text()
        for path in ("latest.json", "changelog.md", "ssi/", "files/"):
            with self.subTest(path=path):
                self.assertIn(f"--exclude {path}", deploy)


if __name__ == "__main__":
    unittest.main()
