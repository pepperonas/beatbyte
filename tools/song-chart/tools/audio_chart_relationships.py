"""Describe relationships between transcribed instrument notes and imported Expert frets.
Diagnostic only: these held-out summaries never update generator configuration.
"""
import sys
from pathlib import Path
from collections import defaultdict
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json
from bbchart.metrics import events
from bbchart.generator import beat_position
corpus={r['folder']:r for r in read_json(ROOT/'reports/corpus.json')};records=[]
for ep in (ROOT/'outputs').glob('full-ml-*/evaluation.json'):
 for row in read_json(ep):
  if row['status']!='ok':continue
  src=corpus[row['folder']];analysis=read_json(Path(row['generated']).parent/'analysis.json');ref=read_json(src['chart'])
  expert=next(c for c in ref['charts'] if c['difficulty']=='expert');ev=events(expert['notes'])
  if not ref.get('audio_trim'):
   shift=analysis['audio_trim']['priming_samples']/analysis['audio_trim']['sample_rate']
   for e in ev:e['time']-=shift
  pg=[]
  for p in sorted(analysis['pitches'],key=lambda p:p['time_s']):
   if pg and p['time_s']-pg[-1]['time']<=.035:
    pg[-1]['pitches'].add(round(p['midi']));pg[-1]['end']=min(pg[-1]['end'],p['end_s'])
   else:pg.append({'time':p['time_s'],'pitches':{round(p['midi'])},'end':p['end_s']})
  pairs=[];i=j=0
  while i<len(ev) and j<len(pg):
   if ev[i]['time']<pg[j]['time']-.08:i+=1
   elif pg[j]['time']<ev[i]['time']-.08:j+=1
   else:pairs.append((ev[i],pg[j]));i+=1;j+=1
  mono=[(e,p) for e,p in pairs if len(e['lanes'])==len(p['pitches'])==1]
  contour=same=contour_n=same_n=0
  for (a,pa),(b,pb) in zip(mono,mono[1:]):
   if b['time']-a['time']>1:continue
   dp=next(iter(pb['pitches']))-next(iter(pa['pitches']));dl=b['lanes'][0]-a['lanes'][0]
   if dp==0:same_n+=1;same+=dl==0
   elif dl!=0:contour_n+=1;contour+=np.sign(dp)==np.sign(dl)
  chords=[(e,p) for e,p in pairs if len(e['lanes'])>1]
  sustain=[abs(e['len']-(p['end']-p['time'])) for e,p in pairs if e['len']>.1]
  beats=ref.get('grid',{}).get('beats',[]);bpm=ref['song']['bpm'];phases=defaultdict(int)
  for e in ev:
   phase=beat_position(e['time'],beats,bpm)%1
   if min(phase,1-phase)<.06:phases['beat']+=1
   elif abs(phase-.5)<.06:phases['eighth']+=1
   elif min(abs(phase-1/3),abs(phase-2/3))<.045:phases['triplet']+=1
   else:phases['finer_or_offgrid']+=1
  records.append({'title':row['title'],'category':row['category'],'split':row['split'],'reference_events':len(ev),'matched_transcribed_onsets_80ms':len(pairs),
     'monophonic_pairs':len(mono),'contour_agree':int(contour),'contour_comparisons':contour_n,'same_pitch_same_fret':same,'same_pitch_comparisons':same_n,
     'matched_reference_chords':len(chords),'with_multiple_transcribed_pitches':sum(len(p['pitches'])>1 for e,p in chords),
     'sustain_duration_median_absolute_difference_s':float(np.median(sustain)) if sustain else None,'reference_beat_positions':dict(phases)})
write_json(ROOT/'reports/audio-chart-relationships.json',records)
lines=['# Beziehung zwischen Audio und Referenznoten','',
 'Deskriptiver Vergleich nach eingefrorener Generierung. Transkribierte Instrumentenanschläge und Referenz-Ereignisse werden bei ±80 ms eins zu eins gepaart. Dies bewertet die gewählte Instrumentenspur und das Tonhöhenmodell gemeinsam; es sind keine verifizierten Tonhöhenlabels.', '',
 '| Genre | Songs | Mono-Kontur stimmt | Gleiche Tonhöhe → gleicher Griff | Referenzakkorde mit Mehrton-Beleg |','|---|---:|---:|---:|---:|']
for cat in ['rock','metal','pop','electronic']:
 rs=[r for r in records if r['category']==cat]
 def ratio(a,b):
  n=sum(r[b] for r in rs);return f'{sum(r[a] for r in rs)/n:.1%} (n={n})' if n else 'keine Paare'
 lines.append(f"| {cat} | {len(rs)} | {ratio('contour_agree','contour_comparisons')} | {ratio('same_pitch_same_fret','same_pitch_comparisons')} | {ratio('with_multiple_transcribed_pitches','matched_reference_chords')} |")
lines+=['','Kontur wird nur bei gepaarten monophonen Tönen, höchstens 1 s Abstand und tatsächlich wechselndem Griff ausgewertet. Verschiedene Tonhöhen mit gleichem Spielgriff gehen nicht in die Richtungsquote ein. Diese Auswahl kann die Quote erhöhen; sie beschreibt die verbleibenden Fälle, keine Gesamtgenauigkeit.', '',
        'Gleiche Tonhöhen können in menschlichen Charts bewusst unterschiedliche Griffe bekommen. Umgekehrt kann ein Spielakkord einen musikalischen Akzent abstrahieren. Der Generator nutzt deshalb Kontur- und Wiederholungsregeln als Gestaltungsprior, keine behauptete exakte Tonhöhe-zu-Taste-Funktion.', '',
        'Akkord-Beleg bedeutet lediglich mehrere nahe beginnende transkribierte Tonhöhen in der ausgewählten Spur. Trennartefakte, Bass-/Keyboard-Leakage oder fehlende Gitarrenanteile können den Wert verändern. Sustain-Unterschiede vergleichen hörbare Modellhaltedauer mit der bewusst spielerisch verkürzten Referenz; sie sind keine unmittelbare Modellfehlerquote.', '',
        'Die Detaildatei enthält außerdem die Referenzverteilung auf Beats, Achtel, Triolen und feinere Positionen. Damit lässt sich erkennen, warum ein starres Viertel-/Achtelraster schnelle / synkopierte Songs nicht abdeckt. Diese Auswertung ändert keine eingefrorenen Parameter.']
(ROOT/'reports/audio-chart-relationships.md').write_text('\n'.join(lines)+'\n')
print('Audio/chart relationship diagnostics:',len(records),'songs')
