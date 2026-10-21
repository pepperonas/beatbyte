from pathlib import Path
import hashlib, os, shutil, subprocess, sys, fcntl
import numpy as np
import soundfile as sf
from scipy.signal import find_peaks, stft
from .common import ROOT, REPO_ROOT, digest, read_json, write_json, run_adapter

def ensure_models(allow_downloads=True):
    import re
    registry=(REPO_ROOT/'crates/beatbyte-ml/src/registry.rs').read_text()
    existing=Path.home()/'Library/Application Support/beatbyte/models'
    for model in ('beat-this','beat-this-mel','basic-pitch'):
        folder=ROOT/'models'/model; dest=folder/'model.onnx';src=existing/model/'model.onnx'
        specs=re.findall(r'id: "([^"]+)".*?sha256: "([^"]+)"',registry,re.S)
        hashes=dict(specs)
        if dest.exists() and digest(dest)==hashes[model]:continue
        if src.exists() and digest(src)==hashes[model]:
            folder.mkdir(parents=True,exist_ok=True);shutil.copy2(src,dest)
        else:
            if not allow_downloads:raise RuntimeError('AI models missing or invalid; run tools/song-chart/setup.sh first.')
            run_adapter('install-models',ROOT/'models');break
    # Keep Demucs model downloads and their upstream hash-verified weights in this project.
    old=Path.home()/'.cache/torch/hub/checkpoints'
    new=ROOT/'models/torch/hub/checkpoints';new.mkdir(parents=True,exist_ok=True)
    if old.exists():
        for src in old.glob('*.th'):
            if not (new/src.name).exists():shutil.copy2(src,new/src.name)

def canonical(audio):
    audio=Path(audio).resolve(); key=digest(audio); folder=ROOT/'cache'/key;folder.mkdir(parents=True,exist_ok=True)
    wav=folder/'canonical.wav'; metadata=folder/'decode.json'
    if not wav.exists() or not metadata.exists():
        meta=run_adapter('decode',audio,wav);write_json(metadata,meta)
    return folder,wav,read_json(metadata)

def features(wav):
    """Multi-band log spectral flux. Harmonic support comes from pitch transcription."""
    x,sr=sf.read(wav,dtype='float32');x=x.mean(axis=1) if x.ndim>1 else x
    if sr>24000:
        from scipy.signal import resample_poly
        x=resample_poly(x,1,2);sr=sr//2
    n=1024;hop=220
    if len(x)<n:x=np.pad(x,(0,n-len(x)))
    f,t,z=stft(x,fs=sr,window='hann',nperseg=n,noverlap=n-hop,boundary='zeros',padded=True)
    mag=np.abs(z); log=np.log1p(100*mag); diff=np.maximum(np.diff(log,axis=1,prepend=log[:,:1]),0)
    bands=[(40,250),(250,800),(800,2500),(2500,8000)]
    flux=np.array([diff[(f>=lo)&(f<hi)].sum(axis=0) for lo,hi in bands])
    total=flux.sum(axis=0); scale=np.percentile(total,95) or 1.
    from scipy.ndimage import median_filter
    residual=np.maximum(total-median_filter(total,size=max(3,int(sr/hop))),0)
    # Require local contrast as well as a positive global floor.
    floor=max(float(np.percentile(residual,65)),float(residual.max())*.025,1e-7)
    peaks,_=find_peaks(residual,height=floor,distance=max(1,int(.055*sr/hop)),prominence=floor*.4)
    power=np.sqrt(np.mean(mag*mag,axis=0)); pscale=np.percentile(power,95) or 1
    return {'times':t,'onset_times':t[peaks], 'onset_strengths':np.clip(residual[peaks]/scale,0,1),
            'flux':total/scale,'energy':power/pscale,'bands':flux,'sample_rate':sr,'hop_s':hop/sr}

def demucs_args(model, device, segment, destination, wav):
    if not (float(segment).is_integer() and 1<=segment<=7):
        raise ValueError('Demucs segment must be an integer between 1 and 7 seconds.')
    return ['-n',model,'--device',device,'--shifts','0','--overlap','0.1','--segment',str(int(segment)),
            '--flac','--int24','-o',str(destination),str(wav)]

