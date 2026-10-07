#!/usr/bin/env python3
"""Find Bridge (Encore) charts for the library's OWN songs — the ones
imported from audio, with their GS and CL twins — that have no BG
version yet, and write them as a pick for fetch.py.

    python3 tools/bridge-bulk/own.py          # writes selection-own.json
    SELECTION=selection-own.json python3 tools/bridge-bulk/fetch.py

Unlike pick.py this asks the live API per song instead of the crawled
catalogue: the catalogue was missing charts the API has. Two facts
about the API this depends on (measured 2026-10-07):

- The search looks at the song NAME only. "Toto Africa" finds nothing,
  "Africa" finds Toto's chart among sixteen. So the query is the title
  (and the artist too, for imports whose artist and title are swapped),
  and the artist is matched afterwards. Common titles need paging:
  "Stan" has 301 hits.
- A community chart may lack Expert (Toto's Africa is Hard only). The
  converter derives the levels below the highest one, so any guitar
  level with 100 notes or more is accepted.

Game rips are candidates (the user, 2026-10-07: "Nimm die Spiel rips
mit hinzu"), but a community chart of the same song still wins;
INCLUDE_RIPS=0 leaves them out (the switch lives in catalog.py, one
rule for every tool). See docs/bridge-bulk-import.md.
"""
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import catalog as E  # noqa: E402
import pick as P  # noqa: E402

OUT = os.environ.get("OUT", "selection-own.json")
INCLUDE_RIPS = E.INCLUDE_RIPS
# Every twin prefix this build and older ones wrote.
PREFIX = re.compile(r"^(\s*\[(?:GS|CL|BG-\d+|GR-\d+|Guitar Study|Classic)\]\s*)+", re.I)


def own_songs():
    """The library's songs that are not Bridge versions, one per song
    (its GS and CL twins folded in), minus those that already have a BG
    version under the same artist and title."""
    songs, bridged = {}, set()
    for folder in os.listdir(P.LIB):
        try:
            s = json.load(open(os.path.join(P.LIB, folder, "chart.json")))["song"]
        except Exception:
            continue
        title = PREFIX.sub("", s.get("title", ""))
        artist = s.get("artist", "")
        k = (P.tkey(title), re.sub(r"[^a-z0-9]", "", E.norm(artist)))
        if folder.startswith(("bridge-", "gamerip-")):
            bridged.add(k)
        else:
            songs.setdefault(k, {"artist": artist, "title": title})
    return [v for k, v in sorted(songs.items(), key=lambda kv: (kv[1]["artist"], kv[1]["title"]))
            if k not in bridged]


def clean(title):
    title = re.sub(r"(?i)\b(official( music)? video|remaster(ed)?|1080p|hd|lyrics?)\b", "", title)
    return re.sub(r"(?i)\s*(ft|feat)\.?\s.*$", "", title)


def search(query):
    hits = []
    for page in range(1, 7):
        d = E.curl_json(["-X", "POST", "https://api.enchor.us/search", "-H", "Content-Type: application/json",
                         "-d", json.dumps({"search": query, "page": page, "per_page": 100, "instrument": "guitar",
                                           "difficulty": None, "drumType": None, "source": "bridge"})])
        got = (d or {}).get("data") or []
        hits += got
        if len(got) < 100:
            break
        time.sleep(0.3)
    return hits


def chart_of(h):
    nd = h.get("notesData") or {}
    return {"md5": h["md5"], "name": h.get("name") or "", "artist": h.get("artist") or "",
            "charter": h.get("charter") or "", "pack": h.get("packName") or "", "path": h.get("drivePath") or "",
            "video": bool(h.get("hasVideoBackground")), "opens": bool(nd.get("hasOpenNotes")),
            "issues": len(nd.get("chartIssues") or []),
            "counts": {x["difficulty"]: x["count"] for x in nd.get("noteCounts", []) if x.get("instrument") == "guitar"},
            "length": h.get("song_length") or 0, "modchart": bool(h.get("modchart"))}


def matches(song, c):
    lt, ct = P.tkey(clean(song["title"])), P.tkey(clean(c["name"]))
    la, ca = P.akeys(song["artist"]), P.akeys(c["artist"])
    short, long_ = sorted((ct, lt), key=len)
    # Containment only when the shorter is most of the longer: "Danke"
    # is not "Danke für nichts", "Nothing Else Matters" is
    # "Metallica- Nothing Else Matters".
    title_ok = bool(ct and lt) and (ct == lt or (short in long_ and len(short) >= 0.6 * len(long_)))
    artist_ok = bool(ca & la)
    swapped = P.tkey(song["artist"]) == ct and bool(ca & P.akeys(song["title"]))
    return (title_ok and artist_ok) or swapped


def main():
    done = set(open(f"{E.WORK}/done.txt").read().split()) if os.path.exists(f"{E.WORK}/done.txt") else set()
    picked, rip_only, nothing = [], [], []
    for song in own_songs():
        queries = {clean(song["title"]).split(" - ")[0].split(" | ")[0].strip(), song["artist"].strip()}
        seen, good, rips = set(), [], 0
        for q in queries:
            for h in search(q):
                if h["md5"] in seen:
                    continue
                seen.add(h["md5"])
                c = chart_of(h)
                if not matches(song, c):
                    continue
                c["rip"] = E.is_rip(c)
                if c["rip"]:
                    rips += 1
                    if not INCLUDE_RIPS:
                        continue
                if c["modchart"] or max(c["counts"].values(), default=0) < 100:
                    continue
                if E.JUNK.search(c["name"]) and not E.JUNK.search(song["title"]):
                    continue
                good.append(c)
        label = f"{song['artist']} - {song['title']}"
        if good:
            best = min(good, key=lambda c: (c["rip"], c["video"], c["opens"], c["issues"], -len(c["counts"]),
                                             -max(c["counts"].values())))
            note = " (already fetched)" if best["md5"] in done else ""
            kind = f" [game rip: {best['pack'] or best['path'] or best['charter']}]" if best["rip"] else ""
            print(f"FOUND  {label}  ->  {best['artist']} - {best['name']} ({best['charter']}) {best['counts']}{kind}{note}")
            if best["md5"] not in done:
                picked.append({"rank": 0, **best})
        elif rips:
            rip_only.append(label)
        else:
            nothing.append(label)
        time.sleep(0.3)
    json.dump(picked, open(os.path.join(E.WORK, OUT), "w"), indent=1, ensure_ascii=False)
    print(f"\n{len(picked)} to fetch -> {OUT}; only game rips: {len(rip_only)}; nothing on Bridge: {len(nothing)}")
    for label in rip_only:
        print(f"  rips only: {label}")


if __name__ == "__main__":
    main()
