#!/usr/bin/env python3
"""Write the list of every Bridge chart in the library, as a selection.

The library keeps no download key: a converted folder remembers its
charter and a fingerprint of the files it came from, not the Encore md5
that `https://files.enchor.us/<md5>.sng` needs. This script finds it
again in the catalogue (`catalog.py crawl`) and writes
`docs/bridge-tracks.json` — the same shape `pick.py` writes, so
`fetch.py` can download all of it (see docs/bridge-bulk-import.md,
"Every Bridge song in one file").

A library chart matches a catalogue chart when the normalised artist,
title and charter agree. 512 songs have several such charts (re-uploads,
revisions): the song length decides (the converter keeps the catalogue's
length to the millisecond), the Expert note count only breaks a tie —
the API counts a chord as one note and the library one per lane, so the
counts never agree. What does not match is listed on stderr and left
out, never guessed.

    BRIDGE_WORK=… BEATBYTE_LIB=… python3 tools/bridge-bulk/tracklist.py [out.json]
"""
import json, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import catalog  # noqa: E402

LIB = os.environ.get("BEATBYTE_LIB", "/Volumes/Samsung SSD/beatbyte/songs")
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.path.join(REPO, "docs", "bridge-tracks.json")
TAG = re.compile(r"^\[[A-Z]{2}-\d+\]\s*")
# What a selection entry carries: what fetch.py reads (md5, artist,
# name, charter, pack) plus where the song sits in the library.
FIELDS = ("md5", "artist", "name", "charter", "pack")


def charter_key(s):
    return catalog.norm(s or "")


def library(lib=None):
    """Every folder with a `bridge-source.json`: what the match needs."""
    lib = lib or LIB
    for folder in sorted(os.listdir(lib)):
        if folder.startswith("."):
            continue
        path = os.path.join(lib, folder)
        try:
            source = json.load(open(os.path.join(path, "bridge-source.json")))
            chart = json.load(open(os.path.join(path, "chart.json")))
        except (OSError, ValueError):
            continue
        expert = next((len(c["notes"]) for c in chart.get("charts", [])
                       if c.get("difficulty") == "expert"), 0)
        yield {
            "folder": folder,
            "title": TAG.sub("", chart["song"].get("title", "")),
            "artist": chart["song"].get("artist", ""),
            "charter": source.get("charter", ""),
            "expert": expert,
            "length_ms": round(1000 * (chart["song"].get("duration_s") or 0)),
        }


def index(catalogue=None):
    """The catalogue by (artist, title, charter)."""
    by_key = {}
    for line in open(catalogue or catalog.CATALOG):
        c = json.loads(line)
        key = (catalog.norm(c["artist"]), catalog.norm(c["name"]), charter_key(c["charter"]))
        by_key.setdefault(key, []).append(c)
    return by_key


def best(song, candidates):
    """The candidate of the same length; the nearest Expert count breaks
    a tie (charts identical in both are re-uploads of one chart)."""
    def distance(c):
        n = (c.get("counts") or {}).get("expert", 0)
        return (abs((c.get("length") or 0) - song["length_ms"]), abs(n - song["expert"]))
    return min(candidates, key=distance)


def select(by_key, songs):
    """(entries to write, songs without a match)."""
    picked, missing, seen = [], [], set()
    for song in songs:
        key = (catalog.norm(song["artist"]), catalog.norm(song["title"]), charter_key(song["charter"]))
        candidates = by_key.get(key)
        if not candidates:
            missing.append(song)
            continue
        c = best(song, candidates)
        if c["md5"] in seen:
            print(f"second folder for one chart: {song['folder']}", file=sys.stderr)
            continue
        seen.add(c["md5"])
        entry = {k: c.get(k, "") for k in FIELDS}
        entry["folder"] = song["folder"]
        picked.append(entry)
    return picked, missing


def write(path, picked):
    with open(path, "w") as out:
        # One chart per line: readable, and a diff between two lists
        # shows exactly the songs that came and went.
        out.write("[\n" + ",\n".join(json.dumps(e, ensure_ascii=False) for e in picked) + "\n]\n")


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_OUT
    if not os.path.exists(catalog.CATALOG):
        raise SystemExit(f"no catalogue at {catalog.CATALOG} — run catalog.py first")
    picked, missing = select(index(), library())
    write(out, picked)
    for song in missing:
        print(f"no catalogue match: {song['folder']} ({song['charter']})", file=sys.stderr)
    print(f"{len(picked)} charts written to {out}, {len(missing)} without a match")


if __name__ == "__main__":
    main()
