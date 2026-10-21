from pathlib import Path
import numpy as np
import soundfile as sf
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from .metrics import events

def preview(chart,wav,dest,start=None,seconds=30):
    dest=Path(dest);dest.mkdir(parents=True,exist_ok=True)
    info=sf.info(wav);start=start if start is not None else chart['song'].get('preview_start_s',0)
    start=max(0,min(start,max(0,info.duration-seconds)))
    x,sr=sf.read(wav,start=int(start*info.samplerate),frames=int(seconds*info.samplerate),dtype='float32')
    if x.ndim>1:x=x.mean(axis=1)
    expert=next(c for c in chart['charts'] if c['difficulty']=='expert'); ev=events(expert['notes'])
    click=np.zeros(len(x),dtype='float32');t=np.arange(int(sr*.025))/sr
    pulse=(np.sin(2*np.pi*1500*t)*np.exp(-t*160)).astype('float32')*.35
    for e in ev:
        i=round((e['time']-start)*sr)
        if 0<=i<len(click):click[i:i+min(len(pulse),len(click)-i)]+=pulse[:min(len(pulse),len(click)-i)]
    # Left: song, right: song + chart clicks; makes timing independently audible.
    sf.write(dest/'preview.wav',np.stack([x*.65,x*.65+click],axis=1).clip(-1,1),sr,subtype='PCM_16')
    fig,axes=plt.subplots(5,1,figsize=(15,9),sharex=True,gridspec_kw={'height_ratios':[1,1.3,1.3,1.3,1.3]})
    stride=max(1,len(x)//12000);axes[0].plot(start+np.arange(0,len(x),stride)/sr,x[::stride],lw=.4,color='#65748b');axes[0].set_ylabel('Audio')
    colors=['#32a852','#d94949','#deb92d','#397bd1','#e98a30']
    for ax,c in zip(axes[1:],chart['charts']):
        for n in c['notes']:
            if start-.1<=n['time']<=start+seconds:
                ax.scatter(n['time'],n['lane'],s=10,color=colors[n['lane']])
                if n.get('len',0):ax.plot([n['time'],n['time']+n['len']],[n['lane']]*2,color=colors[n['lane']],lw=2)
        ax.set_ylabel(c['difficulty']);ax.set_yticks(range(5));ax.grid(alpha=.15);ax.set_ylim(-.5,4.5)
    axes[-1].set_xlabel('Song time (seconds)');axes[-1].set_xlim(start,start+len(x)/sr)
    fig.suptitle(chart['song']['title']+' — generated chart (prototype)');fig.tight_layout();fig.savefig(dest/'chart-preview.png',dpi=130);plt.close(fig)
