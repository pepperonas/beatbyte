#!/usr/bin/env python3
"""Download the picked Encore charts, unpack them (no video) and hand
them to `beatbyte-cli bridge` in batches.

    SELECTION=selection-next.json python3 tools/bridge-bulk/fetch.py

Resumable: done.txt lists the md5s already handled. Stops before the
downloads would pass CAP bytes. Waits while the game runs (the import
refuses then). See docs/bridge-bulk-import.md."""
import json, os, shutil, struct, subprocess, sys, time

try:  # live progress above the Claude Code prompt (taskline); optional
    from taskline import Progress
except ImportError:
    Progress = None

WORK = os.environ.get("BRIDGE_WORK", "/Volumes/Samsung SSD/beatbyte-bridge-work")
LIB = os.environ.get("BEATBYTE_LIB", "/Volumes/Samsung SSD/beatbyte/songs")
# A copy of the CLI, not target/release: a rebuild mid-run would swap
# the converter under a batch.
CLI = os.environ.get("BEATBYTE_CLI", f"{WORK}/beatbyte-cli")
CAP = int(float(os.environ.get("CAP_GB", "200")) * 10**9)
BATCH = 25
DONE = f"{WORK}/done.txt"
BYTES = f"{WORK}/downloaded-bytes.txt"
SKIP_EXT = {".mp4", ".webm", ".avi", ".mkv", ".mov", ".ogv"}

def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)

def unpack(sng_path, out_dir):
    data = open(sng_path, "rb").read()
    if data[:6] != b"SNGPKG":
        raise ValueError("not an SNGPKG")
    mask = data[10:26]
    pos = 26
    _meta_len, meta_count = struct.unpack_from("<QQ", data, pos); pos += 16
    meta = {}
    for _ in range(meta_count):
        (kl,) = struct.unpack_from("<i", data, pos); pos += 4
        key = data[pos:pos + kl].decode("utf-8", "replace"); pos += kl
        (vl,) = struct.unpack_from("<i", data, pos); pos += 4
        meta[key] = data[pos:pos + vl].decode("utf-8", "replace"); pos += vl
    _fm_len, file_count = struct.unpack_from("<QQ", data, pos); pos += 16
    files = []
    for _ in range(file_count):
        nl = data[pos]; pos += 1
        name = data[pos:pos + nl].decode("utf-8", "replace"); pos += nl
        size, offset = struct.unpack_from("<QQ", data, pos); pos += 16
        files.append((name, size, offset))
    os.makedirs(out_dir, exist_ok=True)
    key = bytes(mask[i % 16] ^ (i & 0xFF) for i in range(256 * 16))  # repeats every 4096
    for name, size, offset in files:
        base = os.path.basename(name)
        if not base or os.path.splitext(base)[1].lower() in SKIP_EXT:
            continue
        raw = data[offset:offset + size]
        full = (key * (size // len(key) + 1))[:size]
        plain = bytes(a ^ b for a, b in zip(raw, full))
        open(os.path.join(out_dir, base), "wb").write(plain)
    with open(os.path.join(out_dir, "song.ini"), "w") as ini:
        ini.write("[song]\n")
        for k, v in meta.items():
            ini.write(f"{k} = {v}\n")
    names = {f[0].lower() for f in files}
    if not ({"notes.mid", "notes.chart"} & names):
        raise ValueError("no notes file")

def main():
    selection = json.load(open(f"{WORK}/" + os.environ.get("SELECTION", "selection-next.json")))
    done = set(open(DONE).read().split()) if os.path.exists(DONE) else set()
    total = int(open(BYTES).read()) if os.path.exists(BYTES) else 0
    todo = [c for c in selection if c["md5"] not in done]
    log(f"{len(todo)} to fetch, {total/1e9:.1f} GB already downloaded")
    if Progress is None:
        return run(todo, total, None)
    # stalled_after: a batch ends with the bridge import, which waits while the game runs
    with Progress("bridge", total=len(todo), label="Bridge", unit="songs", icon="⬇", stalled_after=900) as p:
        p.update(0, bytes=total)
        run(todo, total, p)

def run(todo, total, p):
    for start in range(0, len(todo), BATCH):
        batch_dir = f"{WORK}/batch"
        shutil.rmtree(batch_dir, ignore_errors=True)
        os.makedirs(batch_dir)
        handled = []
        for c in todo[start:start + BATCH]:
            head = subprocess.run(["curl", "-sI", "-m", "30", f"https://files.enchor.us/{c['md5']}.sng"],
                                  capture_output=True, text=True).stdout
            size = next((int(l.split(":")[1]) for l in head.splitlines()
                         if l.lower().startswith("content-length")), None)
            if size is None:
                log("no size, skipped:", c["artist"], "-", c["name"]); continue
            if total + size > CAP:
                log(f"cap reached at {total/1e9:.1f} GB — stopping"); return finish(batch_dir, handled)
            sng = f"{batch_dir}/{c['md5']}.sng"
            r = subprocess.run(["curl", "-sf", "-m", "600", "-o", sng, f"https://files.enchor.us/{c['md5']}.sng"])
            if r.returncode != 0 or not os.path.exists(sng):
                log("download failed:", c["artist"], "-", c["name"]); continue
            total += os.path.getsize(sng)
            open(BYTES, "w").write(str(total))
            safe = "".join(ch if ch.isalnum() or ch in " -_()" else "_" for ch in f"{c['artist']} - {c['name']} ({c['charter']})")[:120]
            try:
                unpack(sng, f"{batch_dir}/{safe}")
            except Exception as e:
                log("unpack failed:", safe, e); shutil.rmtree(f"{batch_dir}/{safe}", ignore_errors=True)
            os.remove(sng)
            handled.append(c["md5"])
            if p:
                p.update(start + len(handled), bytes=total)
            time.sleep(0.3)
        finish(batch_dir, handled)
        log(f"batch {start // BATCH + 1}: {start + len(handled)}/{len(todo)}, {total/1e9:.2f} GB downloaded")
    log("all done")

def game_running():
    return subprocess.run(["pgrep", "-x", "beatbyte"], capture_output=True).returncode == 0

def finish(batch_dir, handled):
    if any(os.path.isdir(os.path.join(batch_dir, d)) for d in os.listdir(batch_dir)):
        # The import refuses while the game runs (someone may be testing
        # it): wait rather than lose the batch.
        while True:
            while game_running():
                time.sleep(10)
            r = subprocess.run([CLI, "bridge", batch_dir, "--library", LIB], capture_output=True, text=True)
            out = r.stdout + r.stderr
            if "is running" in out:
                log("game started meanwhile — waiting, then again")
                continue
            break
        for line in out.splitlines():
            print("   ", line, flush=True)
    with open(DONE, "a") as f:
        for m in handled:
            f.write(m + "\n")
    shutil.rmtree(batch_dir, ignore_errors=True)

if __name__ == "__main__":
    main()
