#!/usr/bin/env bash
# Is beatbyte.celox.io current? Run after publishing a release (and whenever in doubt).
#
# Compares the live page against GitHub:
#   - latest.json names the newest published release whose tag starts with v<digit>
#   - every download target redirects to a file of that version, and the file answers 200
#   - changelog.md is byte-identical to CHANGELOG.md on main
# The VPS timer refreshes every 15 minutes, so a release published minutes ago may still show the
# old version; the timer's own log says why it did not move:
#   ssh root@69.62.121.168 'journalctl -u beatbyte-latest -n 5 --no-pager -o cat'
set -euo pipefail
SITE=https://beatbyte.celox.io
REPO=pepperonas/beatbyte
fail=0

newest=$(gh release list --repo "$REPO" --exclude-drafts --limit 30 --json tagName \
  -q '[.[].tagName | select(test("^v[0-9]"))][0]')
live=$(curl -fsS "$SITE/latest.json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])')
if [[ "$live" == "$newest" ]]; then echo "ok    release   $live"; else echo "STALE release   page $live, GitHub $newest"; fail=1; fi

for target in macos macos-intel windows linux-appimage; do
  loc=$(curl -fsS -o /dev/null -w '%{redirect_url}' "$SITE/download/$target")
  code=$(curl -s -o /dev/null -I -w '%{http_code}' "$loc")
  if [[ "$loc" == *"${newest#v}"* && "$code" == 200 ]]; then echo "ok    download  $target"
  else echo "BAD   download  $target -> $loc ($code)"; fail=1; fi
done

if cmp -s <(curl -fsS "$SITE/changelog.md") <(curl -fsS "https://raw.githubusercontent.com/$REPO/main/CHANGELOG.md"); then
  echo "ok    changelog matches main"
else echo "STALE changelog differs from main"; fail=1; fi
exit $fail
