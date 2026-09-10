# Unreleased bootstrap preservation

Generating source: `3a9179b0cd98ae7e5c6803a6de014416320929ba`.
Measured crate version: `safemesh-crdt 0.1.0`; this is not a published release.
No support promise is made. Existing old-zero and collision fixtures remain separate.

## Reproduction

In an isolated checkout of that exact SHA, copy `tests/bootstrap_generate.rs` from this
change into the crate. Set TMPDIR to an existing scratch directory. From its root:

```sh
export PATH=/home/monkey/bin:$PATH
BOOTFIXTURE_GENERATE=/home/monkey/scratch/bootfixture/generated cargo test --manifest-path /home/monkey/scratch/bootfixture/baseline/rust/Cargo.toml -p safemesh-crdt --features local-writer --test bootstrap_generate generate_bootstrap -- --ignored --exact --nocapture
```

The generator creates a NEW output directory (refuses overwrite). It runs the
baseline public encoders and durable writers; it never decodes its output.
Copy its nine files here without editing. Do not regenerate these retained bytes
with a newer source. Replay: `cargo test --manifest-path rust/Cargo.toml -p
safemesh-crdt --features local-writer --test bootstrap`.
`BOOTFIXTURE_DIR` selects a separate corpus copy for destructive negative controls;
it defaults to this directory. Tests create disposable stores beneath TMPDIR.

## Identities and coverage

All nine files share the generating SHA and measured package version above.
The package version is ABSENT from every file's bytes.

| File | Frame tag | Payload schema actually in bytes | EventLog trait identity (not embedded) | Wrapper shape |
| --- | --- | --- | --- | --- |
| counter.log | 0x03 | safemesh/gcounter-delta/v1 | safemesh/event-log/v2/safemesh/gcounter-delta/v1 | ABSENT |
| counter.transaction | 0x03 at offset 24 | safemesh/gcounter-delta/v1 | safemesh/event-log/v2/safemesh/gcounter-delta/v1 | unversioned three-field prefix: writers, writer, last_sequence; 3 LE u64, then log |
| counter.fence | ABSENT | ABSENT | ABSENT | ABSENT (ownership sidecar: writers, writer, generation; 3 LE u64) |
| orset.log | 0x03 | safemesh/orset-delta-utf8-u64/v1 | safemesh/event-log/v2/safemesh/orset-delta-utf8-u64/v1 | ABSENT |
| orset.transaction | 0x03 at offset 24 | safemesh/orset-delta-utf8-u64/v1 | safemesh/event-log/v2/safemesh/orset-delta-utf8-u64/v1 | unversioned three-field prefix: writers, writer, last_sequence; 3 LE u64, then log |
| orset.fence | ABSENT | ABSENT | ABSENT | ABSENT (ownership sidecar: writers, writer, generation; 3 LE u64) |
| pn.log | 0x03 | safemesh/pncounter-delta/v1 | safemesh/event-log/v2/safemesh/pncounter-delta/v1 | ABSENT |
| gset.state | ABSENT (state tag 0x20) | ABSENT | ABSENT | ABSENT |
| rga.state | ABSENT (state tag 0x40) | ABSENT | ABSENT | ABSENT |

State encoders have source-level WireSchema identities `safemesh/gset-u64/v1`
and `safemesh/rga-u64-u64/v1`, but do not embed them. Probe never infers them.
The log encoder embeds D::wire_schema(), NOT EventLog<D>::wire_schema().

Five carriers have byte evidence, but only G-Counter and OR-Set have durable
history coverage. PN-Counter has native event-log replay but no public durable
constructor. G-Set's native delta is u64 and RGA's is RgaDelta; neither has the
required native delta WireEncode/WireDecode/WireSchema implementation, and
neither has OwnedDelta or a durable constructor. Their state snapshots exercise
public decode/merge, not native history replay. Text UTF-8 history is not claimed
by the u64 RGA snapshot. These are explicit missing coverage, not substitute APIs.

## Independent expectations (machine-read by replay)

