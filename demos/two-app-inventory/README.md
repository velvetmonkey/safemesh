# Two receiving applications over HTTP

A clinic and a warehouse each count received units in a G-Counter. Each runs in
its own independently launched OS process, with a private virtual environment
and SQLite store. Each imports the installed **safemesh-python==0.1.0** wheel.
The applications send SafeMesh record bytes directly to one another over real
loopback HTTP sockets. The journey driver sends control requests and reads
status; it never carries CRDT records between applications.

This is an executable acceptance journey for the supported G-Counter path. It
counts cumulative receipts, not current stock: G-Counter cannot subtract units.
The Python package is a locally built artifact; this does not claim a PyPI
release. Only the public `safemesh_python.GCounterReplica` API performs CRDT
mutation, admission, encoding and replay. No private SafeMesh modules or
repository-relative application dependencies are used.

## Build the package and run

Prerequisites: Linux, Python 3.12 with venv support, Rust 1.96.1 (selected by the
repository), and maturin 1.x. Run from the repository root. Choose a fresh
absolute scratch directory; existing journey directories are deliberately
rejected to preserve evidence and prevent accidental reuse of replica identity.

```sh
export PATH=/home/monkey/bin:$PATH
DEMO_WORK="$HOME/scratch/two-app-inventory"
mkdir -p "$DEMO_WORK/wheels" "$DEMO_WORK/tmp"
export TMPDIR="$DEMO_WORK/tmp"
(cd rust/crates/safemesh-python && \
  maturin build --release --locked --features extension-module --out "$DEMO_WORK/wheels")
python3 demos/two-app-inventory/journey.py \
  --wheels "$DEMO_WORK/wheels" --run-dir "$DEMO_WORK/journey"
```

The wheel build necessarily compiles the repository's package sources. Consumer
installation is separate: the driver uses `pip install --no-index --find-links
<WHEELS> -r requirements.txt`, with an exact version requirement, in **two separate
venvs**. It copies `clinic.py` and `node.py` into the clinic deployment and
`warehouse.py` and `node.py` into the warehouse deployment. Neither application
has the other's entry point. Python source-path environment overrides are
removed. Runtime evidence checks the installed package version and that its
module file resides inside the application's own venv.

The default workload makes 1,000 edits per application in each of three phases,
with 25 ms between each pair of edits: **6,001 unique records** including the
negative-control edit, and at least 75 seconds of intentional pacing, plus
installation, HTTP, SQLite and replay time. Every accepted record is retained;
there is no compaction. Each final store holds all 6,001 records. This is a
history/recovery exercise, not a throughput benchmark or a long-running soak.

The driver checks this sequence:

1. Start the applications separately, make local edits, sync, and check expected
   coordinates, sum, record count and normalized full-history digest.
2. Disable both replication links (HTTP 503), verify outgoing sync is blocked,
   and keep editing independently. Local control HTTP remains reachable.
3. SIGKILL the clinic, start a new process against its own store, and require
   identical state, record count, normalized history and raw serialized log.
   Make more local edits on both sides while still partitioned. This also checks
   that replay restores local sequence allocation without record-ID collisions.
4. Reconnect, send retained records in reverse arrival order and send each twice.
   Require both new admissions and duplicates, then check convergence.
5. Add one final clinic receipt and deliberately omit its record from a real
   HTTP delivery. Require the same convergence oracle to reject the divergence.
   Deliver it, heal, SIGKILL and restart **both** applications, then check again.

Output contains JSON events with PIDs, module locations, states, counts, delivery
results and elapsed wall time. Success ends with `"event": "PASS"`; any unmet
acceptance condition raises and exits nonzero. Stores and process logs remain
in the run directory. The driver cleans up its processes even on failure.

To prove the oracle also fails as a command, run the same journey with the
planted dropped record left unrepaired:

```sh
python3 demos/two-app-inventory/journey.py \
  --wheels "$DEMO_WORK/wheels" --run-dir "$DEMO_WORK/negative" \
  --rounds 5 --interval 0 --leave-divergent
```

This must exit **1** with `AssertionError: divergent state`, clinic `[16, 15]`
and warehouse `[15, 15]`. Its small workload is only the negative control;
use the default workload for retained-history evidence.

## Independent manual launches

After a driver run, the deployments remain self-contained and can be launched
from separate terminals. No running driver is necessary:

```sh
"$DEMO_WORK/journey/clinic/venv/bin/python" -E -s \
  "$DEMO_WORK/journey/clinic/clinic.py" \
  --store "$DEMO_WORK/journey/clinic/store" --port 18781 \
  --peer http://127.0.0.1:18782
```

