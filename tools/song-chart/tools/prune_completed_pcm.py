"""Reclaim only reproducible PCM for successfully benchmarked full ML songs.
All charts, original audio copies, compressed instrument stems and analysis JSON stay.
"""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json
from bbchart.preview import preview
reclaimed=0;removed=[]
for ep in (ROOT/'outputs').glob('full-ml-*/evaluation.json'):
 for row in read_json(ep):
  if row['status']!='ok':continue
  cp=Path(row['generated']);analysis=read_json(cp.parent/'analysis.json')
  canonical=Path(analysis['canonical_wav'])
  if not (cp.parent/'preview.wav').exists() or not (cp.parent/'chart-preview.png').exists():
   if canonical.exists():preview(read_json(cp),canonical,cp.parent)
   else:continue
  for key in ('canonical_wav','lead_wav'):
   p=Path(analysis[key])
   if p.is_file() and p.resolve().is_relative_to(ROOT/'cache'):
    size=p.stat().st_size;p.unlink();reclaimed+=size;removed.append({'path':str(p),'bytes':size})
p=ROOT/'reports/pruned-pcm.json';old=read_json(p) if p.exists() else [];write_json(p,old+removed)
print({'reclaimed_mb':round(reclaimed/1e6,1),'files':len(removed)})