This table is authored from the input operations and arithmetic, BEFORE any
decoder runs. The generator does not read this table or produce expectations.
Counter: local tally 5 at (0,1); remote tally 7 at (1,3). PN: increment 9 at
(0,1), decrement 4 at (1,3), hence 9-4=5. The missing remote sequences 1 and 2
keep its contiguous version at zero. OR-Set: add café☕ at (0,1), token 2*1+0=2;
remove that token at (0,2); remote add 東京 at (1,3), token 2*3+1=7.
Removed adds remain in the adds set; token 2 remains tombstoned. Local next
sequences are 2 and 3 respectively. Each fence has words [2,0,1].
G-Set inserts 7 and 42; RGA inserts (1,65),(2,233), then deletes position 1.
State snapshots have no accepted records, contiguous versions or allocation
metadata to assert. Native logs have arity 2 for counters and unbounded for OR-Set.
Every retained native record is redelivered and must leave state/log/versions
unchanged; durable replay also checks allocation and store bytes unchanged.
All five scenario classes occur: duplicate/redelivery, gaps, multi-byte UTF-8
set payloads, tombstones, and writer sequence recovery on a store copy.

```json
{
  "counter": {
    "records": [
      [
        0,
        1,
        "GCounterDelta { replica: 0, tally: 5 }"
      ],
      [
        1,
        3,
        "GCounterDelta { replica: 1, tally: 7 }"
      ]
    ],
    "state": [
      5,
      7
    ],
    "versions": [
      1,
      0
    ],
    "allocation": [
      2,
      0,
      1
    ],
    "next_sequence": 2
  },
  "orset": {
    "records": [
      [
        0,
        1,
        "Add { element: \"café☕\", token: 2 }"
      ],
      [
        0,
        2,
        "Remove { tokens: [2] }"
      ],
      [
        1,
        3,
        "Add { element: \"東京\", token: 7 }"
      ]
    ],
    "state": {
      "adds": [
        [
          "café☕",
          2
        ],
        [
          "東京",
          7
        ]
      ],
      "tombstones": [
        2
      ],
      "elements": [
        "東京"
      ]
    },
    "versions": [
      2,
      0
    ],
    "allocation": [
      2,
      0,
      2
    ],
    "next_sequence": 3
  },
  "pn": {
    "records": [
      [
        0,
        1,
        "Inc { replica: 0, tally: 9 }"
      ],
      [
        1,
        3,
        "Dec { replica: 1, tally: 4 }"
      ]
    ],
    "state": {
      "p": [
        9,
        0
      ],
      "n": [
        0,
        4
      ],
      "value": 5
    },
    "versions": [
      1,
      0
    ]
  },
  "gset": {
    "state": [
      7,
      42
    ]
  },
  "rga": {
    "state": {
      "placed": [
        [
          1,
          65
        ],
        [
          2,
          233
        ]
      ],
      "tombstones": [
        1
      ],
      "live": [
        [
          2,
          233
        ]
      ]
    }
  }
}
```

Probe (read only): `python3 rust/crates/safemesh-crdt/tests/identity_probe.py
PATH.log PATH.transaction PATH.fence PATH.state`. Container suffix is explicit;
it does not establish an identity. The probe validates log length/CRC before
reporting its embedded schema. It is not a complete semantic decoder.

Load-failure regression: `load_failures` copies both durable stores and measures
23-byte truncation, CRC corruption, tag 0x02 and tag 0xff. Current errors are
RecoveryRequired, History(IntegrityMismatch), History(InvalidTag), and
History(InvalidTag), respectively. Every error returns no replica; store and
fence remain unchanged. A future version-dispatch lane should deliberately
update the tag diagnoses to structured version refusals once implemented.
`UnsupportedHistoryVersion { found, supported, migration }` could name the
encountered frame/schema, readable identities and migration availability;
package identity cannot be recovered when absent. Malformed wrapper/CRC cases
should remain malformed/integrity diagnoses, not invented versions.
