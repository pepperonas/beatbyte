import unittest, tempfile, json, wave
from pathlib import Path
import numpy as np
from bbchart.common import config, ADAPTER, validate, run_adapter, ROOT, REPO_ROOT
from bbchart.generator import generate, snap, beat_position, InsufficientEvidence
from bbchart.metrics import events, match_times

(ROOT/'cache').mkdir(exist_ok=True)

class ChartTests(unittest.TestCase):
    def setUp(self):
        self.cfg=config(); self.result={'duration_s':16.,'mode':'dsp','analysis':{'bpm':120.,'beats':np.arange(0,16,.5).tolist(),'downbeats':np.arange(0,16,2).tolist()},
            'pitches':[{'time_s':float(t),'end_s':float(t+.3),'midi':60+(i%5)*2,'strength':.9} for i,t in enumerate(np.arange(1,6,.25))]+[{'time_s':float(t),'end_s':float(t+.3),'midi':60+(i%5)*2,'strength':.9} for i,t in enumerate(np.arange(9,14,.25))],
            'audio_trim':{'priming_samples':0,'sample_rate':44100}}
        self.feat={'onset_times':np.array([p['time_s'] for p in self.result['pitches']]),'onset_strengths':np.ones(40)*.8}
    def chart(self):return generate(self.result,self.feat,self.cfg,'Test','Test','song.wav')[0]
    def test_silence_refuses_invented_chart(self):
        self.result['pitches']=[]
        with self.assertRaises(InsufficientEvidence):self.chart()
    def test_nested_events_and_preserved_rests(self):
        c=self.chart();levels=[events(d['notes']) for d in c['charts']]
        for low,high in zip(levels,levels[1:]):self.assertLessEqual({e['time'] for e in low},{e['time'] for e in high})
        for level in levels:self.assertFalse(any(6.5<e['time']<8.5 for e in level))
        self.assertTrue(all(n['lane']<3 for n in c['charts'][0]['notes']))
        self.assertTrue(all(len(e['lanes'])==1 for e in levels[0]))
    def test_no_duplicate_lane_events_and_sustain_overlap(self):
        for c in self.chart()['charts']:
            ns=c['notes'];self.assertEqual(len(ns),len({(n['time'],n['lane']) for n in ns}))
            for a,b in zip(events(ns),events(ns)[1:]):self.assertLessEqual(a['time']+a['len'],b['time']+1e-6)
    def test_determinism_and_real_validator(self):
        chart=self.chart();self.assertEqual(chart,self.chart())
        if not ADAPTER.exists():self.skipTest('adapter not built')
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            p=Path(folder)/'chart.json';p.write_text(json.dumps(chart));self.assertEqual(validate(p)['errors'],[])
    def test_tempo_changes_and_triplets(self):
        beats=[0,.5,1.,2.,3.]
        self.assertAlmostEqual(beat_position(2.5,beats,120),3.5)
        self.assertAlmostEqual(snap(1.341,beats,120,.025),1+1/3)
        self.assertEqual(snap(1.4,beats,120,.025),1.4)
    def test_one_to_one_matching_penalizes_excess_notes(self):
        score=match_times([1,1.01,1.02],[1],.05)
        self.assertEqual(score['recall'],1);self.assertAlmostEqual(score['precision'],1/3)
    def test_chords_follow_pitch_evidence(self):
        self.result['pitches']=[{'time_s':1.,'end_s':2.,'midi':m,'strength':.9} for m in (60,64,67)]
        self.feat={'onset_times':np.array([1.]),'onset_strengths':np.array([.8])}
        c=self.chart();self.assertEqual(len(events(c['charts'][-1]['notes'])[0]['lanes']),3)
        self.assertEqual(len(events(c['charts'][0]['notes'])[0]['lanes']),1)
    def test_repeated_pitch_attacks_survive(self):
        self.result['pitches']=[{'time_s':1.,'end_s':4.,'midi':60,'strength':.9}]
        self.feat={'onset_times':np.arange(1.,4.,.5),'onset_strengths':np.ones(6)*.8}
        ev=events(self.chart()['charts'][-1]['notes']);self.assertEqual(len(ev),6)
        self.assertEqual(len({tuple(e['lanes']) for e in ev}),1)
    def test_encoder_priming_and_variable_tempo_adapter(self):
        if not ADAPTER.exists():self.skipTest('adapter not built')
        fixture=REPO_ROOT/'crates/beatbyte-audio/tests/fixtures/click-apple.m4a'
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            wav=Path(folder)/'decoded.wav';meta=run_adapter('decode',fixture,wav)
            self.assertGreater(meta['audio_trim']['priming_samples'],0)
            self.assertTrue(wav.exists());self.assertGreater(meta['duration_s'],0)
    def test_chord_grouping_is_anchored(self):
        self.assertEqual(len(events([{'time':1,'lane':0},{'time':1.004,'lane':1},{'time':1.008,'lane':2}])),2)

if __name__=='__main__':unittest.main()
