"""Render missing preview artifacts for full-song benchmark outputs."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json
from bbchart.preview import preview
count=0
for cp in sorted((ROOT/'outputs').glob('full-*/*/chart.json')):
 if not (cp.parent/'preview.wav').exists() or not (cp.parent/'chart-preview.png').exists():
  analysis=read_json(cp.parent/'analysis.json')
  preview(read_json(cp),analysis['canonical_wav'],cp.parent);count+=1
print('Additional previews:',count)
