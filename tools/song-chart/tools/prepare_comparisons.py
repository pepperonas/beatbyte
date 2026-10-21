"""Create playable and audible reference/baseline/new comparisons for 8 diagnostic cases."""
import sys,shutil,copy
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json,validate
from bbchart.audio import canonical
from bbchart.preview import preview
def write_case_readme(folder, chart):
 lines=['# '+chart['song']['title'], '', 'Interpret: '+chart['song']['artist'], '',
        'Gleicher 30-Sekunden-Ausschnitt: links Audio, rechts Audio plus Notenklicks. Die Referenz ist der importierte Chart.', '',
        '| Variante | Hörprobe | Notendarstellung | Spielchart |','|---|---|---|---|']
 for kind in ['reference','baseline-mix','baseline-study','generated']:
  p=folder/kind
  lines.append(f'| {kind} | [WAV]({p/"preview.wav"}) | [PNG]({p/"chart-preview.png"}) | [JSON]({p/"chart.json"}) |')
 (folder/'README.md').write_text('\n'.join(lines)+'\n')

corpus={r['folder']:r for r in read_json(ROOT/'reports/corpus.json')};results=[]
for ep in (ROOT/'outputs').glob('full-ml-*/evaluation.json'):results+=read_json(ep)
lines=['# Hör- und Spielvergleiche','',
 'Je Genre sind der zeitlich beste und schwächste erfolgreiche KI-Fall ausgewählt. Die Auswahl zeigt absichtlich die Spannweite, keine repräsentative Durchschnittsqualität. Referenz ist der importierte Chart; die unteren Referenz-Level können automatisch ergänzt worden sein.', '',
 'Jeder Vergleichsordner enthält vollständiges Audio und einen validierten Chart mit vier Schwierigkeiten. Im Spiel lassen sich diese Ordner separat in einer Testbibliothek laden. Eine automatische Registrierung wurde nicht vorgenommen. Alle Hörvorschauen verwenden dasselbe 30-Sekunden-Fenster.', '',
 '| Genre/Fall | Song | KI Expert F1 ±50 ms | Ordner |','|---|---|---:|---|']
count=0
for cat in ['rock','metal','pop','electronic']:
 good=[r for r in results if r['status']=='ok' and r['category']==cat]
 if not good:continue
 good.sort(key=lambda r:r['ours']['expert']['50']['f1'])
 for label,row in [('weakest',good[0]),('strongest',good[-1])]:
  source=Path(row['generated']).parent;ours=read_json(row['generated']);a=read_json(source/'analysis.json');ref=read_json(corpus[row['folder']]['chart']);audio=source/ours['song']['audio']
  if not ref.get('audio_trim'):
   shift=a['audio_trim']['priming_samples']/a['audio_trim']['sample_rate']
   for c in ref['charts']:
    c['notes']=[{**n,'time':n['time']-shift} for n in c['notes'] if n['time']>=shift]
   if ref.get('grid'):
    for field in ['beats','downbeats']:ref['grid'][field]=[t-shift for t in ref['grid'].get(field,[])]
  variants={'reference':ref,'baseline-mix':read_json(source/'baseline-mix.json'),'baseline-study':read_json(source/'baseline-study.json'),'generated':ours}
  dest=ROOT/'outputs/comparisons'/f'{cat}-{label}'
  _,wav,_=canonical(audio)
  for kind,chart in variants.items():
   target=dest/kind;target.mkdir(parents=True,exist_ok=True);chart=copy.deepcopy(chart)
   chart['song']['audio']=audio.name;chart['song']['title']=f'[{kind}] '+ours['song']['title'];chart['song']['artist']=ours['song']['artist'];chart['song']['duration_s']=ours['song']['duration_s'];chart['audio_trim']=a['audio_trim']
   shutil.copy2(audio,target/audio.name)
   write_json(target/'chart.json',chart);validate(target/'chart.json')
   preview(chart,wav,target,start=ours['song'].get('preview_start_s',0),seconds=30)
  write_case_readme(dest,ours)
  lines.append(f"| {cat}/{label} | {row['title']} | {row['ours']['expert']['50']['f1']:.3f} | [Vergleich]({dest/'README.md'}) |")
  count+=1
(ROOT/'reports/listening-cases.md').write_text('\n'.join(lines)+'\n')
print('Comparison cases:',count)
