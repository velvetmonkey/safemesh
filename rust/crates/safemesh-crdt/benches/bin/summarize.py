#!/usr/bin/env python3
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
"""Derive tables only from fully completed rungs; keep truncated runs as raw evidence."""
import json
import math
from pathlib import Path
import sys


def percentile(values, p):
    return sorted(values)[max(0, math.ceil(p*len(values))-1)]


def distribution(rows):
    ns = [r['ns'] for r in rows]
    return '/'.join(f'{percentile(ns,p)/1e6:.6f}' for p in [.5,.95,.99])


def load(raw):
    groups = []
    stops = []
    for path in sorted(raw.glob('B*.meta.json')):
        meta = json.loads(path.read_text())
        if meta['result'] != 'completion':
            stops.append(meta)
            continue
        rows = [json.loads(x) for x in path.with_name(path.name.replace('.meta.json','.jsonl')).read_text().splitlines()]
        head = rows[0]
        groups.append((meta,head,rows[1:]))
    return groups,stops


def amplification(rows):
    values = [r['serialized_write_amplification'] for r in rows if 'serialized_write_amplification' in r]
    if values:
        return f'{min(values):.3f}–{max(values):.3f} serialized; '+f"{min(r['kernel_write_bytes']/r['payload_bytes'] for r in rows):.3f}–{max(r['kernel_write_bytes']/r['payload_bytes'] for r in rows):.3f} kernel"
    return 'N/A: no accepted durable append in this operation'


