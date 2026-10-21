"""Independent checks on every final generated chart (not baseline JSON)."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json,validate
from bbchart.metrics import events
import soundfile as sf
from PIL import Image
checks=[]
for p in sorted((ROOT/'outputs').glob('**/chart.json')):
 c=read_json(p);levels=c['charts'];errors=[]
 generated_here=p.parent.name not in {'reference','baseline-mix','baseline-study'}
 if generated_here:
  times=[{e['time'] for e in events(l['notes'])} for l in levels]
  for i in range(3):
   if not times[i]<=times[i+1]:errors.append('difficulty event times are not nested')
  for l in levels:
   ev=events(l['notes']);seen=set()
   for n in l['notes']:
    key=(n['time'],n['lane'])
    if key in seen:errors.append('duplicate lane event')
    seen.add(key)
    if n['time']<0 or n['time']+n.get('len',0)>c['song']['duration_s']+.001:errors.append('note outside audio')
   for a,b in zip(ev,ev[1:]):
    if a['time']+a['len']>b['time']+1e-5:errors.append('sustain overlaps next event')
  if any(n['lane']>=3 for n in levels[0]['notes']):errors.append('Easy uses more than 3 lanes')
  if any(len(e['lanes'])>1 for e in events(levels[0]['notes'])):errors.append('Easy contains chord')
 if not (p.parent/c['song']['audio']).is_file():errors.append('missing audio')
 if (p.parent/'preview.wav').exists():
  try:
   info=sf.info(p.parent/'preview.wav')
   if info.channels!=2 or info.frames<=0:errors.append('invalid stereo preview')
   data,_=sf.read(p.parent/'preview.wav')
   if len(data)!=info.frames:errors.append('truncated preview')
  except Exception as e:errors.append('preview: '+str(e))
 if (p.parent/'chart-preview.png').exists():
  try:
   with Image.open(p.parent/'chart-preview.png') as im:im.verify()
  except Exception as e:errors.append('image: '+str(e))
 try:validate(p)
 except Exception as e:errors.append(str(e))
 checks.append({'chart':str(p),'generated_here':generated_here,'errors':errors})
write_json(ROOT/'reports/output-verification.json',checks)
print({'charts':len(checks),'failed':sum(bool(c['errors']) for c in checks)})
if any(c['errors'] for c in checks):sys.exit(1)
