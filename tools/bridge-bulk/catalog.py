#!/usr/bin/env python3
"""Build the ranked catalogue of community guitar charts behind Bridge.

Two phases, each resumable (results cached in $BRIDGE_WORK):

  crawl   every guitar chart from the Encore API (api.enchor.us)  -> catalog.jsonl
  rank    a Deezer popularity number per (artist, title)          -> ranks.json

`pick.py` then chooses what to download and `fetch.py` downloads and
converts it. See docs/bridge-bulk-import.md.

This file is also the shared library of the other two: the title and
artist normalisation (`norm`), the filters (`usable`) and the grouping
of charts into songs (`songs`) live here and nowhere else.
"""
import json, os, re, subprocess, time, unicodedata, urllib.parse

WORK = os.environ.get("BRIDGE_WORK", "/Volumes/Samsung SSD/beatbyte-bridge-work")
CATALOG = f"{WORK}/catalog.jsonl"
# Game rips are taken since 2026-10-07 (the user: "Nimm die Spiel rips
# mit hinzu" — then yes to the bulk runs too). INCLUDE_RIPS=0 leaves
# them out; a community chart of the same song is preferred either way.
INCLUDE_RIPS = os.environ.get("INCLUDE_RIPS", "1") != "0"
RANKS = f"{WORK}/ranks.json"


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def curl_json(args, tries=4):
    # curl, not urllib: files.enchor.us refused Python's client, and one
    # transport for both hosts keeps the failure modes the same.
    for attempt in range(tries):
        r = subprocess.run(["curl", "-s", "-m", "40", *args], capture_output=True, text=True)
        try:
            return json.loads(r.stdout)
        except Exception:
            time.sleep(2 + 3 * attempt)
    return None


