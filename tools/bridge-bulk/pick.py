#!/usr/bin/env python3
"""Pick the next TOP_N songs by Deezer rank that are in NO form in the library yet.

    TOP_N=20 OUT=selection-next.json python3 tools/bridge-bulk/pick.py

Writes $BRIDGE_WORK/$OUT and prints a review list: songs whose TITLE is
already in the library under another artist, and titles that occur twice
in the pick. Most are different songs that share a name ("Stay", "Hurt");
the ones that are covers go into exclusions.json, then pick again. See
docs/bridge-bulk-import.md.
"""
import collections, glob, json, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import catalog as E  # noqa: E402

LIB = os.environ.get("BEATBYTE_LIB", "/Volumes/Samsung SSD/beatbyte/songs")
TOP_N = int(os.environ.get("TOP_N", "20"))
OUT = os.environ.get("OUT", "selection-next.json")
DONE = f"{E.WORK}/done.txt"

_ex = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "exclusions.json")))
COVERS = {tuple(pair) for pair in _ex["covers"]}
BROKEN = set(_ex["broken"])
# The twins' title tags. GR (game rips, 0.18.79) was missing at first,
# and every duplicate check was blind to songs that exist only as a rip.
PREFIX = re.compile(r"^(\s*\[(?:GS|CL|BG-\d+|GR-\d+|Guitar Study)\]\s*)+")


def tkey(title):
    """The title with every non-alphanumeric gone (and the twin tag)."""
    return re.sub(r"[^a-z0-9]", "", E.norm(PREFIX.sub("", title)))


def akeys(artist):
    """The artist, and each credited artist on their own."""
    a = E.norm(artist)
    parts = re.split(r"\b(?:feat|ft|featuring|and|x|with)\b|&|,|/|\+", a)
    out = {re.sub(r"[^a-z0-9]", "", p) for p in parts}
    out.add(re.sub(r"[^a-z0-9]", "", a))
    return {p for p in out if p}


def library_songs():
    """title key -> [artist keys, …] over every chart version and song
    document in the library, imports and twins alike — plus the artist
    names as written, for the review list."""
    have, names = {}, {}
    for folder in os.listdir(LIB):
        d = os.path.join(LIB, folder)
        if not os.path.isdir(d):
            continue
        pairs = set()
        for c in glob.glob(os.path.join(d, "*.json")):
            if c.endswith(".context.json"):
                continue
            try:
                j = json.load(open(c))
            except Exception:
                continue
            if isinstance(j, dict) and isinstance(j.get("song"), dict):
                pairs.add((j["song"].get("title", ""), j["song"].get("artist", "")))
            ident = j.get("identity") if isinstance(j, dict) else None
            if isinstance(ident, dict):  # song.json (ADR-0019)
                t = (ident.get("title") or {}).get("value", "")
                for a in ident.get("artists") or [""]:
                    pairs.add((t, a))
        for t, a in pairs:
            have.setdefault(tkey(t), []).append(akeys(a))
            if a:
                names.setdefault(tkey(t), set()).add(a)
    return have, names


def is_dup(have, artist, title):
    """Same title and at least one shared artist."""
    a = akeys(artist)
    return any(a & other for other in have.get(tkey(title), []))


def main():
    groups = E.songs()
    ranks = json.load(open(E.RANKS))
    have, names = library_songs()
    done = set(open(DONE).read().split()) if os.path.exists(DONE) else set()
    n_lib = sum(len(v) for v in have.values())
    ranked = sorted(((ranks.get("|".join(k), 0), k) for k in groups), reverse=True)
    picked, skipped = [], 0
    for r, k in ranked:
        if r <= 0:
            break
        best = E.best_chart(groups[k])
        cover = (best["artist"], best["name"]) in COVERS or re.search(r"ao vivo|en vivo", best["name"], re.I)
        if cover or best["md5"] in BROKEN or best["md5"] in done or is_dup(have, best["artist"], best["name"]):
            skipped += 1
            continue
        picked.append({"rank": r, **best})
        # a second spelling of the same song inside this pick is a duplicate too
        have.setdefault(tkey(best["name"]), []).append(akeys(best["artist"]))
        if len(picked) >= TOP_N:
            break
    if not picked:
        sys.exit("nothing left to pick")
    json.dump(picked, open(os.path.join(E.WORK, OUT), "w"), indent=1)
    print(f"library songs read: {n_lib}; skipped as already present: {skipped}; "
          f"picked {len(picked)}, rank {picked[0]['rank']} .. {picked[-1]['rank']} -> {OUT}")

    # The review list: what the automatic check cannot decide.
    same = [c for c in picked if tkey(c["name"]) in names]
    if same:
        print(f"\nsame title already in the library under another artist ({len(same)}) — a cover?")
        for c in same:
            print(f"  {c['artist']} - {c['name']}   || library: {', '.join(sorted(names[tkey(c['name'])]))}")
    twice = collections.defaultdict(list)
    for c in picked:
        twice[tkey(c["name"])].append(c["artist"])
    twice = {t: a for t, a in twice.items() if len(a) > 1}
    if twice:
        print(f"\nsame title twice in this pick ({len(twice)}):")
        for t, a in twice.items():
            print(f"  {t}: {', '.join(a)}")


if __name__ == "__main__":
    main()
