import numpy as np

def events(notes, epsilon=.005):
    out=[]
    for note in sorted(notes,key=lambda n:(n['time'],n['lane'])):
        if out and note['time']-out[-1]['time'] <= epsilon+1e-9:
            if note['lane'] not in out[-1]['lanes']: out[-1]['lanes'].append(note['lane'])
            out[-1]['len']=max(out[-1]['len'],note.get('len',0))
            out[-1]['hopo']=out[-1]['hopo'] and note.get('hopo',False)
        else: out.append({'time':note['time'],'lanes':[note['lane']],'len':note.get('len',0),'hopo':note.get('hopo',False)})
    return out

def stats(chart, duration):
    ev=events(chart['notes']); t=np.array([e['time'] for e in ev]); gaps=np.diff(t)
    windows=np.arange(0,max(1,duration),.25)
    nps=np.searchsorted(t,windows+1,side='left')-np.searchsorted(t,windows,side='left')
    centers=np.array([sum(e['lanes'])/len(e['lanes']) for e in ev])
    jumps=np.abs(np.diff(centers))
    return {'gems':len(chart['notes']),'events':len(ev),'events_per_second':len(ev)/max(1,duration),
        'active_events_per_second': len(ev)/max(1,(t[-1]-t[0]) if len(t)>1 else duration),
        'nps_median':float(np.median(nps)),'nps_p95':float(np.percentile(nps,95)),
        'chord_share':sum(len(e['lanes'])>1 for e in ev)/max(1,len(ev)),
        'sustain_share':sum(e['len']>0 for e in ev)/max(1,len(ev)),
        'hopo_share':sum(e['hopo'] for e in ev)/max(1,len(ev)),
        'median_gap_s':float(np.median(gaps)) if len(gaps) else None,
        'mean_lane_jump':float(np.mean(jumps)) if len(jumps) else 0,
        'fast_large_jumps':int(np.sum((gaps<.15)&(jumps>2))),
        'lanes':sorted({n['lane'] for n in chart['notes']})}

def match_times(predicted, reference, tolerance):
    """Maximum monotone one-to-one matching. No double credit for one onset."""
    a=sorted(predicted); b=sorted(reference); i=j=hits=0
    while i<len(a) and j<len(b):
        if a[i] < b[j]-tolerance-1e-9: i+=1
        elif b[j] < a[i]-tolerance-1e-9: j+=1
        else: hits+=1; i+=1; j+=1
    precision=hits/len(a) if a else 0.; recall=hits/len(b) if b else 0.
    return {'hits':hits,'predicted':len(a),'reference':len(b),'precision':precision,'recall':recall,
            'f1':2*precision*recall/(precision+recall) if precision+recall else 0.}

def compare(pred, ref):
    out={}
    for pc in pred['charts']:
        rc=next((c for c in ref['charts'] if c['difficulty']==pc['difficulty']),None)
        if rc:
            a=[x['time'] for x in events(pc['notes'])];b=[x['time'] for x in events(rc['notes'])]
            out[pc['difficulty']]={str(ms):match_times(a,b,ms/1000) for ms in (25,50,100)}
    return out

def grid_compare(pred,ref):
    """Reference grids may represent different metrical levels; report, never retime."""
    duration=min(pred['song'].get('duration_s',7200),ref['song'].get('duration_s',7200))
    out={}
    for field in ('beats','downbeats'):
        a=[t for t in pred.get('grid',{}).get(field,[]) if 0<=t<=duration]
        b=[t for t in ref.get('grid',{}).get(field,[]) if 0<=t<=duration]
        out[field]=match_times(a,b,.07) if b else {'status':'reference_unavailable'}
    return out
