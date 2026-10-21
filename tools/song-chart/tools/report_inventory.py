"""Derive documented corpus facts and development-only difficulty priors."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json,config
import numpy as np
from collections import Counter
rows=read_json(ROOT/'reports/inventory.json');corpus=read_json(ROOT/'reports/corpus.json');summary=read_json(ROOT/'reports/inventory-summary.json')
lines=['# Bibliothek und Referenzkorpus','',f"{summary['readable']} lesbare Songordner, {summary['unique_song_groups']} konservativ gruppierte Songs. Fehler: {summary['errors']}.", '',
       '## Herkunft und Grenzen','', 'BG bezeichnet Community-Importe; GR bezeichnet Game-Rips. Beides ist zunächst eine Herkunftsangabe, kein Nachweis, dass jede enthaltene Schwierigkeit von Hand erstellt wurde.', '',
       'Der Importer ergänzt fehlende Stufen mit `fill_missing_levels` und der Classic-Ladder. Ohne Quelldatei lässt sich die ursprüngliche Levelausstattung nicht sicher rekonstruieren. Entsprechende Levels bleiben unverifiziert.', '',
       '| Kategorie | Ordner |','|---|---:|']
for k,v in sorted(summary['categories'].items()):lines.append(f'| {k} | {v} |')
lines+=['','| Herkunftsnachweis | Ordner |','|---|---:|']
for k,v in sorted(summary['origins'].items()):lines.append(f'| {k} | {v} |')
lines+=['','## Gemessene Expert-Eigenschaften','', '| Genre | n | Median Ereignisse/s | Median Akkordanteil | Median Sustain-Anteil |','|---|---:|---:|---:|---:|']
for cat in ['rock','metal','pop','electronic']:
 rs=[r['stats']['expert'] for r in rows if r['category']==cat and 'expert' in r['stats']]
 if rs:lines.append(f"| {cat} | {len(rs)} | {np.median([s['events_per_second'] for s in rs]):.2f} | {np.median([s['chord_share'] for s in rs]):.2%} | {np.median([s['sustain_share'] for s in rs]):.2%} |")
lines+=['','Diese Bestandsstatistik ist beschreibend; sie wird nicht zum Optimieren auf zurückgehaltenen Prüfsongs verwendet.', '', '## Aufteilung', '', 'Je Genre zwölf Aufnahmen, davon acht Entwicklung und vier Holdout. Namensvarianten und identische Audio-Hashes sind gruppiert; vollständige Dublettenerkennung über verschiedene Titel und neu codierte Aufnahmen ist nicht garantiert. Alle bekannten Varianten bleiben in einer Gruppe. Die Auswahl ist deterministisch.', '', 'Details: `corpus.csv`, `corpus.json`, `inventory.json`, `inventory-errors.json`.']
(ROOT/'reports/corpus-analysis.md').write_text('\n'.join(lines)+'\n')
# Only measured transitions between verified original levels can change keep ratios.
cfg=config();calibration={'source':'development split only','transitions':{},'defaults_kept':[]}
for low,high in [('hard','expert'),('medium','hard'),('easy','medium')]:
 ratios=[]
 for r in corpus:
  levels=r['original_difficulties'] or []
  if r['split']=='development' and low in levels and high in levels and r['stats'][high]['events']:
   ratios.append(r['stats'][low]['events']/r['stats'][high]['events'])
 if len(ratios)>=5:
  cfg['profiles'][low]['keep']=float(np.clip(np.median(ratios),.25,.95));calibration['transitions'][low]={'n':len(ratios),'median_keep':cfg['profiles'][low]['keep']}
 else:calibration['defaults_kept'].append({'level':low,'verified_pairs':len(ratios),'reason':'fewer than five verified original development pairs; explicit game-design prior retained'})
write_json(ROOT/'config/calibrated.json',cfg);write_json(ROOT/'reports/calibration.json',calibration)
print(summary);print(calibration)
