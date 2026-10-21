"""Add independent grid/phrase diagnostics to finished benchmark results."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json
from bbchart.metrics import grid_compare
for ep in (ROOT/'outputs').glob('full-*/evaluation.json'):
 rows=read_json(ep)
 for row in rows:
  if row['status']!='ok':continue
  refrow=next(r for r in read_json(ROOT/'reports/corpus.json') if r['folder']==row['folder'])
  ref=read_json(refrow['chart']);ours=read_json(row['generated'])
  if not ref.get('audio_trim'):
   trim=ours['audio_trim'];shift=trim['priming_samples']/trim['sample_rate']
   if ref.get('grid'):
    for field in ('beats','downbeats'):ref['grid'][field]=[t-shift for t in ref['grid'].get(field,[])]
  row['grid_comparison']=grid_compare(ours,ref)
  report=read_json(Path(row['generated']).parent/'report.json')
  row['phrase_mapping_reused']=sum(p['reused_mapping'] for p in report['phrases'])
  row['phrase_count']=len(report['phrases'])
 write_json(ep,rows)