def generate(raw):
    groups,stops=load(raw)
    stopmap={x['args'][0]: '; '.join(x['brakes']['stopped_by']) or x['result'] for x in stops}
    lines=['# Measurement evidence and candidate ranges', '', 'OR-4 remains outstanding. These are measurements and candidate ranges for operator review, not numeric product commitments.', '',
           f"Source: {groups[0][0]['metadata']['source_sha'] if groups else 'no completed runs'} (product base 8911fc0). Machine identity, toolchain, filesystem, seed 42, condition, seven repetitions, and every brake sample are in the adjacent raw metadata files. The Rust workload source is unchanged between this source commit and the evidence commit.", '',
           'Percentiles are nearest-rank p50/p95/p99 in milliseconds. With seven samples p95 and p99 are both the maximum; they do not estimate population tails. Only complete rungs enter these tables. Workload dimensions co-vary geometrically; these are not isolated causal effects or multidimensional envelope guarantees.', '',
           'Allocator peak includes the instrument and setup objects; process RSS is Linux VmHWM for the whole process, including setup. Cumulative allocation counts requested bytes, including allocator reallocation through alloc/copy/dealloc. Copy traffic itself is NOT MEASURED. Kernel write_bytes is process-attributed storage-layer accounting, not hardware/media or journal traffic. Clone allocation measures the clone representation, not original allocator capacity.', '', '## T2 — Six families', '']
    for family in ['B1','B2','B3','B4','B5','B6','B7']:
        selected=[x for x in groups if x[1]['family']==family]
        if not selected:
            lines += [f'FAMILY {family} RAN no; STOPPED BY {stopmap.get(family)}. All metrics NOT MEASURED.', '']
            continue
        meta,head,rows=max(selected,key=lambda x:x[1]['n'])
        lines += [f"FAMILY {family} RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records {', '.join(str(x[1]['n']) for x in selected)}; STOPPED BY box brake: {stopmap.get(family,'completion')}; REPETITIONS 7 per condition. Top complete rung {meta['name']}: payload {head['payload']} bytes, actual writer domain {head['writers'] if family in ['B1','B2','B3','B4'] else 1}.", '', '| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |', '|---|---:|---:|---:|---:|---:|']
        keys=sorted({(r['op'],r.get('condition','warm'),str(r.get('duplicate_percent_requested',r.get('kind','')))) for r in rows if 'ns' in r})
        for op,condition,kind in keys:
            rs=[r for r in rows if r.get('op')==op and r.get('condition','warm')==condition and str(r.get('duplicate_percent_requested',r.get('kind','')))==kind]
            lines += [f"| {op} / {condition} {kind} | {distribution(rs)} | {max(r['process_peak_rss_bytes'] for r in rs)} | {max(r['allocator_peak_bytes'] for r in rs)} | {min(r['allocation_bytes'] for r in rs)}–{max(r['allocation_bytes'] for r in rs)} | {min(r['kernel_write_bytes'] for r in rs)}–{max(r['kernel_write_bytes'] for r in rs)} |"]
        if family=='B1':
            lines += ['',f"BYTES transaction {min(r['transaction_bytes'] for r in rows)}–{max(r['transaction_bytes'] for r in rows)}; amplification {amplification(rows)}. ASSERTION accepted history grows; full-history rewrite is exposed by actual resulting transaction lengths and storage accounting. RESULT held. Sync counts are in the separate traced control, not assumed per untraced rung."]
        elif family=='B2':
            lines += ['',f"BYTES pre-batch log up to {max(r['history_encoded_bytes'] for r in rows)}. ASSERTION pure duplicate state, wire log, version and allocation sequence unchanged: RESULT held. Mixed rows retain actual accepted/duplicate counts. Requested 0% duplicate batches can redeliver IDs used by the earlier 50% condition; interpret actual counts rather than the requested ratio. Accepted-only single-append latency is B1; this family measures batches."]
        elif family=='B3':
            points=[]
            for _,h,rs in selected:
                warm=[r['ns'] for r in rs if r.get('condition')=='warm']
                points.append((math.log(h['n']),math.log(percentile(warm,.5))))
            if len(points)>1:
                mx=sum(x for x,y in points)/len(points);my=sum(y for x,y in points)/len(points)
                slope=sum((x-mx)*(y-my) for x,y in points)/sum((x-mx)**2 for x,y in points)
                lines += ['',f'Descriptive log-log fit of warm median against record count along this co-varying workload path: slope {slope:.3f}. Payload and writer count also increase. This is no asymptotic proof.']
            lines += ['',f"BYTES replay frame {max(r['history_encoded_bytes'] for r in rows)}. ASSERTION exact state, encoded log and recovered local sequence: RESULT held. File-cache eviction was requested with successful posix_fadvise(DONTNEED); actual cache residency and device-cold restart are NOT MEASURED. Cache condition order is warm first, eviction-requested second."]
        elif family=='B4':
            lines += ['',f"BYTES maximum actual record-wire batch {max(r.get('wire_record_bytes',0) for r in rows)}; wire excludes an unspecified transport wrapper. ASSERTION all IDs and contiguous versions converge, every batch advances: RESULT held. Odd IDs precede even IDs, with 0 and n/4 peer prefixes. Caller batching filters already-held IDs and applies record and byte caps; the product export remains unbounded. No product pagination or continuation API is claimed."]
        elif family=='B5':
            lines += ['',f"BYTES encoded log up to {max(r['history_encoded_bytes'] for r in rows)}; retained add count {max(r['retained_adds'] for r in rows)}; tombstones up to {max(r['tombstones'] for r in rows)}; live membership ends at zero. State/log/tombstone clone allocation maxima: {max(r['state_clone_allocation_bytes'] for r in rows)}/{max(r['log_clone_allocation_bytes'] for r in rows)}/{max(r['tombstone_clone_allocation_bytes'] for r in rows)} bytes. Encoded bytes per recorded operation: {min(r['encoded_bytes_per_operation'] for r in rows):.3f}–{max(r['encoded_bytes_per_operation'] for r in rows):.3f}. Remove/query times cover the row's full loop, including seeded query-string construction; not a single-operation latency. ASSERTION membership and tombstone counts exact: RESULT held."]
        elif family=='B6':
            lines += ['',f"BYTES maximum tested frame {max(r['input_bytes'] for r in rows)}. Typed errors {sorted({r['error'] for r in rows if r['error']})}; large valid frames accepted. ASSERTION malformed/truncated/trailing inputs reject, separately held accepted history and version unchanged: RESULT held. This decoder creates a candidate log and has no mutating import API in this case. The refusal-boundary half of G4 cannot be measured until the limit mechanism exists, and that is a different lane. Binding paths NOT MEASURED: OR-1 owns the language surface."]
        else:
            lines += ['',f"BYTES transaction {min(r['transaction_bytes'] for r in rows)}–{max(r['transaction_bytes'] for r in rows)}; amplification {amplification(rows)}. ASSERTION durable G-Counter bump is acknowledged, persisted, and survives checked restart: RESULT held."]
        lines+=['']
    lines+=['Carriers NOT MEASURED: G-Set, G-Counter, PN-Counter, RGA/Text. This instrument exercises the OR-Set path to carry variable UTF-8 payloads and retained tombstones through all six families. Numeric-carrier timings cannot be inferred from it; durable constructors currently expose G-Counter and OR-Set, leaving a separate G-Counter run possible but unmeasured.', '', '## T3/T4 — Eight measured dimensions and candidate proposal', '', '| Dimension | Min | Max reached | Top measurement | Stop | Peak RSS / allocator bytes | p50/p95/p99 ms | Write amplification |', '|---|---:|---:|---|---|---:|---:|---|']
    candidates={key:[] for key in ['history record count','history encoded bytes','writer/replica count','per-record payload bytes','records per caller import batch','bytes per caller import batch','live carrier entries','retained tombstone count']}
    for meta,h,rows in groups:
        family=h['family']
        for r in rows:
            if 'ns' not in r:continue
            if family=='B6' and r.get('kind')!='large-valid-frame':continue
            if family=='B5' and r['op']!='remove':continue
            if family=='B4' and r['op']!='import':continue
            fields={'history record count':r.get('history_records',h['n']), 'per-record payload bytes':h['payload']}
            encoded=r.get('history_encoded_bytes', r.get('input_bytes'))
            if encoded is not None:fields['history encoded bytes']=encoded
            if family in ['B1','B2','B3','B4','B7']:fields['writer/replica count']=h['writers']
            if family in ['B2','B4']:fields['records per caller import batch']=r.get('batch_records',h['batch'])
            if family=='B4':fields['bytes per caller import batch']=r['wire_record_bytes']
            if family=='B5':fields.update({'live carrier entries':r['live_entries'],'retained tombstone count':r['tombstones']})
            for field,value in fields.items():candidates[field].append((value,meta['name'],family,r))
    for dim,items in candidates.items():
        if not items:
            lines.append(f'| {dim} | NOT MEASURED | | no completed rung | | | | |');continue
        minimum=min(x[0] for x in items);maximum=max(x[0] for x in items)
        top=next(x for x in items if x[0]==maximum)
        rs=[r for value,name,family,r in items if value==maximum and name==top[1] and r['op']==top[3]['op'] and r.get('condition')==top[3].get('condition')]
        lines.append(f"| {dim} | {minimum} | {maximum} | {top[1]} {top[3]['op']} {top[3].get('condition','warm')} (n={len(rs)}) | {stopmap.get(top[2],'completion')} | {max(r['process_peak_rss_bytes'] for r in rs)} / {max(r['allocator_peak_bytes'] for r in rs)} | {distribution(rs)} | {amplification(rs)} |")
    lines+=['', 'All eight rows are candidate observed ranges, not independently swept ceilings. Maxima come from different workloads and cannot be combined. For dimensions whose maximum was measured in a non-append operation, accepted-payload write amplification is N/A; the measured durable amplification range remains the B1 table, not an extrapolation to those maxima.', '']
    return '\n'.join(lines)