```sh
"$DEMO_WORK/journey/warehouse/venv/bin/python" -E -s \
  "$DEMO_WORK/journey/warehouse/warehouse.py" \
  --store "$DEMO_WORK/journey/warehouse/store" --port 18782 \
  --peer http://127.0.0.1:18781
```

`GET /status` returns state and evidence. `POST /add` with `{}` records one local
receipt. `POST /sync` with `{"reverse":true,"duplicate":true}` makes that
application deliver its retained records directly to its configured peer.
`POST /link` with `{"online":false}` blocks incoming and outgoing replication;
`true` reconnects. Use JSON bodies and `Content-Type: application/json`.
Only one process may own a given store. Both applications may sync simultaneously;
each sync sends a snapshot and waits for its peer without holding the local
state lock.

## Public API friction and application workarounds

- **Durability is application-owned.** The binding has no durable replica/open
  operation. SQLite stores opaque record bytes, uses WAL and `synchronous=FULL`,
  and commits before responding. On a failed transaction the application rebuilds
  its in-memory replica from committed records. A crash between commit and reply
  leaves an ambiguous local command result; `/add` has no client idempotency key.
- **Identity and counter shape are external configuration.** Coordinates 0 and 1
  and dimension 2 are fixed here. Store metadata prevents opening a clinic store
  as the warehouse. The binding does not provision globally unique replica IDs.
- **Increment is read/modify/append.** `append_bump` takes an absolute tally, not
  an increment amount. The app reads its coordinate and adds one; a shared lock
  serializes local state access and complete SQLite transactions. Replay uses
  `merge_record_bytes` to rebuild state and the log before accepting new local
  operations.
- **Transport and coordination are hand-written.** The binding supplies bytes,
  not HTTP, peer discovery, partition management, acknowledgements, retry queues,
  or an anti-entropy scheduler. The driver triggers application-to-application
  sync; a subsequent full sync retries missing records. Partition controls model
  a disabled replication link, not firewall rules or failure of the control path.
- **No public record iterator in the Python replica.** The app captures bytes
  returned by `append_bump` and bytes received over HTTP in its own journal.
  It neither decodes nor edits the SafeMesh wire format. JSON/base64 is an
  application envelope, with a 4 MiB request cap, not a SafeMesh protocol.
- **Single-record admission has no verdict.** `merge_record_bytes` returns None
  or raises. After core validation the SQLite unique-byte constraint counts
  exact duplicates; core exceptions still reject record-ID collisions. These
  application counts are not claimed as structured SafeMesh admission results.
- **Serialized log order is not a canonical history order.** The initial smoke
  run converged in value but failed byte-hash equality. The application now
  replays lexicographically sorted opaque records into a temporary public replica
  for `log_sha256`. It also reports `raw_log_sha256`, whose arrival-order bytes
  must survive a restart unchanged but can differ between peers. This preserves
  a complete-history equality check rather than checking the value alone.
- **Resource costs remain unbounded.** Full history lives in SQLite and in the
  replica; full sync, replay and normalized digest computation scan it. Sync
  allocates a whole JSON batch; the receiver's cap is not a bounded sender API.
  There is no garbage collection, checkpoint, incremental cursor or pagination.
- **Errors are not a recovery protocol.** Exceptions become HTTP errors, and the
  app replays on mutation failure. There is no binding-level transaction coupling
  to SQLite. Corrupt stores fail startup; this demo does not repair them.
- **Scope limitation.** Python's OR-Set exposes numeric elements, not the UTF-8
  OR-Set journey requested as an alternative, so this demo uses only G-Counter.
  No internal import or source-tree bypass was needed.

## Claim boundary

Measured SIGKILL recovery covers committed records on a local filesystem, not
power loss, disk failure, arbitrary corruption, process death at every write
instruction, or package upgrades. SQLite/filesystem durability is an application
assumption, not part of the CRDT proof. HTTP is unauthenticated, loopback-only,
with fault-injection controls: do not expose it as a production service.
Cross-host transport, other Python/platform versions, hostile peers, many
replicas, and larger histories remain unverified. Nothing here adds a new proof
or extends the supported CRDT surface.

## Simultaneous-sync regression

With the wheel installed, run `python demos/two-app-inventory/test_sync.py`.
Two forwarding peers rendezvous before delivering records, forcing both apps
into outgoing sync together. Both `/records` probes and syncs must finish within
two seconds. The test also checks convergence and serialized concurrent local
edits. The existing package smoke stage runs this regression automatically.
