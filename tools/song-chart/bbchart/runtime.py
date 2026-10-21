"""Explicit, per-device registration of the installed charting runtime."""
import sys, subprocess, os
from pathlib import Path
from .common import ROOT, ADAPTER, write_json, config
from .audio import ensure_models

def demucs_python():
    python=ROOT/'.demucs-venv/bin/python'
    return python if python.is_file() else Path.home()/'demucs-env/bin/python'

def separator_model(offline=False):
    python=demucs_python()
    if not python.is_file():raise RuntimeError('Demucs missing; run ./setup.sh --demucs.')
    code="from demucs.pretrained import get_model; import sys; get_model(sys.argv[1])"
    if offline:
        code="import torch.hub; torch.hub.download_url_to_file=lambda *a,**k: (_ for _ in ()).throw(RuntimeError('Separator weights missing; run setup first.')); "+code
    result=subprocess.run([str(python),'-c',code,config()['separator']],env={**os.environ,'TORCH_HOME':str(ROOT/'models/torch')},capture_output=True,text=True)
    if result.returncode:raise RuntimeError(result.stderr[-1200:])

def register():
    python=ROOT/('.venv/Scripts/python.exe' if sys.platform=='win32' else '.venv/bin/python')
    demucs=demucs_python()
    if not python.is_file() or not ADAPTER.is_file() or not demucs.is_file():
        raise RuntimeError('Runtime incomplete; run ./setup.sh --demucs first.')
    subprocess.run([str(demucs),'-c','import demucs, torch'],check=True,capture_output=True)
    ensure_models(allow_downloads=False)
    separator_model(offline=True)
    # Registration is explicit: packaged games never execute code found in CWD.
    if sys.platform=='darwin':data=Path.home()/'Library/Application Support'
    elif sys.platform=='win32':
        import os
        data=Path(os.environ['APPDATA'])
    else:
        import os
        data=Path(os.environ.get('XDG_DATA_HOME',Path.home()/'.local/share'))
    target=data/'beatbyte/song-chart-runtime.json'
    write_json(target,{'protocol':1,'root':str(ROOT)})
    return {'registered':str(target),'root':str(ROOT)}
