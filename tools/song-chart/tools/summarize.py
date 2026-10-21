"""Consolidate every benchmark result, including failures, without changing model choices."""
import sys,json,platform,subprocess,re
from pathlib import Path
from collections import defaultdict
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json,digest

def read(path,default):return read_json(path) if Path(path).exists() else default
allruns={}
for name,paths in {'DSP':[ROOT/'outputs/full-dsp/evaluation.json'],'ML':[ROOT/f'outputs/full-ml-{k}/evaluation.json' for k in range(2)]}.items():
 rs=[]
 for p in paths:rs+=read(p,[])
 allruns[name]=rs
write_json(ROOT/'reports/full-ml-evaluation.json',allruns['ML'])
lines=['# Gemessene Ergebnisse','', 'Automatisierter Forschungsprototyp; ein menschlicher Hör-/Spieltest wurde nicht durchgeführt.', '',
       '## Vollständige Songs', '', 'Expert-Ereignisvergleich bei ±50 ms; Makromittel pro erfolgreichem Song. P = Precision, R = Recall. Fehler werden zusätzlich ausgewiesen.', '',
       '| Modus | Genre | Split | erfolgreich / versucht | P | R | F1 | Mix F1 | Lead-Study F1 |','|---|---|---|---:|---:|---:|---:|---:|']
for name,rs in allruns.items():
 groups=defaultdict(list)
 for r in rs:groups[(r['category'],r['split'])].append(r)
 for (cat,split),rows in sorted(groups.items()):
  good=[r for r in rows if r['status']=='ok']
  def mean(method,key):return float(np.mean([r[method].get('expert',{}).get('50',{}).get(key,0) for r in good])) if good else 0
  lines.append(f"| {name} | {cat} | {split} | {len(good)}/{len(rows)} | {mean('ours','precision'):.3f} | {mean('ours','recall'):.3f} | {mean('ours','f1'):.3f} | {mean('baseline_mix','f1'):.3f} | {mean('baseline_study','f1'):.3f} |")
lines+=['', ('Beide vollständigen Durchläufe sind abgeschlossen: je 48 Songs, zusammen 96 Ausgaben.' if all(len(rs)==48 for rs in allruns.values()) else 'Zwischenstand: Der vollständige Benchmark umfasst 48 Songs pro Modus.'), '',
        '## Gesamtergebnis', '']
summary={}
for name,rs in allruns.items():
 good=[r for r in rs if r['status']=='ok'];errors=[r for r in rs if r['status']!='ok']
 get=lambda method:[r[method].get('expert',{}).get('50',{}).get('f1',0) for r in good]
 vals={m:float(np.mean(get(m))) if good else None for m in ['ours','baseline_mix','baseline_study']}
 summary[name]={'attempted':len(rs),'successful':len(good),'errors':len(errors),'macro_expert_f1_50ms':vals,
                'median_generation_s':float(np.median([r['elapsed_s'] for r in good])) if good else None,
                'failure_penalized_f1':sum(get('ours'))/48}
 lines.append(f"- {name}: {len(good)}/{len(rs)} erfolgreich; Expert-F1 ±50 ms: neuer Generator {vals['ours']:.3f}, Mix-Baseline {vals['baseline_mix']:.3f}, Lead-Study {vals['baseline_study']:.3f}. Fehlgeschlagene oder fehlende Songs als 0 gewertet: {summary[name]['failure_penalized_f1']:.3f}." if good else f"- {name}: noch keine erfolgreichen Ergebnisse.")
 if good:
  delta=[r['ours']['expert']['50']['f1']-r['baseline_mix']['expert']['50']['f1'] for r in good]
  wins=sum(x>0 for x in delta);lines.append(f"  Gegen Mix-Baseline: {wins}/{len(good)} Songs mit höherem F1. Median der Generationszeit: {summary[name]['median_generation_s']:.1f} s (ggf. Caches; Benchmark-/Baselinezeit zusätzlich).")
 if errors:
  lines.append('');lines.append('Fehler:')
  for r in errors:lines.append(f"- {r['title']}: {r.get('error','unbekannt')[:500]}")
