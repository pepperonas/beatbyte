#!/usr/bin/env python3
"""Reclaim stale incremental-compilation caches under `target/`.

Cargo writes one directory per *unit* (crate × profile × feature set ×
target kind) under `target/<profile>/incremental/`, and one **session**
inside it per build. It deletes a superseded session only when it builds
that same unit again — so a configuration that ran once (a
`--all-features` clippy pass, a `cargo doc`, a one-off `-p <crate>`
probe) keeps its cache for ever.

Measured in this workspace on 2026-09-14: one edit-and-rebuild of
`beatbyte-game` writes ~196 MB, and half an hour of ordinary work left
95 unit directories holding 2.9 GB — one of them with a 520 MB session
that no later build ever collected.

Deleting an incremental cache can never break a build: the worst case is
one non-incremental compile of that unit (measured 30.5 s against 8.0 s
for `beatbyte-game`). That is why this prunes here and NEVER inside
`deps/` — pruning that by age broke the tree once and cost a full
rebuild (see CLAUDE.md).
"""

from __future__ import annotations

import argparse
import shutil
import sys
import time
from pathlib import Path


def sessions(unit: Path) -> list[Path]:
    """The session directories inside one unit's cache, newest last."""
    found = [p for p in unit.iterdir() if p.is_dir() and p.name.startswith("s-")]
    return sorted(found, key=lambda p: p.stat().st_mtime)


def stale_sessions(unit: Path) -> list[Path]:
    """Every session but the newest — what cargo itself would collect."""
    return sessions(unit)[:-1]


def is_stale_unit(unit: Path, now: float, days: float) -> bool:
    """Whether a whole unit cache has gone untouched for `days`.

    `days <= 0` disables the rule: then only superseded sessions go, and
    every configuration keeps a warm cache.
    """
    if days <= 0:
        return False
    newest = max((p.stat().st_mtime for p in sessions(unit)), default=unit.stat().st_mtime)
    return (now - newest) > days * 86_400.0


def reclaimable_bytes(path: Path) -> int:
    """Bytes that deleting `path` actually frees.

    Cargo hard-links unchanged files from the previous session into the
    new one, so a plain sum of file sizes counts shared blocks that stay
    alive in the surviving session: the first version of this tool
    predicted 814 MB where `du` then measured 393. Only files whose last
    link is in here are really freed.
    """
    total = 0
    for item in path.rglob("*"):
        if item.is_file():
            info = item.stat()
            if info.st_nlink == 1:
                total += info.st_size
    return total


def units(target: Path):
    for incremental in sorted(target.glob("*/incremental")):
        for unit in sorted(incremental.iterdir()):
            if unit.is_dir():
                yield unit


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default="target", type=Path)
    parser.add_argument(
        "--stale-days",
        type=float,
        default=0.0,
        help="also drop whole unit caches untouched this long (0 = keep them)",
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if not args.target.is_dir():
        print(f"no {args.target}/ — nothing to prune")
        return 0

    now = time.time()
    freed = 0
    dropped_units = 0
    dropped_sessions = 0
    for unit in units(args.target):
        if is_stale_unit(unit, now, args.stale_days):
            freed += reclaimable_bytes(unit)
            dropped_units += 1
            if not args.dry_run:
                shutil.rmtree(unit, ignore_errors=True)
            continue
        for session in stale_sessions(unit):
            freed += reclaimable_bytes(session)
            dropped_sessions += 1
            if not args.dry_run:
                shutil.rmtree(session, ignore_errors=True)
                lock = session.parent / f"{session.name.rsplit('-', 1)[0]}.lock"
                lock.unlink(missing_ok=True)

    verb = "would reclaim" if args.dry_run else "reclaimed"
    print(
        f"{verb} {freed / 1024 / 1024:.0f} MB "
        f"({dropped_sessions} superseded sessions, {dropped_units} stale unit caches)"
    )
    return 0


def self_test() -> int:
    """Pin the rule on a synthetic tree — the deletions are the contract."""
    import os
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        unit = root / "debug" / "incremental" / "crate-abc"
        unit.mkdir(parents=True)
        for name, age_s in (("s-old-1-x", 5_000), ("s-new-2-x", 1)):
            session = unit / name
            session.mkdir()
            (session / "blob").write_bytes(b"0" * 16)
            stamp = time.time() - age_s
            os.utime(session, (stamp, stamp))

        assert [p.name for p in stale_sessions(unit)] == ["s-old-1-x"], "the newest session stays"
        now = time.time()
        assert not is_stale_unit(unit, now, 0), "0 days must never drop a whole unit"
        assert not is_stale_unit(unit, now, 1), "a unit touched a second ago is not stale"
        # Staleness reads the NEWEST session, not the oldest: a unit that
        # was rebuilt a second ago stays warm however old its first
        # session is (the assertion that said otherwise was wrong, and
        # the self-test is what said so).
        assert not is_stale_unit(unit, now, 0.01), "one fresh session keeps the unit warm"

        cold = root / "debug" / "incremental" / "crate-cold"
        cold.mkdir()
        session = cold / "s-cold-1-x"
        session.mkdir()
        (session / "blob").write_bytes(b"0" * 16)
        stamp = time.time() - 5_000
        os.utime(session, (stamp, stamp))
        assert is_stale_unit(cold, now, 0.01), "a unit untouched past the window is stale"
        assert not is_stale_unit(cold, now, 7), "and not stale inside a wider window"

        idle = root / "debug" / "incremental" / "crate-empty"
        idle.mkdir()
        assert stale_sessions(idle) == [], "a unit without sessions offers nothing to drop"
    print("self-test ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
