"""Tests for tracklist.py and the list it wrote (docs/bridge-tracks.json).

    python3 -m unittest discover -s tools/bridge-bulk -v
"""
import json, os, re, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import tracklist  # noqa: E402

TRACKS = os.path.join(os.path.dirname(os.path.dirname(HERE)), "docs", "bridge-tracks.json")


def chart(md5, artist, name, charter, length, expert, pack=""):
    return {"md5": md5, "artist": artist, "name": name, "charter": charter,
            "pack": pack, "length": length, "counts": {"expert": expert}}


def song(folder, artist, title, charter, length_ms, expert):
    return {"folder": folder, "artist": artist, "title": title, "charter": charter,
            "length_ms": length_ms, "expert": expert}


class Matching(unittest.TestCase):
    def index(self, charts):
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as f:
            for c in charts:
                f.write(json.dumps(c) + "\n")
        self.addCleanup(os.remove, f.name)
        return tracklist.index(f.name)

    def test_the_length_decides_before_the_note_count(self):
        by_key = self.index([
            chart("a" * 32, "Toto", "Africa", "X", 300_000, 700),
            chart("b" * 32, "Toto", "Africa", "X", 256_000, 900),
        ])
        picked, missing = tracklist.select(by_key, [song("toto--africa--bg01", "Toto", "Africa", "X", 300_000, 900)])
        self.assertEqual([e["md5"] for e in picked], ["a" * 32])
        self.assertEqual(missing, [])

    def test_the_note_count_breaks_a_tie_in_length(self):
        by_key = self.index([
            chart("a" * 32, "Toto", "Africa", "X", 300_000, 500),
            chart("b" * 32, "Toto", "Africa", "X", 300_000, 880),
        ])
        picked, _ = tracklist.select(by_key, [song("f", "Toto", "Africa", "X", 300_000, 900)])
        self.assertEqual(picked[0]["md5"], "b" * 32)

    def test_another_charter_is_no_match(self):
        by_key = self.index([chart("a" * 32, "Toto", "Africa", "Y", 300_000, 900)])
        picked, missing = tracklist.select(by_key, [song("f", "Toto", "Africa", "X", 300_000, 900)])
        self.assertEqual(picked, [])
        self.assertEqual([m["folder"] for m in missing], ["f"])

    def test_spelling_differences_still_match(self):
        by_key = self.index([chart("a" * 32, "The Kinks", "Lola (Remastered)", "<b>X</b>", 0, 1)])
        picked, _ = tracklist.select(by_key, [song("f", "Kinks", "Lola", "X", 243_856, 1)])
        self.assertEqual(len(picked), 1)

    def test_an_entry_carries_what_fetch_reads(self):
        by_key = self.index([chart("a" * 32, "A", "B", "C", 1, 1, pack="P")])
        picked, _ = tracklist.select(by_key, [song("a--b--bg01", "A", "B", "C", 1, 1)])
        self.assertEqual(picked[0], {"md5": "a" * 32, "artist": "A", "name": "B",
                                     "charter": "C", "pack": "P", "folder": "a--b--bg01"})

    def test_written_one_chart_per_line(self):
        entries = [{"md5": "a" * 32, "folder": "x"}, {"md5": "b" * 32, "folder": "y"}]
        with tempfile.NamedTemporaryFile("w", delete=False) as f:
            path = f.name
        self.addCleanup(os.remove, path)
        tracklist.write(path, entries)
        with open(path) as f:
            text = f.read()
        self.assertEqual(json.loads(text), entries)
        self.assertEqual(text.count("\n"), len(entries) + 2)


class TheCommittedList(unittest.TestCase):
    """docs/bridge-tracks.json is what a restore downloads: every entry
    must be fetchable, and no chart or folder may appear twice."""

    @classmethod
    def setUpClass(cls):
        with open(TRACKS) as f:
            cls.entries = json.load(f)

    def test_it_is_the_whole_library_not_a_sample(self):
        self.assertGreater(len(self.entries), 8000)

    def test_every_entry_names_a_download(self):
        for e in self.entries:
            self.assertRegex(e["md5"], r"^[0-9a-f]{32}$", e)
            for field in ("artist", "name", "charter", "pack", "folder"):
                self.assertIsInstance(e[field], str, e)
            self.assertTrue(re.search(r"--(bg|gr)\d\d$", e["folder"]), e)

    def test_no_chart_and_no_folder_twice(self):
        for field in ("md5", "folder"):
            values = [e[field] for e in self.entries]
            self.assertEqual(len(values), len(set(values)), field)


if __name__ == "__main__":
    unittest.main()