def separate(wav, folder, model, device='auto', demucs_python=None, segment=5., offline=False):
    if not (float(segment).is_integer() and 1<=segment<=7):
        raise ValueError('Demucs segment must be an integer between 1 and 7 seconds.')
    separation_root=folder/f'separated-seg-{segment:g}'
    stem=separation_root/model/'canonical'/('guitar.flac' if model=='htdemucs_6s' else 'other.flac')
    meta_path=folder/f'separation-{model}-seg-{segment:g}.json'
    if stem.exists() and meta_path.exists():return stem,read_json(meta_path)
    executable=demucs_python or (str(ROOT/'.demucs-venv/bin/python') if (ROOT/'.demucs-venv/bin/python').exists() else str(Path.home()/'demucs-env/bin/python'))
    if not Path(executable).exists():
        raise RuntimeError('Demucs-Python fehlt. --demucs-python angeben oder ./setup.sh --demucs verwenden.')
    env={**os.environ,'TORCH_HOME':str(ROOT/'models/torch'),'OMP_NUM_THREADS':'4','MKL_NUM_THREADS':'4','PYTHONHASHSEED':'0'}
    # Seed torch before the upstream CLI; shifts disabled for reproducibility.
    runner="import random,numpy as np,torch,sys; random.seed(0); np.random.seed(0); torch.manual_seed(0); torch.set_num_threads(4); from demucs.separate import main; main()"
    if offline:
        runner="import torch.hub; torch.hub.download_url_to_file=lambda *a,**k: (_ for _ in ()).throw(RuntimeError('Model download disabled during game import; run setup first.')); "+runner
    devices=['mps','cpu'] if device=='auto' else [device]
    errors=[]
    for selected in devices:
        cmd=[executable,'-c',runner,*demucs_args(model,selected,segment,separation_root,wav)]
        log=folder/f'demucs-{model}-{selected}.log'
        with (ROOT/'models/separation.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            with log.open('w') as f:proc=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
        if proc.returncode==0 and stem.exists():
            checkpoints={p.name:digest(p) for p in (ROOT/'models/torch/hub/checkpoints').glob('*.th')}
            meta={'model':model,'device':selected,'seed':0,'shifts':0,'overlap':.1,'segment_s':segment,'checkpoints':checkpoints,'source':'guitar' if model=='htdemucs_6s' else 'other','note':'other is not an isolated guitar; 6s guitar is experimental'}
            write_json(meta_path,meta)
            # Only selected lead is retained; other stems are reproducible and consume unnecessary disk.
            for p in stem.parent.iterdir():
                if p.is_file() and p!=stem:p.unlink()
            return stem,meta
        errors.append(f'{selected}: {log.read_text()[-1400:]}')
    raise RuntimeError('Instrumententrennung fehlgeschlagen: '+'\n'.join(errors))

def analyze(audio, cfg, mode='ml', lead=None, bpm=None, first_beat=None, device='auto', demucs_python=None, progress=None, offline=False):
    update=progress or (lambda phase,fraction:None)
    update('decoding',.02)
    folder,wav,decoded=canonical(audio)
    update('beats',.12)
    if mode=='ml':ensure_models(allow_downloads=not offline)
    meter_mode='dsp' if bpm is not None else mode
    mixpath=folder/f'mix-{meter_mode}.json'
    if not mixpath.exists() or (meter_mode=='ml' and read_json(mixpath).get('meter',{}).get('policy')!='explicit-model-grid-v2'):
        write_json(mixpath,run_adapter('analyze',wav,meter_mode))
    mix=read_json(mixpath)
    if mode=='ml' and bpm is None and mix['meter'].get('outcome')!='Adopted':
        raise RuntimeError('Beat-Modell liefert kein übernommenes Raster: '+str(mix['meter'])+'. --bpm und --first-beat oder --mode dsp verwenden.')
    analysis=mix['analysis']; analysis['duration_s']=decoded['duration_s']
    if bpm is not None:
        if not 20<=bpm<=400:raise ValueError('BPM muss zwischen 20 und 400 liegen.')
        origin=float(first_beat or 0)
        if not -60<=origin<=60:raise ValueError('Erster Beat muss zwischen -60 und 60 Sekunden liegen.')
        analysis['bpm']=float(bpm);analysis['beats']=np.arange(origin,decoded['duration_s'],60/bpm).tolist();analysis['downbeats']=analysis['beats'][::4];analysis['repeats']=[]
        mix['meter']={'policy':'manual-constant','bpm':bpm,'first_beat':origin,'meter_assumption':'4/4'}
    elif first_beat is not None:raise ValueError('--first-beat benötigt --bpm.')
    update('instruments',.35)
    separation=None
    if lead:
        _,source,_=canonical(lead)
        actual=sf.info(source).duration
        if abs(actual-decoded['duration_s'])>.025:raise ValueError('Instrumentenspur weicht um mehr als 25 ms in der Länge ab. Identische Zeitachse erforderlich.')
        separation={'source':'user-provided','sha256':digest(lead),'alignment':'length checked only; sample alignment is user responsibility'}
    elif mode=='ml':source,separation=separate(wav,folder,cfg['separator'],device,demucs_python,cfg.get('separator_segment_s',5.),offline=offline)
    else:source=wav
    update('notes',.70)
    leadkey=digest(source); tonalpath=folder/f'tonal-{mode}-{leadkey}.json'
    if not tonalpath.exists():
        tonal=run_adapter('poly' if mode=='ml' else 'analyze',source,*([] if mode=='ml' else ['dsp']))
        write_json(tonalpath,tonal)
    tonal=read_json(tonalpath)
    pitches=tonal['notes'] if mode=='ml' else tonal['analysis']['melody']
    # Convert compressed stem to canonical PCM for shared feature extraction / study baseline.
    if source.suffix!='.wav':
        leadwav=folder/f'lead-{leadkey}.wav'
        if not leadwav.exists():run_adapter('decode',source,leadwav)
    else:leadwav=source
    if abs(sf.info(leadwav).duration-decoded['duration_s'])>.025:
        raise ValueError('Getrennte Instrumentenspur hat eine andere Länge als der kanonische Song.')
    featpath=folder/f'features-{leadkey}.npz'
    if not featpath.exists():np.savez_compressed(featpath,**features(leadwav))
    with np.load(featpath) as f: feat={k:f[k] for k in f.files}
    # Cache key includes upstream bytes; generation config and overrides are stored per run.
    result={'analysis':analysis,'pitches':pitches,'meter':mix.get('meter'),'audio_trim':decoded['audio_trim'],
            'duration_s':decoded['duration_s'],'mode':mode,'separation':separation,'source_audio_sha256':folder.name,
            'tonal_model':{k:v for k,v in tonal.items() if k not in ('notes','analysis')},
            'canonical_wav':str(wav),'lead_wav':str(leadwav),'cache':str(folder),
            'limitations':['DSP full-mix mode cannot identify guitar' ] if mode=='dsp' and not lead else []}
    return result,feat
