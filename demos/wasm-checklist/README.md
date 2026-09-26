# Installed WASM checklist journey

Two independent Node processes use the npm package `safemesh-wasm` over loopback
HTTP, with application-owned identity/log storage. Node 22, Rust's wasm32 target,
wasm-pack and npm are needed. From the repository root, choose an absolute scratch
directory outside the checkout (do not use `/tmp` on the lane estate):

```sh
export PATH=/home/monkey/bin:/home/monkey/.cargo/bin:$PATH
export CHECKLIST_SCRATCH=/home/monkey/scratch/smp3wasmjourney/tmp
export TMPDIR="$CHECKLIST_SCRATCH"
export CARGO_TARGET_DIR=/home/monkey/scratch/smp3wasmjourney/target
export SAFEMESH_CONSUMER=/home/monkey/scratch/smp3wasmjourney/consumer
mkdir -p "$TMPDIR" "$SAFEMESH_CONSUMER"
./scripts/package-wasm.sh nodejs /home/monkey/scratch/smp3wasmjourney/pkg-node
(cd "$SAFEMESH_CONSUMER" && npm init -y && npm install ../pkg-node/safemesh-wasm-0.1.0.tgz)
/home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs
```

Outside the lane estate, use your own writable absolute paths and run `node`
directly if `suiterun` is unavailable. No package is published by these commands.
The app resolves the package by name from the consumer's `package.json`; it never
imports repository bindings. To run one app independently:

```sh
node demos/wasm-checklist/app.mjs "$CHECKLIST_SCRATCH/manual-a" 0
```

It prints its ephemeral loopback port and package resolution. `GET /state` returns
members, history bytes, version vectors and conflict metadata; `GET /log` returns
base64 log bytes. `POST /edit` takes `{"edits":[{"item":"naïve"}]}` (add) or
`{"edits":[{"item":"naïve","remove":true}]}`. `POST /merge` takes a `log` base64
string or `records` array of base64 record strings.

The journey asserts Unicode edits, partitioned add/remove conflict, SIGKILL and
identity/sequence continuity, reversed and duplicate delivery, complete members,
version vectors, retained adds/tombstones, 2,006 retained records, restart of both
apps and duplicate-only exchange. It also measures a cloned live writer: another
process can import it, reuse its next sequence and produce a record collision.
The library explicitly requires application-owned cross-process fencing. This
example has no fencing; never run its clone experiment on production data.

Acceptance after partition, SIGKILL, restart and reordered plus duplicated
delivery is documented convergence, not byte-identical logs. The two apps must
agree on `elements()`, `addEntries()`, `tombstones()` and both writers' version
vectors, and every record either app holds must be admitted as a duplicate by
the other (whole-log merge in both directions returns `duplicate` for every
record). A single record missing on one side, or a late extra record on one
side after the snapshot, fails `CONVERGENCE_WHOLE_LOG_DUPLICATES`. Raw
`logBytes()` are printed as `OBSERVE log-bytes-equal=<true|false>` and are not
a pass or fail condition: logs keep arrival order
(`docs/src/content/docs/using-safemesh.md` lines 502-505).

**Current expected result:** `SECOND_ORDER PASS ...` followed by
`PASS records=2006 ...` and exit 0. Arrival-order log bytes may differ; that is
an observation, not a failure. This journey is not a CI gate.

Planted known-bad modes keep the same oracle and must fail it by name:

```sh
/home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs --plant drop-one-record
/home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs --plant extra-record
```

`--plant drop-one-record` withholds exactly one record from one app's delivery.
`--plant extra-record` appends one late record on one app after the convergence
snapshot. Both exit 1 with `FAIL CONVERGENCE_WHOLE_LOG_DUPLICATES`. The extra
record is `accepted` by the other side rather than ignored or compared by count.

Physical tamper modes each use a disposable store copy after SIGKILL:

```sh
CHECKLIST_TAMPER=delete-log /home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs
CHECKLIST_TAMPER=fresh-identity /home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs
CHECKLIST_TAMPER=corrupt-log /home/monkey/bin/suiterun -- node demos/wasm-checklist/journey.mjs
```

Expected failures: `RESTART_LOG_MISSING`, `RESTART_IDENTITY_CONTINUITY`, and
`RESTART_LOG_MISMATCH`. Originals are retained and untouched; each invocation
creates a new scratch directory. Unknown tamper names should not be used.

Identity is a complete public-API recovery snapshot; the separately saved log is
an integrity mirror. Each file is synced and atomically renamed before an edit is
acknowledged. The pair is not one atomic transaction: a kill between commits can
leave a mismatch and stop restart. SIGKILL after an acknowledged edit is measured;
arbitrary power loss and storage-device durability are not. Missing/corrupt files
never trigger a fresh writer in normal operation. The fresh-identity environment
switch exists solely for the physical tamper experiment.
