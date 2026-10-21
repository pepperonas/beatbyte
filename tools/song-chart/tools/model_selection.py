"""Compare both separators on 60s development excerpts; never reads holdout audio."""
from pathlib import Path
import sys,copy
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import soundfile as sf
import numpy as np
from bbchart.common import ROOT,read_json,write_json,config
from bbchart.audio import canonical
from bbchart.pipeline import generate_song
from bbchart.metrics import compare
corpus=read_json(ROOT/'reports/corpus.json');selected=[]
for cat in ['rock','metal','pop','electronic']:
 selected += [r for r in corpus if r['category']==cat and r['split']=='development'][:2]
results=[]
for i,r in enumerate(selected):
 print(f"Model selection {i+1}/{len(selected)}: {r['title']}",flush=True)
 folder,wav,decoded=canonical(r['audio']);start=min(30,max(0,decoded['duration_s']-61));info=sf.info(wav)
 clip=folder/'development-60s.wav'
 if not clip.exists():
  x,sr=sf.read(wav,start=round(start*info.samplerate),frames=60*info.samplerate);sf.write(clip,x,sr,subtype='PCM_16')
 ref=read_json(r['chart']);shift=0 if ref.get('audio_trim') else decoded['audio_trim']['priming_samples']/decoded['audio_trim']['sample_rate']
 for c in ref['charts']:c['notes']=[{**n,'time':n['time']-shift-start} for n in c['notes'] if start+shift<=n['time']<start+60+shift]
 row={'song':r['title'],'category':r['category'],'split':'development','window':[start,start+60]}
 for model in ['htdemucs','htdemucs_6s']:
  cfg=config();cfg['separator']=model;out=ROOT/'outputs/model-selection-seg5'/f'{i:02}-{model}'
  try:
   if not (out/'chart.json').exists():generate_song(clip,out,cfg=cfg,title=r['title'],artist=r['artist'],make_preview=False)
   row[model]=compare(read_json(out/'chart.json'),ref)['expert']['50']
  except Exception as e:row[model]={'error':str(e)}
 results.append(row);write_json(ROOT/'reports/model-selection.json',results)
# Compare matched successful pairs. Require material margin before replacing shipped default.
pairs=[r for r in results if all('f1' in r.get(m,{}) for m in ['htdemucs','htdemucs_6s'])]
means={m:float(np.mean([r[m]['f1'] for r in pairs])) if pairs else None for m in ['htdemucs','htdemucs_6s']}
chosen='htdemucs_6s' if len(pairs)>=4 and means['htdemucs_6s']>means['htdemucs']+.02 else 'htdemucs'
summary={'development_excerpts':len(selected),'matched_pairs':len(pairs),'macro_expert_f1_50ms':means,'chosen':chosen,
 'policy':'6s replaces default only with >=4 matched development clips and >0.02 absolute macro-F1 gain. Timing proxy only; no subjective guitar-isolation conclusion.'}
write_json(ROOT/'reports/model-selection-summary.json',summary)
cfg=config(ROOT/'config/calibrated.json');cfg['separator']=chosen;write_json(ROOT/'config/frozen.json',cfg)
print(summary,flush=True)
