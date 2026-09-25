# Fieldcheck · local inspection, exchange, and review (Slices 1–3)

A Linux CLI and a separate Rust process owning one durable UTF-8 OR-Set.
The CLI communicates with its service over stdin/stdout pipes. Two services can
exchange inspections through a real loopback TCP connection, with separate store
directories and fixed writer identities 0 and 1. Building initially may download
Cargo dependencies. The checklist is the versioned, compiled-in `checklist.json`;
inspections are immutable JSON string elements. No experimental LWW types are used.

From the repository root:

```sh
cargo build --locked --manifest-path examples/fieldcheck/Cargo.toml
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/new-store
```

For two services, build once and run these in **separate terminals**:

```sh
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/device-b --writer 1 --listen 127.0.0.1:7402
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/device-a --writer 0 --connect 127.0.0.1:7402
```

Each invocation starts its own OS process with exclusive ownership of its own
store. Keep the same writer flag on restart. The connecting service retries after
socket loss; the listener accepts a new connection after the old socket closes.
Independent saves remain available while disconnected and during a live connection.
`--listen` and `--connect` are start-time flags, not prompt commands. To make
opposing offline inspections and then reconnect, start each CLI without its
network flag, using `--writer 1` for device-b and `--writer 0` for device-a.
Enter `draft` and `save` on each. Then enter `quit` at both prompts and restart
the same stores in separate terminals:

```sh
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/device-b --writer 1 --listen 127.0.0.1:7402
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/device-a --writer 0 --connect 127.0.0.1:7402
```

Enter `show` after exchange. The CLI names each item needing review and each
resolved item in plain words, using the service's current status projection.
There is no simulated-offline switch: kill a service or actually close the socket.
Only numeric loopback addresses are supported in this slice. One listener and one
connector form the fixed pair; there is no enrollment or relay.

The store directory must initially **not exist**. Its parent must exist.
Every existing store is reopened with checked replay, including empty or damaged
directories; recovery failure preserves the store and disables inspection editing.
Do not delete fence files, copy an active store, or reuse its identity.

At the prompt, after the service PID appears:

```text
draft Fail "Alex" "Latch slips when pulled — check hinge"
save
show
```

`show` requests a fresh service snapshot, including received records, network
status, durable delivery evidence, and review state. It does not rely on the
CLI’s cached list. The prompt commands are `draft Pass|Fail "inspector" "note"`,
`save`, `show`, `reopen`, and `quit`.

The draft gets a UUID and is fsynced to `new-store.draft.json` beside the store
before submission. Keep that file when resolving an uncertain save.
Record field order is fixed; observed event IDs are sorted. The returned sequence
belongs to SafeMesh, not a clock. A review is a new inspection whose
`observed_event_ids` cite existing records for the same item, including at least
one differing answer. The service rejects unknown cited IDs on local submission.
It accepts a remote review before its cited observations arrive, so exchange order
does not discard the review.

The interface displays `Saving…` until the service responds, then
`Saved on this device` only following successful durable `add`.
A real write error displays `Not saved — local storage error; fix the storage
problem, then reopen` followed by the plain OS reason, and retains the draft.
SafeMesh revokes further writes after persistence failure. After addressing the
storage problem, enter `reopen` in the same CLI session. This closes the running
service through its stdin, waits for it to release the store, and starts a fresh
service with the retained draft. Enter `save` to persist that draft. `reopen`
also restarts a healthy service; wait for a pending save to finish first.

## Force-kill journey

In another terminal, run `kill -KILL PID` using the printed **service** PID.
The interface displays `Local service stopped — no inspection data loaded`,
clears loaded records and refuses edits. Enter `reopen`: a new service PID
opens the same store and displays the recovered canonical record, writer and
sequence. Compare these with the pre-kill `show` output. `quit` is a graceful
exit and is not evidence for this journey.

For a lost reply, start with a fresh store and add
`--reply-barrier /absolute/path/reply-pending` to the Python command.
This is an external journey control, not inspection data. After `save`, the
service creates this marker **after successful add and before replying** and
waits while the marker exists. When it appears, SIGKILL the printed service PID.
The interface displays `Save status unknown — reopen to check` followed by the
stopped state. On `reopen`, it finds the retained draft's event ID, checks the
whole record, reports the save and clears the draft without adding another
record. Alternatively, remove the marker to release a live paused response.

For a reproducible real write failure in a disposable store, create a directory
named `writer-0.tmp` inside it after startup but before saving. File creation
then fails with EISDIR; the interface retains the draft. Remove that empty
directory, enter `reopen`, then `save` to retry in the same CLI session.

