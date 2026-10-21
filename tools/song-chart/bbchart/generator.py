"""Evidence → phrase-consistent five-fret master → nested difficulty reductions.

MIDI pitch is musical evidence, not a physical guitar fingering. The five-fret
mapping is a game design optimization and is deliberately separate from inference.
"""
from itertools import combinations
from copy import deepcopy
import math
import numpy as np
from .metrics import events, stats

class InsufficientEvidence(ValueError): pass

def beat_position(t, beats, bpm):
    if len(beats)<2:return t*bpm/60
    i=int(np.clip(np.searchsorted(beats,t)-1,0,len(beats)-2))
    return i+(t-beats[i])/max(.001,beats[i+1]-beats[i])

def beat_length(t,beats,bpm):
    if len(beats)<2:return 60/bpm
    i=int(np.clip(np.searchsorted(beats,t)-1,0,len(beats)-2))
    return beats[i+1]-beats[i]

def snap(t,beats,bpm,tolerance):
    if len(beats)<2:return t
    i=int(np.clip(np.searchsorted(beats,t)-1,0,len(beats)-2));length=beats[i+1]-beats[i]
    candidates=[beats[i]+length*k/d for d in (1,2,3,4) for k in range(d+1)]
    target=min(candidates,key=lambda x:abs(x-t))
    return target if abs(target-t)<=tolerance else t

def candidates(result,feat,cfg):
    pitches=sorted([p for p in result['pitches'] if p['end_s']>p['time_s'] and p['strength']>=cfg['min_pitch_support'] and 28<=p['midi']<=100],key=lambda p:p['time_s'])
    peaks=feat['onset_times']; strengths=feat['onset_strengths'];duration=result['duration_s']; groups=[]
    for p in pitches:
        t=max(0.,p['time_s']); end=min(duration,p['end_s'])
        if end<=t or t>=duration:continue
        near=np.where(np.abs(peaks-t)<.065)[0]
        attack=float(max(strengths[near],default=0))
        item={'time':t,'end':end,'pitches':[round(p['midi'])], 'salience':float(p['strength'])*.65+attack*.35,'origin':'pitch_onset'}
        if groups and t-groups[-1]['time']<=cfg['chord_window_s']:
            g=groups[-1]
            if item['pitches'][0] not in g['pitches']:g['pitches'].extend(item['pitches'])
            g['end']=min(g['end'],end);g['salience']=max(g['salience'],item['salience'])
        else:groups.append(item)
    # Pitch tracker may join repeated same-pitch strums. Require a strong, local
    # lead-source attack while that pitch is sounding; never infer from grid alone.
    extra=[]
    strong=max(.15,float(np.percentile(strengths,60))) if len(strengths) else 1
    times=np.array([g['time'] for g in groups])
    for t,s in zip(peaks,strengths):
        if s<strong or (len(times) and np.min(np.abs(times-t))<.09):continue
        active=[p for p in pitches if p['time_s']+.08<t<p['end_s']-.06]
        if active:
            extra.append({'time':float(t),'end':min(p['end_s'] for p in active),'pitches':sorted(set(round(p['midi']) for p in active)),
                          'salience':float(s)*.5+max(p['strength'] for p in active)*.5,'origin':'supported_rearticulation'})
    groups=sorted(groups+extra,key=lambda g:g['time']); kept=[]; beats=result['analysis']['beats'];bpm=result['analysis']['bpm']
    for g in groups:
        g['time']=max(0,snap(g['time'],beats,bpm,cfg['snap_tolerance_s']))
        if g['time']>=duration:continue
        g['pitches']=sorted(set(g['pitches']))
        if kept and g['time']-kept[-1]['time']<cfg['min_event_gap_s']:
            # Duplicate / conflicting attacks are collapsed, not counted twice.
            if g['salience']>kept[-1]['salience']:kept[-1]=g
        else:kept.append(g)
    if not kept:raise InsufficientEvidence('Keine ausreichend belegte instrumentale Melodie gefunden; keine Noten erfunden.')
    return kept