if summary['ML']['attempted']==48:
 ml=summary['ML']['macro_expert_f1_50ms']
 if ml['ours'] is not None and ml['ours']<ml['baseline_mix']:
  lines+=['', 'Der KI-Prototyp ist im gesamten zeitlichen Referenzvergleich schwächer als der vorhandene Mix-Generator. Ein musikalischer Qualitätsgewinn und ein Produktionsersatz sind damit nicht nachgewiesen. Die Ergebnisse sind ein belastbarer Ausgangspunkt für Instrumenten-/Anschlagsverbesserungen und den vorbereiteten Hör-/Spielvergleich.']
verified=read(ROOT/'reports/output-verification.json',[])
if verified:
 lines+=['', '## Technische Prüfung', '', f"{len(verified)} Chartdateien einschließlich Referenzen und Vergleichsfassungen geprüft; {sum(bool(v['errors']) for v in verified)} fehlerhaft. Native Chartvalidierung sowie Integrität vorhandener WAV-/PNG-Vorschauen sind enthalten."]
 tests=(ROOT/'reports/tests.log').read_text() if (ROOT/'reports/tests.log').exists() else ''
 match=re.search(r'Ran (\d+) tests',tests)
 if match and '\nOK' in tests:lines.append(f"{match.group(1)} gezielte Tests bestanden. Der CLI-Kontrolllauf ist in `reports/cli-smoke.json` dokumentiert.")
lines+=['', "[Direkte Hör- und Spielvergleiche](listening-cases.md)", '']
write_json(ROOT/'reports/summary.json',summary)
selection=read(ROOT/'reports/model-selection-summary.json',{})
lines+=['', '## Trennmodell-Auswahl','', '```json',json.dumps(selection,ensure_ascii=False,indent=2),'```', '',
        '## Interpretation','',
        'Diese Zahlen prüfen zeitliche Übereinstimmung mit vorhandenen importierten Charts. Sie beweisen weder richtige Gitarrenisolation noch gleichwertiges Spielgefühl. Ein hoher Recall bei geringer Precision bedeutet viele zusätzliche Ereignisse; ein geringer Recall kann fehlende Instrumentenbelege oder eine abweichende musikalische Stimme anzeigen.', '',
        'Die Herkunft der unteren Schwierigkeitsstufen ist überwiegend unbekannt. Dafür werden keine behaupteten menschlichen Trainingslabels verwendet. `reports/calibration.json` dokumentiert die tatsächlich mögliche Kalibrierung.', '',
        'Besteht kein klarer Vorteil gegenüber beiden vorhandenen Generatoren auf dem Holdout, ist das Experiment kein nachgewiesener Ersatz. Nächste Änderungen müssen anhand der dokumentierten Fehlertypen entwickelt und auf neuen Prüfsongs validiert werden.', '',
        '## Direkt ausprobieren','',
        '- `outputs/pilot-duvet/chart.json`, `preview.wav`, `chart-preview.png`',
        '- `outputs/pilot-duvet-6s/chart.json`: experimenteller Gitarren-Trennpfad',
        '- `outputs/pilot-boilermaker/chart.json`: Halbtempo-Abweichung zwischen DSP und KI, auf explizitem KI-Raster verarbeitet', '',
        'Alle Songs bleiben im Testordner. Das Originalprojekt und die USB-Quelldateien wurden nicht geändert.']
(ROOT/'reports/results.md').write_text('\n'.join(lines)+'\n')
env={'python':sys.version,'platform':platform.platform(),'beatbyte_revision':(ROOT/'reports/beatbyte-revision.txt').read_text().strip(),
     'adapter_sha256':digest(ROOT/'adapter/target/release/bb-chart-adapter'),'config':read(ROOT/'config/frozen.json',{}),
     'human_playtest':'not_performed'}
write_json(ROOT/'reports/environment.json',env)
print(summary)