`Store parent directory does not exist; create the parent and reopen` identifies
a missing parent path. `Another Fieldcheck process owns this store; close it and
reopen` identifies a live owner. `Could not recover this checklist; inspect the
damaged store before reopening` means replay or application validation failed.
The application never substitutes an empty checklist for a damaged store.
Unsupported schemas, removals, unexpected writers and event-ID collisions are
rejected. Resubmitting exactly the same event returns its original record and
sequence; the same ID with different content is refused.

This adapts the create/restart pattern from `../gold-path/rust`, whose sequential
example has no service/UI scaffold and also demonstrates a second writer.
The service's `ready` and `status` JSON list `needs_review` and `resolved` item
IDs, projected from the durable records. A review resolves a disagreement only
when it cites every other observation currently present for that item. An
unreferenced later observation or a competing review reopens it. Original records
remain unchanged. The Python CLI still creates ordinary inspections with an
empty citation list. To file a review today, quit the Python CLI so it releases
the store, then start the Rust service directly on that store with the same
`--writer` value. Send a new inspection JSON line on its stdin with a fresh
`event_id`, the same `item_id`, your chosen `Pass` or `Fail` answer and note, and
an `observed_event_ids` array containing every other observation for that item
shown by `show`. The array must be sorted; at least one cited answer must differ
from yours. The service responds with `saved` or an error. Exit that service,
restart the Python CLI with the same writer and network flags, and enter `show`.
There is no browser UI. The forced-kill journey
establishes process-crash recovery on the tested Linux filesystem, not arbitrary
hardware/power-loss guarantees.

## Exchange and durable evidence

Each frame contains a big-endian u32 payload length, a big-endian CRC32, and a
JSON protocol message. The receiver checks the 16 MiB frame limit before allocation
and the CRC before decoding. CRC detects accidental corruption; this protocol is
**not authenticated or encrypted**. Use the fixed pair on a trusted loopback link.
A complete batch or map must fit the frame limit; oversized histories report a
network error rather than agreement. Pagination is not implemented.

A round exchanges protocol-v1 summaries: writer identity, sorted positive
contiguous writer/sequence prefixes, and zero acknowledgements (empty because
Fieldcheck allocates only positive sequences). `EventLog::since` chooses missing
records without hiding sequence gaps. The connector sends its batch and receives
a receipt, then the listener does the same. The connection repeats rounds every
250 ms; saves during a round are picked up in later rounds.

Records carry their full SafeMesh canonical wire bytes and RecordId. Admission
checks schema, canonical encoding, writer/token ownership, RecordId collisions and
application event-ID collisions before writing anything. The service atomically
replaces and fsyncs `incoming.json` before calling `DurableReplica::receive`.
That API also commits each record durably. An interrupted batch is replayed from
the journal at restart; exact duplicate records remain one projected observation.
The journal is removed and the directory synced after the entire batch succeeds.
A persistence error disables further saves and confirmations until restart.

The receiver returns its complete sorted RecordId-to-canonical-bytes map only
after durable acceptance. The sender atomically replaces and fsyncs
`peer-receipt.json`, including the fixed peer identity, before exposing confirmation.
Both files use temporary-file sync, rename and directory sync on Linux.
Receipts are checked and reloaded on restart. A socket send alone confirms nothing.

`Both devices have these N records (last durable peer confirmation)` requires
exact equality of the local sorted map and the persisted peer receipt map.
Neither counts nor hashes substitute for the maps. The qualifier identifies a
confirmed snapshot, not a claim to know a disconnected peer’s later saves.
A local save changes the map and returns delivery to waiting. `peer_confirmed`
counts byte-identical local records in the durable receipt; a partial count does
not produce the exact-set statement. Network connectivity and delivery evidence
are separate facts. Convergence does not resolve inspectors’ answers.

For external crash-boundary measurements, set `FIELDCHECK_BEFORE_ACCEPT` to an
absolute marker path on the receiver, or `FIELDCHECK_BEFORE_RECEIPT` on the sender.
The first pauses after validation but before journal persistence; the second
pauses after receiving durable peer confirmation but before storing it locally.
The process creates the marker and waits for its removal. SIGKILL that process,
remove the marker, and restart without the environment variable to measure repair.
These controls never inject inspection answers or simulate disconnection.

Loopback with two processes does not establish behavior on a real Wi-Fi link or
two physical devices. That is Slice 4 and requires Ben. Lean-backed OR-Set semantics,
finite Rust conformance evidence, and these application journeys are three separate
evidence layers; none alone proves the complete network/storage application.
