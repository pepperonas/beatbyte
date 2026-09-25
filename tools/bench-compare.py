#!/usr/bin/env python3
"""Two bench results side by side: tools/bench-compare.py before.json after.json

Prints each measure with the change in percent. Lower is better for
every row. With --fail-over PERCENT the exit code is 1 when the p99
frame time got worse by more than that — the gate a regression check
uses.
"""
import argparse
import json
import sys


def value(result, path):
    node = result['summary']
    for key in path:
        if node is None:
            return None
        node = node.get(key) if isinstance(node, dict) else None
    return node


ROWS = [
    ('frame avg ms', ('frame_avg', 'mean')),
    ('frame median ms', ('frame_median', 'mean')),
    ('frame p95 ms', ('frame_p95', 'mean')),
    ('frame p99 ms', ('frame_p99', 'mean')),
    ('frame max ms', ('frame_max', 'max')),
    ('cpu main p99 ms', ('cpu_main_p99', 'mean')),
    ('gpu p99 ms', ('gpu_p99', 'mean')),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('before')
    parser.add_argument('after')
    parser.add_argument('--fail-over', type=float, default=None)
    args = parser.parse_args()
    a, b = json.load(open(args.before)), json.load(open(args.after))
    if a['scenario'] != b['scenario']:
        print(f"warning: comparing scenario {a['scenario']} with {b['scenario']}", file=sys.stderr)
    ga, gb = a['runs'][0]['gpu'], b['runs'][0]['gpu']
    if ga != gb:
        print(f'warning: different GPUs: {ga} / {gb}', file=sys.stderr)
    print(f"{a['scenario']}: {a['commit']} ({len(a['runs'])} runs) → {b['commit']} ({len(b['runs'])} runs) on {gb}")
    print(f"{'':18}{'before':>10}{'after':>10}{'Δ':>9}")
    for label, path in ROWS:
        x, y = value(a, path), value(b, path)
        if x is None or y is None:
            print(f'{label:18}{"—":>10}{"—":>10}')
            continue
        delta = (y - x) / x * 100 if x else 0.0
        print(f'{label:18}{x:10.2f}{y:10.2f}{delta:+8.1f}%')
    for label, key in (('over 16.7 ms', 'frames_over_16_7'), ('song start ms', 'song_start_ms')):
        print(f"{label:18}{str(a['summary'][key]):>10}  →  {b['summary'][key]}")
    if args.fail_over is not None:
        x, y = value(a, ('frame_p99', 'mean')), value(b, ('frame_p99', 'mean'))
        change = (y - x) / x * 100
        if change > args.fail_over:
            print(f'FAIL: p99 worse by {change:.1f} % (limit {args.fail_over} %)')
            return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
