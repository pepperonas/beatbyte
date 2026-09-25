#!/bin/bash
# The BeatByte performance bench — one scenario, repeated.
#
#   tools/bench.sh <scenario> [out.json]     scenario: normal | dense | hype | soak
#
# Environment:
#   BIN      the game binary        (default target/release/beatbyte)
#   RUNS     repetitions            (default 3; soak ignores it)
#   SOAK_MIN soak length in minutes (default 15)
#   COMMIT   the commit the binary was built from, when not run in its checkout
#
# Each run is the autopilot playing one song perfectly under
# BEATBYTE_UNCAPPED=1, full screen, with the bench (`BEATBYTE_BENCH`)
# writing what the frames did; the first 5 s of song time are dropped
# inside the game. A run whose autopilot verdict fails is reported and
# NOT counted: a bench number from a broken run measures nothing.
# The window is raised every two seconds — an occluded window is not
# rendered, and its frame times are the compositor's, not the game's.
#
# The result is one JSON with every run, the machine and the commit;
# compare two with tools/bench-compare.py.
set -u
scenario=${1:?scenario: normal | dense | hype | soak}
out=${2:-bench-$scenario-$(hostname -s)-$(date +%Y%m%d-%H%M%S).json}
here=$(cd "$(dirname "$0")/.." && pwd)
bin=${BIN:-$here/target/release/beatbyte}
runs=${RUNS:-3}
work=$(mktemp -d "${TMPDIR:-/tmp}/beatbyte-bench.XXXXXX")

case $scenario in
  normal) song='Maria';                         diff=medium ;;
  dense)  song='[CL] What You Get Is What You'; diff=expert ;;
  hype)   song='"Heroes"';                      diff=expert ;;
  soak)   song='[CL] What You Get Is What You'; diff=expert
          runs=$(( ( ${SOAK_MIN:-15} * 60 + 263 ) / 264 )) ;;
  *) echo "unknown scenario: $scenario" >&2; exit 2 ;;
esac

[ -x "$bin" ] || { echo "no binary at $bin" >&2; exit 2; }
if python3 -c "import Quartz,sys; sys.exit(0 if dict(Quartz.CGSessionCopyCurrentDictionary()).get('CGSSessionScreenIsLocked') else 1)" 2>/dev/null; then
  echo "the screen is locked: every frame would be the compositor's, not the game's" >&2
  exit 3
fi

# A busy machine measures the other processes. The load goes into the
# result; above MAX_LOAD (default: the core count) the bench refuses.
cores=$(sysctl -n hw.ncpu)
load=$(sysctl -n vm.loadavg | awk '{print $2}' | tr , .)
if [ -z "${FORCE:-}" ] && awk -v l="$load" -v m="${MAX_LOAD:-$cores}" 'BEGIN{exit !(l>m)}'; then
  echo "load average $load over ${MAX_LOAD:-$cores}: the numbers would measure the other processes (FORCE=1 to run anyway)" >&2
  exit 4
fi
echo "bench $scenario: $song ($diff), $runs run(s), binary $bin, load $load on $cores cores"
for i in $(seq 1 "$runs"); do
  env BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_MUTE=1 BEATBYTE_UNCAPPED=1 \
      BEATBYTE_AUTOPILOT_SONG="$song" BEATBYTE_AUTOPILOT_DIFFICULTY="$diff" \
      BEATBYTE_BENCH="$scenario" BEATBYTE_BENCH_OUT="$work/run-$i.json" \
      caffeinate -disu "$bin" > "$work/run-$i.log" 2>&1 &
  pid=$!
  while kill -0 $pid 2>/dev/null; do
    osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "beatbyte") to true' >/dev/null 2>&1
    sleep 2
  done
  wait $pid; rc=$?
  sysctl -n vm.loadavg | awk '{print $2}' | tr , . > "$work/load-$i.txt"
  if [ $rc -ne 0 ] || [ ! -f "$work/run-$i.json" ]; then
    echo "  run $i: FAILED (exit $rc) — not counted; log kept at $work/run-$i.log"
    rm -f "$work/run-$i.json"
    continue
  fi
  python3 -c "import json,sys; r=json.load(open(sys.argv[1])); f=r['frame_ms']; print(f\"  run $i: p99 {f['p99']:.2f} ms, max {f['max']:.2f} ms, over 16.7 ms {r['frames_over_16_7']}, gpu {r['gpu']}\")" "$work/run-$i.json"
done

python3 - "$out" "$scenario" "$work" "$here" <<'PY'
import json, glob, os, subprocess, sys, statistics
out, scenario, work, here = sys.argv[1:5]
runs = [json.load(open(p)) for p in sorted(glob.glob(os.path.join(work, 'run-*.json')),
                                             key=lambda p: int(p.rsplit('-', 1)[1].split('.')[0]))]
if not runs:
    sys.exit('no run produced a result')
def sh(*cmd):
    try: return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL).strip()
    except Exception: return None
def spread(key, field):
    values = [r[key][field] for r in runs if r.get(key)]
    if not values: return None
    return {'mean': statistics.fmean(values), 'min': min(values), 'max': max(values),
            'stdev': statistics.stdev(values) if len(values) > 1 else 0.0}
summary = {f'frame_{f}': spread('frame_ms', f) for f in ('avg', 'median', 'p95', 'p99', 'max')}
summary['cpu_main_p99'] = spread('cpu_main_ms', 'p99')
summary['gpu_p99'] = spread('gpu_ms', 'p99')
summary['frames_over_16_7'] = [r['frames_over_16_7'] for r in runs]
summary['song_start_ms'] = [r['song_start_ms'] for r in runs]
summary['load_after'] = [float(open(p).read()) for p in sorted(glob.glob(os.path.join(work, 'load-*.txt')))]
result = {
    'scenario': scenario,
    'machine': {'model': sh('sysctl', '-n', 'hw.model'), 'cpu': sh('sysctl', '-n', 'machdep.cpu.brand_string'),
                'arch': sh('uname', '-m'), 'macos': sh('sw_vers', '-productVersion'), 'host': sh('hostname', '-s')},
    # COMMIT for a binary measured away from its checkout (the 2015).
    'commit': os.environ.get('COMMIT') or sh('git', '-C', here, 'rev-parse', '--short', 'HEAD'),
    'dirty': bool(sh('git', '-C', here, 'status', '--porcelain', '--untracked-files=no')),
    'runs': runs,
    'summary': summary,
}
if scenario == 'soak':
    result['summary']['warm_p99'] = runs[-1]['frame_ms']['p99']
json.dump(result, open(out, 'w'), indent=2)
s = summary
print(f"{scenario}: {len(runs)} run(s) on {runs[0]['gpu']} at {runs[0]['resolution'][0]}x{runs[0]['resolution'][1]}"
      f" — p99 {s['frame_p99']['mean']:.2f} ms (±{s['frame_p99']['stdev']:.2f}), max {s['frame_max']['max']:.2f} ms,"
      f" over 16.7 ms {s['frames_over_16_7']} → {out}")
PY
