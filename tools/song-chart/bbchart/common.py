from pathlib import Path
import hashlib, json, os, subprocess, tempfile
ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = ROOT.parent.parent
ADAPTER = ROOT / 'adapter/target/release/bb-chart-adapter'

def read_json(path):
    return json.loads(Path(path).read_text())

def write_json(path, value):
    path = Path(path); path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile('w', dir=path.parent, delete=False) as f:
        json.dump(value, f, ensure_ascii=False, indent=2, allow_nan=False); f.write('\n')
        name = f.name
    os.replace(name, path)

def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda:f.read(1<<20), b''): h.update(block)
    return h.hexdigest()

def run_adapter(command, path, *args):
    if not ADAPTER.exists():
        raise RuntimeError('Rust-Adapter fehlt. Zuerst ./setup.sh ausführen.')
    proc = subprocess.run([str(ADAPTER), command, str(path), str(ROOT/'models'), *map(str,args)], capture_output=True, text=True)
    if proc.returncode:
        raise RuntimeError(proc.stderr.strip() or proc.stdout.strip())
    return json.loads(proc.stdout)

def validate(path):
    proc = subprocess.run([str(ADAPTER),'validate',str(path)], capture_output=True,text=True)
    if proc.returncode: raise RuntimeError(proc.stdout + proc.stderr)
    return json.loads(proc.stdout)

def config(path=None):
    return read_json(path or (ROOT/'config/frozen.json' if (ROOT/'config/frozen.json').exists() else ROOT/'config/default.json'))
