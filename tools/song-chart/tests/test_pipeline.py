import tempfile, unittest, subprocess, json
from pathlib import Path
import numpy as np
import soundfile as sf
from bbchart.audio import features, demucs_args, analyze
from bbchart.common import ROOT,config
from bbchart.pipeline import generate_song

(ROOT/'cache').mkdir(exist_ok=True)

class PipelineTests(unittest.TestCase):
    def test_real_demucs_cli_accepts_resource_profile(self):
        python=Path.home()/'demucs-env/bin/python'
        if not python.exists():self.skipTest('local Demucs environment missing')
        args=demucs_args('htdemucs','mps',5.,'out','input.wav')
        code="import json,sys; from demucs.separate import get_parser; opts=get_parser().parse_args(json.loads(sys.argv[1])); assert opts.segment==5"
        result=subprocess.run([str(python),'-c',code,json.dumps(args)],capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)
        with self.assertRaises(ValueError):demucs_args('htdemucs','mps',5.5,'out','in.wav')

    def test_audio_transients_have_correct_time_axis(self):
        sr=22050;x=np.zeros(sr*4,dtype='float32');known=np.array([.5,1.25,2.,3.])
        for t in known:
            idx=round(t*sr);n=np.arange(2205);x[idx:idx+len(n)]+=(np.sin(n*2*np.pi*440/sr)*np.exp(-n/180)).astype('float32')
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            wav=Path(folder)/'plucks.wav';sf.write(wav,x,sr);f=features(wav)
            for t in known:self.assertLess(float(np.min(np.abs(f['onset_times']-t))),.025)
            self.assertFalse(any(f['onset_times']<.45))
    def test_manual_grid_works_without_periodic_audio(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            audio=Path(folder)/'no-beats.wav';sf.write(audio,np.zeros(44100*2),44100)
            result,_=analyze(audio,config(),mode='dsp',bpm=100,first_beat=.37)
            self.assertAlmostEqual(result['analysis']['beats'][0],.37)
            self.assertAlmostEqual(result['analysis']['beats'][1],.97)
            self.assertEqual(result['meter']['policy'],'manual-constant')

    def test_existing_chart_is_preserved_before_any_analysis(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            folder=Path(folder);chart=folder/'chart.json';chart.write_text('preserve me')
            with self.assertRaises(FileExistsError):generate_song(folder/'missing.mp3',folder,mode='dsp')
            self.assertEqual(chart.read_text(),'preserve me')
    def test_different_existing_audio_is_never_overwritten(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            folder=Path(folder);audio=folder/'input.mp3';audio.write_bytes(b'new audio')
            out=folder/'out';out.mkdir();existing=out/'song.mp3';existing.write_bytes(b'existing audio')
            with self.assertRaises(FileExistsError):generate_song(audio,out,mode='dsp',make_preview=False)
            self.assertEqual(existing.read_bytes(),b'existing audio')

    def test_silent_audio_never_publishes_playable_chart(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            folder=Path(folder);audio=folder/'silence.wav';sf.write(audio,np.zeros(44100*3),44100)
            with self.assertRaises(ValueError):generate_song(audio,folder/'out',mode='dsp',make_preview=False)
            self.assertFalse((folder/'out/chart.json').exists())
    def test_invalid_source_reports_error(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'cache') as folder:
            folder=Path(folder);p=folder/'invalid.mp3';p.write_text('not audio')
            with self.assertRaises(RuntimeError):generate_song(p,folder/'out',mode='dsp',make_preview=False)
            self.assertFalse((folder/'out/chart.json').exists())

if __name__=='__main__':unittest.main()