def phrases(master,result,cfg):
    beats=result['analysis']['beats'];bpm=result['analysis']['bpm'];out=[];current=[];firstbeat=None
    for g in master:
        b=beat_position(g['time'],beats,bpm)
        rest=(g['time']-current[-1]['end'])/beat_length(g['time'],beats,bpm) if current else 0
        if current and (rest>cfg['phrase_rest_beats'] or b-firstbeat>=cfg['phrase_beats']):
            out.append(current);current=[]
        if not current:firstbeat=b
        current.append(g)
    if current:out.append(current)
    return out

def map_phrase(phrase,cfg):
    pitches=sorted({p for e in phrase for p in e['pitches']}); ranks={p:i*4/max(1,len(pitches)-1) for i,p in enumerate(pitches)}
    if len(pitches)==1:ranks[pitches[0]]=2
    states=[]
    for e in phrase:
        count=min(cfg['max_chord_size'],len(e['pitches']))
        states.append(list(combinations(range(5),count)))
    costs=[];backs=[]
    for i,(e,opts) in enumerate(zip(phrase,states)):
        # Roots and contour dominate; an actual 3+ tone chord can be represented
        # by a 2/3-button shape without pretending buttons are piano keys.
        selected=np.linspace(0,len(e['pitches'])-1,len(opts[0])).round().astype(int)
        targets=np.array([ranks[e['pitches'][k]] for k in selected]); row=[]; back=[]
        for shape in opts:
            emission=float(np.mean((np.array(shape)-targets)**2))
            if i==0:row.append(emission);back.append(-1);continue
            prev=phrase[i-1];gap=e['time']-prev['time']; pitchdelta=float(np.mean(e['pitches'])-np.mean(prev['pitches']))
            options=[]
            for k,old in enumerate(states[i-1]):
                delta=float(np.mean(shape)-np.mean(old)); move=abs(delta)
                contour=2.5 if pitchdelta*delta<0 else (1.2 if pitchdelta and delta==0 else 0)
                repeated=5.0 if e['pitches']==prev['pitches'] and shape!=old else 0
                fast=max(0,move-1)**2*max(0,.25-gap)*20
                options.append(costs[-1][k]+emission+contour+repeated+fast+.08*move)
            idx=int(np.argmin(options));row.append(options[idx]);back.append(idx)
        costs.append(row);backs.append(back)
    idx=int(np.argmin(costs[-1]));mapped=[]
    for i in range(len(phrase)-1,-1,-1):mapped.append(list(states[i][idx]));idx=backs[i][idx]
    return list(reversed(mapped))

def master_chart(result,feat,cfg):
    raw=candidates(result,feat,cfg); parts=phrases(raw,result,cfg);memo={}; allnotes=[];phrase_info=[]
    beats=result['analysis']['beats'];bpm=result['analysis']['bpm']
    for pid,part in enumerate(parts):
        origin=part[0]['time'];minpitch=min(p for e in part for p in e['pitches'])
        signature=tuple((round(beat_position(e['time'],beats,bpm)-beat_position(origin,beats,bpm),2),tuple(p-minpitch for p in e['pitches'])) for e in part)
        cached=signature in memo
        mapping=deepcopy(memo[signature]) if cached else map_phrase(part,cfg);memo[signature]=mapping
        for e,lanes in zip(part,mapping):allnotes.append({**e,'lanes':lanes,'phrase':pid})
        phrase_info.append({'id':pid,'start':part[0]['time'],'end':part[-1]['end'],'events':len(part),'reused_mapping':cached})
    # A physically extreme reach in a short interval is removed, not moved to an unrelated pitch.
    filtered=[]
    for e in allnotes:
        if filtered and e['time']-filtered[-1]['time']<.12 and abs(np.mean(e['lanes'])-np.mean(filtered[-1]['lanes']))>2:
            if e['salience']>filtered[-1]['salience']:filtered[-1]=e
        else:filtered.append(e)
    return filtered,phrase_info

