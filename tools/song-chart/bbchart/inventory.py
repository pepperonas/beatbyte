from pathlib import Path
from collections import Counter
import csv, hashlib, json, re
from .common import ROOT, read_json, write_json, run_adapter
from .metrics import stats

def clean(s): return re.sub(r'[^\w]+',' ',re.sub(r'^\[[^]]+\]\s*','',s).casefold()).strip()
def category(genres):
    s=' '.join(genres).lower()
    if any(x in s for x in ['metal','deathcore','djent','hardcore']): return 'metal'
    if any(x in s for x in ['house','techno','electro','dance','disco','trance','dubstep','drum and bass','edm','synth']): return 'electronic'
    if any(x in s for x in ['rock','punk','grunge']): return 'rock'
    if any(x in s for x in ['pop','r&b','soul','funk']): return 'pop'
    return 'other'

def scan(library, out=None):
    library=Path(library); out=Path(out or ROOT/'reports');out.mkdir(parents=True,exist_ok=True)
    rows=[]; errors=[]; raw_sources={}
    for folder in sorted((library.parent/'charts-bridge').glob('*')):
        if folder.is_dir():
            p=next(iter(folder.glob('notes.chart')),None) or next(iter(folder.glob('notes.mid')),None)
            if p:
                try: raw_sources[folder.name]=run_adapter('source-info',p)
                except Exception as e: errors.append({'folder':str(folder),'error':str(e)})
    folders=sorted(d for d in library.iterdir() if d.is_dir() and not d.name.startswith('.'))
    for i,d in enumerate(folders):
        try:
            song=read_json(d/'song.json') if (d/'song.json').exists() else {}
            active='chart.json'
            if (d/'chart-active.json').exists():
                target=read_json(d/'chart-active.json').get('active','')
                if re.fullmatch(r'chart(?:\.v[1-9][0-9]*)?\.json',target) and (d/target).is_file(): active=target
            chart=read_json(d/active); meta=chart['song']; source=read_json(d/'bridge-source.json') if (d/'bridge-source.json').exists() else {}
            genres=[g.get('value','') for g in song.get('descriptive',{}).get('genres',[])] or [meta.get('genre','')]
            dur=meta.get('duration_s') or song.get('musical',{}).get('duration_s',0) or 0
            title=meta['title'];artist=meta.get('artist','Unknown')
            # Group every rendition of this named song, also live/remastered suffix variants conservatively.
            group=clean(artist)+'|'+clean(re.sub(r'\([^)]*\)|\[[^]]*\]','',title))
            originals=raw_sources.get(Path(source.get('source','')).name,{}).get('original_difficulties')
            if originals is None:
                sp=Path(source.get('source','/nonexistent'))
                raw=sp/'notes.chart' if (sp/'notes.chart').is_file() else sp/'notes.mid'
                if raw.is_file():
                    try: originals=run_adapter('source-info',raw)['original_difficulties']
                    except Exception: pass
            provenance=chart.get('provenance')
            origin='edited_or_generated' if provenance else ('original_levels_verified' if originals else 'imported_levels_unverified')
            row={'folder':str(d),'chart':str(d/active),'audio':str(d/meta['audio']), 'title':title,'artist':artist,'duration_s':dur,'genres':genres,'category':category(genres),
                 'group':group,'content_hash':song.get('file',{}).get('content_hash'), 'charter':source.get('charter'),'source_format':source.get('format'),
                 'origin':origin,'original_difficulties':originals,'audio_trim':chart.get('audio_trim'),'provenance':provenance,
                 'has_grid':bool(chart.get('grid',{}).get('beats')), 'stats':{c['difficulty']:stats(c,dur) for c in chart['charts']}}
            rows.append(row)
        except Exception as e: errors.append({'folder':str(d),'error':str(e)})
        if (i+1)%500==0: print(f'Inventar: {i+1}/{len(folders)}',flush=True)
    # Audio hashes connect differently named copies, in addition to conservative song identity.
    hash_groups={}
    for r in rows:
        if r['content_hash']: hash_groups.setdefault(r['content_hash'],set()).add(r['group'])
    for r in rows:
        if r['content_hash']:r['group']=min(hash_groups[r['content_hash']])
    write_json(out/'inventory.json',rows);write_json(out/'inventory-errors.json',errors)
    selected=[]; used=set(); cats=['rock','metal','pop','electronic']
    for cat in cats:
        candidates=[r for r in rows if r['category']==cat and 90<=r['duration_s']<=480 and 'expert' in r['stats'] and r['stats']['expert']['events']>80 and r['origin']!='edited_or_generated' and Path(r['audio']).is_file()]
        candidates.sort(key=lambda r:(not bool(r['original_difficulties']),hashlib.sha256(r['group'].encode()).hexdigest(),r['folder']))
        group_rows=[]
        for r in candidates:
            if r['group'] in used:continue
            used.add(r['group']);group_rows.append(r)
            if len(group_rows)==12:break
        for i,r in enumerate(group_rows): selected.append({**r,'split':'development' if i<8 else 'holdout'})
    write_json(out/'corpus.json',selected)
    summary={'folders':len(folders),'readable':len(rows),'errors':len(errors),'unique_song_groups':len(set(r['group'] for r in rows)),
             'categories':dict(Counter(r['category'] for r in rows)),'origins':dict(Counter(r['origin'] for r in rows)),
             'corpus':dict(Counter(r['category']+'/'+r['split'] for r in selected)),
             'limitation':'Difficulty origin is unknown unless original source tracks were available. Unverified lower levels are not human ground truth.'}
    write_json(out/'inventory-summary.json',summary)
    with (out/'corpus.csv').open('w') as f:
        w=csv.DictWriter(f,fieldnames=['category','split','artist','title','duration_s','origin','folder']);w.writeheader();w.writerows({k:r[k] for k in w.fieldnames} for r in selected)
    return summary
