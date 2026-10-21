from pathlib import Path
import hashlib, json, shutil, subprocess, time
from .common import ROOT, read_json, write_json, config, validate, digest, run_adapter
from .audio import analyze
from .generator import generate
from .preview import preview
from .metrics import compare, stats, grid_compare

def generate_song(audio,out,*,cfg=None,mode='ml',lead=None,bpm=None,first_beat=None,title=None,artist='Unknown',device='auto',demucs_python=None,make_preview=True,progress=None,offline=False):
    update=progress or (lambda phase,fraction:None)
    cfg=cfg or config();audio=Path(audio).resolve();out=Path(out).resolve()
    # Caller must choose a new run directory; never overwrite an existing playable chart.
    if (out/'chart.json').exists():raise FileExistsError(f'Chart existiert bereits: {out}. Neuen --out Ordner wählen.')
    if out==audio.parent:raise ValueError('Ausgabe muss in einem separaten Testordner liegen.')
    existing_audio=out/('song'+audio.suffix.lower())
    if existing_audio.exists() and digest(existing_audio)!=digest(audio):
        raise FileExistsError(f'Ausgabe enthält bereits eine andere Audiodatei: {existing_audio}. Neuen --out Ordner wählen.')
    started=time.monotonic();result,feat=analyze(audio,cfg,mode,lead,bpm,first_beat,device,demucs_python,progress=progress,offline=offline)
    update('patterns',.88)
    chart,report=generate(result,feat,cfg,title or audio.stem,artist,'song'+audio.suffix.lower())
    out.mkdir(parents=True,exist_ok=True);audio_out=out/chart['song']['audio']
    shutil.copy2(audio,audio_out)
    # Write provisional chart, validate, then publish playable chart last.
    update('validating',.95)
    temp=out/'chart.pending.json';write_json(temp,chart)
    try: report['validation']=validate(temp)
    except Exception:
        temp.unlink(missing_ok=True);raise
    report.update({'elapsed_s':round(time.monotonic()-started,3),'source_audio_sha256':result['source_audio_sha256'],
                   'config_sha256':hashlib.sha256(json.dumps(cfg,sort_keys=True).encode()).hexdigest(),
                   'meter':result['meter'],'separation':result['separation'],'tonal_model':result['tonal_model'],
                   'limitations':result['limitations'],
                   'generator_code_sha256':hashlib.sha256(''.join(p.name+digest(p) for p in sorted((ROOT/'bbchart').glob('*.py'))).encode()).hexdigest(),
                   'adapter_sha256':digest(ROOT/'adapter/target/release/bb-chart-adapter')})
    write_json(out/'config.json',cfg);write_json(out/'analysis.json',result);write_json(out/'report.json',report)
    if make_preview:preview(chart,result['canonical_wav'],out)
    temp.rename(out/'chart.json')
    update('done',1.)
    return report

def evaluate(corpus_path,mode='dsp',limit=None,split='all',cfg_path=None,device='auto',tag=None):
    corpus=read_json(corpus_path);cfg=config(cfg_path)
    selected=[r for r in corpus if split=='all' or r['split']==split]
    if limit:selected=selected[:limit]
    tag=tag or f'{mode}-{int(time.time())}'; root=ROOT/'outputs'/tag;root.mkdir(parents=True,exist_ok=True)
    results=[]
    for i,r in enumerate(selected):
        print(f"[{i+1}/{len(selected)}] {mode}: {r['artist']} / {r['title']}",flush=True)
        folder=root/(r['category']+'-'+Path(r['folder']).name)
        row={'folder':r['folder'],'category':r['category'],'split':r['split'],'title':r['title'],'origin':r['origin'],'original_difficulties':r['original_difficulties']}
        try:
            if not (folder/'chart.json').exists():generate_song(r['audio'],folder,cfg=cfg,mode=mode,title=r['title'],artist=r['artist'],device=device,make_preview=(i<4))
            ours=read_json(folder/'chart.json');analysis=read_json(folder/'analysis.json');ref=read_json(r['chart'])
            # Legacy charts predate trim: move reference once to the canonical axis in memory.
            if not ref.get('audio_trim'):
                shift=analysis['audio_trim']['priming_samples']/analysis['audio_trim']['sample_rate']
                for c in ref['charts']:
                    for n in c['notes']:n['time']-=shift
                if ref.get('grid'):
                    for field in ('beats','downbeats'):ref['grid'][field]=[t-shift for t in ref['grid'].get(field,[])]
                row['legacy_reference_shift_s']=-shift
            baseline_path=folder/'baseline-mix.json'
            if not baseline_path.exists():write_json(baseline_path,run_adapter('baseline',analysis['canonical_wav']))
            study_path=folder/'baseline-study.json'
            mixpath=Path(analysis['cache'])/f'mix-{mode}.json'
            if not study_path.exists():write_json(study_path,run_adapter('study',analysis['lead_wav'],mixpath))
            baseline=read_json(baseline_path);study=read_json(study_path)
            row.update({'status':'ok','generated':str(folder/'chart.json'),'ours':compare(ours,ref),'baseline_mix':compare(baseline,ref),'baseline_study':compare(study,ref),
                        'stats':read_json(folder/'report.json')['stats'],'reference_stats':r['stats'],'grid_comparison':grid_compare(ours,ref),
                        'human_playtest':'not_performed','elapsed_s':read_json(folder/'report.json')['elapsed_s']})
            # Onset score measures rhythmic proximity, not exact fret identity or subjective quality.
        except Exception as e:row.update({'status':'error','error':str(e)})
        results.append(row);write_json(root/'evaluation.json',results)
    from collections import defaultdict
    import numpy as np
    groups=defaultdict(list)
    for r in results:
        if r['status']=='ok':groups[(r['category'],r['split'])].append(r)
    lines=['# Vergleich gegen Referenzcharts','', 'Zeitliche Ereignistreffer, Makromittel pro Song bei ±50 ms. Keine Aussage über exakte Griffidentität oder Spielspaß.', '',
           '| Genre | Split | n | Neuer Generator P/R/F1 | Mix F1 | Lead-Study F1 |','|---|---|---:|---|---:|---:|']
    for (cat,sp),rs in sorted(groups.items()):
        means=lambda method,key:np.mean([r[method].get('expert',{}).get('50',{}).get(key,0) for r in rs])
        lines.append(f"| {cat} | {sp} | {len(rs)} | {means('ours','precision'):.3f}/{means('ours','recall'):.3f}/{means('ours','f1'):.3f} | {means('baseline_mix','f1'):.3f} | {means('baseline_study','f1'):.3f} |")
    lines.extend(['',f"Erfolgreich: {sum(r['status']=='ok' for r in results)} / {len(results)}.", '', 'Unverifizierte untere Schwierigkeiten werden separat als Importvergleich berichtet und nicht als menschliche Ground Truth zur Kalibrierung verwendet.', '', 'Abweichende Chart-Stile und Instrumentenwahl können korrekte musikalische Alternativen bestrafen. Die Referenz ist kein vollständiges Transkript. Kein menschlicher Spieltest durchgeführt.'])
    (root/'comparison.md').write_text('\n'.join(lines)+'\n')
    write_json(root/'run-config.json',cfg)
    return {'root':str(root),'songs':len(results),'ok':sum(r['status']=='ok' for r in results)}