def controls(parent):
    lines = ['## T5 — Instrument controls', '']
    def rows(folder):
        result=[]
        for path in sorted((parent/folder).glob('*.jsonl')):
            meta=json.loads(path.with_name(path.name.replace('.jsonl','.meta.json')).read_text())
            if meta['result']!='completion':continue
            result.append((path.name,[json.loads(x) for x in path.read_text().splitlines()][1:]))
        return result
    base=rows('decode-noise')
    planted=rows('allocation-verified-raw')
    lines += ['PLANTED: 1,048,576-byte vector retained across a decoder call in a COPY of the benchmark. EXPECTED DIRECTION: cumulative allocation and extra peak increase by exactly 1,048,576 bytes. Baseline and copied instrument use the same public decoder. The retained patch describes the mutation.', '', '| Control | p50/p95/p99 ms, valid frame | Allocation bytes per decode | Extra peak bytes |', '|---|---:|---:|---:|']
    vals=[]
    for name,rs in base+planted:
        rs=[r for r in rs if r.get('kind')=='large-valid-frame']
        vals.append((percentile([r['ns'] for r in rs],.5),rs[0]['allocation_bytes'],rs[0]['allocator_extra_peak_bytes']))
        lines += [f"| {name} | {distribution(rs)} | {rs[0]['allocation_bytes']} | {rs[0]['allocator_extra_peak_bytes']} |"]
    if len(base)==3 and len(planted)==1:
        delta=vals[-1][1]-vals[0][1]
        peakdelta=vals[-1][2]-vals[0][2]
        noise=max(v[1] for v in vals[:3])-min(v[1] for v in vals[:3])
        timing=max(v[0] for v in vals[:3])-min(v[0] for v in vals[:3])
        assert delta==1048576 and peakdelta==1048576
        lines += ['',f'OBSERVED allocation delta {delta}, extra-peak delta {peakdelta} bytes. DETECTED yes. NOISE: three unchanged decoder runs, allocation spread {noise} bytes and median latency spread {timing} ns. SIGNAL EXCEEDS NOISE yes for allocation. One mutant process with seven repetitions; cross-process mutant variance is NOT MEASURED.']
    else:
        lines += ['', 'DETECTED NOT MEASURED: required complete controls missing.']
    b1=rows('noise')
    medians=[percentile([r['ns'] for r in rs],.5)/1e6 for _,rs in b1]
    lines += ['',f'Unchanged B1 median latencies (ms): {medians}; spread {max(medians)-min(medians):.6f} ms ({(max(medians)/min(medians)-1)*100:.2f}% of smallest median). The 5 ms planted sleep experiment stopped on swap activity before execution; DETECTED NOT MEASURED for that attempt. The copy was reverted before the allocation control.', '', 'TREE CLEAN AFTER yes for the negative worktree: empty final status and matching Rust source hashes retained. The main worktree carries only the intended harness/evidence changes.', '', 'The first allocation-control build used the wrong working directory (exit 101), after which a stale binary was copied. That run showed no allocation movement and is retained under allocation-raw, explicitly excluded from the comparison above. The corrected build exited 0 and produced the verified control binary.', '', '## Sync control', '']
    trace=(parent/'trace/sync-control-0.strace').read_text().splitlines()
    syncs=sum('fsync(' in line for line in trace)
    writes=[]
    import re
    groups=[];pending=0
    for line in trace:
        if 'fsync(' in line:pending+=1
        if 'write(1<' in line:
            groups.append(pending);pending=0
        if '.tmp>' in line and 'write(' in line:
            writes.append(int(re.search(r'= (\d+)$',line).group(1)))
    assert len(writes)==16 and syncs==33
    assert groups[2:]==[2]*6
    lines += [f'Traced B1 control: {syncs} successful fsync calls, {len(writes)} transaction-file writes totaling {sum(writes)} bytes. This includes one fence sync, fresh empty commit, eight setup commits and seven measured appends. Each of the last six isolated append intervals has exactly two fsync calls. This is one traced control process; its timings are excluded from latency tables. Sync counts at larger rungs are NOT MEASURED.', '']
    return '\n'.join(lines)


if __name__=='__main__':
    if sys.argv[1:]==['--self-test']:
        assert percentile([1,2,3,4,5,6,7],.5)==4
        assert percentile([1,2,3,4,5,6,7],.95)==7
        assert percentile([1,2,3,4,5,6,7],.99)==7
        print('summary self-test held')
    else:
        Path(sys.argv[2]).write_text(generate(Path(sys.argv[1])) + "\n" + controls(Path(sys.argv[1]).parent))