def reduce_level(parent,profile,result):
    beats=result['analysis']['beats'];bpm=result['analysis']['bpm'];chosen=[]
    # Local budgets keep quiet verses; no global strongest-note competition.
    buckets={}
    for e in parent:buckets.setdefault(int(math.floor(beat_position(e['time'],beats,bpm)/4)),[]).append(e)
    for group in buckets.values():
        count=max(1,round(len(group)*profile['keep']))
        def importance(e):
            phase=beat_position(e['time'],beats,bpm)%1
            beatbonus=.25 if min(phase,1-phase)<.06 else (.1 if abs(phase-.5)<.06 else 0)
            return e['salience']+beatbonus+(.12 if e is group[0] else 0)
        kept=[]
        for e in sorted(group,key=lambda e:(-importance(e),e['time'])):
            if all(abs(e['time']-k['time'])>=profile['min_gap_s'] for k in kept):kept.append(e)
            if len(kept)>=count:break
        chosen.extend(kept)
    chosen.sort(key=lambda e:e['time']);out=[];burst=0
    for e in chosen:
        if out and e['time']-out[-1]['time']<profile['min_gap_s']:continue
        gap=e['time']-out[-1]['time'] if out else 1
        burst=burst+1 if gap<.13 else 1
        if burst>profile['max_burst']:continue
        out.append(deepcopy(e))
    # Simplify/fold within each phrase, preserving a consistent relationship.
    for e in out:
        cap=profile['chord_cap']
        lanes=([e['lanes'][-1]] if cap==1 else [e['lanes'][i] for i in np.linspace(0,len(e['lanes'])-1,min(cap,len(e['lanes']))).round().astype(int)])
        if profile['lanes']<5:lanes=sorted({round(l*(profile['lanes']-1)/4) for l in lanes})
        e['lanes']=lanes
    return out

def export_notes(master,difficulty,result,cfg):
    out=[];beats=result['analysis']['beats'];bpm=result['analysis']['bpm']
    for i,e in enumerate(master):
        end=min(e['end'],result['duration_s'],e['time']+cfg['max_sustain_s'])
        if i+1<len(master):end=min(end,master[i+1]['time']-cfg['sustain_gap_s'])
        length=max(0,end-e['time']);length=length if length>=cfg['sustain_min_s'] else 0
        prev=master[i-1] if i else None
        hopo=difficulty in ('hard','expert') and prev and len(e['lanes'])==len(prev['lanes'])==1 and prev['lanes']!=e['lanes'] and 0<beat_position(e['time'],beats,bpm)-beat_position(prev['time'],beats,bpm)<=cfg['hopo_max_beats']
        for lane in e['lanes']:
            n={'time':round(e['time'],6),'lane':int(lane)}
            if length:n['len']=round(length,6)
            if hopo:n['hopo']=True
            out.append(n)
    return out

def generate(result,feat,cfg,title,artist,audio_name):
    master,parts=master_chart(result,feat,cfg)
    # Cap master tails once; lower difficulties must not lengthen a removed attack.
    for i,e in enumerate(master[:-1]):e['end']=min(e['end'],master[i+1]['time']-cfg['sustain_gap_s'])
    levels={'expert':master}
    for difficulty,parent in [('hard','expert'),('medium','hard'),('easy','medium')]:levels[difficulty]=reduce_level(levels[parent],cfg['profiles'][difficulty],result)
    charts=[{'difficulty':d,'lanes':5,'notes':export_notes(levels[d],d,result,cfg),'phrases':[]} for d in ('easy','medium','hard','expert')]
    a=result['analysis'];beats=[round(t,6) for t in a['beats'] if -60<=t<=result['duration_s']+60]
    downbeats=[round(t,6) for t in a['downbeats'] if round(t,6) in set(beats)]
    chart={'format_version':1,'song':{'title':title,'artist':artist,'audio':audio_name,'bpm':min(400,max(20,a['bpm'])),'offset_s':min(60,max(-60,beats[0] if beats else 0)),
        'duration_s':result['duration_s'],'preview_start_s':min(max(0,result['duration_s']*.3),max(0,result['duration_s']-20))},
        'charts':charts,'audio_trim':result['audio_trim'],'grid':{'beats':beats,'downbeats':downbeats}}
    report={'status':'generated','mode':result['mode'],'master_events':len(master),'phrases':parts,
            'stats':{c['difficulty']:stats(c,result['duration_s']) for c in charts},
            'human_playtest':'not_performed','warning':'Prototype: model errors and source leakage can create musically wrong notes. Timing agreement alone does not prove playability.'}
    return chart,report
