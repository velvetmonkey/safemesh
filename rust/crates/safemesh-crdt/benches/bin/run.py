#!/usr/bin/env python3
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
"""Serial Linux measurement runner. All ceilings below protect the shared box only."""
import argparse
import hashlib
import datetime
import json
import os
from pathlib import Path
import resource
import subprocess
import sys


def command(args):
    p = subprocess.run(args, text=True, capture_output=True, check=False)
    if p.returncode:
        raise RuntimeError(f'{args}: exit {p.returncode}: {p.stderr}')
    return p.stdout


def brakes():
    ram = command(['free', '-m'])
    vm = command(['vmstat', '1', '3'])
    disk = command(['df', '-h', '/'])
    available = int(next(x for x in ram.splitlines() if x.startswith('Mem:')).split()[-1])
    rows = [list(map(int, x.split())) for x in vm.splitlines()[-2:]]
    fs = os.statvfs('/')
    free_bytes = fs.f_bavail * fs.f_frsize
    reasons = classify(available, rows, free_bytes)
    return dict(time=datetime.datetime.now(datetime.timezone.utc).isoformat(), free_m=ram,
                vmstat=vm, df_h=disk, available_mb=available,
                idle=[x[14] for x in rows], si=[x[6] for x in rows], so=[x[7] for x in rows],
                root_free_bytes=free_bytes, stopped_by=reasons)


def classify(available, rows, free_bytes):
    reasons = []
    if available < 4000:
        reasons.append('available RAM below 4000 MB')
    if any(x[14] < 10 for x in rows):
        reasons.append('CPU idle below 10 percent')
    if any(x[6] > 1000 or x[7] > 1000 for x in rows):
        reasons.append('swap activity above 1000 KB/s')
    if free_bytes < 30 * 1024**3:
        reasons.append('root free space below 30 GiB')
    return reasons


def metadata(repo):
    cpu = Path('/proc/cpuinfo').read_text()
    return dict(source_sha=command(['git', '-C', str(repo), 'rev-parse', 'HEAD']).strip(),
                source_dirty=bool(command(['git', '-C', str(repo), 'status', '--porcelain']).strip()),
                crate_version='0.1.0', toolchain=command(['rustc', '-Vv']),
                os=command(['uname', '-a']), cpu_model=next(x for x in cpu.splitlines() if x.startswith('model name')),
                core_count=os.cpu_count(), affinity=sorted(os.sched_getaffinity(0)),
                filesystem=command(['findmnt', '-T', str(repo), '-o', 'SOURCE,FSTYPE,OPTIONS']),
                seed=42, repetitions=7, condition='warm except B3 explicit eviction request',
                memory_brake_bytes=512*1024**2, wall_time_brake_seconds=20)


def limits():
    resource.setrlimit(resource.RLIMIT_AS, (512*1024**2, 512*1024**2))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.nice(15)


def run(binary, out, name, args, meta, trace=False):
    reading = brakes()
    entry = dict(name=name, metadata=meta, brakes=reading, args=args)
    (out / f'{name}.meta.json').write_text(json.dumps(entry, indent=2)+'\n')
    if reading['stopped_by']:
        entry['result'] = 'box brake before rung'
    else:
        root = out / f'{name}.store'
        cmd = [str(binary), *map(str, args), str(root)]
        if trace:
            cmd = ['strace', '-qq', '-f', '-yy', '-e', 'trace=write,fsync,fdatasync', '-o', str(out/f'{name}.strace'), *cmd]
        entry['command'] = cmd
        with (out/f'{name}.jsonl').open('w') as stdout, (out/f'{name}.stderr').open('w') as stderr:
            try:
                p = subprocess.run(cmd, stdout=stdout, stderr=stderr, preexec_fn=limits, timeout=20, check=False)
                entry['exit_code'] = p.returncode
                (out/f'{name}.exit').write_text(str(p.returncode)+'\n')
                entry['result'] = 'completion' if p.returncode == 0 else 'failed; inspect stderr for allocation brake or assertion'
            except subprocess.TimeoutExpired:
                entry['result'] = 'box wall-time brake 20 seconds'
                (out/f'{name}.exit').write_text('timeout\n')
        # Remove only the explicitly named benchmark files; retain raw evidence.
        for path in root.glob('writer-*'):
            if path.name.endswith(('.transaction', '.fence', '.tmp')):
                path.unlink()
        if root.exists():
            root.rmdir()
    (out / f'{name}.meta.json').write_text(json.dumps(entry, indent=2)+'\n')
    print(name, entry['result'], reading['stopped_by'], flush=True)
    return entry['result'] == 'completion'


def main():
    p = argparse.ArgumentParser()
    p.add_argument('binary', type=Path)
    p.add_argument('output', type=Path)
    p.add_argument('--mode', choices=['sweep', 'noise', 'trace', 'single'], default='sweep')
    p.add_argument('--label', default='baseline')
    p.add_argument('--family', choices=['B1','B6'], default='B1')
    a = p.parse_args()
    out = a.output.resolve()
    if not (out.is_relative_to('/home/monkey/scratch/benchraw') or out.is_relative_to('/mnt/scratch/tmp')):
        p.error('output must be in a permitted scratch root')
    out.mkdir(parents=True, exist_ok=False)
    repo = Path(__file__).resolve().parents[5]
    meta = metadata(repo)
    meta["binary_sha256"] = hashlib.sha256(a.binary.read_bytes()).hexdigest()
    if a.mode == 'sweep':
        for family in ['B1','B2','B3','B4','B5','B6']:
            rung = 0
            while True:
                n = 8 * 4**rung
                payload = 32 * 2**rung
                writers = 2**rung
                batch = 2**rung
                cap = batch * (payload+64)
                if not run(a.binary.resolve(), out, f'{family}-r{rung}', [family,n,payload,writers,batch,cap,7,42], meta):
                    break
                rung += 1
    else:
        for i in range(3 if a.mode == 'noise' else 1):
            if not run(a.binary.resolve(), out, f'{a.label}-{i}', [a.family,8,32,1,1,96,7,42], meta, a.mode=='trace'):
                break


if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        row = [0]*17
        row[14] = 90
        assert not classify(4000, [row,row], 30*1024**3)
        assert classify(3999, [row,row], 30*1024**3)
        assert classify(4000, [row,row], 29*1024**3)
        for index, value in [(14,9),(6,1001),(7,1001)]:
            bad = row.copy()
            bad[index] = value
            assert classify(4000,[row,bad],30*1024**3)
            assert classify(4000,[bad,row],30*1024**3)
        print('runner self-test held: both vmstat rows and every brake')
    else:
        main()
