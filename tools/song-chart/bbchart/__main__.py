import argparse,json,sys
from pathlib import Path
from .common import ROOT, config, write_json

def main():
    parser=argparse.ArgumentParser(description='Beatbyte Audio → Guitar-Hero-Stil Charts (Forschungsprototyp)')
    sub=parser.add_subparsers(dest='command',required=True)
    scan=sub.add_parser('scan');scan.add_argument('library',type=Path);scan.add_argument('--out',type=Path)
    for name in ('analyze','generate'):
        p=sub.add_parser(name);p.add_argument('audio',type=Path);p.add_argument('--out',type=Path,required=True);p.add_argument('--mode',choices=['ml','dsp'],default='ml')
        p.add_argument('--lead',type=Path);p.add_argument('--bpm',type=float);p.add_argument('--first-beat',type=float);p.add_argument('--config',type=Path)
        p.add_argument('--device',choices=['auto','cpu','mps'],default='auto');p.add_argument('--demucs-python');p.add_argument('--separator',choices=['htdemucs','htdemucs_6s'])
        if name=='generate':p.add_argument('--title');p.add_argument('--artist',default='Unknown');p.add_argument('--no-preview',action='store_true')
    e=sub.add_parser('evaluate');e.add_argument('--corpus',type=Path,default=ROOT/'reports/corpus.json');e.add_argument('--mode',choices=['ml','dsp'],default='dsp');e.add_argument('--limit',type=int);e.add_argument('--split',choices=['all','development','holdout'],default='all');e.add_argument('--config',type=Path);e.add_argument('--device',choices=['auto','cpu','mps'],default='auto');e.add_argument('--tag')
    sub.add_parser('models')
    a=parser.parse_args()
    try:
        if a.command=='scan':
            from .inventory import scan
            result=scan(a.library,a.out)
        elif a.command=='models':
            from .audio import ensure_models
            ensure_models();result={'models':'verified in '+str(ROOT/'models')}
        elif a.command=='evaluate':
            from .pipeline import evaluate
            result=evaluate(a.corpus,a.mode,a.limit,a.split,a.config,a.device,a.tag)
        else:
            cfg=config(a.config)
            if a.separator:cfg['separator']=a.separator
            if a.command=='generate':
                from .pipeline import generate_song
                result=generate_song(a.audio,a.out,cfg=cfg,mode=a.mode,lead=a.lead,bpm=a.bpm,first_beat=a.first_beat,title=a.title,artist=a.artist,device=a.device,demucs_python=a.demucs_python,make_preview=not a.no_preview)
            else:
                from .audio import analyze
                result,_=analyze(a.audio,cfg,a.mode,a.lead,a.bpm,a.first_beat,a.device,a.demucs_python)
                a.out.mkdir(parents=True,exist_ok=True);write_json(a.out/'analysis.json',result)
        print(json.dumps(result,ensure_ascii=False,indent=2))
    except Exception as exc:print(f'bbchart: {exc}',file=sys.stderr);return 1
    return 0
if __name__=='__main__':sys.exit(main())
