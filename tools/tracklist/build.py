#!/usr/bin/env python3
"""Generate the public track collection (docs/tracks/) from a song library.

    python3 -I tools/tracklist/build.py <library songs dir> [<out dir>]

Reads each song folder's song.json and chart.json and writes one Markdown
page per first letter of the artist plus an index. One row per song: two
chart versions of the same song (a community chart and another one) are
merged into one row (the community chart's line is shown).

Deliberately NOT written: download keys, charters, or which chart came
from which source. The page lists what is in the library, not how to
fetch it — that lives in docs/bridge-bulk-import.md.
"""
import collections
import datetime
import json
import os
import re
import sys

BINS = 24  # cells in the density line
LEVELS = "▁▂▃▄▅▆▇█"
STEP_NPS = 1.75  # notes per second per level: 8 levels reach ~14 nps (p99)
TAG = re.compile(r"^\[[A-Z]+-\d+\]\s*")
DIFFS = ("easy", "medium", "hard", "expert")


def density(notes, duration):
    """Expert note density over the song as one line of block glyphs, on a
    scale shared by every song, so a sparse song looks sparse."""
    if duration <= 0:
        return ""
    width = duration / BINS
    counts = [0] * BINS
    for n in notes:
        counts[min(BINS - 1, max(0, int(n["time"] / width)))] += 1
    out = []
    for c in counts:
        if c == 0:
            out.append("·")
        else:
            level = min(len(LEVELS), max(1, -(-c / width // STEP_NPS)))
            out.append(LEVELS[int(level) - 1])
    return "".join(out)


def cell(s):
    return str(s).replace("|", "\\|").replace("\n", " ").strip()


def norm(s):
    return re.sub(r"[^a-z0-9]+", "", s.lower())


def letter(artist):
    a = re.sub(r"^the\s+", "", artist.strip(), flags=re.I)
    first = a[:1].upper()
    if "A" <= first <= "Z":
        return first
    return "0-9" if first.isdigit() else "other"


def read(folder):
    song = json.load(open(os.path.join(folder, "song.json"), encoding="utf-8"))
    chart_file = (song.get("gameplay") or {}).get("chart_file") or "chart.json"
    chart = json.load(open(os.path.join(folder, chart_file), encoding="utf-8"))
    meta = chart.get("song") or {}
    raw_title = (song.get("identity") or {}).get("title", {}).get("value") or meta.get("title", "")
    artists = (song.get("identity") or {}).get("artists") or [meta.get("artist", "")]
    genres = [g.get("value") for g in (song.get("descriptive") or {}).get("genres", []) if g.get("value")]
    duration = (song.get("musical") or {}).get("duration_s") or meta.get("duration_s") or 0
    bpm = ((song.get("musical") or {}).get("bpm") or {}).get("value") or meta.get("bpm") or 0
    by_diff = {c["difficulty"]: c for c in chart.get("charts", [])}
    expert = by_diff.get("expert") or (chart["charts"][-1] if chart.get("charts") else {"notes": []})
    return {
        "community": raw_title.startswith("[BG"),
        "title": TAG.sub("", raw_title).strip() or meta.get("title", ""),
        "artist": ", ".join(a for a in artists if a) or "Unknown",
        "genre": genres[0] if genres else (meta.get("genre") or ""),
        "duration": duration,
        "bpm": bpm,
        "notes": [len(by_diff[d]["notes"]) if d in by_diff else None for d in DIFFS],
        "density": density(expert["notes"], duration),
    }


def fmt_len(s):
    s = int(round(s))
    return f"{s // 60}:{s % 60:02d}"


def main():
    lib = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "..", "..", "docs", "tracks")
    songs, failed = {}, []
    for d in sorted(os.listdir(lib)):
        p = os.path.join(lib, d)
        if d.startswith(".") or not os.path.isdir(p):
            continue
        try:
            r = read(p)
        except Exception as e:  # a folder the game would also refuse
            failed.append(f"{d}: {type(e).__name__}")
            continue
        key = (norm(r["artist"]), norm(r["title"]))
        prev = songs.get(key)
        if prev is None:
            r["charts"] = 1
            songs[key] = r
        else:
            prev["charts"] += 1
            if r["community"] and not prev["community"]:  # show the community chart's line
                r["charts"] = prev["charts"]
                songs[key] = r

    by_letter = collections.defaultdict(list)
    for r in songs.values():
        by_letter[letter(r["artist"])].append(r)
    order = [chr(c) for c in range(ord("A"), ord("Z") + 1)] + ["0-9", "other"]
    pages = [l for l in order if by_letter.get(l)]
    os.makedirs(out, exist_ok=True)
    for name in os.listdir(out):
        if name.endswith(".md"):
            os.remove(os.path.join(out, name))

    nav = " · ".join(f"[{l}]({l}.md)" for l in pages)
    for l in pages:
        rows = sorted(by_letter[l], key=lambda r: (r["artist"].lower(), r["title"].lower()))
        lines = [
            f"# Tracks — {l}",
            "",
            f"[← Index](README.md) · {nav}",
            "",
            f"{len(rows)} songs. Notes = easy · medium · hard · expert. "
            "Density = expert notes over the song, start to end ([legend](README.md#reading-the-density-line)).",
            "",
            "| Artist | Title | Genre | Length | BPM | Notes | Density (expert) |",
            "|---|---|---|--:|--:|--:|---|",
        ]
        for r in rows:
            notes = " · ".join("–" if n is None else str(n) for n in r["notes"])
            lines.append(
                f"| {cell(r['artist'])} | {cell(r['title'])} | {cell(r['genre'])} | {fmt_len(r['duration'])} "
                f"| {round(r['bpm'])} | {notes} | `{r['density']}` |"
            )
        open(os.path.join(out, f"{l}.md"), "w", encoding="utf-8").write("\n".join(lines) + "\n")

    allrows = list(songs.values())
    genres = collections.Counter(r["genre"] or "unknown" for r in allrows).most_common(12)
    total_s = sum(r["duration"] for r in allrows)
    charts = sum(r["charts"] for r in allrows)
    idx = [
        "# Track collection",
        "",
        f"Every song in the author's BeatByte library: **{len(allrows):,} songs** with **{charts:,} charts**, "
        f"{total_s / 3600:,.0f} hours of music. Generated on {datetime.date.today().isoformat()} by "
        "`tools/tracklist/build.py` — edit the script, not these pages.",
        "",
        "BeatByte ships no songs. These charts were imported from the community catalogue behind the "
        "Bridge app (Encore); how that works, and how to import charts into your own library, is in "
        "[docs/bridge-bulk-import.md](../bridge-bulk-import.md). The audio is the player's own; nothing "
        "here is downloadable.",
        "",
        "## By artist",
        "",
        "| Letter | Songs |",
        "|---|--:|",
    ]
    idx += [f"| [{l}]({l}.md) | {len(by_letter[l]):,} |" for l in pages]
    idx += [
        "",
        "## Reading the density line",
        "",
        f"Each song gets one line of {BINS} cells: the song split into {BINS} equal slices, start on the "
        "left, and each cell shows how many expert notes fall into its slice. The scale is the same for "
        "every song, so a calm song looks calm next to a fast one.",
        "",
        "| Cell | Notes per second |",
        "|---|---|",
        "| `·` | none — intro, break, outro |",
    ]
    for i, g in enumerate(LEVELS):
        lo, hi = i * STEP_NPS, (i + 1) * STEP_NPS
        idx.append(f"| `{g}` | {'over ' + format(lo, 'g') if i == len(LEVELS) - 1 else format(lo, 'g') + '–' + format(hi, 'g')} |")
    idx += [
        "",
        "Example: `··▂▃▃▅▅▃▃▅▇▇▅▃▃▅▅▇██▇▅▃▂·` — a quiet intro, verses and choruses that build, "
        "a dense solo near the end and a short outro.",
        "",
        "## Genres",
        "",
        "| Genre | Songs |",
        "|---|--:|",
    ]
    idx += [f"| {cell(g)} | {n:,} |" for g, n in genres]
    idx += ["", "## Regenerating", "", "```bash", "python3 -I tools/tracklist/build.py <library>/songs", "```", ""]
    open(os.path.join(out, "README.md"), "w", encoding="utf-8").write("\n".join(idx))
    print(f"{len(allrows)} songs, {charts} charts, {len(pages)} pages; {len(failed)} folders skipped")
    for f in failed:
        print("  skipped", f)


if __name__ == "__main__":
    main()