# ---------------------------------------------------------------- crawl
def crawl():
    if os.path.exists(CATALOG + ".done"):
        return
    # The order shifts while paging (charts get re-indexed), so one pass
    # misses some and repeats others: two full passes, kept by md5.
    seen = {}
    if os.path.exists(CATALOG):
        for line in open(CATALOG):
            c = json.loads(line)
            seen[c["md5"]] = c
    for sweep in (1, 2):
        page, pages = 1, None
        while pages is None or page <= pages:
            rows = []
            for attempt in range(5):  # the API answers 500 now and then
                d = curl_json(["-X", "POST", "https://api.enchor.us/search",
                               "-H", "Content-Type: application/json",
                               "-d", json.dumps({"search": "", "page": page, "per_page": 250,
                                                 "instrument": "guitar", "difficulty": None,
                                                 "drumType": None, "source": "bridge"})])
                if d and d.get("data"):
                    rows = d["data"]
                    pages = -(-d["found"] // 250)
                    break
                time.sleep(3 + 3 * attempt)
            if not rows:
                log("crawl: page", page, "stayed empty")
            for h in rows:
                nd = h.get("notesData") or {}
                counts = {c["difficulty"]: c["count"] for c in nd.get("noteCounts", [])
                          if c.get("instrument") == "guitar"}
                seen[h["md5"]] = {
                    "md5": h["md5"], "name": h.get("name") or "", "artist": h.get("artist") or "",
                    "charter": h.get("charter") or "", "pack": h.get("packName") or "",
                    "path": h.get("drivePath") or "", "video": bool(h.get("hasVideoBackground")),
                    "opens": bool(nd.get("hasOpenNotes")), "issues": len(nd.get("chartIssues") or []),
                    "counts": counts, "length": h.get("song_length") or 0,
                    "modchart": bool(h.get("modchart")),
                }
            if page % 25 == 0:
                log(f"crawl sweep {sweep}: page {page}/{pages}, {len(seen)} unique charts")
            page += 1
            time.sleep(0.5)
        with open(CATALOG, "w") as out:
            for c in seen.values():
                out.write(json.dumps(c) + "\n")
        log(f"crawl sweep {sweep} done: {len(seen)} unique charts")
    open(CATALOG + ".done", "w").close()


# --------------------------------------------------------------- filter
# Game rips (the commercial games' own charts and recordings) by charter,
# pack or drive path: the library takes community charts only.
RIP = re.compile(r"harmonix|neversoft|rock ?band|guitar ?hero|\bgh[1-9]\b|\brb[1-4]\b|wavegroup|"
                 r"rocksmith|beatles rock|lego rock|band hero|dj hero|official|\brip\b|\bdlc\b", re.I)
# Not the original recording, or not a song at all.
JUNK = re.compile(r"\b(live|remix|cover|karaoke|instrumental|acoustic|demo|medley|mashup|"
                  r"full album|but |parody|sung by|8[- ]?bit|nightcore|sped up|slowed|"
                  r"meme|troll|joke|version|edit|mix|reprise|intro|outro)\b|\(feat|\[|\bvs\.?\b", re.I)


def norm(s):
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode().lower()
    s = re.sub(r"<[^>]+>", "", s)   # charters' rich-text tags
    s = re.sub(r"\(.*?\)", "", s)
    s = re.sub(r"[^a-z0-9]+", " ", s)
    return re.sub(r"^the ", "", s).strip()


# Who made the commercial games' charts.
STUDIOS = re.compile(r"\b(harmonix|neversoft|vicarious visions|redoctane|budcat|"
                     r"freestylegames|wavegroup|activision|beenox|ubisoft)\b", re.I)
# The packs that ARE a commercial game or its DLC. Anchored at the start
# and closed at the end or before " DLC"/":"/"(": "J-Rock Band Project"
# (a community project) and "Guitar Hero X-II" (a fan game) are not games.
GAME_PACK = re.compile(
    r"^(the beatles:? |green day:? |lego |ac/dc |)"
    r"(rock band( [1-4]| 2| 3| 4| network| blitz| unplugged| rivals| vr)?|"
    r"guitar hero( ii| iii| 2| 3| 5| world tour| smash hits| metallica| aerosmith| van halen|"
    r" warriors of rock| on tour| encore| 80s| live| greatest hits)?|"
    r"band hero|dj hero( 2)?|rocksmith( 2014)?)"
    r"(\s*dlc\b|\s*[:(-].*|\s*$)", re.I)


def is_rip(c):
    """A chart of a commercial game: made by one of its studios, or in a
    pack that IS one of the games. The broad RIP pattern above only ever
    served to leave things out; to MARK a chart as a rip (`GR-NN`) a
    community pack with "Rock Band" in its name must not count
    (2026-10-07: "J-Rock Band Project", "Guitar Hero X-II" did)."""
    return bool(STUDIOS.search(c["charter"] or "")) or bool(GAME_PACK.match((c["pack"] or "").strip()))


def usable(c):
    if c["modchart"] or not c["counts"].get("expert"):
        return False
    if c["counts"]["expert"] < 100 or not (60_000 <= c["length"] <= 600_000):
        return False
    if not INCLUDE_RIPS and is_rip(c):
        return False
    if JUNK.search(c["name"]) or JUNK.search(c["artist"]):
        return False
    return bool(norm(c["name"]) and norm(c["artist"]))


def songs():
    """Usable charts grouped by song: (artist, title) -> [chart, …]."""
    groups = {}
    for line in open(CATALOG):
        c = json.loads(line)
        if usable(c):
            groups.setdefault((norm(c["artist"]), norm(c["name"])), []).append(c)
    return groups


def best_chart(charts):
    """One chart per song: a community chart before a game rip, then no
    video, no open notes, fewest reported issues, most levels charted by
    hand, most Expert notes."""
    return min(charts, key=lambda c: (is_rip(c), c["video"], c["opens"], c["issues"],
                                      -len(c["counts"]), -c["counts"]["expert"]))


# ----------------------------------------------------------------- rank
def deezer_rank(artist, title):
    q = urllib.parse.quote(f"{artist} {title}")
    d = curl_json([f"https://api.deezer.com/search?q={q}&limit=10"])
    if not d or "data" not in d:
        return None
    best = 0  # 0 = Deezer has no exact match: never picked
    for t in d["data"]:
        if norm(t["artist"]["name"]) == norm(artist) and norm(t["title"]) == norm(title):
            best = max(best, t.get("rank", 0))
    return best


def rank():
    import threading
    from concurrent.futures import ThreadPoolExecutor
    groups = songs()
    ranks = json.load(open(RANKS)) if os.path.exists(RANKS) else {}
    todo = [k for k in groups if "|".join(k) not in ranks]
    log(f"rank: {len(groups)} songs, {len(todo)} to ask Deezer")
    lock = threading.Lock()
    slot = [time.monotonic()]

    def throttle():
        # 8 requests per second overall; Deezer allows 50 per 5 s
        with lock:
            now = time.monotonic()
            wait = slot[0] - now
            slot[0] = max(now, slot[0]) + 0.125
        if wait > 0:
            time.sleep(wait)

    def one(k):
        c = groups[k][0]
        for _ in range(3):
            throttle()
            r = deezer_rank(c["artist"], c["name"])
            if r is not None:
                return k, r
            time.sleep(5)
        return k, None

    done = 0
    with ThreadPoolExecutor(6) as pool:
        for k, r in pool.map(one, todo):
            done += 1
            if r is not None:
                ranks["|".join(k)] = r
            if done % 1000 == 0:
                json.dump(ranks, open(RANKS, "w"))
                log(f"rank: {done}/{len(todo)}")
    json.dump(ranks, open(RANKS, "w"))
    log("rank done")


if __name__ == "__main__":
    os.makedirs(WORK, exist_ok=True)
    crawl()
    rank()
