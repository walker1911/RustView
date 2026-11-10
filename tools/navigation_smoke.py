"""Exercise real same-Viewer navigation and close during encrypted-RAR extraction.

Generate fixtures with encrypted_rar_psd_smoke.py; no RAR writer is needed to run
this checker against those existing public-password fixtures. No native window is
created. Native-window keyboard acceptance is separate from this headless test.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

project=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(project/'tools'))
from encrypted_rar_psd_smoke import ProcessTracker
from compat_smoke import FLAGS, MIB, digest
from smoke_test import stop_own_tree

parser=argparse.ArgumentParser()
parser.add_argument('--exe',type=Path,required=True)
parser.add_argument('--fixtures',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
if os.name != 'nt':
    parser.error('The process-handle checks require Windows')
root=args.fixtures.resolve()
temporary=args.output.resolve().parent/'navigation-temp'
temporary.mkdir(parents=True,exist_ok=True)
assert not list(temporary.iterdir()), 'Use an empty test-only temporary directory'
cases=[(kind,delay) for kind in ['data','headers','stored-data','stored-headers'] for delay in [10,250]]
results=[]
for kind,delay in cases:
    stored=kind.startswith('stored-')
    archive=root/(f'large-{kind}.rar' if stored else f'large-encrypted-{kind}.rar')
    request=dict(archive=str(archive),password='fixture-password',
                 seed_entry='small-seed.png',large_entry='large-a.psd',final_entry='small-seed.png' if stored else 'large-b.psd',
                 switch_delay_ms=delay,temp_root=str(temporary))
    tracker=ProcessTracker()
    process=subprocess.Popen([str(args.exe),'--navigation-probe'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE,creationflags=FLAGS)
    done,saved=threading.Event(),{}
    def communicate():
        try:
            saved['streams']=process.communicate(json.dumps(request).encode('utf-8'))
        finally:
            done.set()
    thread=threading.Thread(target=communicate,daemon=True)
    thread.start()
    started=time.monotonic()
    samples=[]
    while True:
        tracker.observe()
        sample=tracker.private_bytes()
        if sample is not None:
            samples.append(sample)
        if done.wait(0.02):
            break
        if time.monotonic()-started>150:
            stop_own_tree(process)
            thread.join(10)
            raise TimeoutError('Owned navigation probe timed out')
    thread.join()
    processes,alive,wait_ms=tracker.finish()
    stdout,stderr=saved['streams']
    events=[json.loads(line) for line in stdout.decode('utf-8-sig').splitlines() if line.strip()]
    report=next((event for event in events if event.get('event')=='result'),{})
    stats=report.get('stats',{})
    good=(process.returncode==0 and report.get('passed') and report.get('jobs_drained')
          and report.get('temporary_empty') and report.get('final_image',{}).get('final_target_matched')
          and stats.get('welcome_frames_after_seed')==0 and stats.get('cancel_requests',0)>0
          and stats.get('retired_stale_generations',0)>0
          and stats.get('partial_bytes_before_first_switch',0)>0
          and report.get('close_deferred') and report.get('close_token_after_drain')
          and report.get('close_canceled_inflight_read') and not alive and not list(temporary.iterdir())
          and report.get('native_close_acknowledged_without_cancel')
          and bool(processes) and any(p['name'].lower()=='7z.exe' for p in processes)
          and bool(samples) and max(samples)<3*1024*MIB)
    result=dict(case=f'{kind}-{delay}ms',status='pass' if good else 'fail',report=report,
                exit_code=process.returncode,seconds=round(time.monotonic()-started,3),
                peak_private_tree_bytes=max(samples) if samples else None,
                observed_processes=processes,surviving_pids=alive,process_wait_ms=wait_ms,
                temporary_empty=not list(temporary.iterdir()),stderr=stderr.decode('utf-8','replace')[:2000])
    results.append(result)
    print(json.dumps({k:result[k] for k in ['case','status','seconds','peak_private_tree_bytes','surviving_pids','temporary_empty']}),flush=True)
    if not good:
        print(json.dumps(report,ensure_ascii=False),flush=True)
report=dict(passed=sum(r['status']=='pass' for r in results),failed=sum(r['status']!='pass' for r in results),
            executable_sha256=digest(args.exe),cases=results,
            scope='One real headless Viewer per case, real navigation/poll/draw and workers; actual native GUI is checked separately.')
args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
raise SystemExit(bool(report['failed']))
