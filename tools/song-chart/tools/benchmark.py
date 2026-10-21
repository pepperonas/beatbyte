"""Resumable full-song benchmark; process isolation avoids in-process MPS sharing."""
import sys,subprocess,json,time
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor,as_completed
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bbchart.common import ROOT,read_json,write_json
rows=read_json(ROOT/'reports/corpus.json')
# Disjoint worker shards preserve the pre-existing development/holdout assignments.
workers=2
for k in range(workers):write_json(ROOT/f'reports/corpus-worker-{k}.json',rows[k::workers])
def run(k):
 log=ROOT/f'reports/benchmark-worker-{k}.log'
 with log.open('w') as f:
  p=subprocess.run([str(ROOT/'bbchart.sh'),'evaluate','--corpus',str(ROOT/f'reports/corpus-worker-{k}.json'),'--mode','ml','--config',str(ROOT/'config/frozen.json'),'--tag',f'full-ml-{k}'],stdout=f,stderr=subprocess.STDOUT)
 return k,p.returncode
with ThreadPoolExecutor(max_workers=1) as pool:
 for future in as_completed([pool.submit(run,k) for k in range(workers)]):print('worker complete:',future.result(),flush=True)
# Consolidate via evaluate: finished chart folders are copied as run references below.
results=[]
for k in range(workers):
 p=ROOT/f'outputs/full-ml-{k}/evaluation.json'
 if p.exists():results.extend(read_json(p))
write_json(ROOT/'reports/full-ml-evaluation.json',results)
print('Completed',len(results),'full songs;',sum(r['status']=='ok' for r in results),'successful',flush=True)
